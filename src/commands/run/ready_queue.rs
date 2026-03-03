use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::bean::{Bean, Status};
use crate::config::Config;
use crate::index::{Index, IndexEntry};
use crate::pi_output::{self, AgentEvent};
use crate::prompt::{build_agent_prompt, PromptOptions};
use crate::stream::{self, StreamEvent};
use crate::timeout::{self, MonitorResult, TimeoutConfig};
use crate::util::natural_cmp;

use super::plan::SizedBean;
use super::wave::compute_waves;
use super::AgentResult;

/// Check if all dependencies of an index entry are closed.
pub(super) fn all_deps_closed(entry: &IndexEntry, index: &Index) -> bool {
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
pub(super) fn run_ready_queue_direct(
    beans_dir: &Path,
    all_beans: &[SizedBean],
    index: &Index,
    cfg: &super::RunConfig,
    keep_going: bool,
) -> Result<(Vec<AgentResult>, bool)> {
    let max_jobs = cfg.max_jobs;
    let timeout_minutes = cfg.timeout_minutes;
    let idle_timeout_minutes = cfg.idle_timeout_minutes;
    let json_stream = cfg.json_stream;
    let file_locking = cfg.file_locking;
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
                let result = run_single_direct(
                    &beans_dir,
                    &sb,
                    timeout_min,
                    idle_min,
                    json_stream,
                    file_locking,
                );
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
pub(super) fn run_single_direct(
    beans_dir: &Path,
    sb: &SizedBean,
    timeout_minutes: u32,
    idle_timeout_minutes: u32,
    json_stream: bool,
    file_locking: bool,
) -> AgentResult {
    let started = Instant::now();

    // Pre-emptive file locking: lock files listed in the bean's `paths` field.
    if file_locking && !sb.paths.is_empty() {
        let pid = std::process::id();
        for path in &sb.paths {
            match crate::locks::acquire(beans_dir, &sb.id, pid, path) {
                Ok(true) => {}
                Ok(false) => {
                    // Already locked by another agent — check who holds it
                    let holder = crate::locks::check_lock(beans_dir, path)
                        .ok()
                        .flatten()
                        .map(|l| format!("bean {} (pid {})", l.bean_id, l.pid))
                        .unwrap_or_else(|| "unknown".to_string());
                    eprintln!(
                        "  ⚠ Cannot lock {} for bean {} — held by {}",
                        path, sb.id, holder
                    );
                }
                Err(e) => {
                    eprintln!("  ⚠ Lock error for {}: {}", path, e);
                }
            }
        }
    }

    // Load the full bean for prompt construction
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

    let bean = match Bean::from_file(&bean_file) {
        Ok(b) => b,
        Err(e) => {
            return AgentResult {
                id: sb.id.clone(),
                title: sb.title.clone(),
                action: sb.action,
                success: false,
                duration: started.elapsed(),
                total_tokens: None,
                total_cost: None,
                error: Some(format!("Cannot parse bean file: {}", e)),
            };
        }
    };

    // Build structured prompt via prompt module
    let prompt_options = PromptOptions {
        beans_dir: beans_dir.to_path_buf(),
        instructions: None,
        concurrent_overlaps: None,
    };

    let prompt_result = match build_agent_prompt(&bean, &prompt_options) {
        Ok(r) => r,
        Err(e) => {
            return AgentResult {
                id: sb.id.clone(),
                title: sb.title.clone(),
                action: sb.action,
                success: false,
                duration: started.elapsed(),
                total_tokens: None,
                total_cost: None,
                error: Some(format!("Failed to build prompt: {}", e)),
            };
        }
    };

    // Build pi command using structured prompt fields
    let mut cmd = Command::new("pi");
    cmd.args(["--mode", "json", "--print", "--no-session"]);

    if !prompt_result.system_prompt.is_empty() {
        cmd.args(["--append-system-prompt", &prompt_result.system_prompt]);
    }

    if !prompt_result.file_ref.is_empty() {
        cmd.arg(&prompt_result.file_ref);
    }
    cmd.arg(&prompt_result.user_message);
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
                            let file_path = pi_output::extract_file_path(name, arguments);
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
            Some(format!("Total timeout exceeded ({}m)", timeout_minutes)),
        ),
        MonitorResult::IdleTimeout => (
            false,
            Some(format!("Idle timeout exceeded ({}m)", idle_timeout_minutes)),
        ),
    };

    // Release all file locks held by this bean.
    if file_locking {
        let _ = crate::locks::release_all_for_bean(beans_dir, &sb.id);
    }

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



#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::run::BeanAction;
    use crate::index::Index;
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

    fn make_sized_bean(
        id: &str,
        deps: Vec<&str>,
        produces: Vec<&str>,
        requires: Vec<&str>,
    ) -> SizedBean {
        SizedBean {
            id: id.to_string(),
            title: format!("Bean {}", id),
            action: BeanAction::Implement,
            priority: 2,
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            parent: Some("parent".to_string()),
            produces: produces.into_iter().map(|s| s.to_string()).collect(),
            requires: requires.into_iter().map(|s| s.to_string()).collect(),
            paths: vec![],
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

    #[test]
    fn build_prompt_returns_err_for_missing_bean() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, None);

        // build_agent_prompt requires a Bean struct, so a missing bean is
        // handled by the caller (run_single_direct) before we get here.
        // Instead, verify that a bean with no description still produces a prompt.
        let bean = crate::bean::Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1-test.md")).unwrap();

        let options = PromptOptions {
            beans_dir: beans_dir.clone(),
            instructions: None,
            concurrent_overlaps: None,
        };
        let result = build_agent_prompt(&bean, &options);
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.system_prompt.contains("Bean Assignment"));
        assert!(prompt.user_message.contains("bn close 1"));
    }

    #[test]
    fn build_prompt_includes_rules() {
        let (_dir, beans_dir) = make_beans_dir();
        write_config(&beans_dir, None);

        // Write a rules file
        fs::write(beans_dir.join("RULES.md"), "# Project Rules\nAlways test.").unwrap();

        // Write a simple bean
        let bean = crate::bean::Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1-test.md")).unwrap();

        let options = PromptOptions {
            beans_dir: beans_dir.clone(),
            instructions: None,
            concurrent_overlaps: None,
        };
        let result = build_agent_prompt(&bean, &options).unwrap();
        assert!(result.system_prompt.contains("Project Rules"));
        assert!(result.system_prompt.contains("Always test."));
    }
}
