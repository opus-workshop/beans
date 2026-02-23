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
//! - `bn run --json-stream` — emit JSON stream events to stdout
//!
//! Spawning modes:
//! - **Template mode** (backward compat): If `config.run` is set, spawn via `sh -c <template>`.
//! - **Direct mode**: If no template is configured but `pi` is on PATH, spawn pi directly
//!   with `--mode json --print --no-session`, monitoring with timeouts and parsing events.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::bean::Status;
use crate::config::Config;
use crate::index::{Index, IndexEntry};
use crate::pi_output::{self, AgentEvent};
use crate::stream::{self, StreamEvent};
use crate::timeout::{self, MonitorResult, TimeoutConfig};
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
    pub json_stream: bool,
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
    /// Flat list of all beans to dispatch (for ready-queue mode).
    pub all_beans: Vec<SizedBean>,
    /// The index snapshot used for planning.
    pub index: Index,
}

/// Result of a completed agent.
#[derive(Debug)]
struct AgentResult {
    id: String,
    title: String,
    action: BeanAction,
    success: bool,
    duration: Duration,
    total_tokens: Option<u64>,
    total_cost: Option<f64>,
    error: Option<String>,
}

/// Which spawning mode to use.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnMode {
    /// Use shell template from config (backward compat).
    Template {
        run_template: String,
        plan_template: Option<String>,
    },
    /// Spawn pi directly with JSON output and monitoring.
    Direct,
}

/// Execute the `bn run` command.
pub fn cmd_run(beans_dir: &Path, args: RunArgs) -> Result<()> {
    // Determine spawn mode
    let config = Config::load_with_extends(beans_dir)?;
    let spawn_mode = determine_spawn_mode(&config);

    if spawn_mode == SpawnMode::Direct && !pi_available() {
        anyhow::bail!(
            "No agent configured and `pi` not found on PATH.\n\n\
             Either:\n  \
               1. Install pi: npm i -g @anthropic/pi\n  \
               2. Set a run template: bn config set run \"<command>\"\n\n\
             The command template uses {{id}} as a placeholder for the bean ID.\n\n\
             Examples:\n  \
               bn config set run \"pi @.beans/{{id}}-*.md 'implement and bn close {{id}}'\"\n  \
               bn config set run \"claude -p 'implement bean {{id}} and run bn close {{id}}'\""
        );
    }

    if let SpawnMode::Template { ref run_template, .. } = spawn_mode {
        // Validate template exists (kept for backward compat error message)
        let _ = run_template;
    }

    if args.loop_mode {
        run_loop(beans_dir, &config, &args, &spawn_mode)
    } else {
        run_once(beans_dir, &config, &args, &spawn_mode)
    }
}

/// Determine the spawn mode based on config.
fn determine_spawn_mode(config: &Config) -> SpawnMode {
    if let Some(ref run) = config.run {
        SpawnMode::Template {
            run_template: run.clone(),
            plan_template: config.plan.clone(),
        }
    } else {
        SpawnMode::Direct
    }
}

/// Check if `pi` is available on PATH.
fn pi_available() -> bool {
    Command::new("pi")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Single dispatch pass: plan → print/execute → report.
fn run_once(
    beans_dir: &Path,
    config: &Config,
    args: &RunArgs,
    spawn_mode: &SpawnMode,
) -> Result<()> {
    let plan = plan_dispatch(beans_dir, config, args.id.as_deref(), args.auto_plan)?;

    if plan.waves.is_empty() && plan.skipped.is_empty() {
        if args.json_stream {
            stream::emit_error("No ready beans");
        } else {
            eprintln!("No ready beans. Use `bn status` to see what's going on.");
        }
        return Ok(());
    }

    if args.dry_run {
        if args.json_stream {
            print_plan_json(&plan, args.id.as_deref());
        } else {
            print_plan(&plan);
        }
        return Ok(());
    }

    // Report skipped beans
    if !plan.skipped.is_empty() && !args.json_stream {
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

    let total_beans: usize = plan.waves.iter().map(|w| w.beans.len()).sum();
    let total_waves = plan.waves.len();
    let parent_id = args.id.as_deref().unwrap_or("all");

    if args.json_stream {
        let beans_info: Vec<stream::BeanInfo> = plan
            .waves
            .iter()
            .enumerate()
            .flat_map(|(wave_idx, wave)| {
                wave.beans.iter().map(move |b| stream::BeanInfo {
                    id: b.id.clone(),
                    title: b.title.clone(),
                    round: wave_idx + 1,
                })
            })
            .collect();
        stream::emit(&StreamEvent::RunStart {
            parent_id: parent_id.to_string(),
            total_beans,
            total_rounds: total_waves,
            beans: beans_info,
        });
    }

    let max_jobs = args.jobs.min(config.max_concurrent) as usize;
    let run_start = Instant::now();
    let total_done;
    let total_failed;
    let any_failed;

    match spawn_mode {
        SpawnMode::Direct => {
            // Ready-queue: start each bean as soon as its specific deps finish
            let (results, had_failure) = run_ready_queue_direct(
                beans_dir,
                &plan.all_beans,
                &plan.index,
                max_jobs,
                args.timeout,
                args.idle_timeout,
                args.json_stream,
                args.keep_going,
            )?;

            let mut done = 0u32;
            let mut failed = 0u32;
            for result in &results {
                let duration = format_duration(result.duration);
                if result.success {
                    if args.json_stream {
                        stream::emit(&StreamEvent::BeanDone {
                            id: result.id.clone(),
                            success: true,
                            duration_secs: result.duration.as_secs(),
                            error: None,
                            total_tokens: result.total_tokens,
                            total_cost: result.total_cost,
                        });
                    } else {
                        eprintln!(
                            "  ✓ {}  {}  {}  {}",
                            result.id, result.title, result.action, duration
                        );
                    }
                    done += 1;
                } else {
                    if args.json_stream {
                        stream::emit(&StreamEvent::BeanDone {
                            id: result.id.clone(),
                            success: false,
                            duration_secs: result.duration.as_secs(),
                            error: result.error.clone(),
                            total_tokens: result.total_tokens,
                            total_cost: result.total_cost,
                        });
                    } else {
                        eprintln!(
                            "  ✗ {}  {}  {}  {} (failed)",
                            result.id, result.title, result.action, duration
                        );
                    }
                    failed += 1;
                }
            }
            total_done = done;
            total_failed = failed;
            any_failed = had_failure;
        }

        SpawnMode::Template { .. } => {
            // Template mode: wave-based execution (legacy)
            let mut done = 0u32;
            let mut failed = 0u32;
            let mut had_failure = false;

            for (wave_idx, wave) in plan.waves.iter().enumerate() {
                if args.json_stream {
                    stream::emit(&StreamEvent::RoundStart {
                        round: wave_idx + 1,
                        total_rounds: total_waves,
                        bean_count: wave.beans.len(),
                    });
                } else {
                    eprintln!("Wave {}: {} bean(s)", wave_idx + 1, wave.beans.len());
                }

                let results = run_wave(
                    beans_dir,
                    &wave.beans,
                    spawn_mode,
                    max_jobs,
                    args.timeout,
                    args.idle_timeout,
                    args.json_stream,
                    wave_idx + 1,
                )?;

                let mut wave_success = 0usize;
                let mut wave_failed = 0usize;

                for result in &results {
                    let duration = format_duration(result.duration);
                    if result.success {
                        if args.json_stream {
                            stream::emit(&StreamEvent::BeanDone {
                                id: result.id.clone(),
                                success: true,
                                duration_secs: result.duration.as_secs(),
                                error: None,
                                total_tokens: result.total_tokens,
                                total_cost: result.total_cost,
                            });
                        } else {
                            eprintln!(
                                "  ✓ {}  {}  {}  {}",
                                result.id, result.title, result.action, duration
                            );
                        }
                        done += 1;
                        wave_success += 1;
                    } else {
                        if args.json_stream {
                            stream::emit(&StreamEvent::BeanDone {
                                id: result.id.clone(),
                                success: false,
                                duration_secs: result.duration.as_secs(),
                                error: result.error.clone(),
                                total_tokens: result.total_tokens,
                                total_cost: result.total_cost,
                            });
                        } else {
                            eprintln!(
                                "  ✗ {}  {}  {}  {} (failed)",
                                result.id, result.title, result.action, duration
                            );
                        }
                        failed += 1;
                        wave_failed += 1;
                        had_failure = true;
                    }
                }

                if args.json_stream {
                    stream::emit(&StreamEvent::RoundEnd {
                        round: wave_idx + 1,
                        success_count: wave_success,
                        failed_count: wave_failed,
                    });
                }

                if had_failure && !args.keep_going {
                    break;
                }
            }

            total_done = done;
            total_failed = failed;
            any_failed = had_failure;
        }
    }

    if args.json_stream {
        stream::emit(&StreamEvent::RunEnd {
            total_success: total_done as usize,
            total_failed: total_failed as usize,
            duration_secs: run_start.elapsed().as_secs(),
        });
    } else {
        eprintln!();
        eprintln!(
            "Summary: {} done, {} failed, {} skipped",
            total_done,
            total_failed,
            plan.skipped.len()
        );
    }

    if any_failed && !args.keep_going {
        anyhow::bail!("Some agents failed");
    }

    Ok(())
}

/// Loop mode: keep dispatching until no ready beans remain.
fn run_loop(
    beans_dir: &Path,
    config: &Config,
    args: &RunArgs,
    _spawn_mode: &SpawnMode,
) -> Result<()> {
    let max_loops = if config.max_loops == 0 {
        u32::MAX
    } else {
        config.max_loops
    };

    for iteration in 0..max_loops {
        if iteration > 0 {
            if !args.json_stream {
                eprintln!("\n--- Loop iteration {} ---\n", iteration + 1);
            }
        }

        let plan = plan_dispatch(beans_dir, config, args.id.as_deref(), args.auto_plan)?;

        if plan.waves.is_empty() {
            if !args.json_stream {
                if iteration == 0 {
                    eprintln!("No ready beans. Use `bn status` to see what's going on.");
                } else {
                    eprintln!("No more ready beans. Stopping.");
                }
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
            json_stream: args.json_stream,
        };

        // Reload config each iteration (agents may have changed beans)
        let config = Config::load_with_extends(beans_dir)?;
        let spawn_mode = determine_spawn_mode(&config);
        match run_once(beans_dir, &config, &inner_args, &spawn_mode) {
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

    Ok(DispatchPlan {
        waves,
        skipped,
        all_beans: dispatch_beans,
        index,
    })
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

// ---------------------------------------------------------------------------
// Wave execution
// ---------------------------------------------------------------------------

/// Spawn agents for a wave of beans, respecting max parallelism.
fn run_wave(
    beans_dir: &Path,
    beans: &[SizedBean],
    spawn_mode: &SpawnMode,
    max_jobs: usize,
    timeout_minutes: u32,
    idle_timeout_minutes: u32,
    json_stream: bool,
    wave_number: usize,
) -> Result<Vec<AgentResult>> {
    match spawn_mode {
        SpawnMode::Template {
            run_template,
            plan_template,
        } => run_wave_template(
            beans,
            run_template,
            plan_template.as_deref(),
            max_jobs,
            timeout_minutes,
        ),
        SpawnMode::Direct => run_wave_direct(
            beans_dir,
            beans,
            max_jobs,
            timeout_minutes,
            idle_timeout_minutes,
            json_stream,
            wave_number,
        ),
    }
}

/// Template mode: spawn agents via `sh -c <template>` (backward compat).
fn run_wave_template(
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
                            total_tokens: None,
                            total_cost: None,
                            error: Some("No plan template configured".to_string()),
                        });
                        continue;
                    }
                }
            };

            let cmd = template.replace("{id}", &sb.id);
            match Command::new("sh").args(["-c", &cmd]).spawn() {
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
                        total_tokens: None,
                        total_cost: None,
                        error: Some(format!("Failed to spawn: {}", e)),
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
                        total_tokens: None,
                        total_cost: None,
                        error: if status.success() {
                            None
                        } else {
                            Some(format!("Exit code {}", status.code().unwrap_or(-1)))
                        },
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
                        total_tokens: None,
                        total_cost: None,
                        error: Some(format!("Error checking process: {}", e)),
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

/// Direct mode: spawn pi directly with JSON output and monitoring.
fn run_wave_direct(
    beans_dir: &Path,
    beans: &[SizedBean],
    max_jobs: usize,
    timeout_minutes: u32,
    idle_timeout_minutes: u32,
    json_stream: bool,
    wave_number: usize,
) -> Result<Vec<AgentResult>> {
    let results = Arc::new(Mutex::new(Vec::new()));
    let mut pending: Vec<SizedBean> = beans.to_vec();
    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();

    while !pending.is_empty() || !handles.is_empty() {
        // Spawn up to max_jobs threads
        while handles.len() < max_jobs && !pending.is_empty() {
            let sb = pending.remove(0);
            let beans_dir = beans_dir.to_path_buf();
            let results = Arc::clone(&results);
            let timeout_min = timeout_minutes;
            let idle_min = idle_timeout_minutes;

            if json_stream {
                stream::emit(&StreamEvent::BeanStart {
                    id: sb.id.clone(),
                    title: sb.title.clone(),
                    round: wave_number,
                });
            }

            let handle = std::thread::spawn(move || {
                let result =
                    run_single_direct(&beans_dir, &sb, timeout_min, idle_min, json_stream);
                results.lock().unwrap().push(result);
            });
            handles.push(handle);
        }

        // Wait for at least one thread to finish
        let prev_count = handles.len();
        let mut still_running = Vec::new();
        for handle in handles.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                still_running.push(handle);
            }
        }

        // If nothing finished, wait briefly before polling again
        if still_running.len() == prev_count && !still_running.is_empty() {
            std::thread::sleep(Duration::from_millis(200));
        }

        handles = still_running;
    }

    // Wait for any remaining threads
    for handle in handles {
        let _ = handle.join();
    }

    Ok(Arc::try_unwrap(results).unwrap().into_inner().unwrap())
}

/// Check if a bean's dependencies are all satisfied.
fn is_bean_ready(
    bean: &SizedBean,
    completed: &HashSet<String>,
    all_bean_ids: &HashSet<String>,
    all_beans: &[SizedBean],
) -> bool {
    // All explicit deps must be completed or not in our dispatch set
    let explicit_ok = bean
        .dependencies
        .iter()
        .all(|d| completed.contains(d) || !all_bean_ids.contains(d));

    // All requires must be satisfied (producer completed or not in set)
    let requires_ok = bean.requires.iter().all(|req| {
        if let Some(producer) = all_beans.iter().find(|other| {
            other.id != bean.id && other.parent == bean.parent && other.produces.contains(req)
        }) {
            completed.contains(&producer.id)
        } else {
            true // No producer in set, assume satisfied
        }
    });

    explicit_ok && requires_ok
}

/// Run beans using a ready-queue: start each bean as soon as its specific deps
/// complete, rather than waiting for an entire wave to finish.
fn run_ready_queue_direct(
    beans_dir: &Path,
    all_beans: &[SizedBean],
    index: &Index,
    max_jobs: usize,
    timeout_minutes: u32,
    idle_timeout_minutes: u32,
    json_stream: bool,
    keep_going: bool,
) -> Result<(Vec<AgentResult>, bool)> {
    let all_bean_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();

    // Already-closed beans count as completed (same logic as compute_waves)
    let mut completed: HashSet<String> = index
        .beans
        .iter()
        .filter(|e| e.status == Status::Closed)
        .map(|e| e.id.clone())
        .collect();

    let mut remaining: HashMap<String, SizedBean> = all_beans
        .iter()
        .map(|b| (b.id.clone(), b.clone()))
        .collect();

    let mut results: Vec<AgentResult> = Vec::new();
    let mut running_count: usize = 0;
    let mut any_failed = false;

    // Channel for completed agents to report back
    let (tx, rx) = mpsc::channel::<AgentResult>();

    // Assign a "round" number for display: use compute_waves to figure out
    // which wave each bean would be in (for json_stream events)
    let wave_map: HashMap<String, usize> = {
        let waves = compute_waves(all_beans, index);
        let mut m = HashMap::new();
        for (i, wave) in waves.iter().enumerate() {
            for b in &wave.beans {
                m.insert(b.id.clone(), i + 1);
            }
        }
        m
    };

    loop {
        // Find beans that are ready and we have capacity for
        let mut newly_started = 0;
        let ready_ids: Vec<String> = remaining
            .values()
            .filter(|b| is_bean_ready(b, &completed, &all_bean_ids, all_beans))
            .map(|b| b.id.clone())
            .collect();

        // Sort ready beans by priority then ID (stable ordering)
        let mut ready_beans: Vec<SizedBean> = ready_ids
            .iter()
            .filter_map(|id| remaining.get(id).cloned())
            .collect();
        ready_beans.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| natural_cmp(&a.id, &b.id))
        });

        for sb in ready_beans {
            if running_count >= max_jobs {
                break;
            }

            remaining.remove(&sb.id);
            running_count += 1;
            let round = wave_map.get(&sb.id).copied().unwrap_or(1);

            if json_stream {
                stream::emit(&StreamEvent::BeanStart {
                    id: sb.id.clone(),
                    title: sb.title.clone(),
                    round,
                });
            }

            let beans_dir = beans_dir.to_path_buf();
            let tx = tx.clone();
            let timeout_min = timeout_minutes;
            let idle_min = idle_timeout_minutes;

            std::thread::spawn(move || {
                let result = run_single_direct(&beans_dir, &sb, timeout_min, idle_min, json_stream);
                let _ = tx.send(result);
            });
            newly_started += 1;
        }

        // If nothing is running and nothing can start, we're done (or stuck)
        if running_count == 0 && newly_started == 0 {
            if !remaining.is_empty() {
                // Remaining beans have unresolvable deps
                if json_stream {
                    stream::emit_error(&format!(
                        "{} bean(s) have unresolvable dependencies",
                        remaining.len()
                    ));
                } else {
                    eprintln!(
                        "Warning: {} bean(s) have unresolvable dependencies:",
                        remaining.len()
                    );
                    for b in remaining.values() {
                        eprintln!("  {} {}", b.id, b.title);
                    }
                }
            }
            break;
        }

        // If nothing is running (but we just started some), loop to check for
        // more readiness after spawning
        if running_count > 0 {
            // Wait for any one agent to complete
            let result = rx.recv().expect("channel closed unexpectedly");
            running_count -= 1;

            let success = result.success;
            let bean_id = result.id.clone();

            if success {
                completed.insert(bean_id.clone());
            } else {
                any_failed = true;
                // If not keep_going, drain remaining and stop spawning
                if !keep_going {
                    results.push(result);
                    // Wait for currently running agents to finish
                    while running_count > 0 {
                        if let Ok(r) = rx.recv() {
                            running_count -= 1;
                            results.push(r);
                        }
                    }
                    return Ok((results, true));
                }
            }

            results.push(result);
        }
    }

    // Drain any remaining results (shouldn't happen, but safety)
    drop(tx);
    while let Ok(result) = rx.try_recv() {
        results.push(result);
    }

    Ok((results, any_failed))
}

/// Run a single bean by spawning pi directly.
fn run_single_direct(
    beans_dir: &Path,
    sb: &SizedBean,
    timeout_minutes: u32,
    idle_timeout_minutes: u32,
    json_stream: bool,
) -> AgentResult {
    let started = Instant::now();

    // Find the bean file
    let bean_file = match crate::discovery::find_bean_file(beans_dir, &sb.id) {
        Ok(p) => p,
        Err(e) => {
            return AgentResult {
                id: sb.id.clone(),
                title: sb.title.clone(),
                action: sb.action,
                success: false,
                duration: started.elapsed(),
                total_tokens: None,
                total_cost: None,
                error: Some(format!("Cannot find bean file: {}", e)),
            };
        }
    };

    // Assemble context from the bean's description
    let context = assemble_bean_context(beans_dir, &sb.id);

    // Build pi command
    let mut cmd = Command::new("pi");
    cmd.args(["--mode", "json", "--print", "--no-session"]);

    if !context.is_empty() {
        cmd.args(["--append-system-prompt", &context]);
    }

    cmd.arg(format!("@{}", bean_file.display()));
    cmd.arg(format!("Implement this bean and run `bn close {}`", sb.id));
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Spawn the process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return AgentResult {
                id: sb.id.clone(),
                title: sb.title.clone(),
                action: sb.action,
                success: false,
                duration: started.elapsed(),
                total_tokens: None,
                total_cost: None,
                error: Some(format!("Failed to spawn pi: {}", e)),
            };
        }
    };

    // Take stdout for monitoring
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            return AgentResult {
                id: sb.id.clone(),
                title: sb.title.clone(),
                action: sb.action,
                success: false,
                duration: started.elapsed(),
                total_tokens: None,
                total_cost: None,
                error: Some("Failed to capture stdout".to_string()),
            };
        }
    };

    // Set up timeout config
    let timeout_config = TimeoutConfig {
        total_timeout: Duration::from_secs(timeout_minutes as u64 * 60),
        idle_timeout: Duration::from_secs(idle_timeout_minutes as u64 * 60),
    };

    // Track cumulative tokens/cost
    let mut cumulative_tokens: u64 = 0;
    let mut cumulative_cost: f64 = 0.0;
    let mut tool_count: usize = 0;
    let bean_id = sb.id.clone();

    // Monitor the process, parsing JSON events
    let monitor_result = timeout::monitor_process(&mut child, stdout, &timeout_config, |line| {
        // Try to parse each line as a JSON event from pi
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(event) = pi_output::parse_agent_event(&raw) {
                match event {
                    AgentEvent::Thinking { ref text } => {
                        if json_stream {
                            stream::emit(&StreamEvent::BeanThinking {
                                id: bean_id.clone(),
                                text: text.clone(),
                            });
                        }
                    }
                    AgentEvent::ToolStart { ref name, .. } => {
                        tool_count += 1;
                        if json_stream {
                            stream::emit(&StreamEvent::BeanTool {
                                id: bean_id.clone(),
                                tool_name: name.clone(),
                                tool_count,
                                file_path: None,
                            });
                        }
                    }
                    AgentEvent::ToolEnd {
                        ref name,
                        ref arguments,
                    } => {
                        if json_stream {
                            let file_path =
                                pi_output::extract_file_path(name, arguments);
                            stream::emit(&StreamEvent::BeanTool {
                                id: bean_id.clone(),
                                tool_name: name.clone(),
                                tool_count,
                                file_path,
                            });
                        }
                    }
                    AgentEvent::TokenUpdate {
                        input_tokens,
                        output_tokens,
                        cache_read,
                        cache_write,
                        cost,
                    } => {
                        cumulative_tokens += input_tokens + output_tokens;
                        cumulative_cost += cost;
                        if json_stream {
                            stream::emit(&StreamEvent::BeanTokens {
                                id: bean_id.clone(),
                                input_tokens,
                                output_tokens,
                                cache_read,
                                cache_write,
                                cost,
                            });
                        }
                    }
                    AgentEvent::Finished { total_tokens, cost } => {
                        cumulative_tokens = total_tokens;
                        cumulative_cost = cost;
                    }
                    _ => {}
                }
            }
        }
    });

    let duration = started.elapsed();

    // Determine success
    let (success, error) = match monitor_result {
        MonitorResult::Completed => {
            // Check exit status
            match child.wait() {
                Ok(status) if status.success() => (true, None),
                Ok(status) => (
                    false,
                    Some(format!("Exit code {}", status.code().unwrap_or(-1))),
                ),
                Err(e) => (false, Some(format!("Wait error: {}", e))),
            }
        }
        MonitorResult::TotalTimeout => (
            false,
            Some(format!(
                "Total timeout exceeded ({}m)",
                timeout_minutes
            )),
        ),
        MonitorResult::IdleTimeout => (
            false,
            Some(format!(
                "Idle timeout exceeded ({}m)",
                idle_timeout_minutes
            )),
        ),
        MonitorResult::Killed => (false, Some("Process was killed".to_string())),
    };

    AgentResult {
        id: sb.id.clone(),
        title: sb.title.clone(),
        action: sb.action,
        success,
        duration,
        total_tokens: if cumulative_tokens > 0 {
            Some(cumulative_tokens)
        } else {
            None
        },
        total_cost: if cumulative_cost > 0.0 {
            Some(cumulative_cost)
        } else {
            None
        },
        error,
    }
}

/// Assemble context string for a bean (rules + referenced files).
fn assemble_bean_context(beans_dir: &Path, bean_id: &str) -> String {
    let config = match Config::load_with_extends(beans_dir) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let mut context_parts = Vec::new();

    // Include rules file if present
    let rules_path = config.rules_path(beans_dir);
    if rules_path.exists() {
        if let Ok(rules) = std::fs::read_to_string(&rules_path) {
            if !rules.trim().is_empty() {
                context_parts.push(rules);
            }
        }
    }

    // Gather file references from the bean's description
    let bean_path = match crate::discovery::find_bean_file(beans_dir, bean_id) {
        Ok(p) => p,
        Err(_) => return context_parts.join("\n\n"),
    };

    let bean = match crate::bean::Bean::from_file(&bean_path) {
        Ok(b) => b,
        Err(_) => return context_parts.join("\n\n"),
    };

    // Extract file paths from description + acceptance
    let mut text_to_scan = String::new();
    if let Some(ref desc) = bean.description {
        text_to_scan.push_str(desc);
    }
    if let Some(ref acc) = bean.acceptance {
        text_to_scan.push_str("\n");
        text_to_scan.push_str(acc);
    }

    let workspace = beans_dir.parent().unwrap_or(Path::new("."));
    let paths = crate::ctx_assembler::extract_paths(&text_to_scan);
    if !paths.is_empty() {
        if let Ok(file_context) = crate::ctx_assembler::assemble_context(paths, workspace) {
            if !file_context.trim().is_empty() {
                context_parts.push(file_context);
            }
        }
    }

    context_parts.join("\n\n")
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

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

/// Print the dispatch plan as JSON stream events.
fn print_plan_json(plan: &DispatchPlan, parent_id: Option<&str>) {
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

    stream::emit(&StreamEvent::DryRun {
        parent_id,
        rounds,
    });
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

// ---------------------------------------------------------------------------
// Helpers (public for testing)
// ---------------------------------------------------------------------------

/// Find the bean file path. Public wrapper for use in tests.
pub fn find_bean_file(beans_dir: &Path, id: &str) -> Result<PathBuf> {
    crate::discovery::find_bean_file(beans_dir, id)
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

    fn default_args() -> RunArgs {
        RunArgs {
            id: None,
            jobs: 4,
            dry_run: false,
            loop_mode: false,
            auto_plan: false,
            keep_going: false,
            timeout: 30,
            idle_timeout: 5,
            json_stream: false,
        }
    }

    #[test]
    fn cmd_run_errors_when_no_run_template_and_no_pi() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, None);

        let args = default_args();

        let result = cmd_run(&beans_dir, args);
        // With no template and no pi on PATH, should error
        // (The exact error depends on whether pi is installed)
        // In CI/test without pi, it should bail
        if !pi_available() {
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("No agent configured") || err.contains("not found"),
                "Error should mention missing agent: {}",
                err
            );
        }
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
            dry_run: true,
            ..default_args()
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

    // -- New tests for direct mode and json-stream --

    #[test]
    fn determine_spawn_mode_template_when_run_set() {
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: Some("echo {id}".to_string()),
            plan: Some("plan {id}".to_string()),
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
        };
        let mode = determine_spawn_mode(&config);
        assert_eq!(
            mode,
            SpawnMode::Template {
                run_template: "echo {id}".to_string(),
                plan_template: Some("plan {id}".to_string()),
            }
        );
    }

    #[test]
    fn determine_spawn_mode_direct_when_no_run() {
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
        };
        let mode = determine_spawn_mode(&config);
        assert_eq!(mode, SpawnMode::Direct);
    }

    #[test]
    fn dry_run_with_json_stream() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, Some("echo {id}"));

        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.verify = Some("echo ok".to_string());
        bean.to_file(beans_dir.join("1-test.md")).unwrap();

        let args = RunArgs {
            dry_run: true,
            json_stream: true,
            ..default_args()
        };

        // Should succeed and emit JSON events (captured to stdout)
        let result = cmd_run(&beans_dir, args);
        assert!(result.is_ok());
    }

    #[test]
    fn template_wave_execution_with_echo() {
        let beans = vec![SizedBean {
            id: "1".to_string(),
            title: "Test".to_string(),
            tokens: 100,
            action: BeanAction::Implement,
            priority: 2,
            dependencies: vec![],
            parent: None,
            produces: vec![],
            requires: vec![],
        }];

        let results =
            run_wave_template(&beans, "echo {id}", None, 4, 30).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn template_wave_plan_without_template_errors() {
        let beans = vec![SizedBean {
            id: "1".to_string(),
            title: "Test".to_string(),
            tokens: 100,
            action: BeanAction::Plan,
            priority: 2,
            dependencies: vec![],
            parent: None,
            produces: vec![],
            requires: vec![],
        }];

        let results =
            run_wave_template(&beans, "echo {id}", None, 4, 30).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.as_ref().unwrap().contains("No plan template"));
    }

    #[test]
    fn template_wave_failed_command() {
        let beans = vec![SizedBean {
            id: "1".to_string(),
            title: "Fail".to_string(),
            tokens: 100,
            action: BeanAction::Implement,
            priority: 2,
            dependencies: vec![],
            parent: None,
            produces: vec![],
            requires: vec![],
        }];

        let results =
            run_wave_template(&beans, "false", None, 4, 30).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.is_some());
    }

    #[test]
    fn assemble_bean_context_returns_empty_for_missing_bean() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, None);

        let ctx = assemble_bean_context(&beans_dir, "nonexistent");
        assert!(ctx.is_empty());
    }

    #[test]
    fn assemble_bean_context_includes_rules() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, None);

        // Write a rules file
        fs::write(beans_dir.join("RULES.md"), "# Project Rules\nAlways test.").unwrap();

        // Write a simple bean
        let bean = crate::bean::Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1-test.md")).unwrap();

        let ctx = assemble_bean_context(&beans_dir, "1");
        assert!(ctx.contains("Project Rules"));
    }

    #[test]
    fn agent_result_tracks_tokens_and_cost() {
        let result = AgentResult {
            id: "1".to_string(),
            title: "Test".to_string(),
            action: BeanAction::Implement,
            success: true,
            duration: Duration::from_secs(10),
            total_tokens: Some(5000),
            total_cost: Some(0.03),
            error: None,
        };
        assert_eq!(result.total_tokens, Some(5000));
        assert_eq!(result.total_cost, Some(0.03));
    }

    // -- Ready-queue tests --

    fn make_sized_bean(id: &str, deps: Vec<&str>, produces: Vec<&str>, requires: Vec<&str>) -> SizedBean {
        SizedBean {
            id: id.to_string(),
            title: format!("Bean {}", id),
            tokens: 100,
            action: BeanAction::Implement,
            priority: 2,
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            parent: Some("parent".to_string()),
            produces: produces.into_iter().map(|s| s.to_string()).collect(),
            requires: requires.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn is_bean_ready_no_deps() {
        let bean = make_sized_bean("1", vec![], vec![], vec![]);
        let all_beans = vec![bean.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();
        let completed = HashSet::new();

        assert!(is_bean_ready(&bean, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn is_bean_ready_explicit_dep_not_met() {
        let bean = make_sized_bean("2", vec!["1"], vec![], vec![]);
        let dep = make_sized_bean("1", vec![], vec![], vec![]);
        let all_beans = vec![dep, bean.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();
        let completed = HashSet::new();

        assert!(!is_bean_ready(&bean, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn is_bean_ready_explicit_dep_met() {
        let bean = make_sized_bean("2", vec!["1"], vec![], vec![]);
        let dep = make_sized_bean("1", vec![], vec![], vec![]);
        let all_beans = vec![dep, bean.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();
        let mut completed = HashSet::new();
        completed.insert("1".to_string());

        assert!(is_bean_ready(&bean, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn is_bean_ready_requires_not_met() {
        let producer = make_sized_bean("1", vec![], vec!["TypesFile"], vec![]);
        let consumer = make_sized_bean("2", vec![], vec![], vec!["TypesFile"]);
        let all_beans = vec![producer, consumer.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();
        let completed = HashSet::new();

        assert!(!is_bean_ready(&consumer, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn is_bean_ready_requires_met() {
        let producer = make_sized_bean("1", vec![], vec!["TypesFile"], vec![]);
        let consumer = make_sized_bean("2", vec![], vec![], vec!["TypesFile"]);
        let all_beans = vec![producer, consumer.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();
        let mut completed = HashSet::new();
        completed.insert("1".to_string());

        assert!(is_bean_ready(&consumer, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn is_bean_ready_dep_outside_set_treated_as_met() {
        // If a dependency isn't in the dispatch set, treat as satisfied
        let bean = make_sized_bean("2", vec!["external"], vec![], vec![]);
        let all_beans = vec![bean.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();
        let completed = HashSet::new();

        // "external" is not in all_ids, so it's treated as met
        assert!(is_bean_ready(&bean, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn is_bean_ready_diamond_both_deps_needed() {
        // C requires both A and B
        let a = make_sized_bean("A", vec![], vec!["X"], vec![]);
        let b = make_sized_bean("B", vec![], vec!["Y"], vec![]);
        let c = make_sized_bean("C", vec![], vec![], vec!["X", "Y"]);
        let all_beans = vec![a, b, c.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();

        // Only A completed — C not ready
        let mut completed = HashSet::new();
        completed.insert("A".to_string());
        assert!(!is_bean_ready(&c, &completed, &all_ids, &all_beans));

        // Both completed — C ready
        completed.insert("B".to_string());
        assert!(is_bean_ready(&c, &completed, &all_ids, &all_beans));
    }

    #[test]
    fn ready_queue_starts_independent_beans_immediately() {
        // Simulate: A (no deps), B (no deps), C (depends on A only)
        // In wave model: wave 1 = [A, B], wave 2 = [C]
        // In ready-queue: A and B start immediately, C starts when A finishes
        // (even if B is still running)
        let index = Index { beans: vec![] };
        let a = make_sized_bean("A", vec![], vec!["X"], vec![]);
        let b = make_sized_bean("B", vec![], vec![], vec![]);
        let c = make_sized_bean("C", vec![], vec![], vec!["X"]);
        let all_beans = vec![a.clone(), b.clone(), c.clone()];
        let all_ids: HashSet<String> = all_beans.iter().map(|b| b.id.clone()).collect();

        // Initially: A and B are ready, C is not
        let completed = HashSet::new();
        assert!(is_bean_ready(&a, &completed, &all_ids, &all_beans));
        assert!(is_bean_ready(&b, &completed, &all_ids, &all_beans));
        assert!(!is_bean_ready(&c, &completed, &all_ids, &all_beans));

        // After A completes: C becomes ready (even though B hasn't finished)
        let mut completed = HashSet::new();
        completed.insert("A".to_string());
        assert!(is_bean_ready(&c, &completed, &all_ids, &all_beans));

        // Verify wave model would have put C in wave 2 (after both A and B)
        let waves = compute_waves(&all_beans, &index);
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0].beans.len(), 2); // A and B
        assert_eq!(waves[1].beans.len(), 1); // C
        assert_eq!(waves[1].beans[0].id, "C");
    }
}
