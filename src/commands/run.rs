//! `bn run` — Dispatch ready beans to agents.
//!
//! Finds ready beans, groups them into waves by dependency order,
//! and spawns agents for each wave.
//!
//! Modes:
//! - `bn run` — one-shot: dispatch all ready beans, then exit
//! - `bn run 5.1` — dispatch a single bean (or its ready children if parent)
//! - `bn run --dry-run` — show plan without spawning
//! - `bn run --loop` — keep running until no ready beans remain
//! - `bn run --watch` — daemon: watch `.beans/` and continuously spawn agents
//! - `bn run --stop` — stop a running daemon

use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::bean::Status;
use crate::config::Config;
use crate::daemon;
use crate::index::{Index, IndexEntry};
use crate::tokens;
use crate::util::natural_cmp;

/// Arguments for cmd_run, matching the CLI definition.
pub struct RunArgs {
    pub id: Option<String>,
    pub jobs: u32,
    pub dry_run: bool,
    pub loop_mode: bool,
    pub auto_plan: bool,
    pub keep_going: bool,
    pub timeout: u32,
    pub idle_timeout: u32,
    pub watch: bool,
    pub foreground: bool,
    pub stop: bool,
}

/// What action to take for a bean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeanAction {
    Implement,
    Plan,
}

impl fmt::Display for BeanAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BeanAction::Implement => write!(f, "implement"),
            BeanAction::Plan => write!(f, "plan"),
        }
    }
}

/// A bean with sizing and dispatch action.
#[derive(Debug, Clone)]
pub struct SizedBean {
    pub id: String,
    pub title: String,
    pub tokens: u64,
    pub action: BeanAction,
    pub priority: u8,
    pub dependencies: Vec<String>,
    pub parent: Option<String>,
    pub produces: Vec<String>,
    pub requires: Vec<String>,
}

/// A wave of beans that can be dispatched in parallel.
pub struct Wave {
    pub beans: Vec<SizedBean>,
}

/// Result from planning dispatch.
pub struct DispatchPlan {
    pub waves: Vec<Wave>,
    pub skipped: Vec<SizedBean>,
}

/// Result of a completed agent.
struct AgentResult {
    id: String,
    title: String,
    action: BeanAction,
    success: bool,
    duration: Duration,
}

/// Execute the `bn run` command.
pub fn cmd_run(beans_dir: &Path, args: RunArgs) -> Result<()> {
    if args.stop {
        return daemon::stop_daemon();
    }

    if args.watch {
        if !args.foreground {
            if daemon::is_daemon_running()? {
                anyhow::bail!("Daemon is already running. Use `bn run --stop` to stop it first.");
            }
        }

        let result = daemon::start_daemon(beans_dir, args.foreground);

        if !args.foreground {
            daemon::print_started_message();
        }

        return result;
    }

    // Validate run template exists
    let config = Config::load_with_extends(beans_dir)?;
    if config.run.is_none() {
        anyhow::bail!(
            "No agent configured. Run `bn init --setup`\n\n\
             Or set it manually: bn config set run \"<command>\"\n\n\
             The command template uses {{id}} as a placeholder for the bean ID.\n\n\
             Examples:\n  \
               bn config set run \"deli spawn {{id}}\"\n  \
               bn config set run \"claude -p 'implement bean {{id}} and run bn close {{id}}'\""
        );
    }

    if args.loop_mode {
        run_loop(beans_dir, &config, &args)
    } else {
        run_once(beans_dir, &config, &args)
    }
}

/// Single dispatch pass: plan → print/execute → report.
fn run_once(beans_dir: &Path, config: &Config, args: &RunArgs) -> Result<()> {
    let plan = plan_dispatch(beans_dir, config, args.id.as_deref(), args.auto_plan)?;

    if plan.waves.is_empty() && plan.skipped.is_empty() {
        eprintln!("No ready beans. Use `bn status` to see what's going on.");
        return Ok(());
    }

    if args.dry_run {
        print_plan(&plan);
        return Ok(());
    }

    // Report skipped beans
    if !plan.skipped.is_empty() {
        eprintln!(
            "{} bean(s) need planning, run `bn plan`:",
            plan.skipped.len()
        );
        for sb in &plan.skipped {
            eprintln!(
                "  ⚠ {}  {}  ({}k tokens)",
                sb.id,
                sb.title,
                sb.tokens / 1000
            );
        }
        eprintln!();
    }

    let run_template = config.run.as_ref().unwrap();
    let plan_template = config.plan.as_deref();
    let max_jobs = args.jobs.min(config.max_concurrent) as usize;

    let mut total_done = 0u32;
    let mut total_failed = 0u32;
    let mut any_failed = false;

    for (wave_idx, wave) in plan.waves.iter().enumerate() {
        eprintln!("Wave {}: {} bean(s)", wave_idx + 1, wave.beans.len());

        let results = run_wave(
            &wave.beans,
            run_template,
            plan_template,
            max_jobs,
            args.timeout,
        )?;

        for result in &results {
            let duration = format_duration(result.duration);
            if result.success {
                eprintln!(
                    "  ✓ {}  {}  {}  {}",
                    result.id, result.title, result.action, duration
                );
                total_done += 1;
            } else {
                eprintln!(
                    "  ✗ {}  {}  {}  {} (failed)",
                    result.id, result.title, result.action, duration
                );
                total_failed += 1;
                any_failed = true;
            }
        }

        if any_failed && !args.keep_going {
            break;
        }
    }

    eprintln!();
    eprintln!(
        "Summary: {} done, {} failed, {} skipped",
        total_done,
        total_failed,
        plan.skipped.len()
    );

    if any_failed && !args.keep_going {
        anyhow::bail!("Some agents failed");
    }

    Ok(())
}

/// Loop mode: keep dispatching until no ready beans remain.
fn run_loop(beans_dir: &Path, config: &Config, args: &RunArgs) -> Result<()> {
    let max_loops = if config.max_loops == 0 {
        u32::MAX
    } else {
        config.max_loops
    };

    for iteration in 0..max_loops {
        if iteration > 0 {
            eprintln!("\n--- Loop iteration {} ---\n", iteration + 1);
        }

        let plan = plan_dispatch(beans_dir, config, args.id.as_deref(), args.auto_plan)?;

        if plan.waves.is_empty() {
            if iteration == 0 {
                eprintln!("No ready beans. Use `bn status` to see what's going on.");
            } else {
                eprintln!("No more ready beans. Stopping.");
            }
            return Ok(());
        }

        // Run one pass (non-loop, non-dry-run)
        let inner_args = RunArgs {
            id: args.id.clone(),
            jobs: args.jobs,
            dry_run: false,
            loop_mode: false,
            auto_plan: args.auto_plan,
            keep_going: args.keep_going,
            timeout: args.timeout,
            idle_timeout: args.idle_timeout,
            watch: false,
            foreground: false,
            stop: false,
        };

        // Reload config each iteration (agents may have changed beans)
        let config = Config::load_with_extends(beans_dir)?;
        match run_once(beans_dir, &config, &inner_args) {
            Ok(()) => {}
            Err(e) => {
                if args.keep_going {
                    eprintln!("Warning: {}", e);
                } else {
                    return Err(e);
                }
            }
        }
    }

    eprintln!("Reached max_loops ({}). Stopping.", max_loops);
    Ok(())
}

/// Plan dispatch: get ready beans, size them, compute waves.
fn plan_dispatch(
    beans_dir: &Path,
    config: &Config,
    filter_id: Option<&str>,
    auto_plan: bool,
) -> Result<DispatchPlan> {
    let index = Index::load_or_rebuild(beans_dir)?;
    let workspace = beans_dir.parent().unwrap_or(Path::new("."));

    // Get ready beans (open, has verify, all deps met)
    let mut ready_entries: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|e| e.has_verify && e.status == Status::Open && all_deps_closed(e, &index))
        .collect();

    // Filter by ID if provided
    if let Some(filter_id) = filter_id {
        // Check if it's a parent — if so, get its ready children
        let is_parent = index
            .beans
            .iter()
            .any(|e| e.parent.as_deref() == Some(filter_id));
        if is_parent {
            ready_entries.retain(|e| e.parent.as_deref() == Some(filter_id));
        } else {
            ready_entries.retain(|e| e.id == filter_id);
        }
    }

    // Size each bean
    let mut sized: Vec<SizedBean> = Vec::new();
    for entry in &ready_entries {
        let bean_path = crate::discovery::find_bean_file(beans_dir, &entry.id)?;
        let bean = crate::bean::Bean::from_file(&bean_path)?;
        let token_count = tokens::calculate_tokens(&bean, workspace);
        let action = if token_count > config.max_tokens as u64 {
            BeanAction::Plan
        } else {
            BeanAction::Implement
        };

        sized.push(SizedBean {
            id: entry.id.clone(),
            title: entry.title.clone(),
            tokens: token_count,
            action,
            priority: entry.priority,
            dependencies: entry.dependencies.clone(),
            parent: entry.parent.clone(),
            produces: entry.produces.clone(),
            requires: entry.requires.clone(),
        });
    }

    // Separate: implement beans go into waves; plan beans go to skipped (unless auto_plan)
    let (implement_beans, plan_beans): (Vec<SizedBean>, Vec<SizedBean>) = sized
        .into_iter()
        .partition(|sb| sb.action == BeanAction::Implement);

    let skipped = if auto_plan {
        // Include plan beans in waves too (they use the plan template)
        Vec::new()
    } else {
        plan_beans.clone()
    };

    let dispatch_beans = if auto_plan {
        let mut all = implement_beans;
        all.extend(plan_beans);
        all
    } else {
        implement_beans
    };

    let waves = compute_waves(&dispatch_beans, &index);

    Ok(DispatchPlan { waves, skipped })
}

/// Compute waves of beans grouped by dependency order.
/// Wave 0: no deps. Wave 1: deps all in wave 0. Etc.
fn compute_waves(beans: &[SizedBean], index: &Index) -> Vec<Wave> {
    let mut waves = Vec::new();
    let bean_ids: HashSet<String> = beans.iter().map(|b| b.id.clone()).collect();

    // Already-closed beans count as completed
    let mut completed: HashSet<String> = index
        .beans
        .iter()
        .filter(|e| e.status == Status::Closed)
        .map(|e| e.id.clone())
        .collect();

    let mut remaining: Vec<SizedBean> = beans.to_vec();

    while !remaining.is_empty() {
        let (ready, blocked): (Vec<SizedBean>, Vec<SizedBean>) =
            remaining.into_iter().partition(|b| {
                // All explicit deps must be completed or not in our dispatch set
                let explicit_ok = b
                    .dependencies
                    .iter()
                    .all(|d| completed.contains(d) || !bean_ids.contains(d));

                // All requires must be satisfied (producer completed or not in set)
                let requires_ok = b.requires.iter().all(|req| {
                    // Find the sibling producer for this artifact
                    if let Some(producer) = beans.iter().find(|other| {
                        other.id != b.id && other.parent == b.parent && other.produces.contains(req)
                    }) {
                        completed.contains(&producer.id)
                    } else {
                        true // No producer in set, assume satisfied
                    }
                });

                explicit_ok && requires_ok
            });

        if ready.is_empty() {
            // Remaining beans have unresolvable deps (cycle or missing)
            // Add them all as a final wave to avoid losing them
            eprintln!(
                "Warning: {} bean(s) have unresolvable dependencies, adding to final wave",
                blocked.len()
            );
            waves.push(Wave { beans: blocked });
            break;
        }

        for b in &ready {
            completed.insert(b.id.clone());
        }

        waves.push(Wave { beans: ready });
        remaining = blocked;
    }

    // Sort beans within each wave by priority then ID
    for wave in &mut waves {
        wave.beans.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| natural_cmp(&a.id, &b.id))
        });
    }

    waves
}

/// Spawn agents for a wave of beans, respecting max parallelism.
fn run_wave(
    beans: &[SizedBean],
    run_template: &str,
    plan_template: Option<&str>,
    max_jobs: usize,
    _timeout_minutes: u32,
) -> Result<Vec<AgentResult>> {
    let mut results = Vec::new();
    let mut children: Vec<(SizedBean, std::process::Child, Instant)> = Vec::new();

    let mut pending: Vec<&SizedBean> = beans.iter().collect();

    while !pending.is_empty() || !children.is_empty() {
        // Spawn up to max_jobs
        while children.len() < max_jobs && !pending.is_empty() {
            let sb = pending.remove(0);
            let template = match sb.action {
                BeanAction::Implement => run_template,
                BeanAction::Plan => {
                    if let Some(pt) = plan_template {
                        pt
                    } else {
                        // No plan template — skip with error
                        results.push(AgentResult {
                            id: sb.id.clone(),
                            title: sb.title.clone(),
                            action: sb.action,
                            success: false,
                            duration: Duration::ZERO,
                        });
                        continue;
                    }
                }
            };

            let cmd = template.replace("{id}", &sb.id);
            match std::process::Command::new("sh").args(["-c", &cmd]).spawn() {
                Ok(child) => {
                    children.push((sb.clone(), child, Instant::now()));
                }
                Err(e) => {
                    eprintln!("  Failed to spawn agent for {}: {}", sb.id, e);
                    results.push(AgentResult {
                        id: sb.id.clone(),
                        title: sb.title.clone(),
                        action: sb.action,
                        success: false,
                        duration: Duration::ZERO,
                    });
                }
            }
        }

        if children.is_empty() {
            break;
        }

        // Poll for completions
        let mut still_running = Vec::new();
        for (sb, mut child, started) in children {
            match child.try_wait() {
                Ok(Some(status)) => {
                    results.push(AgentResult {
                        id: sb.id.clone(),
                        title: sb.title.clone(),
                        action: sb.action,
                        success: status.success(),
                        duration: started.elapsed(),
                    });
                }
                Ok(None) => {
                    still_running.push((sb, child, started));
                }
                Err(e) => {
                    eprintln!("  Error checking agent for {}: {}", sb.id, e);
                    results.push(AgentResult {
                        id: sb.id.clone(),
                        title: sb.title.clone(),
                        action: sb.action,
                        success: false,
                        duration: started.elapsed(),
                    });
                }
            }
        }
        children = still_running;

        if !children.is_empty() {
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    Ok(results)
}

/// Print the dispatch plan without executing.
fn print_plan(plan: &DispatchPlan) {
    for (wave_idx, wave) in plan.waves.iter().enumerate() {
        println!("Wave {}: {} bean(s)", wave_idx + 1, wave.beans.len());
        for sb in &wave.beans {
            println!(
                "  {}  {}  {}  ({}k tokens)",
                sb.id,
                sb.title,
                sb.action,
                sb.tokens / 1000
            );
        }
    }

    if !plan.skipped.is_empty() {
        println!();
        println!("Skipped ({} — need planning):", plan.skipped.len());
        for sb in &plan.skipped {
            println!(
                "  ⚠ {}  {}  ({}k tokens)",
                sb.id,
                sb.title,
                sb.tokens / 1000
            );
        }
    }
}

/// Format a duration as M:SS.
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Check if all dependencies of an index entry are closed.
fn all_deps_closed(entry: &IndexEntry, index: &Index) -> bool {
    for dep_id in &entry.dependencies {
        match index.beans.iter().find(|e| e.id == *dep_id) {
            Some(dep) if dep.status == Status::Closed => {}
            _ => return false,
        }
    }

    for required in &entry.requires {
        if let Some(producer) = index
            .beans
            .iter()
            .find(|e| e.id != entry.id && e.parent == entry.parent && e.produces.contains(required))
        {
            if producer.status != Status::Closed {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn cmd_run_errors_when_no_run_template() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, None);

        let args = RunArgs {
            id: None,
            jobs: 4,
            dry_run: false,
            loop_mode: false,
            auto_plan: false,
            keep_going: false,
            timeout: 30,
            idle_timeout: 5,
            watch: false,
            foreground: false,
            stop: false,
        };

        let result = cmd_run(&beans_dir, args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No agent configured"),
            "Error should mention no agent: {}",
            err
        );
    }

    #[test]
    fn dry_run_does_not_spawn() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        // Create a ready bean
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.verify = Some("echo ok".to_string());
        bean.to_file(beans_dir.join("1-test.md")).unwrap();

        let args = RunArgs {
            id: None,
            jobs: 4,
            dry_run: true,
            loop_mode: false,
            auto_plan: false,
            keep_going: false,
            timeout: 30,
            idle_timeout: 5,
            watch: false,
            foreground: false,
            stop: false,
        };

        // dry_run should succeed without spawning any processes
        let result = cmd_run(&beans_dir, args);
        assert!(result.is_ok());
    }

    #[test]
    fn plan_dispatch_no_ready_beans() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false).unwrap();

        assert!(plan.waves.is_empty());
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn plan_dispatch_returns_ready_beans() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Task one");
        bean.verify = Some("echo ok".to_string());
        bean.to_file(beans_dir.join("1-task-one.md")).unwrap();

        let mut bean2 = crate::bean::Bean::new("2", "Task two");
        bean2.verify = Some("echo ok".to_string());
        bean2.to_file(beans_dir.join("2-task-two.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 2);
    }

    #[test]
    fn plan_dispatch_filters_by_id() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Task one");
        bean.verify = Some("echo ok".to_string());
        bean.to_file(beans_dir.join("1-task-one.md")).unwrap();

        let mut bean2 = crate::bean::Bean::new("2", "Task two");
        bean2.verify = Some("echo ok".to_string());
        bean2.to_file(beans_dir.join("2-task-two.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false).unwrap();

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
        child1.to_file(beans_dir.join("1.1-child-one.md")).unwrap();

        let mut child2 = crate::bean::Bean::new("1.2", "Child two");
        child2.parent = Some("1".to_string());
        child2.verify = Some("echo ok".to_string());
        child2.to_file(beans_dir.join("1.2-child-two.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, Some("1"), false).unwrap();

        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans.len(), 2);
    }

    #[test]
    fn compute_waves_no_deps() {
        let index = Index { beans: vec![] };
        let beans = vec![
            SizedBean {
                id: "1".to_string(),
                title: "A".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
            SizedBean {
                id: "2".to_string(),
                title: "B".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
        ];
        let waves = compute_waves(&beans, &index);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].beans.len(), 2);
    }

    #[test]
    fn compute_waves_linear_chain() {
        let index = Index { beans: vec![] };
        let beans = vec![
            SizedBean {
                id: "1".to_string(),
                title: "A".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
            SizedBean {
                id: "2".to_string(),
                title: "B".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["1".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
            SizedBean {
                id: "3".to_string(),
                title: "C".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["2".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
        ];
        let waves = compute_waves(&beans, &index);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].beans[0].id, "1");
        assert_eq!(waves[1].beans[0].id, "2");
        assert_eq!(waves[2].beans[0].id, "3");
    }

    #[test]
    fn compute_waves_diamond() {
        let index = Index { beans: vec![] };
        // 1 → (2, 3) → 4
        let beans = vec![
            SizedBean {
                id: "1".to_string(),
                title: "Root".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
            SizedBean {
                id: "2".to_string(),
                title: "Left".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["1".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
            SizedBean {
                id: "3".to_string(),
                title: "Right".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["1".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
            SizedBean {
                id: "4".to_string(),
                title: "Join".to_string(),
                tokens: 100,
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["2".to_string(), "3".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
            },
        ];
        let waves = compute_waves(&beans, &index);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].beans.len(), 1); // 1
        assert_eq!(waves[1].beans.len(), 2); // 2, 3
        assert_eq!(waves[2].beans.len(), 1); // 4
    }

    #[test]
    fn format_duration_formats_correctly() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_secs(32)), "0:32");
        assert_eq!(format_duration(Duration::from_secs(62)), "1:02");
        assert_eq!(format_duration(Duration::from_secs(600)), "10:00");
    }

    #[test]
    fn large_bean_classified_as_plan() {
        let (_dir, beans_dir) = make_beans_dir();
        // Use a very low max_tokens so our bean is "large"
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 1\nrun: \"echo {id}\"\nmax_tokens: 1\n",
        )
        .unwrap();

        let mut bean = crate::bean::Bean::new(
            "1",
            "Large bean with lots of description text that should exceed the token limit",
        );
        bean.verify = Some("echo ok".to_string());
        bean.description = Some("x".repeat(1000));
        bean.to_file(beans_dir.join("1-large.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, false).unwrap();

        // Should be skipped (needs planning)
        assert_eq!(plan.skipped.len(), 1);
        assert_eq!(plan.skipped[0].action, BeanAction::Plan);
    }

    #[test]
    fn auto_plan_includes_large_beans_in_waves() {
        let (_dir, beans_dir) = make_beans_dir();
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 1\nrun: \"echo {id}\"\nmax_tokens: 1\n",
        )
        .unwrap();

        let mut bean = crate::bean::Bean::new("1", "Large bean");
        bean.verify = Some("echo ok".to_string());
        bean.description = Some("x".repeat(1000));
        bean.to_file(beans_dir.join("1-large.md")).unwrap();

        let config = Config::load_with_extends(&beans_dir).unwrap();
        let plan = plan_dispatch(&beans_dir, &config, None, true).unwrap();

        // With auto_plan, large beans go into waves, not skipped
        assert!(plan.skipped.is_empty());
        assert_eq!(plan.waves.len(), 1);
        assert_eq!(plan.waves[0].beans[0].action, BeanAction::Plan);
    }
}
