use std::path::Path;

use anyhow::Result;

use crate::bean::Status;
use crate::blocking::{check_blocked, check_scope_warning, BlockReason, ScopeWarning};
use crate::config::Config;
use crate::index::{ArchiveIndex, Index, IndexEntry};
use crate::stream::{self, StreamEvent};

use super::ready_queue::all_deps_closed;
use super::wave::{compute_waves, Wave};
use super::BeanAction;

/// A bean ready for dispatch.
#[derive(Debug, Clone)]
pub struct SizedBean {
    pub id: String,
    pub title: String,
    pub action: BeanAction,
    pub priority: u8,
    pub dependencies: Vec<String>,
    pub parent: Option<String>,
    pub produces: Vec<String>,
    pub requires: Vec<String>,
    pub paths: Vec<String>,
}

/// A bean that was excluded from dispatch due to scope issues.
#[derive(Debug, Clone)]
pub struct BlockedBean {
    pub id: String,
    pub title: String,
    pub reason: BlockReason,
}

/// Result from planning dispatch.
pub struct DispatchPlan {
    pub waves: Vec<Wave>,
    pub skipped: Vec<BlockedBean>,
    /// Scope warnings for beans that will dispatch but have large scope.
    pub warnings: Vec<(String, ScopeWarning)>,
    /// Flat list of all beans to dispatch (for ready-queue mode).
    pub all_beans: Vec<SizedBean>,
    /// The index snapshot used for planning.
    pub index: Index,
}

/// Plan dispatch: get ready beans, filter by scope, compute waves.
pub(super) fn plan_dispatch(
    beans_dir: &Path,
    _config: &Config,
    filter_id: Option<&str>,
    _auto_plan: bool,
    simulate: bool,
) -> Result<DispatchPlan> {
    let index = Index::load_or_rebuild(beans_dir)?;
    let archive = ArchiveIndex::load_or_rebuild(beans_dir).unwrap_or_else(|_| ArchiveIndex {
        beans: Vec::new(),
    });

    // Get candidate beans: open with verify.
    // In simulate mode (dry-run), include all open beans with verify — even those
    // whose deps aren't met yet — so compute_waves can show the full execution plan.
    // In normal mode, only include beans whose deps are already closed.
    let mut candidate_entries: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|e| {
            e.has_verify
                && e.status == Status::Open
                && (simulate || all_deps_closed(e, &index, &archive))
        })
        .collect();

    // Filter by ID if provided
    if let Some(filter_id) = filter_id {
        // Check if it's a parent — if so, get its ready children
        let is_parent = index
            .beans
            .iter()
            .any(|e| e.parent.as_deref() == Some(filter_id));
        if is_parent {
            candidate_entries.retain(|e| e.parent.as_deref() == Some(filter_id));
        } else {
            candidate_entries.retain(|e| e.id == filter_id);
        }
    }

    // Partition into dispatchable vs blocked.
    // In simulate mode, skip blocking checks — we want to show the full plan.
    // In normal mode, dependency blocking is already handled by all_deps_closed above,
    // but check_blocked catches edge cases (e.g., missing deps not in index).
    // Scope warnings (oversized) are non-blocking — beans dispatch with a warning.
    let mut dispatch_beans: Vec<SizedBean> = Vec::new();
    let mut skipped: Vec<BlockedBean> = Vec::new();
    let mut warnings: Vec<(String, ScopeWarning)> = Vec::new();

    for entry in &candidate_entries {
        if !simulate {
            if let Some(reason) = check_blocked(entry, &index) {
                skipped.push(BlockedBean {
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    reason,
                });
                continue;
            }
        }
        // Check for scope warnings (non-blocking)
        if let Some(warning) = check_scope_warning(entry) {
            warnings.push((entry.id.clone(), warning));
        }
        dispatch_beans.push(SizedBean {
            id: entry.id.clone(),
            title: entry.title.clone(),
            action: BeanAction::Implement,
            priority: entry.priority,
            dependencies: entry.dependencies.clone(),
            parent: entry.parent.clone(),
            produces: entry.produces.clone(),
            requires: entry.requires.clone(),
            paths: entry.paths.clone(),
        });
    }

    let waves = compute_waves(&dispatch_beans, &index);

    Ok(DispatchPlan {
        waves,
        skipped,
        warnings,
        all_beans: dispatch_beans,
        index,
    })
}

/// Print the dispatch plan without executing.
pub(super) fn print_plan(plan: &DispatchPlan) {
    for (wave_idx, wave) in plan.waves.iter().enumerate() {
        println!("Wave {}: {} bean(s)", wave_idx + 1, wave.beans.len());
        for sb in &wave.beans {
            let warning = plan
                .warnings
                .iter()
                .find(|(id, _)| id == &sb.id)
                .map(|(_, w)| format!("  ⚠ {}", w))
                .unwrap_or_default();
            println!("  {}  {}  {}{}", sb.id, sb.title, sb.action, warning);
        }
    }

    if !plan.skipped.is_empty() {
        println!();
        println!("Blocked ({}):", plan.skipped.len());
        for bb in &plan.skipped {
            println!("  ⚠ {}  {}  ({})", bb.id, bb.title, bb.reason);
        }
    }
}

/// Print the dispatch plan as JSON stream events.
pub(super) fn print_plan_json(plan: &DispatchPlan, parent_id: Option<&str>) {
    let parent_id = parent_id.unwrap_or("all").to_string();
    let rounds: Vec<stream::RoundPlan> = plan
        .waves
        .iter()
        .enumerate()
        .map(|(i, wave)| stream::RoundPlan {
            round: i + 1,
            beans: wave
                .beans
                .iter()
                .map(|b| stream::BeanInfo {
                    id: b.id.clone(),
                    title: b.title.clone(),
                    round: i + 1,
                })
                .collect(),
        })
        .collect();

    stream::emit(&StreamEvent::DryRun { parent_id, rounds });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    fn write_config(beans_dir: &Path, run: Option<&str>) {
        let run_line = match run {
            Some(r) => format!("run: \"{}\"\n", r),
            None => String::new(),
        };
        fs::write(
            beans_dir.join("config.yaml"),
            format!("project: test\nnext_id: 1\n{}", run_line),
        )
        .unwrap();
    }

    #[test]
    fn plan_dispatch_no_ready_beans() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false, false).unwrap();

        assert!(plan.waves.is_empty());
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn plan_dispatch_returns_ready_beans() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Task one");
        bean.verify = Some("echo ok".to_string());
        bean.produces = vec!["X".to_string()];
        bean.paths = vec!["src/x.rs".to_string()];
        bean.to_file(beans_dir.join("1-task-one.md")).unwrap();

        let mut bean2 = crate::bean::Bean::new("2", "Task two");
        bean2.verify = Some("echo ok".to_string());
        bean2.produces = vec!["Y".to_string()];
        bean2.paths = vec!["src/y.rs".to_string()];
        bean2.to_file(beans_dir.join("2-task-two.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 2);
    }

    #[test]
    fn plan_dispatch_filters_by_id() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Task one");
        bean.verify = Some("echo ok".to_string());
        bean.produces = vec!["X".to_string()];
        bean.paths = vec!["src/x.rs".to_string()];
        bean.to_file(beans_dir.join("1-task-one.md")).unwrap();

        let mut bean2 = crate::bean::Bean::new("2", "Task two");
        bean2.verify = Some("echo ok".to_string());
        bean2.produces = vec!["Y".to_string()];
        bean2.paths = vec!["src/y.rs".to_string()];
        bean2.to_file(beans_dir.join("2-task-two.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 1);
        assert_eq!(plan.waves[0].beans[0].id, "1");
    }

    #[test]
    fn plan_dispatch_parent_id_gets_children() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let parent = crate::bean::Bean::new("1", "Parent");
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        let mut child1 = crate::bean::Bean::new("1.1", "Child one");
        child1.parent = Some("1".to_string());
        child1.verify = Some("echo ok".to_string());
        child1.produces = vec!["A".to_string()];
        child1.paths = vec!["src/a.rs".to_string()];
        child1.to_file(beans_dir.join("1.1-child-one.md")).unwrap();

        let mut child2 = crate::bean::Bean::new("1.2", "Child two");
        child2.parent = Some("1".to_string());
        child2.verify = Some("echo ok".to_string());
        child2.produces = vec!["B".to_string()];
        child2.paths = vec!["src/b.rs".to_string()];
        child2.to_file(beans_dir.join("1.2-child-two.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 2);
    }

    #[test]
    fn oversized_bean_dispatched_with_warning() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Oversized bean");
        bean.verify = Some("echo ok".to_string());
        // 4 produces exceeds MAX_PRODUCES (3) — warning but not blocked
        bean.produces = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        bean.paths = vec!["src/a.rs".to_string()];
        bean.to_file(beans_dir.join("1-oversized.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 1);
        assert!(plan.skipped.is_empty());
        assert_eq!(plan.warnings.len(), 1);
        assert_eq!(plan.warnings[0].0, "1");
    }

    #[test]
    fn unscoped_bean_dispatched_normally() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Unscoped bean");
        bean.verify = Some("echo ok".to_string());
        // No produces, no paths — dispatched normally
        bean.to_file(beans_dir.join("1-unscoped.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 1);
        assert!(plan.skipped.is_empty());
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn well_scoped_bean_dispatched() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Well scoped");
        bean.verify = Some("echo ok".to_string());
        bean.produces = vec!["Widget".to_string()];
        bean.paths = vec!["src/widget.rs".to_string()];
        bean.to_file(beans_dir.join("1-well-scoped.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 1);
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn dry_run_simulate_shows_all_waves() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        // Create a chain: 1.1 → 1.2 → 1.3 (parent=1)
        let parent = crate::bean::Bean::new("1", "Parent");
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        let mut a = crate::bean::Bean::new("1.1", "Step A");
        a.parent = Some("1".to_string());
        a.verify = Some("echo ok".to_string());
        a.produces = vec!["A".to_string()];
        a.paths = vec!["src/a.rs".to_string()];
        a.to_file(beans_dir.join("1.1-step-a.md")).unwrap();

        let mut b = crate::bean::Bean::new("1.2", "Step B");
        b.parent = Some("1".to_string());
        b.verify = Some("echo ok".to_string());
        b.dependencies = vec!["1.1".to_string()];
        b.produces = vec!["B".to_string()];
        b.paths = vec!["src/b.rs".to_string()];
        b.to_file(beans_dir.join("1.2-step-b.md")).unwrap();

        let mut c = crate::bean::Bean::new("1.3", "Step C");
        c.parent = Some("1".to_string());
        c.verify = Some("echo ok".to_string());
        c.dependencies = vec!["1.2".to_string()];
        c.produces = vec!["C".to_string()];
        c.paths = vec!["src/c.rs".to_string()];
        c.to_file(beans_dir.join("1.3-step-c.md")).unwrap();

        // Without simulate: only wave 1 (1.1) is ready
        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false, false).unwrap();
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 1);
        assert_eq!(plan.waves[0].beans[0].id, "1.1");

        // With simulate: all 3 waves shown
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false, true).unwrap();
        assert_eq!(plan.waves.len(), 3);
        assert_eq!(plan.waves[0].beans[0].id, "1.1");
        assert_eq!(plan.waves[1].beans[0].id, "1.2");
        assert_eq!(plan.waves[2].beans[0].id, "1.3");
    }

    #[test]
    fn dry_run_simulate_respects_produces_requires() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let parent = crate::bean::Bean::new("1", "Parent");
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        let mut a = crate::bean::Bean::new("1.1", "Types");
        a.parent = Some("1".to_string());
        a.verify = Some("echo ok".to_string());
        a.produces = vec!["types".to_string()];
        a.paths = vec!["src/types.rs".to_string()];
        a.to_file(beans_dir.join("1.1-types.md")).unwrap();

        let mut b = crate::bean::Bean::new("1.2", "Impl");
        b.parent = Some("1".to_string());
        b.verify = Some("echo ok".to_string());
        b.requires = vec!["types".to_string()];
        b.produces = vec!["impl".to_string()];
        b.paths = vec!["src/impl.rs".to_string()];
        b.to_file(beans_dir.join("1.2-impl.md")).unwrap();

        // Without simulate: only 1.1 is ready (1.2 blocked on requires)
        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false, false).unwrap();
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans[0].id, "1.1");

        // With simulate: both shown in correct wave order
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false, true).unwrap();
        assert_eq!(plan.waves.len(), 2);
        assert_eq!(plan.waves[0].beans[0].id, "1.1");
        assert_eq!(plan.waves[1].beans[0].id, "1.2");
    }
}
