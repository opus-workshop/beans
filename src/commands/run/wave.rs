use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::bean::Status;
use crate::index::Index;
use crate::stream::{self, StreamEvent};
use crate::util::natural_cmp;

use super::plan::SizedBean;
use super::ready_queue::run_single_direct;
use super::{AgentResult, BeanAction, SpawnMode};

/// A wave of beans that can be dispatched in parallel.
pub struct Wave {
    pub beans: Vec<SizedBean>,
}

/// Compute waves of beans grouped by dependency order.
/// Wave 0: no deps. Wave 1: deps all in wave 0. Etc.
pub(super) fn compute_waves(beans: &[SizedBean], index: &Index) -> Vec<Wave> {
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
pub(super) fn run_wave(
    beans_dir: &Path,
    beans: &[SizedBean],
    spawn_mode: &SpawnMode,
    cfg: &super::RunConfig,
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
            cfg.max_jobs,
            cfg.timeout_minutes,
        ),
        SpawnMode::Direct => run_wave_direct(
            beans_dir,
            beans,
            cfg.max_jobs,
            cfg.timeout_minutes,
            cfg.idle_timeout_minutes,
            cfg.json_stream,
            wave_number,
            cfg.file_locking,
        ),
    }
}

/// Template mode: spawn agents via `sh -c <template>` (backward compat).
fn run_wave_template(
    beans: &[SizedBean],
    run_template: &str,
    _plan_template: Option<&str>,
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
#[allow(clippy::too_many_arguments)]
fn run_wave_direct(
    beans_dir: &Path,
    beans: &[SizedBean],
    max_jobs: usize,
    timeout_minutes: u32,
    idle_timeout_minutes: u32,
    json_stream: bool,
    wave_number: usize,
    file_locking: bool,
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
                let result = run_single_direct(
                    &beans_dir,
                    &sb,
                    timeout_min,
                    idle_min,
                    json_stream,
                    file_locking,
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::run::BeanAction;
    use crate::index::Index;

    #[test]
    fn compute_waves_no_deps() {
        let index = Index { beans: vec![] };
        let beans = vec![
            SizedBean {
                id: "1".to_string(),
                title: "A".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
            SizedBean {
                id: "2".to_string(),
                title: "B".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
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
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
            SizedBean {
                id: "2".to_string(),
                title: "B".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["1".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
            SizedBean {
                id: "3".to_string(),
                title: "C".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["2".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
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
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec![],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
            SizedBean {
                id: "2".to_string(),
                title: "Left".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["1".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
            SizedBean {
                id: "3".to_string(),
                title: "Right".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["1".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
            SizedBean {
                id: "4".to_string(),
                title: "Join".to_string(),
                action: BeanAction::Implement,
                priority: 2,
                dependencies: vec!["2".to_string(), "3".to_string()],
                parent: None,
                produces: vec![],
                requires: vec![],
                paths: vec![],
            },
        ];
        let waves = compute_waves(&beans, &index);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].beans.len(), 1); // 1
        assert_eq!(waves[1].beans.len(), 2); // 2, 3
        assert_eq!(waves[2].beans.len(), 1); // 4
    }

    #[test]
    fn template_wave_execution_with_echo() {
        let beans = vec![SizedBean {
            id: "1".to_string(),
            title: "Test".to_string(),
            action: BeanAction::Implement,
            priority: 2,
            dependencies: vec![],
            parent: None,
            produces: vec![],
            requires: vec![],
            paths: vec![],
        }];

        let results = run_wave_template(&beans, "echo {id}", None, 4, 30).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn template_wave_runs_implement_action() {
        let beans = vec![SizedBean {
            id: "1".to_string(),
            title: "Test".to_string(),
            action: BeanAction::Implement,
            priority: 2,
            dependencies: vec![],
            parent: None,
            produces: vec![],
            requires: vec![],
            paths: vec![],
        }];

        let results = run_wave_template(&beans, "echo {id}", None, 4, 30).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn template_wave_failed_command() {
        let beans = vec![SizedBean {
            id: "1".to_string(),
            title: "Fail".to_string(),
            action: BeanAction::Implement,
            priority: 2,
            dependencies: vec![],
            parent: None,
            produces: vec![],
            requires: vec![],
            paths: vec![],
        }];

        let results = run_wave_template(&beans, "false", None, 4, 30).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.is_some());
    }
}
