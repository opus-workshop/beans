use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use shell_escape::unix::escape;

use crate::bean::Bean;
use crate::index::Index;

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

    for id in &ids {
        let bean_path = beans_dir.join(format!("{}.yaml", id));
        if !bean_path.exists() {
            return Err(anyhow!("Bean not found: {}", id));
        }

        let mut bean = Bean::from_file(&bean_path)
            .with_context(|| format!("Failed to load bean: {}", id))?;

        // Check if bean has a verify command
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

        println!("Closed bean {}: {}", id, bean.title);
        any_closed = true;
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
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.close_reason.is_none());
    }

    #[test]
    fn test_close_with_reason() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], Some("Fixed".to_string())).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert_eq!(updated.close_reason, Some("Fixed".to_string()));
    }

    #[test]
    fn test_close_multiple_beans() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        let bean3 = Bean::new("3", "Task 3");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None).unwrap();

        for id in &["1", "2", "3"] {
            let bean = Bean::from_file(beans_dir.join(format!("{}.yaml", id))).unwrap();
            assert_eq!(bean.status, Status::Closed);
            assert!(bean.closed_at.is_some());
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
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 2);
        let entry1 = index.beans.iter().find(|e| e.id == "1").unwrap();
        assert_eq!(entry1.status, Status::Closed);
    }

    #[test]
    fn test_close_sets_updated_at() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        let original_updated_at = bean.updated_at;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert!(updated.updated_at > original_updated_at);
    }

    #[test]
    fn test_close_with_passing_verify() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with verify");
        bean.verify = Some("true".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
    }

    #[test]
    fn test_close_with_failing_verify_increments_attempts() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with failing verify");
        bean.verify = Some("false".to_string());
        bean.attempts = 0;
        bean.max_attempts = 3;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
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
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // First attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();
        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.attempts, 1);
        assert_eq!(updated.status, Status::Open);

        // Second attempt
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();
        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.attempts, 2);
        assert_eq!(updated.status, Status::Open);

        // Third attempt - should hit max
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();
        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
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
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Should not run verify again, just print message
        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.attempts, 3); // Not incremented
        assert_eq!(updated.status, Status::Open); // Still not closed
    }

    #[test]
    fn test_close_without_verify_still_works() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task without verify");
        // No verify command set
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
    }

    #[test]
    fn test_close_with_shell_metacharacters_safely_escaped() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with shell metacharacters");
        // Try to inject commands with shell metacharacters - should not execute
        // The escaped version should treat everything as a literal command name
        bean.verify = Some("echo test; rm -rf .".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // This should fail because 'echo test; rm -rf .' is not a valid command
        // after escaping (it becomes a literal string)
        let _ = cmd_close(&beans_dir, vec!["1".to_string()], None);

        // Verify command should fail (not found), not execute the injected commands
        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Open); // Not closed due to verification failure
        assert_eq!(updated.attempts, 1); // Attempts incremented
    }

    #[test]
    fn test_close_with_pipe_metacharacters_safely_escaped() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task with pipe characters");
        // Try to pipe commands - should not execute
        bean.verify = Some("true | false".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let _ = cmd_close(&beans_dir, vec!["1".to_string()], None);

        // The escaped command should fail because the full string is treated literally
        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Open); // Not closed
        assert_eq!(updated.attempts, 1); // Attempts incremented
    }
}
