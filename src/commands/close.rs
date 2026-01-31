use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use shell_escape::unix::escape;

use crate::bean::Bean;
use crate::discovery::{archive_path_for_bean, find_bean_file};
use crate::index::Index;
use crate::util::title_to_slug;
use crate::hooks::{execute_hook, HookEvent};

#[cfg(test)]
use std::fs;
#[cfg(test)]
use crate::bean::Status;

/// Run a verify command for a bean.
///
/// Returns `Ok(true)` if the command exits 0, `Ok(false)` if non-zero.
/// The verify command is shell-escaped to prevent injection attacks.
fn run_verify(beans_dir: &Path, verify_cmd: &str) -> Result<bool> {
    // Run in the project root (parent of .beans/)
    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    println!("Running verify: {}", verify_cmd);

    // Escape the verify command to prevent shell injection by treating it as a single argument
    let escaped = escape(verify_cmd.into());

    let status = ShellCommand::new("sh")
        .arg("-c")
        .arg(escaped.as_ref())
        .current_dir(project_root)
        .status()
        .with_context(|| format!("Failed to execute verify command: {}", verify_cmd))?;

    Ok(status.success())
}

/// Close one or more beans.
///
/// Sets status=closed, closed_at=now, and optionally close_reason.
/// If the bean has a verify command, it must pass before closing.
/// Calls pre-close hook before verify (can block close if hook fails).
/// Rebuilds the index.
pub fn cmd_close(
    beans_dir: &Path,
    ids: Vec<String>,
    reason: Option<String>,
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
            // Check if we've already exceeded max attempts
            if bean.attempts >= bean.max_attempts {
                println!(
                    "Bean {} has exceeded max attempts ({}/{}), needs human review",
                    id, bean.attempts, bean.max_attempts
                );
                continue;
            }

            // Run the verify command
            let passed = run_verify(beans_dir, verify_cmd)?;

            if !passed {
                // Increment attempts and save
                bean.attempts += 1;
                bean.updated_at = Utc::now();
                bean.to_file(&bean_path)
                    .with_context(|| format!("Failed to save bean: {}", id))?;

                if bean.attempts >= bean.max_attempts {
                    println!(
                        "Verify failed for bean {} ({}/{}), exceeded max attempts, needs human review",
                        id, bean.attempts, bean.max_attempts
                    );
                } else {
                    println!(
                        "Verify failed for bean {} ({}/{} attempts)",
                        id, bean.attempts, bean.max_attempts
                    );
                }
                continue;
            }

            println!("Verify passed for bean {}", id);
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
        let today = chrono::Local::now().naive_local().date();
        let archive_path = archive_path_for_bean(beans_dir, id, &slug, today);

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

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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

        cmd_close(&beans_dir, vec!["1".to_string()], Some("Fixed".to_string())).unwrap();

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

        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None).unwrap();

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
        let result = cmd_close(&beans_dir, vec!["99".to_string()], None);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_no_ids() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_close(&beans_dir, vec![], None);
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

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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
        bean.max_attempts = 3;
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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
        bean.max_attempts = 3;
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // First attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 1);
        assert_eq!(updated.status, Status::Open);

        // Second attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 2);
        assert_eq!(updated.status, Status::Open);

        // Third attempt - should hit max
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 3);
        assert_eq!(updated.status, Status::Open);
    }

    #[test]
    fn test_close_exceeded_max_attempts_blocks_close() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task at max attempts");
        bean.verify = Some("false".to_string());
        bean.attempts = 3;
        bean.max_attempts = 3;
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Should not run verify again, just print message
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.attempts, 3); // Not incremented
        assert_eq!(updated.status, Status::Open); // Still not closed
    }

    #[test]
    fn test_close_without_verify_still_works() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task without verify");
        // No verify command set
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        // Verify bean is archived
        let archived = crate::discovery::find_archived_bean(&beans_dir, "1").unwrap();
        let updated = Bean::from_file(&archived).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.is_archived);
    }

    #[test]
    fn test_close_with_shell_metacharacters_safely_escaped() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with shell metacharacters");
        // Try to inject commands with shell metacharacters - should not execute
        // The escaped version should treat everything as a literal command name
        bean.verify = Some("echo test; rm -rf .".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // This should fail because 'echo test; rm -rf .' is not a valid command
        // after escaping (it becomes a literal string)
        let _ = cmd_close(&beans_dir, vec!["1".to_string()], None);

        // Verify command should fail (not found), not execute the injected commands
        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::Open); // Not closed due to verification failure
        assert_eq!(updated.attempts, 1); // Attempts incremented
    }

    #[test]
    fn test_close_with_pipe_metacharacters_safely_escaped() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with pipe characters");
        // Try to pipe commands - should not execute
        bean.verify = Some("true | false".to_string());
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        let _ = cmd_close(&beans_dir, vec!["1".to_string()], None);

        // The escaped command should fail because the full string is treated literally
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
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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
        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None).unwrap();

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
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

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
        cmd_close(&beans_dir, vec!["1".to_string()], Some("Completed".to_string())).unwrap();

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
        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None).unwrap();

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
}
