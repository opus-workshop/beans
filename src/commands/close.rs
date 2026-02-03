use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::{Bean, Status};
use crate::config::Config;
use crate::discovery::{archive_path_for_bean, find_bean_file};
use crate::index::Index;
use crate::util::title_to_slug;
use crate::hooks::{execute_hook, HookEvent};

#[cfg(test)]
use std::fs;

/// Result of running a verify command
struct VerifyResult {
    success: bool,
    exit_code: Option<i32>,
    output: String,
}

/// Run a verify command for a bean.
///
/// Returns VerifyResult with success status, exit code, and combined stdout/stderr.
fn run_verify(beans_dir: &Path, verify_cmd: &str) -> Result<VerifyResult> {
    // Run in the project root (parent of .beans/)
    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    println!("Running verify: {}", verify_cmd);

    let output = ShellCommand::new("sh")
        .args(["-c", verify_cmd])
        .current_dir(project_root)
        .output()
        .with_context(|| format!("Failed to execute verify command: {}", verify_cmd))?;

    let combined_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ).trim().to_string();

    Ok(VerifyResult {
        success: output.status.success(),
        exit_code: output.status.code(),
        output: combined_output,
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
    let slug = bean.slug.clone()
        .unwrap_or_else(|| title_to_slug(&bean.title));
    let ext = bean_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("md");
    let today = chrono::Local::now().naive_local().date();
    let archive_path = archive_path_for_bean(beans_dir, parent_id, &slug, ext, today);

    // Create archive directories if needed
    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create archive directories for bean {}", parent_id))?;
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

    for id in &ids {
        let bean_path = find_bean_file(beans_dir, id)
            .with_context(|| format!("Bean not found: {}", id))?;

        let mut bean = Bean::from_file(&bean_path)
            .with_context(|| format!("Failed to load bean: {}", id))?;

        // Execute pre-close hook BEFORE verify command
        // hooks.rs expects the project root (parent of .beans), not the .beans dir itself
        let project_root = beans_dir
            .parent()
            .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;
        
        let pre_close_result = execute_hook(HookEvent::PreClose, &bean, project_root, reason.clone());
        
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
            if force {
                println!("Skipping verify for bean {} (--force)", id);
            } else {
                // Run the verify command
                let verify_result = run_verify(beans_dir, verify_cmd)?;

                if !verify_result.success {
                    // Increment attempts
                    bean.attempts += 1;
                    bean.updated_at = Utc::now();

                    // Append failure to notes for future agents
                    let failure_note = format_failure_note(
                        bean.attempts,
                        verify_result.exit_code,
                        &verify_result.output,
                    );
                    match &mut bean.notes {
                        Some(notes) => notes.push_str(&failure_note),
                        None => bean.notes = Some(failure_note),
                    }

                    bean.to_file(&bean_path)
                        .with_context(|| format!("Failed to save bean: {}", id))?;

                    // Display detailed failure feedback
                    println!("✗ Verify failed for bean {}", id);
                    println!();
                    println!("Command: {}", verify_cmd);
                    if let Some(code) = verify_result.exit_code {
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

                    continue;
                }

                println!("Verify passed for bean {}", id);
            }
        }

        // Close the bean
        bean.status = crate::bean::Status::Closed;
        bean.closed_at = Some(now);
        bean.close_reason = reason.clone();
        bean.updated_at = now;

        bean.to_file(&bean_path)
            .with_context(|| format!("Failed to save bean: {}", id))?;

        // Archive the closed bean
        let slug = bean.slug.clone()
            .unwrap_or_else(|| title_to_slug(&bean.title));
        let ext = bean_path.extension()
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

        // Check if parent should be auto-closed
        if let Some(parent_id) = &bean.parent {
            // Check config for auto_close_parent setting
            let auto_close_enabled = Config::load(beans_dir)
                .map(|c| c.auto_close_parent)
                .unwrap_or(true); // Default to true

            if auto_close_enabled && all_children_closed(beans_dir, parent_id)? {
                auto_close_parent(beans_dir, parent_id)?;
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
    if any_closed || !ids.is_empty() {
        let index = Index::build(beans_dir)
            .with_context(|| "Failed to rebuild index")?;
        index.save(beans_dir)
            .with_context(|| "Failed to save index")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::util::title_to_slug;

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], Some("Fixed".to_string()), false).unwrap();

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
        bean1.to_file(beans_dir.join(format!("1-{}.md", slug1))).unwrap();
        bean2.to_file(beans_dir.join(format!("2-{}.md", slug2))).unwrap();
        bean3.to_file(beans_dir.join(format!("3-{}.md", slug3))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None, false).unwrap();

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
        bean1.to_file(beans_dir.join(format!("1-{}.md", slug1))).unwrap();
        bean2.to_file(beans_dir.join(format!("2-{}.md", slug2))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // First attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 1);
        assert_eq!(updated.status, Status::Open);

        // Second attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 2);
        assert_eq!(updated.status, Status::Open);

        // Third attempt - no limit, keeps incrementing
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 3);
        assert_eq!(updated.status, Status::Open);

        // Fourth attempt - still works, no max
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
    fn test_close_with_shell_operators_work() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with shell operators");
        // Shell operators like && should work in verify commands
        bean.verify = Some("true && true".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        let _ = cmd_close(&beans_dir, vec!["1".to_string()], None, false);

        // Verify fails because `false` returns exit code 1
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Close should still succeed (returns Ok), but bean not closed
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should NOT be archived or closed
        let not_archived = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(not_archived.is_ok());
        let updated = Bean::from_file(&not_archived.unwrap()).unwrap();
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
        bean1.to_file(beans_dir.join(format!("1-{}.md", slug1))).unwrap();
        bean2.to_file(beans_dir.join(format!("2-{}.md", slug2))).unwrap();
        bean3.to_file(beans_dir.join(format!("3-{}.md", slug3))).unwrap();

        // Close all three (hook passes for all)
        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None, false).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

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
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Close with a reason
        cmd_close(&beans_dir, vec!["1".to_string()], Some("Completed".to_string()), false).unwrap();

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
        bean1.to_file(beans_dir.join(format!("1-{}.md", slug1))).unwrap();
        bean2.to_file(beans_dir.join(format!("2-{}.md", slug2))).unwrap();
        bean3.to_file(beans_dir.join(format!("3-{}.md", slug3))).unwrap();

        // Try to close all three
        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None, false).unwrap();

        // Bean 1 should be archived
        let archived1 = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(archived1.is_ok());
        let bean1_result = Bean::from_file(&archived1.unwrap()).unwrap();
        assert_eq!(bean1_result.status, Status::Closed);

        // Bean 2 should NOT be archived (rejected by hook)
        let open2 = crate::discovery::find_bean_file(&beans_dir, "2");
        assert!(open2.is_ok());
        let bean2_result = Bean::from_file(&open2.unwrap()).unwrap();
        assert_eq!(bean2_result.status, Status::Open);

        // Bean 3 should be archived
        let archived3 = crate::discovery::find_archived_bean(&beans_dir, "3");
        assert!(archived3.is_ok());
        let bean3_result = Bean::from_file(&archived3.unwrap()).unwrap();
        assert_eq!(bean3_result.status, Status::Closed);
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
        parent.to_file(beans_dir.join(format!("1-{}.md", parent_slug))).unwrap();

        // Create child beans
        let mut child1 = Bean::new("1.1", "Child 1");
        child1.parent = Some("1".to_string());
        let child1_slug = title_to_slug(&child1.title);
        child1.to_file(beans_dir.join(format!("1.1-{}.md", child1_slug))).unwrap();

        let mut child2 = Bean::new("1.2", "Child 2");
        child2.parent = Some("1".to_string());
        let child2_slug = title_to_slug(&child2.title);
        child2.to_file(beans_dir.join(format!("1.2-{}.md", child2_slug))).unwrap();

        // Close first child - parent should NOT auto-close yet
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();
        
        // Parent should still be open
        let parent_still_open = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(parent_still_open.is_ok());
        let parent_bean = Bean::from_file(&parent_still_open.unwrap()).unwrap();
        assert_eq!(parent_bean.status, Status::Open);

        // Close second child - parent should auto-close now
        cmd_close(&beans_dir, vec!["1.2".to_string()], None, false).unwrap();

        // Parent should now be archived
        let parent_archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(parent_archived.is_ok(), "Parent should be auto-archived");
        let parent_result = Bean::from_file(&parent_archived.unwrap()).unwrap();
        assert_eq!(parent_result.status, Status::Closed);
        assert!(parent_result.close_reason.as_ref().unwrap().contains("Auto-closed"));
    }

    #[test]
    fn test_no_auto_close_when_children_still_open() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent.to_file(beans_dir.join(format!("1-{}.md", parent_slug))).unwrap();

        // Create child beans
        let mut child1 = Bean::new("1.1", "Child 1");
        child1.parent = Some("1".to_string());
        let child1_slug = title_to_slug(&child1.title);
        child1.to_file(beans_dir.join(format!("1.1-{}.md", child1_slug))).unwrap();

        let mut child2 = Bean::new("1.2", "Child 2");
        child2.parent = Some("1".to_string());
        let child2_slug = title_to_slug(&child2.title);
        child2.to_file(beans_dir.join(format!("1.2-{}.md", child2_slug))).unwrap();

        // Close first child only
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Parent should still be open (not all children closed)
        let parent_still_open = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(parent_still_open.is_ok());
        let parent_bean = Bean::from_file(&parent_still_open.unwrap()).unwrap();
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
        };
        config.save(&beans_dir).unwrap();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent.to_file(beans_dir.join(format!("1-{}.md", parent_slug))).unwrap();

        // Create single child bean
        let mut child = Bean::new("1.1", "Only Child");
        child.parent = Some("1".to_string());
        let child_slug = title_to_slug(&child.title);
        child.to_file(beans_dir.join(format!("1.1-{}.md", child_slug))).unwrap();

        // Close the child
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Parent should still be open (auto-close disabled)
        let parent_still_open = crate::discovery::find_bean_file(&beans_dir, "1");
        assert!(parent_still_open.is_ok());
        let parent_bean = Bean::from_file(&parent_still_open.unwrap()).unwrap();
        assert_eq!(parent_bean.status, Status::Open);
    }

    #[test]
    fn test_auto_close_recursive_grandparent() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create grandparent bean
        let grandparent = Bean::new("1", "Grandparent");
        let gp_slug = title_to_slug(&grandparent.title);
        grandparent.to_file(beans_dir.join(format!("1-{}.md", gp_slug))).unwrap();

        // Create parent bean (child of grandparent)
        let mut parent = Bean::new("1.1", "Parent");
        parent.parent = Some("1".to_string());
        let p_slug = title_to_slug(&parent.title);
        parent.to_file(beans_dir.join(format!("1.1-{}.md", p_slug))).unwrap();

        // Create grandchild bean (child of parent)
        let mut grandchild = Bean::new("1.1.1", "Grandchild");
        grandchild.parent = Some("1.1".to_string());
        let gc_slug = title_to_slug(&grandchild.title);
        grandchild.to_file(beans_dir.join(format!("1.1.1-{}.md", gc_slug))).unwrap();

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
        let p_bean = Bean::from_file(&p_archived.unwrap()).unwrap();
        assert!(p_bean.close_reason.as_ref().unwrap().contains("Auto-closed"));

        let gp_bean = Bean::from_file(&gp_archived.unwrap()).unwrap();
        assert!(gp_bean.close_reason.as_ref().unwrap().contains("Auto-closed"));
    }

    #[test]
    fn test_auto_close_with_no_parent() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create a standalone bean (no parent)
        let bean = Bean::new("1", "Standalone Task");
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Close the bean - should work fine with no parent
        cmd_close(&beans_dir, vec!["1".to_string()], None, false).unwrap();

        // Bean should be archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(archived.is_ok());
        let bean_result = Bean::from_file(&archived.unwrap()).unwrap();
        assert_eq!(bean_result.status, Status::Closed);
    }

    #[test]
    fn test_all_children_closed_checks_archived_beans() {
        let (_dir, beans_dir) = setup_test_beans_dir_with_config();

        // Create parent bean
        let parent = Bean::new("1", "Parent Task");
        let parent_slug = title_to_slug(&parent.title);
        parent.to_file(beans_dir.join(format!("1-{}.md", parent_slug))).unwrap();

        // Create two child beans
        let mut child1 = Bean::new("1.1", "Child 1");
        child1.parent = Some("1".to_string());
        let child1_slug = title_to_slug(&child1.title);
        child1.to_file(beans_dir.join(format!("1.1-{}.md", child1_slug))).unwrap();

        let mut child2 = Bean::new("1.2", "Child 2");
        child2.parent = Some("1".to_string());
        let child2_slug = title_to_slug(&child2.title);
        child2.to_file(beans_dir.join(format!("1.2-{}.md", child2_slug))).unwrap();

        // Close first child (will be archived)
        cmd_close(&beans_dir, vec!["1.1".to_string()], None, false).unwrap();

        // Verify child1 is archived
        let child1_archived = crate::discovery::find_archived_bean(&beans_dir, "1.1");
        assert!(child1_archived.is_ok(), "Child 1 should be archived");

        // Now close child2 - parent should auto-close even though child1 is in archive
        cmd_close(&beans_dir, vec!["1.2".to_string()], None, false).unwrap();

        // Parent should be archived
        let parent_archived = crate::discovery::find_archived_bean(&beans_dir, "1");
        assert!(parent_archived.is_ok(), "Parent should be auto-archived when all children (including archived) are closed");
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
    fn test_format_failure_note() {
        let note = format_failure_note(1, Some(1), "error message");
        
        assert!(note.contains("## Attempt 1"));
        assert!(note.contains("Exit code: 1"));
        assert!(note.contains("error message"));
        assert!(note.contains("```")); // Fenced code block
    }
}
