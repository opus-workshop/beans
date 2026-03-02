use std::io::Read;
use std::path::Path;
use std::process::{Command as ShellCommand, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::{Bean, OnCloseAction, OnFailAction, RunRecord, RunResult, Status};
use crate::config::Config;
use crate::discovery::{archive_path_for_bean, find_archived_bean, find_bean_file};
use crate::hooks::{
    current_git_branch, execute_config_hook, execute_hook, is_trusted, HookEvent, HookVars,
};
use crate::index::Index;
use crate::util::title_to_slug;
use crate::worktree;

#[cfg(test)]
use std::fs;

/// Maximum stdout size to capture as outputs (64 KB).
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Find the largest byte index <= `max_bytes` that falls on a UTF-8 char boundary.
///
/// Slicing a `&str` at an arbitrary byte offset panics if it lands inside a
/// multi-byte character. This helper walks backward to find a safe boundary.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Result of running a verify command
struct VerifyResult {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    #[allow(dead_code)]
    stderr: String,
    output: String, // combined stdout+stderr, for backward compat
    /// True when the process was killed due to verify_timeout being exceeded.
    timed_out: bool,
}

/// Run a verify command for a bean.
///
/// Returns VerifyResult with success status, exit code, and combined stdout/stderr.
/// If `timeout_secs` is Some(n), the process is killed after n seconds and
/// the result has `timed_out = true`.
fn run_verify(
    beans_dir: &Path,
    verify_cmd: &str,
    timeout_secs: Option<u64>,
) -> Result<VerifyResult> {
    // Run in the project root (parent of .beans/)
    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    println!("Running verify: {}", verify_cmd);

    let mut child = ShellCommand::new("sh")
        .args(["-c", verify_cmd])
        .current_dir(project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn verify command: {}", verify_cmd))?;

    // Drain stdout/stderr in background threads to prevent pipe buffer deadlock.
    let stdout_thread = {
        let stdout = child.stdout.take().expect("stdout is piped");
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut reader = std::io::BufReader::new(stdout);
            let _ = reader.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).trim().to_string()
        })
    };
    let stderr_thread = {
        let stderr = child.stderr.take().expect("stderr is piped");
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut reader = std::io::BufReader::new(stderr);
            let _ = reader.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).trim().to_string()
        })
    };

    // Poll for process exit, enforcing the timeout if set.
    let timeout = timeout_secs.map(Duration::from_secs);
    let start = Instant::now();

    let (timed_out, exit_status) = loop {
        match child
            .try_wait()
            .with_context(|| "Failed to poll verify process")?
        {
            Some(status) => break (false, Some(status)),
            None => {
                if let Some(limit) = timeout {
                    if start.elapsed() >= limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        break (true, None);
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let stdout_str = stdout_thread.join().unwrap_or_default();
    let stderr_str = stderr_thread.join().unwrap_or_default();

    if timed_out {
        let secs = timeout_secs.unwrap_or(0);
        return Ok(VerifyResult {
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            output: format!("Verify timed out after {}s", secs),
            timed_out: true,
        });
    }

    let status = exit_status.expect("exit_status is Some when not timed_out");
    let combined_output = {
        let mut combined = stdout_str.clone();
        if !stderr_str.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&stderr_str);
        }
        combined
    };

    Ok(VerifyResult {
        success: status.success(),
        exit_code: status.code(),
        stdout: stdout_str,
        stderr: stderr_str,
        output: combined_output,
        timed_out: false,
    })
}

/// Truncate output to first N + last N lines.
/// If output has fewer than 2*N lines, return it unchanged.
fn truncate_output(output: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();

    if lines.len() <= max_lines * 2 {
        return output.to_string();
    }

    let first = &lines[..max_lines];
    let last = &lines[lines.len() - max_lines..];

    format!(
        "{}\n\n... ({} lines omitted) ...\n\n{}",
        first.join("\n"),
        lines.len() - max_lines * 2,
        last.join("\n")
    )
}

/// Format a verify failure as a Markdown block to append to notes.
fn format_failure_note(attempt: u32, exit_code: Option<i32>, output: &str) -> String {
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let truncated = truncate_output(output, 50);
    let exit_str = exit_code
        .map(|c| format!("Exit code: {}\n", c))
        .unwrap_or_default();

    format!(
        "\n## Attempt {} — {}\n{}\n```\n{}\n```\n",
        attempt, timestamp, exit_str, truncated
    )
}

/// Check if all children of a parent bean are closed (in archive or with status=closed).
///
/// Returns true if:
/// - The parent has no children, OR
/// - All children are either in the archive (closed) or have status=closed
fn all_children_closed(beans_dir: &Path, parent_id: &str) -> Result<bool> {
    // Always rebuild the index fresh - we can't rely on staleness check because
    // files may have just been moved to archive (which isn't tracked in staleness)
    let index = Index::build(beans_dir)?;
    let archived = Index::collect_archived(beans_dir).unwrap_or_default();

    // Combine active and archived beans
    let mut all_beans = index.beans;
    all_beans.extend(archived);

    // Find children of this parent
    let children: Vec<_> = all_beans
        .iter()
        .filter(|b| b.parent.as_deref() == Some(parent_id))
        .collect();

    // If no children, return true (nothing to check)
    if children.is_empty() {
        return Ok(true);
    }

    // Check if all children are closed
    for child in children {
        if child.status != Status::Closed {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Close a parent bean automatically when all its children are closed.
/// This is called recursively to close ancestor beans.
///
/// Unlike normal close, auto-close:
/// - Skips verify command (children already verified)
/// - Sets close_reason to indicate auto-close
/// - Recursively checks grandparent
fn auto_close_parent(beans_dir: &Path, parent_id: &str) -> Result<()> {
    // Find the parent bean
    let bean_path = match find_bean_file(beans_dir, parent_id) {
        Ok(path) => path,
        Err(_) => {
            // Parent might already be archived, skip
            return Ok(());
        }
    };

    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load parent bean: {}", parent_id))?;

    // Skip if already closed
    if bean.status == Status::Closed {
        return Ok(());
    }

    let now = Utc::now();

    // Close the parent (skip verify - children already verified)
    bean.status = Status::Closed;
    bean.closed_at = Some(now);
    bean.close_reason = Some("Auto-closed: all children completed".to_string());
    bean.updated_at = now;

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save parent bean: {}", parent_id))?;

    // Archive the closed bean
    let slug = bean
        .slug
        .clone()
        .unwrap_or_else(|| title_to_slug(&bean.title));
    let ext = bean_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("md");
    let today = chrono::Local::now().naive_local().date();
    let archive_path = archive_path_for_bean(beans_dir, parent_id, &slug, ext, today);

    // Create archive directories if needed
    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create archive directories for bean {}",
                parent_id
            )
        })?;
    }

    // Move the bean file to archive
    std::fs::rename(&bean_path, &archive_path)
        .with_context(|| format!("Failed to move bean {} to archive", parent_id))?;

    // Update bean metadata to mark as archived
    bean.is_archived = true;
    bean.to_file(&archive_path)
        .with_context(|| format!("Failed to save archived parent bean: {}", parent_id))?;

    println!("Auto-closed parent bean {}: {}", parent_id, bean.title);

    // Recursively check if this bean's parent should also be auto-closed
    if let Some(grandparent_id) = &bean.parent {
        if all_children_closed(beans_dir, grandparent_id)? {
            auto_close_parent(beans_dir, grandparent_id)?;
        }
    }

    Ok(())
}

/// Walk up the parent chain to find the root ancestor of a bean.
///
/// Returns the ID of the topmost parent (the bean with no parent).
/// If the bean itself has no parent, returns its own ID.
/// Handles archived parents gracefully by checking both active and archived beans.
fn find_root_parent(beans_dir: &Path, bean: &Bean) -> Result<String> {
    let mut current_id = match &bean.parent {
        None => return Ok(bean.id.clone()),
        Some(pid) => pid.clone(),
    };

    loop {
        let path = find_bean_file(beans_dir, &current_id)
            .or_else(|_| find_archived_bean(beans_dir, &current_id));

        match path {
            Ok(p) => {
                let b = Bean::from_file(&p)
                    .with_context(|| format!("Failed to load parent bean: {}", current_id))?;
                match b.parent {
                    Some(parent_id) => current_id = parent_id,
                    None => return Ok(current_id),
                }
            }
            Err(_) => return Ok(current_id), // Can't find parent, assume it's root
        }
    }
}

/// Close one or more beans.
///
/// Sets status=closed, closed_at=now, and optionally close_reason.
/// If the bean has a verify command, it must pass before closing (unless force=true).
/// Calls pre-close hook before verify (can block close if hook fails).
/// Auto-closes parent beans when all children are closed (if enabled in config).
/// Rebuilds the index.
pub fn cmd_close(
    beans_dir: &Path,
    ids: Vec<String>,
    reason: Option<String>,
    force: bool,
) -> Result<()> {
    if ids.is_empty() {
        return Err(anyhow!("At least one bean ID is required"));
    }

    let now = Utc::now();
    let mut any_closed = false;
    let mut rejected_beans = Vec::new();

    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    let config = Config::load(beans_dir).ok();

    for id in &ids {
        let bean_path =
            find_bean_file(beans_dir, id).with_context(|| format!("Bean not found: {}", id))?;

        let mut bean =
            Bean::from_file(&bean_path).with_context(|| format!("Failed to load bean: {}", id))?;

        let pre_close_result =
            execute_hook(HookEvent::PreClose, &bean, project_root, reason.clone());

        let pre_close_passed = match pre_close_result {
            Ok(hook_passed) => {
                // Hook executed successfully, use its result
                hook_passed
            }
            Err(e) => {
                // Hook execution failed (not executable, timeout, etc.), log but don't block
                eprintln!("Bean {} pre-close hook error: {}", id, e);
                true // Silently pass (allow close to proceed)
            }
        };

        if !pre_close_passed {
            eprintln!("Bean {} rejected by pre-close hook", id);
            rejected_beans.push(id.clone());
            continue;
        }

        // Check if bean has a verify command (runs AFTER pre-close hook passes)
        if let Some(ref verify_cmd) = bean.verify {
            if verify_cmd.trim().is_empty() {
                eprintln!("Warning: bean {} has empty verify command, skipping", id);
            } else if force {
                println!("Skipping verify for bean {} (--force)", id);
            } else {
                // Record timing for history
                let started_at = Utc::now();

                // Compute effective timeout: bean-level overrides config-level.
                let timeout_secs =
                    bean.effective_verify_timeout(config.as_ref().and_then(|c| c.verify_timeout));

                // Run the verify command
                let verify_result = run_verify(beans_dir, verify_cmd, timeout_secs)?;

                let finished_at = Utc::now();
                let duration_secs = (finished_at - started_at).num_milliseconds() as f64 / 1000.0;

                // Read agent name from env var (deli/bw set this when spawning)
                let agent = std::env::var("BEANS_AGENT").ok();

                if !verify_result.success {
                    // Increment attempts
                    bean.attempts += 1;
                    bean.updated_at = Utc::now();

                    // Surface timeout prominently
                    if verify_result.timed_out {
                        let secs = timeout_secs.unwrap_or(0);
                        println!("Verify timed out after {}s for bean {}", secs, id);
                    }

                    // Append failure to notes for future agents (backward compat)
                    let failure_note = format_failure_note(
                        bean.attempts,
                        verify_result.exit_code,
                        &verify_result.output,
                    );
                    match &mut bean.notes {
                        Some(notes) => notes.push_str(&failure_note),
                        None => bean.notes = Some(failure_note),
                    }

                    // Record structured history entry
                    let output_snippet = if verify_result.output.is_empty() {
                        None
                    } else {
                        Some(truncate_output(&verify_result.output, 20))
                    };
                    bean.history.push(RunRecord {
                        attempt: bean.attempts,
                        started_at,
                        finished_at: Some(finished_at),
                        duration_secs: Some(duration_secs),
                        agent: agent.clone(),
                        result: if verify_result.timed_out {
                            RunResult::Timeout
                        } else {
                            RunResult::Fail
                        },
                        exit_code: verify_result.exit_code,
                        tokens: None,
                        cost: None,
                        output_snippet,
                    });

                    // Circuit breaker: check if subtree attempts exceed max_loops
                    let root_id = find_root_parent(beans_dir, &bean)?;
                    let config_max = config.as_ref().map(|c| c.max_loops).unwrap_or(10);
                    let max_loops_limit = if root_id == bean.id {
                        bean.effective_max_loops(config_max)
                    } else {
                        let root_path = find_bean_file(beans_dir, &root_id)
                            .or_else(|_| find_archived_bean(beans_dir, &root_id));
                        match root_path {
                            Ok(p) => Bean::from_file(&p)
                                .map(|b| b.effective_max_loops(config_max))
                                .unwrap_or(config_max),
                            Err(_) => config_max,
                        }
                    };

                    if max_loops_limit > 0 {
                        // Save bean first so subtree count is accurate
                        bean.to_file(&bean_path)
                            .with_context(|| format!("Failed to save bean: {}", id))?;

                        let subtree_total =
                            crate::graph::count_subtree_attempts(beans_dir, &root_id)?;
                        if subtree_total >= max_loops_limit {
                            // Trip circuit breaker
                            if !bean.labels.contains(&"circuit-breaker".to_string()) {
                                bean.labels.push("circuit-breaker".to_string());
                            }
                            bean.priority = 0;
                            bean.to_file(&bean_path)
                                .with_context(|| format!("Failed to save bean: {}", id))?;

                            eprintln!(
                                "⚡ Circuit breaker tripped for bean {} \
                                 (subtree total {} >= max_loops {} across root {})",
                                id, subtree_total, max_loops_limit, root_id
                            );
                            eprintln!(
                                "Bean {} escalated to P0 with 'circuit-breaker' label. \
                                 Manual intervention required.",
                                id
                            );
                            continue;
                        }
                    }

                    // Process on_fail action
                    if let Some(ref on_fail) = bean.on_fail {
                        match on_fail {
                            OnFailAction::Retry { max, delay_secs } => {
                                let max_retries = max.unwrap_or(bean.max_attempts);
                                if bean.attempts < max_retries {
                                    println!(
                                        "on_fail: will retry (attempt {}/{})",
                                        bean.attempts, max_retries
                                    );
                                    if let Some(delay) = delay_secs {
                                        println!(
                                            "on_fail: retry delay {}s (enforced by orchestrator)",
                                            delay
                                        );
                                    }
                                    // Release claim so bw/deli can pick it up
                                    bean.claimed_by = None;
                                    bean.claimed_at = None;
                                } else {
                                    println!("on_fail: max retries ({}) exhausted", max_retries);
                                }
                            }
                            OnFailAction::Escalate { priority, message } => {
                                if let Some(p) = priority {
                                    let old_priority = bean.priority;
                                    bean.priority = *p;
                                    println!(
                                        "on_fail: escalated priority P{} → P{}",
                                        old_priority, p
                                    );
                                }
                                if let Some(msg) = message {
                                    // Append escalation message to notes
                                    let note = format!(
                                        "\n## Escalated — {}\n{}",
                                        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
                                        msg
                                    );
                                    match &mut bean.notes {
                                        Some(notes) => notes.push_str(&note),
                                        None => bean.notes = Some(note),
                                    }
                                    println!("on_fail: {}", msg);
                                }
                                // Add escalated label
                                if !bean.labels.contains(&"escalated".to_string()) {
                                    bean.labels.push("escalated".to_string());
                                }
                            }
                        }
                    }

                    bean.to_file(&bean_path)
                        .with_context(|| format!("Failed to save bean: {}", id))?;

                    // Display detailed failure feedback
                    if verify_result.timed_out {
                        println!("✗ Verify timed out for bean {}", id);
                    } else {
                        println!("✗ Verify failed for bean {}", id);
                    }
                    println!();
                    println!("Command: {}", verify_cmd);
                    if verify_result.timed_out {
                        println!("Timed out after {}s", timeout_secs.unwrap_or(0));
                    } else if let Some(code) = verify_result.exit_code {
                        println!("Exit code: {}", code);
                    }
                    if !verify_result.output.is_empty() {
                        println!("Output:");
                        for line in verify_result.output.lines() {
                            println!("  {}", line);
                        }
                    }
                    println!();
                    println!("Attempt {}. Bean remains open.", bean.attempts);
                    println!("Tip: Run `bn verify {}` to test without closing.", id);
                    println!("Tip: Use `bn close {} --force` to skip verify.", id);

                    // Fire on_fail config hook (async, non-blocking)
                    if let Some(ref config) = config {
                        if let Some(ref on_fail_template) = config.on_fail {
                            let output_text = &verify_result.output;
                            let vars = HookVars {
                                id: Some(id.clone()),
                                title: Some(bean.title.clone()),
                                status: Some(format!("{}", bean.status)),
                                attempt: Some(bean.attempts),
                                output: Some(output_text.clone()),
                                branch: current_git_branch(),
                                ..Default::default()
                            };
                            execute_config_hook("on_fail", on_fail_template, &vars, project_root);
                        }
                    }

                    continue;
                }

                // Record success in history
                bean.history.push(RunRecord {
                    attempt: bean.attempts + 1,
                    started_at,
                    finished_at: Some(finished_at),
                    duration_secs: Some(duration_secs),
                    agent,
                    result: RunResult::Pass,
                    exit_code: verify_result.exit_code,
                    tokens: None,
                    cost: None,
                    output_snippet: None,
                });

                // Capture stdout as bean outputs
                let stdout = &verify_result.stdout;
                if !stdout.is_empty() {
                    if stdout.len() > MAX_OUTPUT_BYTES {
                        let end = truncate_to_char_boundary(stdout, MAX_OUTPUT_BYTES);
                        let truncated = &stdout[..end];
                        eprintln!(
                            "Warning: verify stdout ({} bytes) exceeds 64KB, truncating",
                            stdout.len()
                        );
                        bean.outputs = Some(serde_json::json!({
                            "text": truncated,
                            "truncated": true,
                            "original_bytes": stdout.len()
                        }));
                    } else {
                        match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                            Ok(json) => {
                                bean.outputs = Some(json);
                            }
                            Err(_) => {
                                bean.outputs = Some(serde_json::json!({
                                    "text": stdout.trim()
                                }));
                            }
                        }
                    }
                }

                println!("Verify passed for bean {}", id);
            }
        }

        // Handle worktree merge (after verify passes, before archiving)
        //
        // detect_worktree() uses the process-global CWD. If CWD was deleted or
        // points to an unrelated directory (e.g. during parallel test execution),
        // we gracefully skip worktree operations. We also validate that the
        // detected worktree actually contains this project's root — this prevents
        // acting on a foreign repository when CWD is polluted.
        let worktree_info = worktree::detect_worktree().unwrap_or(None);
        let worktree_info = worktree_info.filter(|wt_info| {
            let canonical_root =
                std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
            canonical_root.starts_with(&wt_info.worktree_path)
        });
        if let Some(ref wt_info) = worktree_info {
            // Commit any uncommitted changes
            worktree::commit_worktree_changes(&format!("Close bean {}: {}", id, bean.title))?;

            // Merge to main
            match worktree::merge_to_main(wt_info, id)? {
                worktree::MergeResult::Success | worktree::MergeResult::NothingToCommit => {
                    // Continue to archive
                }
                worktree::MergeResult::Conflict { files } => {
                    eprintln!("Merge conflict in files: {:?}", files);
                    eprintln!("Resolve conflicts and run `bn close {}` again", id);
                    return Ok(()); // Don't archive yet
                }
            }
        }

        // Close the bean
        bean.status = crate::bean::Status::Closed;
        bean.closed_at = Some(now);
        bean.close_reason = reason.clone();
        bean.updated_at = now;

        // Finalize the current attempt as success (memory system tracking)
        if let Some(attempt) = bean.attempt_log.last_mut() {
            if attempt.finished_at.is_none() {
                attempt.outcome = crate::bean::AttemptOutcome::Success;
                attempt.finished_at = Some(now);
                attempt.notes = reason.clone();
            }
        }

        // Update last_verified for facts (staleness tracking)
        if bean.bean_type == "fact" {
            bean.last_verified = Some(now);
        }

        bean.to_file(&bean_path)
            .with_context(|| format!("Failed to save bean: {}", id))?;

        // Archive the closed bean
        let slug = bean
            .slug
            .clone()
            .unwrap_or_else(|| title_to_slug(&bean.title));
        let ext = bean_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("md");
        let today = chrono::Local::now().naive_local().date();
        let archive_path = archive_path_for_bean(beans_dir, id, &slug, ext, today);

        // Create archive directories if needed
        if let Some(parent) = archive_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create archive directories for bean {}", id))?;
        }

        // Move the bean file to archive
        std::fs::rename(&bean_path, &archive_path)
            .with_context(|| format!("Failed to move bean {} to archive", id))?;

        // Update bean metadata to mark as archived
        bean.is_archived = true;
        bean.to_file(&archive_path)
            .with_context(|| format!("Failed to save archived bean: {}", id))?;

        println!("Closed bean {}: {}", id, bean.title);
        any_closed = true;

        // Fire post-close hook (failure warns but does NOT revert the close)
        match execute_hook(HookEvent::PostClose, &bean, project_root, reason.clone()) {
            Ok(false) => {
                eprintln!("Warning: post-close hook returned non-zero for bean {}", id);
            }
            Err(e) => {
                eprintln!("Warning: post-close hook error for bean {}: {}", id, e);
            }
            Ok(true) => {}
        }

        // Process on_close actions (after post-close hook)
        for action in &bean.on_close {
            match action {
                OnCloseAction::Run { command } => {
                    if !is_trusted(project_root) {
                        eprintln!(
                            "on_close: skipping `{}` (not trusted — run `bn trust` to enable)",
                            command
                        );
                        continue;
                    }
                    eprintln!("on_close: running `{}`", command);
                    let status = std::process::Command::new("sh")
                        .args(["-c", command.as_str()])
                        .current_dir(project_root)
                        .status();
                    match status {
                        Ok(s) if !s.success() => {
                            eprintln!("on_close run command failed: {}", command)
                        }
                        Err(e) => eprintln!("on_close run command error: {}", e),
                        _ => {}
                    }
                }
                OnCloseAction::Notify { message } => {
                    println!("[bean {}] {}", id, message);
                }
            }
        }

        // Fire on_close config hook (async, non-blocking)
        if let Some(ref config) = config {
            if let Some(ref on_close_template) = config.on_close {
                let vars = HookVars {
                    id: Some(id.clone()),
                    title: Some(bean.title.clone()),
                    status: Some("closed".into()),
                    branch: current_git_branch(),
                    ..Default::default()
                };
                execute_config_hook("on_close", on_close_template, &vars, project_root);
            }
        }

        // Clean up worktree after successful close
        if let Some(ref wt_info) = worktree_info {
            if let Err(e) = worktree::cleanup_worktree(wt_info) {
                eprintln!("Warning: failed to clean up worktree: {}", e);
            }
        }

        // Check if parent should be auto-closed
        // (skip if beans_dir was removed by worktree cleanup)
        if beans_dir.exists() {
            if let Some(parent_id) = &bean.parent {
                // Check config for auto_close_parent setting
                let auto_close_enabled =
                    config.as_ref().map(|c| c.auto_close_parent).unwrap_or(true); // Default to true

                if auto_close_enabled && all_children_closed(beans_dir, parent_id)? {
                    auto_close_parent(beans_dir, parent_id)?;
                }
            }
        }
    }

    // Report rejected beans
    if !rejected_beans.is_empty() {
        eprintln!(
            "Failed to close {} bean(s) due to pre-close hook rejection: {}",
            rejected_beans.len(),
            rejected_beans.join(", ")
        );
    }

    // Rebuild index once after all updates (even if some failed verification)
    // Skip if beans_dir was removed by worktree cleanup
    if (any_closed || !ids.is_empty()) && beans_dir.exists() {
        let index = Index::build(beans_dir).with_context(|| "Failed to rebuild index")?;
        index
            .save(beans_dir)
            .with_context(|| "Failed to save index")?;
    }

    Ok(())
}

/// Mark an attempt as explicitly failed.
///
/// The bean stays open and the claim is released so another agent can retry.
/// Records the failure in attempt_log for episodic memory.
pub fn cmd_close_failed(beans_dir: &Path, ids: Vec<String>, reason: Option<String>) -> Result<()> {
    if ids.is_empty() {
        return Err(anyhow!("At least one bean ID is required"));
    }

    let now = Utc::now();

    for id in &ids {
        let bean_path =
            find_bean_file(beans_dir, id).with_context(|| format!("Bean not found: {}", id))?;

        let mut bean =
            Bean::from_file(&bean_path).with_context(|| format!("Failed to load bean: {}", id))?;

        // Finalize the current attempt as failed
        if let Some(attempt) = bean.attempt_log.last_mut() {
            if attempt.finished_at.is_none() {
                attempt.outcome = crate::bean::AttemptOutcome::Failed;
                attempt.finished_at = Some(now);
                attempt.notes = reason.clone();
            }
        }

        // Release the claim (bean stays open for retry)
        bean.claimed_by = None;
        bean.claimed_at = None;
        bean.status = Status::Open;
        bean.updated_at = now;

        // Append failure to notes for visibility
        if let Some(ref reason_text) = reason {
            let failure_note = format!(
                "\n## Failed attempt — {}\n{}\n",
                now.format("%Y-%m-%dT%H:%M:%SZ"),
                reason_text
            );
            match &mut bean.notes {
                Some(notes) => notes.push_str(&failure_note),
                None => bean.notes = Some(failure_note),
            }
        }

        bean.to_file(&bean_path)
            .with_context(|| format!("Failed to save bean: {}", id))?;

        let attempt_count = bean.attempt_log.len();
        println!(
            "Marked bean {} as failed (attempt #{}): {}",
            id, attempt_count, bean.title
        );
        if let Some(ref reason_text) = reason {
            println!("  Reason: {}", reason_text);
        }
        println!("  Bean remains open for retry.");
    }

    // Rebuild index
    let index = Index::build(beans_dir).with_context(|| "Failed to rebuild index")?;
    index
        .save(beans_dir)
        .with_context(|| "Failed to save index")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::title_to_slug;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_close_single_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived, not in root beans dir
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.close_reason.is_none());
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_reason() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(
            &beans_dir,
            vec!["1".to_string()],
            Some("Fixed".to_string()),
            false,
        )
        .unwrap();

        // Bean should be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert_eq!(updated.close_reason, Some("Fixed".to_string()));
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_multiple_beans() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        let bean3 = Bean::new("3", "Task 3");
        let slug1 = title_to_slug(&bean1.title);
        let slug2 = title_to_slug(&bean2.title);
        let slug3 = title_to_slug(&bean3.title);
        bean1
            .to_file(beans_dir.join(format!("1-{}.md", slug1)))
            .unwrap();
        bean2
            .to_file(beans_dir.join(format!("2-{}.md", slug2)))
            .unwrap();
        bean3
            .to_file(beans_dir.join(format!("3-{}.md", slug3)))
            .unwrap();

        cmd_close(
            &beans_dir,
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            None,
            false,
        )
        .unwrap();

        for id in &["1", "2", "3"] {
            // All beans should be archived
            let archived = crate::discovery::find_archived_bean(&beans_dir, id).unwrap();
            let bean = Bean::from_file(&archived).unwrap();
            assert_eq!(bean.status, Status::Closed);
            assert!(bean.closed_at.is_some());
            assert!(bean.is_archived);
        }
    }

    #[test]
    fn test_close_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_close(&beans_dir, vec!["99".to_string()], None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_no_ids() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_close(&beans_dir, vec![], None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        let slug1 = title_to_slug(&bean1.title);
        let slug2 = title_to_slug(&bean2.title);
        bean1
            .to_file(beans_dir.join(format!("1-{}.md", slug1)))
            .unwrap();
        bean2
            .to_file(beans_dir.join(format!("2-{}.md", slug2)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let index = Index::load(&beans_dir).unwrap();
        // After closing, bean 1 is archived, so only bean 2 should be in the index
        assert_eq!(index.beans.len(), 1);
        let entry2 = index.beans.iter().find(|e| e.id == "2").unwrap();
        assert_eq!(entry2.status, Status::Open);

        // Verify bean 1 was archived and still closed
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let bean1_archived = Bean::from_file(&archived).unwrap();
        assert_eq!(bean1_archived.status, Status::Closed);
    }

    #[test]
    fn test_close_sets_updated_at() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        let original_updated_at = bean.updated_at;
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Read from archive
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert!(updated.updated_at > original_updated_at);
    }

    #[test]
    fn test_close_with_passing_verify() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with verify");
        bean.verify = Some("true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Verify bean is archived after passing verify
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_failing_verify_increments_attempts() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with failing verify");
        bean.verify = Some("false".to_string());
        bean.attempts = 0;
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open); // Not closed
        assert_eq!(updated.attempts, 1); // Incremented
        assert!(updated.closed_at.is_none());
    }

    #[test]
    fn test_close_with_failing_verify_multiple_attempts() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with failing verify");
        bean.verify = Some("false".to_string());
        bean.attempts = 0;
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // First attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 1);
        assert_eq!(updated.status, Status::Open);

        // Second attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 2);
        assert_eq!(updated.status, Status::Open);

        // Third attempt - no limit, keeps incrementing
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 3);
        assert_eq!(updated.status, Status::Open);

        // Fourth attempt - still works, no max
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 4);
        assert_eq!(updated.status, Status::Open);
    }

    #[test]
    fn test_close_failure_appends_to_notes() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with failing verify");
        // Use a command that produces output
        bean.verify = Some("echo 'test error output' && exit 1".to_string());
        bean.notes = Some("Original notes".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        let notes = updated.notes.unwrap();

        // Original notes preserved
        assert!(notes.contains("Original notes"));
        // Failure appended
        assert!(notes.contains("## Attempt 1"));
        assert!(notes.contains("Exit code: 1"));
        assert!(notes.contains("test error output"));
    }

    #[test]
    fn test_close_failure_creates_notes_if_none() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with no notes");
        bean.verify = Some("echo 'failure' && exit 1".to_string());
        // No notes set
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        let notes = updated.notes.unwrap();

        assert!(notes.contains("## Attempt 1"));
        assert!(notes.contains("failure"));
    }

    #[test]
    fn test_close_without_verify_still_works() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task without verify");
        // No verify command set
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Verify bean is archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_force_skips_verify() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with failing verify");
        // This verify command would normally fail
        bean.verify = Some("false".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close with force=true should skip verify and close anyway
        cmd_close(&beans_dir, vec!["1".to_string()], None, true).unwrap();

        // Bean should be archived despite failing verify
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
        assert_eq!(updated.attempts, 0); // Attempts should not be incremented
    }

    #[test]
    fn test_close_with_empty_verify_still_closes() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with empty verify");
        bean.verify = Some("".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be closed (empty verify treated as no-verify)
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
        assert_eq!(updated.attempts, 0); // No attempts recorded
    }

    #[test]
    fn test_close_with_whitespace_verify_still_closes() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with whitespace verify");
        bean.verify = Some("   ".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_shell_operators_work() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with shell operators");
        // Shell operators like && should work in verify commands
        bean.verify = Some("true && true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived after passing verify
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_pipe_propagates_exit_code() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with pipe");
        // Pipe exit code is determined by last command: false returns 1
        bean.verify = Some("true | false".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        let _ = cmd_close(&beans_dir, vec!["1".to_string()], None, false);

        // Verify fails because `false` returns exit code 1
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open); // Not closed
        assert_eq!(updated.attempts, 1); // Attempts incremented
    }

    // =====================================================================
    // Pre-Close Hook Tests
    // =====================================================================

    #[test]
    fn test_close_with_passing_pre_close_hook() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hooks execute - pass project root, not .beans dir
        crate::hooks::create_trust(project_root).unwrap();

        // Create a pre-close hook that passes (exits 0)
        let hook_path = hooks_dir.join("pre-close");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = Bean::new("1", "Task with passing hook");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close should succeed
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_failing_pre_close_hook_blocks_close() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hooks execute - pass project root, not .beans dir
        crate::hooks::create_trust(project_root).unwrap();

        // Create a pre-close hook that fails (exits 1)
        let hook_path = hooks_dir.join("pre-close");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = Bean::new("1", "Task with failing hook");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close should still succeed (returns Ok), but bean not closed
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should NOT be archived or closed
        let not_archived = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(not_archived.is_ok());
        let updated = Bean::from_file(not_archived.unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert!(!updated.is_archived);
    }

    #[test]
    fn test_close_batch_with_mixed_hook_results() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hooks execute - pass project root, not .beans dir
        crate::hooks::create_trust(project_root).unwrap();

        // Create a pre-close hook that passes
        let hook_path = hooks_dir.join("pre-close");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Create three beans
        let bean1 = Bean::new("1", "Task 1 - will close");
        let bean2 = Bean::new("2", "Task 2 - will close");
        let bean3 = Bean::new("3", "Task 3 - will close");
        let slug1 = title_to_slug(&bean1.title);
        let slug2 = title_to_slug(&bean2.title);
        let slug3 = title_to_slug(&bean3.title);
        bean1
            .to_file(beans_dir.join(format!("1-{}.md", slug1)))
            .unwrap();
        bean2
            .to_file(beans_dir.join(format!("2-{}.md", slug2)))
            .unwrap();
        bean3
            .to_file(beans_dir.join(format!("3-{}.md", slug3)))
            .unwrap();

        // Close all three (hook passes for all)
        cmd_close(
            &beans_dir,
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            None,
            false,
        )
        .unwrap();

        // All should be archived
        for id in &["1", "2", "3"] {
            let archived = crate::discovery::find_archived_bean(&beans_dir, id).unwrap();
            let bean = Bean::from_file(&archived).unwrap();
            assert_eq!(bean.status, Status::Closed);
            assert!(bean.is_archived);
        }
    }

    #[test]
    fn test_close_with_untrusted_hooks_silently_skips() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // DO NOT enable trust - hooks should not execute

        // Create a pre-close hook that would fail if executed
        let hook_path = hooks_dir.join("pre-close");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = Bean::new("1", "Task with untrusted hook");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close should succeed (hooks are untrusted so they're skipped)
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_missing_hook_silently_succeeds() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();

        // Enable trust but don't create hook - pass project root, not .beans dir
        crate::hooks::create_trust(project_root).unwrap();

        let bean = Bean::new("1", "Task with missing hook");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close should succeed (missing hooks silently pass)
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_passes_reason_to_pre_close_hook() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust - pass project root, not .beans dir
        crate::hooks::create_trust(project_root).unwrap();

        // Create a simple passing hook
        let hook_path = hooks_dir.join("pre-close");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = Bean::new("1", "Task with reason");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close with a reason
        cmd_close(
            &beans_dir,
            vec!["1".to_string()],
            Some("Completed".to_string()),
            false,
        )
        .unwrap();

        // Verify bean is closed with reason
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert_eq!(updated.close_reason, Some("Completed".to_string()));
    }

    #[test]
    fn test_close_batch_partial_rejection_by_hook() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust - pass project root, not .beans dir
        crate::hooks::create_trust(project_root).unwrap();

        // Create a hook that checks bean ID - reject ID 2
        // Use dd with timeout to consume stdin and check content
        let hook_path = hooks_dir.join("pre-close");
        fs::write(&hook_path, "#!/bin/bash\ntimeout 5 dd bs=1M 2>/dev/null | grep -q '\"id\":\"2\"' && exit 1 || exit 0").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Create three beans
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2 - will be rejected");
        let bean3 = Bean::new("3", "Task 3");
        let slug1 = title_to_slug(&bean1.title);
        let slug2 = title_to_slug(&bean2.title);
        let slug3 = title_to_slug(&bean3.title);
        bean1
            .to_file(beans_dir.join(format!("1-{}.md", slug1)))
            .unwrap();
        bean2
            .to_file(beans_dir.join(format!("2-{}.md", slug2)))
            .unwrap();
        bean3
            .to_file(beans_dir.join(format!("3-{}.md", slug3)))
            .unwrap();

        // Try to close all three
        cmd_close(
            &beans_dir,
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            None,
            false,
        )
        .unwrap();

        // Bean 1 should be archived
        let archived1 = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(archived1.is_ok());
        let bean1_result = Bean::from_file(archived1.unwrap()).unwrap();
        assert_eq!(bean1_result.status, Status::Closed);

        // Bean 2 should NOT be archived (rejected by hook)
        let open2 = crate::discovery::find_bean_file(&beans_dir, "2");
        assert!(open2.is_ok());
        let bean2_result = Bean::from_file(open2.unwrap()).unwrap();
        assert_eq!(bean2_result.status, Status::Open);

        // Bean 3 should be archived
        let archived3 = crate::discovery::find_archived_bean(&beans_dir, "3");
        assert!(archived3.is_ok());
        let bean3_result = Bean::from_file(archived3.unwrap()).unwrap();
        assert_eq!(bean3_result.status, Status::Closed);
    }

    // =====================================================================
    // Post-Close Hook Tests
    // =====================================================================

    #[test]
    fn test_post_close_hook_fires_after_successful_close() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust
        crate::hooks::create_trust(project_root).unwrap();

        // Create a post-close hook that writes a marker file
        let marker = project_root.join("post-close-fired");
        let hook_path = hooks_dir.join("post-close");
        fs::write(
            &hook_path,
            format!("#!/bin/bash\ntouch {}\nexit 0", marker.display()),
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = Bean::new("1", "Task with post-close hook");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Marker file should exist, proving the post-close hook fired
        assert!(marker.exists(), "post-close hook should have fired");

        // Bean should still be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn test_post_close_hook_failure_does_not_prevent_close() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust
        crate::hooks::create_trust(project_root).unwrap();

        // Create a post-close hook that FAILS (exits 1)
        let hook_path = hooks_dir.join("post-close");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = Bean::new("1", "Task with failing post-close hook");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close should succeed even though post-close hook fails
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should still be archived and closed
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    // =====================================================================
    // Auto-Close Parent Tests
    // =====================================================================

    fn setup_test_beans_dir_with_config() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create config with auto_close_parent enabled
        let config = crate::config::Config {
            project: "test".to_string(),
            next_id: 100,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
            on_close: None,
            on_fail: None,
            post_plan: None,
            verify_timeout: None,
            review: None,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn test_auto_close_parent_when_all_children_closed() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        // Create child beans
        let mut child1 = Bean::new("1.1", "Child 1");
        child1.parent = Some("1".to_string());
        let child1_slug = title_to_slug(&child1.title);
        child1
            .to_file(beans_dir.join(format!("1.1-{}.md", child1_slug)))
            .unwrap();

        let mut child2 = Bean::new("1.2", "Child 2");
        child2.parent = Some("1".to_string());
        let child2_slug = title_to_slug(&child2.title);
        child2
            .to_file(beans_dir.join(format!("1.2-{}.md", child2_slug)))
            .unwrap();

        // Close first child - parent should NOT auto-close yet
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Parent should still be open
        let parent_still_open = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(parent_still_open.is_ok());
        let parent_bean = Bean::from_file(parent_still_open.unwrap()).unwrap();
        assert_eq!(parent_bean.status, Status::Open);

        // Close second child - parent should auto-close now
        cmd_close(&beans_dir, vec!["1.2".to_string()], None, false).unwrap();

        // Parent should now be archived
        let parent_archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(parent_archived.is_ok(), "Parent should be auto-archived");
        let parent_result = Bean::from_file(parent_archived.unwrap()).unwrap();
        assert_eq!(parent_result.status, Status::Closed);
        assert!(parent_result
            .close_reason
            .as_ref()
            .unwrap()
            .contains("Auto-closed"));
    }

    #[test]
    fn test_no_auto_close_when_children_still_open() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        // Create child beans
        let mut child1 = Bean::new("1.1", "Child 1");
        child1.parent = Some("1".to_string());
        let child1_slug = title_to_slug(&child1.title);
        child1
            .to_file(beans_dir.join(format!("1.1-{}.md", child1_slug)))
            .unwrap();

        let mut child2 = Bean::new("1.2", "Child 2");
        child2.parent = Some("1".to_string());
        let child2_slug = title_to_slug(&child2.title);
        child2
            .to_file(beans_dir.join(format!("1.2-{}.md", child2_slug)))
            .unwrap();

        // Close first child only
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Parent should still be open (not all children closed)
        let parent_still_open = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(parent_still_open.is_ok());
        let parent_bean = Bean::from_file(parent_still_open.unwrap()).unwrap();
        assert_eq!(parent_bean.status, Status::Open);
    }

    #[test]
    fn test_auto_close_disabled_via_config() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create config with auto_close_parent DISABLED
        let config = crate::config::Config {
            project: "test".to_string(),
            next_id: 100,
            auto_close_parent: false,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
            on_close: None,
            on_fail: None,
            post_plan: None,
            verify_timeout: None,
            review: None,
        };
        config.save(&beans_dir).unwrap();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        // Create single child bean
        let mut child = Bean::new("1.1", "Only Child");
        child.parent = Some("1".to_string());
        let child_slug = title_to_slug(&child.title);
        child
            .to_file(beans_dir.join(format!("1.1-{}.md", child_slug)))
            .unwrap();

        // Close the child
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Parent should still be open (auto-close disabled)
        let parent_still_open = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(parent_still_open.is_ok());
        let parent_bean = Bean::from_file(parent_still_open.unwrap()).unwrap();
        assert_eq!(parent_bean.status, Status::Open);
    }

    #[test]
    fn test_auto_close_recursive_grandparent() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create grandparent bean
        let grandparent = Bean::new("1", "Grandparent");
        let gp_slug = title_to_slug(&grandparent.title);
        grandparent
            .to_file(beans_dir.join(format!("1-{}.md", gp_slug)))
            .unwrap();

        // Create parent bean (child of grandparent)
        let mut parent = Bean::new("1.1", "Parent");
        parent.parent = Some("1".to_string());
        let p_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1.1-{}.md", p_slug)))
            .unwrap();

        // Create grandchild bean (child of parent)
        let mut grandchild = Bean::new("1.1.1", "Grandchild");
        grandchild.parent = Some("1.1".to_string());
        let gc_slug = title_to_slug(&grandchild.title);
        grandchild
            .to_file(beans_dir.join(format!("1.1.1-{}.md", gc_slug)))
            .unwrap();

        // Close the grandchild - should cascade up
        cmd_close(&beans_dir, vec!["1.1.1".to_string()], None, false).unwrap();

        // All three should be archived
        let gc_archived = crate::discovery::find_archived_bean(&beans_dir, "1.1.1");
        assert!(gc_archived.is_ok(), "Grandchild should be archived");

        let p_archived = crate::discovery::find_archived_bean(&beans_dir, "1.1");
        assert!(p_archived.is_ok(), "Parent should be auto-archived");

        let gp_archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(gp_archived.is_ok(), "Grandparent should be auto-archived");

        // Check auto-close reasons
        let p_bean = Bean::from_file(p_archived.unwrap()).unwrap();
        assert!(p_bean
            .close_reason
            .as_ref()
            .unwrap()
            .contains("Auto-closed"));

        let gp_bean = Bean::from_file(gp_archived.unwrap()).unwrap();
        assert!(gp_bean
            .close_reason
            .as_ref()
            .unwrap()
            .contains("Auto-closed"));
    }

    #[test]
    fn test_auto_close_with_no_parent() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create a standalone bean (no parent)
        let bean = Bean::new("1", "Standalone Task");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Close the bean - should work fine with no parent
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(archived.is_ok());
        let bean_result = Bean::from_file(archived.unwrap()).unwrap();
        assert_eq!(bean_result.status, Status::Closed);
    }

    #[test]
    fn test_all_children_closed_checks_archived_beans() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        // Create two child beans
        let mut child1 = Bean::new("1.1", "Child 1");
        child1.parent = Some("1".to_string());
        let child1_slug = title_to_slug(&child1.title);
        child1
            .to_file(beans_dir.join(format!("1.1-{}.md", child1_slug)))
            .unwrap();

        let mut child2 = Bean::new("1.2", "Child 2");
        child2.parent = Some("1".to_string());
        let child2_slug = title_to_slug(&child2.title);
        child2
            .to_file(beans_dir.join(format!("1.2-{}.md", child2_slug)))
            .unwrap();

        // Close first child (will be archived)
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Verify child1 is archived
        let child1_archived = crate::discovery::find_archived_bean(&beans_dir, "1.1");
        assert!(child1_archived.is_ok(), "Child 1 should be archived");

        // Now close child2 - parent should auto-close even though child1 is in archive
        cmd_close(&beans_dir, vec!["1.2".to_string()], None, false).unwrap();

        // Parent should be archived
        let parent_archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(
            parent_archived.is_ok(),
            "Parent should be auto-archived when all children (including archived) are closed"
        );
    }

    // =====================================================================
    // Truncation Helper Tests
    // =====================================================================

    #[test]
    fn test_truncate_output_short() {
        let output = "line1\nline2\nline3";
        let result = truncate_output(output, 50);
        assert_eq!(result, output); // No truncation needed
    }

    #[test]
    fn test_truncate_output_exact_boundary() {
        // Exactly 100 lines (50*2), should not truncate
        let lines: Vec<String> = (1..=100).map(|i| format!("line{}", i)).collect();
        let output = lines.join("\n");
        let result = truncate_output(&output, 50);
        assert_eq!(result, output);
    }

    #[test]
    fn test_truncate_output_long() {
        // 150 lines, should truncate to first 50 + last 50
        let lines: Vec<String> = (1..=150).map(|i| format!("line{}", i)).collect();
        let output = lines.join("\n");
        let result = truncate_output(&output, 50);

        assert!(result.contains("line1"));
        assert!(result.contains("line50"));
        assert!(!result.contains("line51"));
        assert!(!result.contains("line100"));
        assert!(result.contains("line101"));
        assert!(result.contains("line150"));
        assert!(result.contains("(50 lines omitted)"));
    }

    #[test]
    fn test_truncate_to_char_boundary_ascii() {
        let s = "hello world";
        assert_eq!(truncate_to_char_boundary(s, 5), 5);
        assert_eq!(&s[..truncate_to_char_boundary(s, 5)], "hello");
    }

    #[test]
    fn test_truncate_to_char_boundary_multibyte() {
        // Each emoji is 4 bytes: "😀😁😂" = 12 bytes
        let s = "😀😁😂";
        assert_eq!(s.len(), 12);

        // Truncating at byte 5 (mid-codepoint) should back up to byte 4
        assert_eq!(truncate_to_char_boundary(s, 5), 4);
        assert_eq!(&s[..truncate_to_char_boundary(s, 5)], "😀");

        // Truncating at byte 8 (exact boundary) should stay at 8
        assert_eq!(truncate_to_char_boundary(s, 8), 8);
        assert_eq!(&s[..truncate_to_char_boundary(s, 8)], "😀😁");
    }

    #[test]
    fn test_truncate_to_char_boundary_beyond_len() {
        let s = "short";
        assert_eq!(truncate_to_char_boundary(s, 100), 5);
    }

    #[test]
    fn test_truncate_to_char_boundary_zero() {
        let s = "hello";
        assert_eq!(truncate_to_char_boundary(s, 0), 0);
    }

    #[test]
    fn test_format_failure_note() {
        let note = format_failure_note(1, Some(1), "error message");

        assert!(note.contains("## Attempt 1"));
        assert!(note.contains("Exit code: 1"));
        assert!(note.contains("error message"));
        assert!(note.contains("```")); // Fenced code block
    }

    // =====================================================================
    // on_close Action Tests
    // =====================================================================

    #[test]
    fn on_close_run_action_executes_command() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        crate::hooks::create_trust(project_root).unwrap();
        let marker = project_root.join("on_close_ran");

        let mut bean = Bean::new("1", "Task with on_close run");
        bean.on_close = vec![OnCloseAction::Run {
            command: format!("touch {}", marker.display()),
        }];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        assert!(marker.exists(), "on_close run command should have executed");
    }

    #[test]
    fn on_close_notify_action_prints_message() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        let mut bean = Bean::new("1", "Task with on_close notify");
        bean.on_close = vec![OnCloseAction::Notify {
            message: "All done!".to_string(),
        }];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Should not error — notify just prints
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should still be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
    }

    #[test]
    fn on_close_run_failure_does_not_prevent_close() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        crate::hooks::create_trust(project_root).unwrap();

        let mut bean = Bean::new("1", "Task with failing on_close");
        bean.on_close = vec![OnCloseAction::Run {
            command: "false".to_string(), // exits 1
        }];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should still be archived despite on_close failure
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn on_close_multiple_actions_all_run() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        crate::hooks::create_trust(project_root).unwrap();
        let marker1 = project_root.join("on_close_1");
        let marker2 = project_root.join("on_close_2");

        let mut bean = Bean::new("1", "Task with multiple on_close");
        bean.on_close = vec![
            OnCloseAction::Run {
                command: format!("touch {}", marker1.display()),
            },
            OnCloseAction::Notify {
                message: "Between actions".to_string(),
            },
            OnCloseAction::Run {
                command: format!("touch {}", marker2.display()),
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        assert!(marker1.exists(), "First on_close run should have executed");
        assert!(marker2.exists(), "Second on_close run should have executed");
    }

    #[test]
    fn on_close_run_skipped_without_trust() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        // DO NOT enable trust — on_close Run should be skipped
        let marker = project_root.join("on_close_should_not_exist");

        let mut bean = Bean::new("1", "Task with untrusted on_close");
        bean.on_close = vec![OnCloseAction::Run {
            command: format!("touch {}", marker.display()),
        }];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Command should NOT have executed (no trust)
        assert!(
            !marker.exists(),
            "on_close run should be skipped without trust"
        );

        // Bean should still be archived (on_close skip doesn't block close)
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    #[test]
    fn on_close_runs_in_project_root() {
        let (dir, beans_dir) = setup_test_beans_dir();
        let project_root = dir.path();
        crate::hooks::create_trust(project_root).unwrap();

        let mut bean = Bean::new("1", "Task with pwd check");
        // Write the working directory to a file so we can verify it
        let pwd_file = project_root.join("on_close_pwd");
        bean.on_close = vec![OnCloseAction::Run {
            command: format!("pwd > {}", pwd_file.display()),
        }];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let pwd_output = fs::read_to_string(&pwd_file).unwrap();
        // Resolve symlinks for macOS /private/var/... vs /var/...
        let expected = std::fs::canonicalize(project_root).unwrap();
        let actual = std::fs::canonicalize(pwd_output.trim()).unwrap();
        assert_eq!(actual, expected);
    }

    // =====================================================================
    // History Recording Tests
    // =====================================================================

    #[test]
    fn history_failure_creates_run_record() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with failing verify");
        bean.verify = Some("echo 'some error' && exit 1".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.history.len(), 1);
        let record = &updated.history[0];
        assert_eq!(record.result, RunResult::Fail);
        assert_eq!(record.attempt, 1);
        assert_eq!(record.exit_code, Some(1));
        assert!(record.output_snippet.is_some());
        assert!(record
            .output_snippet
            .as_ref()
            .unwrap()
            .contains("some error"));
    }

    #[test]
    fn history_success_creates_run_record() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with passing verify");
        bean.verify = Some("true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.history.len(), 1);
        let record = &updated.history[0];
        assert_eq!(record.result, RunResult::Pass);
        assert_eq!(record.attempt, 1);
        assert!(record.output_snippet.is_none());
    }

    #[test]
    fn history_has_correct_duration() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with timed verify");
        // sleep 0.1 to ensure measurable duration
        bean.verify = Some("sleep 0.1 && true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.history.len(), 1);
        let record = &updated.history[0];
        assert!(record.finished_at.is_some());
        assert!(record.duration_secs.is_some());
        let dur = record.duration_secs.unwrap();
        assert!(dur >= 0.05, "Duration should be >= 0.05s, got {}", dur);
        // Verify finished_at > started_at
        assert!(record.finished_at.unwrap() >= record.started_at);
    }

    #[test]
    fn history_records_exit_code() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with exit code 42");
        bean.verify = Some("exit 42".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.history.len(), 1);
        assert_eq!(updated.history[0].exit_code, Some(42));
        assert_eq!(updated.history[0].result, RunResult::Fail);
    }

    #[test]
    fn history_multiple_attempts_accumulate() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with multiple failures");
        bean.verify = Some("false".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // Three failed attempts
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.history.len(), 3);
        assert_eq!(updated.history[0].attempt, 1);
        assert_eq!(updated.history[1].attempt, 2);
        assert_eq!(updated.history[2].attempt, 3);
        for record in &updated.history {
            assert_eq!(record.result, RunResult::Fail);
        }
    }

    #[test]
    fn history_agent_from_env_var() {
        // Set env var before close, then verify it's captured
        // NOTE: env var tests are inherently racy with parallel execution,
        // but set_var + close + remove_var in sequence is the best we can do.
        std::env::set_var("BEANS_AGENT", "test-agent-42");

        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with agent env");
        bean.verify = Some("true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Clean up env var immediately
        std::env::remove_var("BEANS_AGENT");

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.history.len(), 1);
        assert_eq!(updated.history[0].agent, Some("test-agent-42".to_string()));
    }

    #[test]
    fn history_no_record_without_verify() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task without verify");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert!(
            updated.history.is_empty(),
            "No history when no verify command"
        );
    }

    #[test]
    fn history_no_record_when_force_skip() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task force closed");
        bean.verify = Some("false".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, true).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert!(
            updated.history.is_empty(),
            "No history when verify skipped with --force"
        );
    }

    #[test]
    fn history_failure_then_success_accumulates() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task that eventually passes");
        bean.verify = Some("false".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // First attempt fails
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Change verify to pass
        let mut updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        updated.verify = Some("true".to_string());
        updated
            .to_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap())
            .unwrap();

        // Second attempt succeeds
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let final_bean = Bean::from_file(&archived).unwrap();
        assert_eq!(final_bean.history.len(), 2);
        assert_eq!(final_bean.history[0].result, RunResult::Fail);
        assert_eq!(final_bean.history[0].attempt, 1);
        assert_eq!(final_bean.history[1].result, RunResult::Pass);
        assert_eq!(final_bean.history[1].attempt, 2);
    }

    // =====================================================================
    // on_fail Action Tests
    // =====================================================================

    #[test]
    fn on_fail_retry_releases_claim_when_under_max() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with retry on_fail");
        bean.verify = Some("false".to_string());
        bean.on_fail = Some(OnFailAction::Retry {
            max: Some(5),
            delay_secs: None,
        });
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.attempts, 1);
        // Claim should be released for retry
        assert!(updated.claimed_by.is_none());
        assert!(updated.claimed_at.is_none());
    }

    #[test]
    fn on_fail_retry_keeps_claim_when_at_max() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task exhausted retries");
        bean.verify = Some("false".to_string());
        bean.on_fail = Some(OnFailAction::Retry {
            max: Some(2),
            delay_secs: None,
        });
        bean.attempts = 1; // Next failure will be attempt 2 == max
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 2);
        // Claim should NOT be released (max exhausted)
        assert_eq!(updated.claimed_by, Some("agent-1".to_string()));
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn on_fail_retry_max_defaults_to_max_attempts() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with default max");
        bean.verify = Some("false".to_string());
        bean.max_attempts = 3;
        bean.on_fail = Some(OnFailAction::Retry {
            max: None, // Should default to bean.max_attempts (3)
            delay_secs: None,
        });
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        // First attempt (1 < 3) — should release
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 1);
        assert!(updated.claimed_by.is_none());

        // Re-claim and fail again (2 < 3) — should release
        let mut bean2 =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        bean2.claimed_by = Some("agent-2".to_string());
        bean2.claimed_at = Some(Utc::now());
        bean2
            .to_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap())
            .unwrap();
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 2);
        assert!(updated.claimed_by.is_none());

        // Re-claim and fail again (3 >= 3) — should NOT release
        let mut bean3 =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        bean3.claimed_by = Some("agent-3".to_string());
        bean3.claimed_at = Some(Utc::now());
        bean3
            .to_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap())
            .unwrap();
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 3);
        assert_eq!(updated.claimed_by, Some("agent-3".to_string()));
    }

    #[test]
    fn on_fail_retry_with_delay_releases_claim() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with delay");
        bean.verify = Some("false".to_string());
        bean.on_fail = Some(OnFailAction::Retry {
            max: Some(3),
            delay_secs: Some(30),
        });
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 1);
        // Claim released even with delay (delay is enforced by orchestrator)
        assert!(updated.claimed_by.is_none());
        assert!(updated.claimed_at.is_none());
    }

    #[test]
    fn on_fail_escalate_updates_priority() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task to escalate");
        bean.verify = Some("false".to_string());
        bean.priority = 2;
        bean.on_fail = Some(OnFailAction::Escalate {
            priority: Some(0),
            message: None,
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.priority, 0);
        assert!(updated.labels.contains(&"escalated".to_string()));
    }

    #[test]
    fn on_fail_escalate_appends_message_to_notes() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with escalation message");
        bean.verify = Some("false".to_string());
        bean.notes = Some("Existing notes".to_string());
        bean.on_fail = Some(OnFailAction::Escalate {
            priority: None,
            message: Some("Needs human review".to_string()),
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        let notes = updated.notes.unwrap();
        assert!(notes.contains("Existing notes"));
        assert!(notes.contains("## Escalated"));
        assert!(notes.contains("Needs human review"));
        assert!(updated.labels.contains(&"escalated".to_string()));
    }

    #[test]
    fn on_fail_escalate_adds_label() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task to label");
        bean.verify = Some("false".to_string());
        bean.on_fail = Some(OnFailAction::Escalate {
            priority: None,
            message: None,
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert!(updated.labels.contains(&"escalated".to_string()));
    }

    #[test]
    fn on_fail_escalate_no_duplicate_label() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task already escalated");
        bean.verify = Some("false".to_string());
        bean.labels = vec!["escalated".to_string()];
        bean.on_fail = Some(OnFailAction::Escalate {
            priority: None,
            message: None,
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        let count = updated
            .labels
            .iter()
            .filter(|l| l.as_str() == "escalated")
            .count();
        assert_eq!(count, 1, "Should not duplicate 'escalated' label");
    }

    #[test]
    fn on_fail_none_existing_behavior_unchanged() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with no on_fail");
        bean.verify = Some("false".to_string());
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        // on_fail is None by default
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.attempts, 1);
        // Claim should remain (no on_fail to release it)
        assert_eq!(updated.claimed_by, Some("agent-1".to_string()));
        assert!(updated.labels.is_empty());
    }

    // =====================================================================
    // Output Capture Tests
    // =====================================================================

    #[test]
    fn output_capture_json_stdout_stored_as_outputs() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with JSON output");
        bean.verify = Some(r#"echo '{"passed":42,"failed":0}'"#.to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        let outputs = updated.outputs.expect("outputs should be set");
        assert_eq!(outputs["passed"], 42);
        assert_eq!(outputs["failed"], 0);
    }

    #[test]
    fn output_capture_non_json_stdout_stored_as_text() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with plain text output");
        bean.verify = Some("echo 'hello world'".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        let outputs = updated.outputs.expect("outputs should be set");
        assert_eq!(outputs["text"], "hello world");
    }

    #[test]
    fn output_capture_empty_stdout_no_outputs() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with no stdout");
        bean.verify = Some("true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert!(
            updated.outputs.is_none(),
            "empty stdout should not set outputs"
        );
    }

    #[test]
    fn output_capture_large_stdout_truncated() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with large output");
        // Generate >64KB of stdout using printf (faster than many echos)
        bean.verify = Some("python3 -c \"print('x' * 70000)\"".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        let outputs = updated
            .outputs
            .expect("outputs should be set for large output");
        assert_eq!(outputs["truncated"], true);
        assert!(outputs["original_bytes"].as_u64().unwrap() > 64 * 1024);
        // The text should be truncated to 64KB
        let text = outputs["text"].as_str().unwrap();
        assert!(text.len() <= 64 * 1024);
    }

    #[test]
    fn output_capture_stderr_not_captured_as_outputs() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with stderr only");
        // Write to stderr only, nothing to stdout
        bean.verify = Some("echo 'error info' >&2".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert!(
            updated.outputs.is_none(),
            "stderr-only output should not set outputs"
        );
    }

    #[test]
    fn output_capture_failure_unchanged() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task that fails with output");
        bean.verify = Some(r#"echo '{"result":"data"}' && exit 1"#.to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert!(
            updated.outputs.is_none(),
            "failed verify should not capture outputs"
        );
    }

    #[test]
    fn output_capture_json_array() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with JSON array output");
        bean.verify = Some(r#"echo '["a","b","c"]'"#.to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        let outputs = updated.outputs.expect("outputs should be set");
        let arr = outputs.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], "a");
    }

    #[test]
    fn output_capture_mixed_stdout_stderr() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with mixed output");
        // stdout has JSON, stderr has logs — only stdout should be captured
        bean.verify = Some(r#"echo '{"key":"value"}' && echo 'debug log' >&2"#.to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        let outputs = updated.outputs.expect("outputs should capture stdout only");
        assert_eq!(outputs["key"], "value");
        // stderr content should NOT be in outputs
        assert!(
            outputs.get("text").is_none()
                || !outputs["text"].as_str().unwrap_or("").contains("debug log")
        );
    }

    // =====================================================================
    // Circuit Breaker (max_loops) Tests
    // =====================================================================

    /// Helper: set up beans dir with config specifying max_loops.
    fn setup_beans_dir_with_max_loops(max_loops: u32) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let config = crate::config::Config {
            project: "test".to_string(),
            next_id: 100,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
            on_close: None,
            on_fail: None,
            post_plan: None,
            verify_timeout: None,
            review: None,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn max_loops_circuit_breaker_triggers_at_limit() {
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(3);

        // Create parent and child beans
        let parent = Bean::new("1", "Parent");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        let mut child1 = Bean::new("1.1", "Child with attempts");
        child1.parent = Some("1".to_string());
        child1.verify = Some("false".to_string());
        child1.attempts = 2; // Already has 2 attempts
        let child1_slug = title_to_slug(&child1.title);
        child1
            .to_file(beans_dir.join(format!("1.1-{}.md", child1_slug)))
            .unwrap();

        // Close child1 → attempts becomes 3, subtree total = 0+3 = 3 >= 3
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1.1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.attempts, 3);
        assert!(
            updated.labels.contains(&"circuit-breaker".to_string()),
            "Circuit breaker label should be added"
        );
        assert_eq!(updated.priority, 0, "Priority should be escalated to P0");
    }

    #[test]
    fn max_loops_circuit_breaker_does_not_trigger_below_limit() {
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(5);

        let parent = Bean::new("1", "Parent");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        let mut child = Bean::new("1.1", "Child");
        child.parent = Some("1".to_string());
        child.verify = Some("false".to_string());
        child.attempts = 1; // After fail: 2, subtree = 0+2 = 2 < 5
        let child_slug = title_to_slug(&child.title);
        child
            .to_file(beans_dir.join(format!("1.1-{}.md", child_slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1.1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 2);
        assert!(
            !updated.labels.contains(&"circuit-breaker".to_string()),
            "Circuit breaker should NOT trigger below limit"
        );
        assert_ne!(updated.priority, 0, "Priority should not change");
    }

    #[test]
    fn max_loops_zero_disables_circuit_breaker() {
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(0);

        let mut bean = Bean::new("1", "Unlimited retries");
        bean.verify = Some("false".to_string());
        bean.attempts = 100; // Many attempts — should not trip if max_loops=0
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 101);
        assert!(
            !updated.labels.contains(&"circuit-breaker".to_string()),
            "Circuit breaker should not trigger when max_loops=0"
        );
    }

    #[test]
    fn max_loops_per_bean_overrides_config() {
        // Config has max_loops=100 (high), but root bean has max_loops=3 (low)
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(100);

        let mut parent = Bean::new("1", "Parent with low max_loops");
        parent.max_loops = Some(3); // Override: only 3 allowed
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        let mut child = Bean::new("1.1", "Child");
        child.parent = Some("1".to_string());
        child.verify = Some("false".to_string());
        child.attempts = 2; // After fail: 3, subtree = 0+3 = 3 >= 3
        let child_slug = title_to_slug(&child.title);
        child
            .to_file(beans_dir.join(format!("1.1-{}.md", child_slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1.1").unwrap()).unwrap();
        assert!(
            updated.labels.contains(&"circuit-breaker".to_string()),
            "Per-bean max_loops should override config"
        );
        assert_eq!(updated.priority, 0);
    }

    #[test]
    fn max_loops_circuit_breaker_skips_on_fail_retry() {
        // Circuit breaker should prevent on_fail retry from releasing the claim
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(2);

        let mut bean = Bean::new("1", "Bean with retry that should be blocked");
        bean.verify = Some("false".to_string());
        bean.attempts = 1; // After fail: 2 >= max_loops=2 → circuit breaker
        bean.on_fail = Some(OnFailAction::Retry {
            max: Some(10),
            delay_secs: None,
        });
        bean.claimed_by = Some("agent-1".to_string());
        bean.claimed_at = Some(Utc::now());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        // Circuit breaker should have tripped, preventing on_fail retry
        assert!(updated.labels.contains(&"circuit-breaker".to_string()));
        assert_eq!(updated.priority, 0);
        // Claim should NOT be released (circuit breaker bypasses on_fail)
        assert_eq!(
            updated.claimed_by,
            Some("agent-1".to_string()),
            "on_fail retry should not release claim when circuit breaker trips"
        );
    }

    #[test]
    fn max_loops_counts_across_siblings() {
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(5);

        let parent = Bean::new("1", "Parent");
        let parent_slug = title_to_slug(&parent.title);
        parent
            .to_file(beans_dir.join(format!("1-{}.md", parent_slug)))
            .unwrap();

        // Sibling 1.1 already has 2 attempts
        let mut sibling = Bean::new("1.1", "Sibling");
        sibling.parent = Some("1".to_string());
        sibling.attempts = 2;
        let sib_slug = title_to_slug(&sibling.title);
        sibling
            .to_file(beans_dir.join(format!("1.1-{}.md", sib_slug)))
            .unwrap();

        // Child 1.2 has 2 attempts, will increment to 3
        // subtree total = 0 + 2 + 3 = 5 >= 5
        let mut child = Bean::new("1.2", "Child");
        child.parent = Some("1".to_string());
        child.verify = Some("false".to_string());
        child.attempts = 2;
        let child_slug = title_to_slug(&child.title);
        child
            .to_file(beans_dir.join(format!("1.2-{}.md", child_slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1.2".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1.2").unwrap()).unwrap();
        assert!(
            updated.labels.contains(&"circuit-breaker".to_string()),
            "Circuit breaker should count sibling attempts"
        );
        assert_eq!(updated.priority, 0);
    }

    #[test]
    fn max_loops_standalone_bean_uses_own_max_loops() {
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(100);

        // Standalone bean (no parent) with its own max_loops
        let mut bean = Bean::new("1", "Standalone");
        bean.verify = Some("false".to_string());
        bean.max_loops = Some(2);
        bean.attempts = 1; // After fail: 2, subtree(self) = 2 >= 2
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert!(updated.labels.contains(&"circuit-breaker".to_string()));
        assert_eq!(updated.priority, 0);
    }

    #[test]
    fn max_loops_no_config_defaults_to_10() {
        // No config file — should default max_loops to 10
        let (_dir, beans_dir) = setup_test_beans_dir();

        let mut bean = Bean::new("1", "No config");
        bean.verify = Some("false".to_string());
        bean.attempts = 9; // After fail: 10, subtree = 10 >= default 10
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert!(
            updated.labels.contains(&"circuit-breaker".to_string()),
            "Should use default max_loops=10"
        );
    }

    #[test]
    fn max_loops_no_duplicate_label() {
        let (_dir, beans_dir) = setup_beans_dir_with_max_loops(1);

        let mut bean = Bean::new("1", "Already has label");
        bean.verify = Some("false".to_string());
        bean.labels = vec!["circuit-breaker".to_string()];
        bean.attempts = 0; // After fail: 1 >= 1
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        let count = updated
            .labels
            .iter()
            .filter(|l| l.as_str() == "circuit-breaker")
            .count();
        assert_eq!(count, 1, "Should not duplicate 'circuit-breaker' label");
    }

    // =====================================================================
    // Close Failed Tests
    // =====================================================================

    #[test]
    fn test_close_failed_marks_attempt_as_failed() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.claimed_by = Some("agent-1".to_string());
        // Simulate a claim-started attempt
        bean.attempt_log.push(crate::bean::AttemptRecord {
            num: 1,
            outcome: crate::bean::AttemptOutcome::Abandoned,
            notes: None,
            agent: Some("agent-1".to_string()),
            started_at: Some(Utc::now()),
            finished_at: None,
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close_failed(
            &beans_dir,
            vec!["1".to_string()],
            Some("blocked by upstream".to_string()),
        )
        .unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert!(updated.claimed_by.is_none());
        assert_eq!(updated.attempt_log.len(), 1);
        assert_eq!(
            updated.attempt_log[0].outcome,
            crate::bean::AttemptOutcome::Failed
        );
        assert!(updated.attempt_log[0].finished_at.is_some());
        assert_eq!(
            updated.attempt_log[0].notes.as_deref(),
            Some("blocked by upstream")
        );
    }

    #[test]
    fn test_close_failed_appends_to_notes() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.attempt_log.push(crate::bean::AttemptRecord {
            num: 1,
            outcome: crate::bean::AttemptOutcome::Abandoned,
            notes: None,
            agent: None,
            started_at: Some(Utc::now()),
            finished_at: None,
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close_failed(
            &beans_dir,
            vec!["1".to_string()],
            Some("JWT incompatible".to_string()),
        )
        .unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert!(updated.notes.is_some());
        assert!(updated.notes.unwrap().contains("JWT incompatible"));
    }

    #[test]
    fn test_close_failed_without_reason() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.attempt_log.push(crate::bean::AttemptRecord {
            num: 1,
            outcome: crate::bean::AttemptOutcome::Abandoned,
            notes: None,
            agent: None,
            started_at: Some(Utc::now()),
            finished_at: None,
        });
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close_failed(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(
            updated.attempt_log[0].outcome,
            crate::bean::AttemptOutcome::Failed
        );
    }

    // =====================================================================
    // Worktree Merge Integration Tests
    // =====================================================================

    mod worktree_merge {
        use super::*;
        use std::path::PathBuf;
        use std::sync::Mutex;

        /// Serialize worktree tests that change the process-global CWD.
        /// These tests must not run concurrently with each other because they
        /// all call set_current_dir, which is process-global.
        static CWD_LOCK: Mutex<()> = Mutex::new(());

        /// RAII guard that restores the process CWD on drop (even on panic).
        struct CwdGuard(PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        /// Run a git command in the given directory, panicking on failure.
        fn run_git(dir: &Path, args: &[&str]) {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap_or_else(|e| unreachable!("git {:?} failed to execute: {}", args, e));
            assert!(
                output.status.success(),
                "git {:?} in {} failed (exit {:?}):\nstdout: {}\nstderr: {}",
                args,
                dir.display(),
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        /// Create a git repo with a secondary worktree for testing.
        ///
        /// Returns (tempdir, main_repo_dir, worktree_beans_dir).
        /// Main repo is on `main` branch; worktree is on `feature` branch.
        fn setup_git_worktree() -> (TempDir, PathBuf, PathBuf) {
            let dir = TempDir::new().unwrap();
            let base = std::fs::canonicalize(dir.path()).unwrap();
            let main_dir = base.join("main");
            let worktree_dir = base.join("worktree");
            fs::create_dir(&main_dir).unwrap();

            // Initialize git repo on an explicit "main" branch
            run_git(&main_dir, &["init"]);
            run_git(&main_dir, &["config", "user.email", "test@test.com"]);
            run_git(&main_dir, &["config", "user.name", "Test"]);
            run_git(&main_dir, &["checkout", "-b", "main"]);

            // Create an initial commit so the branch exists
            fs::write(main_dir.join("initial.txt"), "initial content").unwrap();
            run_git(&main_dir, &["add", "-A"]);
            run_git(&main_dir, &["commit", "-m", "Initial commit"]);

            // Add .beans/ directory and commit it
            let beans_dir = main_dir.join(".beans");
            fs::create_dir(&beans_dir).unwrap();
            fs::write(beans_dir.join(".gitkeep"), "").unwrap();
            run_git(&main_dir, &["add", "-A"]);
            run_git(&main_dir, &["commit", "-m", "Add .beans directory"]);

            // Create a secondary worktree on a feature branch
            run_git(
                &main_dir,
                &[
                    "worktree",
                    "add",
                    worktree_dir.to_str().unwrap(),
                    "-b",
                    "feature",
                ],
            );

            let worktree_beans_dir = worktree_dir.join(".beans");

            (dir, main_dir, worktree_beans_dir)
        }

        #[test]
        fn test_close_in_worktree_commits_and_merges() {
            let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = CwdGuard(std::env::current_dir().unwrap());

            let (_dir, main_dir, worktree_beans_dir) = setup_git_worktree();
            let worktree_dir = worktree_beans_dir.parent().unwrap();

            // Create a bean in the worktree's .beans/
            let bean = Bean::new("1", "Worktree Task");
            let slug = title_to_slug(&bean.title);
            bean.to_file(worktree_beans_dir.join(format!("1-{}.md", slug)))
                .unwrap();

            // Make a feature change in the worktree
            fs::write(worktree_dir.join("feature.txt"), "feature content").unwrap();

            // Set CWD to worktree so detect_worktree() identifies it
            std::env::set_current_dir(worktree_dir).unwrap();

            // Close the bean — should commit changes, merge to main, and archive
            cmd_close(&worktree_beans_dir, vec!["1".to_string()], None, false).unwrap();

            // Verify: feature changes were merged into the main branch
            assert!(
                main_dir.join("feature.txt").exists(),
                "feature.txt should be merged to main"
            );
            let content = fs::read_to_string(main_dir.join("feature.txt")).unwrap();
            assert_eq!(content, "feature content");
        }

        #[test]
        fn test_close_with_merge_conflict_aborts() {
            let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = CwdGuard(std::env::current_dir().unwrap());

            let (_dir, main_dir, worktree_beans_dir) = setup_git_worktree();
            let worktree_dir = worktree_beans_dir.parent().unwrap();

            // Create a conflicting change on main (modify initial.txt)
            fs::write(main_dir.join("initial.txt"), "main version").unwrap();
            run_git(&main_dir, &["add", "-A"]);
            run_git(&main_dir, &["commit", "-m", "Diverge on main"]);

            // Create a conflicting change in the worktree (same file, different content)
            fs::write(worktree_dir.join("initial.txt"), "feature version").unwrap();

            // Create a bean in the worktree
            let bean = Bean::new("1", "Conflict Task");
            let slug = title_to_slug(&bean.title);
            bean.to_file(worktree_beans_dir.join(format!("1-{}.md", slug)))
                .unwrap();

            // Set CWD to worktree
            std::env::set_current_dir(worktree_dir).unwrap();

            // Close should detect conflict, abort merge, and leave bean open
            cmd_close(&worktree_beans_dir, vec!["1".to_string()], None, false).unwrap();

            // Bean should NOT be closed — merge conflict prevents archiving
            let bean_file = crate::discovery::find_bean_file(&worktree_beans_dir, "1").unwrap();
            let updated = Bean::from_file(&bean_file).unwrap();
            assert_eq!(
                updated.status,
                Status::Open,
                "Bean should remain open when merge conflicts"
            );
        }

        #[test]
        fn test_close_in_main_worktree_skips_merge() {
            let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = CwdGuard(std::env::current_dir().unwrap());

            let dir = TempDir::new().unwrap();
            let base = std::fs::canonicalize(dir.path()).unwrap();
            let repo_dir = base.join("repo");
            fs::create_dir(&repo_dir).unwrap();

            // Initialize a git repo (no secondary worktrees)
            run_git(&repo_dir, &["init"]);
            run_git(&repo_dir, &["config", "user.email", "test@test.com"]);
            run_git(&repo_dir, &["config", "user.name", "Test"]);
            run_git(&repo_dir, &["checkout", "-b", "main"]);

            fs::write(repo_dir.join("file.txt"), "content").unwrap();
            run_git(&repo_dir, &["add", "-A"]);
            run_git(&repo_dir, &["commit", "-m", "Initial commit"]);

            // Create .beans/ and a bean
            let beans_dir = repo_dir.join(".beans");
            fs::create_dir(&beans_dir).unwrap();

            let bean = Bean::new("1", "Main Worktree Task");
            let slug = title_to_slug(&bean.title);
            bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
                .unwrap();

            // CWD is the main worktree — detect_worktree() should return None
            std::env::set_current_dir(&repo_dir).unwrap();

            cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

            // Bean should be archived normally (no merge step)
            let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
            let updated = Bean::from_file(&archived).unwrap();
            assert_eq!(updated.status, Status::Closed);
            assert!(updated.is_archived);
        }

        #[test]
        fn test_close_outside_git_repo_works() {
            let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let _guard = CwdGuard(std::env::current_dir().unwrap());

            // Plain temp directory — no git repo at all
            let dir = TempDir::new().unwrap();
            let base = std::fs::canonicalize(dir.path()).unwrap();
            let beans_dir = base.join(".beans");
            fs::create_dir(&beans_dir).unwrap();

            let bean = Bean::new("1", "No Git Task");
            let slug = title_to_slug(&bean.title);
            bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
                .unwrap();

            // CWD in a non-git directory — detect_worktree() should return None
            std::env::set_current_dir(&base).unwrap();

            cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

            // Bean should be archived normally
            let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
            let updated = Bean::from_file(&archived).unwrap();
            assert_eq!(updated.status, Status::Closed);
            assert!(updated.is_archived);
        }
    }
}

// =====================================================================
// verify_timeout tests (live outside the git-worktree module)
// =====================================================================

#[cfg(test)]
mod verify_timeout_tests {
    use super::*;
    use crate::bean::{Bean, RunResult, Status};
    use crate::util::title_to_slug;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    /// A verify command that takes longer than the timeout is killed and
    /// treated as a failure. The bean remains open, attempts is incremented,
    /// and the history entry records RunResult::Timeout.
    #[test]
    fn verify_timeout_kills_slow_process_and_records_timeout() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        let mut bean = Bean::new("1", "Slow verify task");
        bean.verify = Some("sleep 60".to_string());
        bean.verify_timeout = Some(1); // 1-second timeout
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should still be open (verify timed out = failure)
        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.attempts, 1);
        assert!(updated.closed_at.is_none());

        // History should contain a Timeout entry
        assert_eq!(updated.history.len(), 1);
        assert_eq!(updated.history[0].result, RunResult::Timeout);
        assert!(updated.history[0].exit_code.is_none()); // killed, no exit code

        // The output_snippet should mention the timeout
        let snippet = updated.history[0].output_snippet.as_deref().unwrap_or("");
        assert!(
            snippet.contains("timed out"),
            "expected snippet to contain 'timed out', got: {:?}",
            snippet
        );
    }

    /// A verify command that finishes within the timeout is not affected.
    #[test]
    fn verify_timeout_does_not_affect_fast_commands() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        let mut bean = Bean::new("1", "Fast verify task");
        bean.verify = Some("true".to_string());
        bean.verify_timeout = Some(30); // generous timeout
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be closed normally
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.is_archived);
    }

    /// Bean-level verify_timeout overrides the config-level default.
    #[test]
    fn verify_timeout_bean_level_overrides_config() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Write a config with a generous timeout (should be overridden by bean)
        let config_yaml = "project: test\nnext_id: 2\nverify_timeout: 60\n";
        fs::write(beans_dir.join("config.yaml"), config_yaml).unwrap();

        let mut bean = Bean::new("1", "Bean timeout overrides config");
        bean.verify = Some("sleep 60".to_string());
        bean.verify_timeout = Some(1); // bean says 1s — should override config's 60s
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.history[0].result, RunResult::Timeout);
    }

    /// Config-level verify_timeout applies when bean has no per-bean override.
    #[test]
    fn verify_timeout_config_level_applies_when_bean_has_none() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Write a config with a short timeout
        let config_yaml = "project: test\nnext_id: 2\nverify_timeout: 1\n";
        fs::write(beans_dir.join("config.yaml"), config_yaml).unwrap();

        let mut bean = Bean::new("1", "Config timeout applies");
        bean.verify = Some("sleep 60".to_string());
        // No bean-level verify_timeout — config's 1s should apply
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.history[0].result, RunResult::Timeout);
    }

    /// Notes are updated with a timeout message when verify times out.
    #[test]
    fn verify_timeout_appends_to_notes() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        let mut bean = Bean::new("1", "Timeout notes test");
        bean.verify = Some("sleep 60".to_string());
        bean.verify_timeout = Some(1);
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug)))
            .unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated =
            Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        let notes = updated.notes.unwrap_or_default();
        // Notes should contain the timeout message
        assert!(
            notes.contains("timed out"),
            "expected notes to contain 'timed out', got: {:?}",
            notes
        );
    }

    /// effective_verify_timeout: bean overrides config when both set.
    #[test]
    fn effective_verify_timeout_bean_wins_over_config() {
        let bean = {
            let mut b = Bean::new("1", "Test");
            b.verify_timeout = Some(5);
            b
        };
        assert_eq!(bean.effective_verify_timeout(Some(30)), Some(5));
    }

    /// effective_verify_timeout: config applies when bean has none.
    #[test]
    fn effective_verify_timeout_config_fallback() {
        let bean = Bean::new("1", "Test");
        assert_eq!(bean.effective_verify_timeout(Some(30)), Some(30));
    }

    /// effective_verify_timeout: both None → None (no limit).
    #[test]
    fn effective_verify_timeout_both_none() {
        let bean = Bean::new("1", "Test");
        assert_eq!(bean.effective_verify_timeout(None), None);
    }
}
