//! Orchestrator: wave computation, bean sizing, and dispatch planning.
//!
//! Pure logic module — no process spawning. Uses internal Bean/Index types
//! directly rather than shelling out to the `bn` CLI.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::bean::{Bean, Status};
use crate::config::Config;
use crate::index::Index;
use crate::tokens::calculate_tokens;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What action should be taken for a sized bean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeanAction {
    /// Bean fits within token budget — implement directly.
    Implement,
    /// Bean exceeds token budget — needs planning/decomposition.
    Plan,
}

/// A bean annotated with its estimated token count and recommended action.
#[derive(Debug, Clone)]
pub struct SizedBean {
    pub bean: Bean,
    pub tokens: u64,
    pub action: BeanAction,
}

/// A group of beans that can execute in parallel (all deps satisfied).
#[derive(Debug, Clone)]
pub struct Wave {
    pub beans: Vec<SizedBean>,
}

/// The full dispatch plan: waves of work plus any skipped beans.
#[derive(Debug, Clone)]
pub struct DispatchPlan {
    /// Ordered waves. Wave 0 has no deps, wave 1 depends only on wave 0, etc.
    pub waves: Vec<Wave>,
    /// Beans that exceed max_tokens and auto-plan is not enabled.
    pub skipped: Vec<SizedBean>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get all open beans that are ready to work on (no unmet dependencies).
///
/// Uses the index and the same dependency-resolution logic as `bn ready`.
/// Returns full `Bean` objects (not just index entries).
pub fn get_ready_beans(beans_dir: &Path) -> Result<Vec<Bean>> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let ready_ids: Vec<String> = index
        .beans
        .iter()
        .filter(|entry| {
            // Must have verify (SPECs, not GOALs)
            if !entry.has_verify {
                return false;
            }
            if entry.status != Status::Open {
                return false;
            }
            // All explicit deps must be closed
            entry.dependencies.iter().all(|dep_id| {
                index
                    .beans
                    .iter()
                    .find(|e| &e.id == dep_id)
                    .map_or(false, |e| e.status == Status::Closed)
            })
            // All smart deps (requires/produces) must be satisfied
            && entry.requires.iter().all(|required| {
                // Find sibling that produces this artifact
                match index.beans.iter().find(|e| {
                    e.id != entry.id
                        && e.parent == entry.parent
                        && e.produces.contains(required)
                }) {
                    Some(producer) => producer.status == Status::Closed,
                    None => true, // no producer found = not blocked
                }
            })
        })
        .map(|e| e.id.clone())
        .collect();

    // Load full beans from disk
    let mut beans = Vec::new();
    for id in &ready_ids {
        let bean = find_and_load_bean(beans_dir, id)?;
        if let Some(b) = bean {
            beans.push(b);
        }
    }

    Ok(beans)
}

/// Size a bean: estimate tokens and classify as Implement or Plan.
///
/// Uses the same `calculate_tokens` heuristic from `src/tokens.rs`.
/// The workspace path is the project root (parent of `.beans`).
pub fn size_bean(bean: &Bean, beans_dir: &Path, max_tokens: u32) -> SizedBean {
    let workspace = beans_dir.parent().unwrap_or(beans_dir);
    let tokens = calculate_tokens(bean, workspace);
    let action = if tokens > max_tokens as u64 {
        BeanAction::Plan
    } else {
        BeanAction::Implement
    };
    SizedBean {
        bean: bean.clone(),
        tokens,
        action,
    }
}

/// Compute execution waves from a set of beans.
///
/// Groups beans by dependency order:
/// - Wave 0: beans with no dependencies (or all deps already closed)
/// - Wave 1: beans whose deps are all in wave 0
/// - Wave N: beans whose deps are all in waves 0..N-1
///
/// Beans involved in dependency cycles are silently dropped (with an
/// `eprintln!` warning).
pub fn compute_waves(beans: &[Bean]) -> Vec<Vec<Bean>> {
    let mut waves: Vec<Vec<Bean>> = Vec::new();

    // Start with open beans only
    let mut remaining: Vec<Bean> = beans
        .iter()
        .filter(|b| b.status == Status::Open)
        .cloned()
        .collect();

    // Seed completed set with closed beans
    let mut completed: HashSet<String> = beans
        .iter()
        .filter(|b| b.status == Status::Closed)
        .map(|b| b.id.clone())
        .collect();

    while !remaining.is_empty() {
        let (ready, blocked): (Vec<Bean>, Vec<Bean>) = remaining
            .into_iter()
            .partition(|b| b.dependencies.iter().all(|d| completed.contains(d)));

        if ready.is_empty() {
            // Remaining beans form a cycle — warn and break
            let ids: Vec<&str> = blocked.iter().map(|b| b.id.as_str()).collect();
            eprintln!(
                "Warning: dependency cycle detected, skipping beans: {}",
                ids.join(", ")
            );
            break;
        }

        for b in &ready {
            completed.insert(b.id.clone());
        }
        waves.push(ready);
        remaining = blocked;
    }

    waves
}

/// Build a complete dispatch plan: ready beans → sized → waved → plan.
///
/// `auto_plan` controls whether large beans are included (as Plan actions)
/// or pushed to `skipped`.
pub fn plan_dispatch(beans_dir: &Path, auto_plan: bool) -> Result<DispatchPlan> {
    let config = Config::load(beans_dir).unwrap_or_else(|_| Config {
        project: String::new(),
        next_id: 0,
        auto_close_parent: true,
        max_tokens: 30000,
        run: None,
        plan: None,
        max_loops: 10,
        max_concurrent: 4,
        poll_interval: 30,
        extends: vec![],
    });

    let ready = get_ready_beans(beans_dir)?;
    let raw_waves = compute_waves(&ready);

    let mut waves = Vec::new();
    let mut skipped = Vec::new();

    for wave_beans in raw_waves {
        let mut wave_sized = Vec::new();
        for bean in wave_beans {
            let sized = size_bean(&bean, beans_dir, config.max_tokens);
            match sized.action {
                BeanAction::Implement => wave_sized.push(sized),
                BeanAction::Plan => {
                    if auto_plan {
                        wave_sized.push(sized);
                    } else {
                        skipped.push(sized);
                    }
                }
            }
        }
        if !wave_sized.is_empty() {
            waves.push(Wave { beans: wave_sized });
        }
    }

    Ok(DispatchPlan { waves, skipped })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find and load a bean by ID from the beans directory.
fn find_and_load_bean(beans_dir: &Path, id: &str) -> Result<Option<Bean>> {
    let entries = std::fs::read_dir(beans_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        // Match new format: {id}-{slug}.md
        if filename.ends_with(".md") && filename.starts_with(&format!("{}-", id)) {
            return Ok(Some(Bean::from_file(&path)?));
        }
        // Match legacy format: {id}.yaml
        if filename == format!("{}.yaml", id) {
            return Ok(Some(Bean::from_file(&path)?));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a beans dir with config.
    fn setup_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 100\nmax_tokens: 100\n",
        )
        .unwrap();
        (dir, beans_dir)
    }

    /// Helper: write a bean to disk.
    fn write_bean(beans_dir: &Path, bean: &Bean) {
        let filename = format!("{}.yaml", bean.id);
        bean.to_file(beans_dir.join(filename)).unwrap();
    }

    // -----------------------------------------------------------------------
    // compute_waves tests
    // -----------------------------------------------------------------------

    #[test]
    fn compute_waves_no_deps_single_wave() {
        let beans = vec![
            Bean::new("1", "A"),
            Bean::new("2", "B"),
            Bean::new("3", "C"),
        ];
        let waves = compute_waves(&beans);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].len(), 3);
    }

    #[test]
    fn compute_waves_linear_chain() {
        // 1 -> 2 -> 3
        let b1 = Bean::new("1", "A");
        let mut b2 = Bean::new("2", "B");
        b2.dependencies = vec!["1".to_string()];
        let mut b3 = Bean::new("3", "C");
        b3.dependencies = vec!["2".to_string()];

        let waves = compute_waves(&[b1, b2, b3]);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), 1);
        assert_eq!(waves[0][0].id, "1");
        assert_eq!(waves[1].len(), 1);
        assert_eq!(waves[1][0].id, "2");
        assert_eq!(waves[2].len(), 1);
        assert_eq!(waves[2][0].id, "3");
    }

    #[test]
    fn compute_waves_diamond() {
        //     1
        //    / \
        //   2   3
        //    \ /
        //     4
        let b1 = Bean::new("1", "Root");
        let mut b2 = Bean::new("2", "Left");
        b2.dependencies = vec!["1".to_string()];
        let mut b3 = Bean::new("3", "Right");
        b3.dependencies = vec!["1".to_string()];
        let mut b4 = Bean::new("4", "Join");
        b4.dependencies = vec!["2".to_string(), "3".to_string()];

        let waves = compute_waves(&[b1, b2, b3, b4]);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), 1); // 1
        assert_eq!(waves[1].len(), 2); // 2, 3
        assert_eq!(waves[2].len(), 1); // 4
    }

    #[test]
    fn compute_waves_with_closed_deps() {
        // b1 is closed, b2 depends on b1 → b2 should be in wave 0
        let mut b1 = Bean::new("1", "Done");
        b1.status = Status::Closed;
        let mut b2 = Bean::new("2", "Depends on done");
        b2.dependencies = vec!["1".to_string()];

        let waves = compute_waves(&[b1, b2]);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0][0].id, "2");
    }

    #[test]
    fn compute_waves_cycle_skips() {
        // a -> b -> a (cycle)
        let mut a = Bean::new("a", "A");
        a.dependencies = vec!["b".to_string()];
        let mut b = Bean::new("b", "B");
        b.dependencies = vec!["a".to_string()];

        let waves = compute_waves(&[a, b]);
        assert!(waves.is_empty(), "Cycles should produce no waves");
    }

    #[test]
    fn compute_waves_empty_input() {
        let waves = compute_waves(&[]);
        assert!(waves.is_empty());
    }

    // -----------------------------------------------------------------------
    // size_bean tests
    // -----------------------------------------------------------------------

    #[test]
    fn size_bean_small_is_implement() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let bean = Bean::new("1", "Small task");
        let sized = size_bean(&bean, &beans_dir, 30000);

        assert_eq!(sized.action, BeanAction::Implement);
        assert!(sized.tokens < 30000);
    }

    #[test]
    fn size_bean_large_is_plan() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let mut bean = Bean::new("1", "Large task");
        // Create a huge description to exceed even a small threshold
        bean.description = Some("x".repeat(500));

        // Use a very low max_tokens threshold (1 token = 4 chars, so 500 chars = 125 tokens)
        let sized = size_bean(&bean, &beans_dir, 10);
        assert_eq!(sized.action, BeanAction::Plan);
        assert!(sized.tokens > 10);
    }

    // -----------------------------------------------------------------------
    // get_ready_beans tests
    // -----------------------------------------------------------------------

    #[test]
    fn get_ready_beans_returns_open_with_verify() {
        let (_dir, beans_dir) = setup_beans_dir();

        let mut b1 = Bean::new("1", "Ready");
        b1.verify = Some("echo ok".to_string());
        write_bean(&beans_dir, &b1);

        // No verify → GOAL, not ready
        let b2 = Bean::new("2", "Goal");
        write_bean(&beans_dir, &b2);

        let ready = get_ready_beans(&beans_dir).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "1");
    }

    #[test]
    fn get_ready_beans_excludes_blocked() {
        let (_dir, beans_dir) = setup_beans_dir();

        let mut b1 = Bean::new("1", "Blocker");
        b1.verify = Some("echo ok".to_string());
        write_bean(&beans_dir, &b1);

        let mut b2 = Bean::new("2", "Blocked");
        b2.verify = Some("echo ok".to_string());
        b2.dependencies = vec!["1".to_string()];
        write_bean(&beans_dir, &b2);

        let ready = get_ready_beans(&beans_dir).unwrap();
        // Only b1 should be ready (b2 is blocked by open b1)
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "1");
    }

    #[test]
    fn get_ready_beans_unblocked_when_dep_closed() {
        let (_dir, beans_dir) = setup_beans_dir();

        let mut b1 = Bean::new("1", "Done");
        b1.status = Status::Closed;
        b1.verify = Some("echo ok".to_string());
        write_bean(&beans_dir, &b1);

        let mut b2 = Bean::new("2", "Ready now");
        b2.verify = Some("echo ok".to_string());
        b2.dependencies = vec!["1".to_string()];
        write_bean(&beans_dir, &b2);

        let ready = get_ready_beans(&beans_dir).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "2");
    }

    // -----------------------------------------------------------------------
    // plan_dispatch tests
    // -----------------------------------------------------------------------

    #[test]
    fn plan_dispatch_returns_correct_waves() {
        let (_dir, beans_dir) = setup_beans_dir();

        // Two independent beans → 1 wave
        let mut b1 = Bean::new("1", "First");
        b1.verify = Some("echo ok".to_string());
        write_bean(&beans_dir, &b1);

        let mut b2 = Bean::new("2", "Second");
        b2.verify = Some("echo ok".to_string());
        write_bean(&beans_dir, &b2);

        let plan = plan_dispatch(&beans_dir, false).unwrap();
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 2);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn plan_dispatch_skips_large_beans() {
        let (_dir, beans_dir) = setup_beans_dir();
        // config has max_tokens: 100

        let mut small = Bean::new("1", "Small");
        small.verify = Some("echo ok".to_string());
        write_bean(&beans_dir, &small);

        let mut large = Bean::new("2", "Large");
        large.verify = Some("echo ok".to_string());
        large.description = Some("x".repeat(2000)); // 2000 chars = ~500 tokens > 100
        write_bean(&beans_dir, &large);

        let plan = plan_dispatch(&beans_dir, false).unwrap();
        // Small bean in waves, large in skipped
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 1);
        assert_eq!(plan.waves[0].beans[0].bean.id, "1");
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].bean.id, "2");
    }

    #[test]
    fn plan_dispatch_includes_large_with_auto_plan() {
        let (_dir, beans_dir) = setup_beans_dir();
        // config has max_tokens: 100

        let mut large = Bean::new("1", "Large");
        large.verify = Some("echo ok".to_string());
        large.description = Some("x".repeat(2000));
        write_bean(&beans_dir, &large);

        let plan = plan_dispatch(&beans_dir, true).unwrap();
        // With auto_plan, large beans go into waves as Plan action
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans[0].action, BeanAction::Plan);
        assert!(plan.skipped.is_empty());
    }
}
