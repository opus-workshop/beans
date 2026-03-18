use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::{AttemptOutcome, AttemptRecord, Bean, Status};
use crate::config::resolve_identity;
use crate::discovery::find_bean_file;
use crate::index::Index;

/// Try to get the current git HEAD SHA. Returns None if not in a git repo.
fn git_head_sha(working_dir: &Path) -> Option<String> {
    ShellCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(working_dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run the verify command and return whether it passed (exit 0).
fn run_verify_check(verify_cmd: &str, project_root: &Path) -> Result<bool> {
    let output = ShellCommand::new("sh")
        .args(["-c", verify_cmd])
        .current_dir(project_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to execute verify command: {}", verify_cmd))?;

    Ok(output.success())
}

/// Claim a bean for work.
///
/// Sets status to InProgress, records who claimed it and when.
/// The bean must be in Open status to be claimed.
///
/// If the bean has a verify command and `force` is false, the verify command
/// is run first. If it already passes, the claim is rejected (nothing to do).
/// If it fails, the claim is granted with `fail_first: true` and the current
/// git HEAD SHA is stored as `checkpoint`.
pub fn cmd_claim(beans_dir: &Path, id: &str, by: Option<String>, force: bool) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id).map_err(|_| anyhow!("Bean not found: {}", id))?;

    let mut bean =
        Bean::from_file(&bean_path).with_context(|| format!("Failed to load bean: {}", id))?;

    if bean.status != Status::Open {
        return Err(anyhow!(
            "Bean {} is {} -- only open beans can be claimed",
            id,
            bean.status
        ));
    }

    // Warn if bean has no verify command (GOAL vs SPEC)
    let has_verify = bean.verify.as_ref().is_some_and(|v| !v.trim().is_empty());
    if !has_verify {
        eprintln!(
            "Warning: Claiming GOAL (no verify). Consider decomposing with: bn create \"spec\" --parent {} --verify \"test\"",
            id
        );
    }

    // Verify-on-claim: run verify before granting claim (TDD enforcement)
    // Skip when fail_first is false (bean created with -p / pass-ok)
    if has_verify && !force && bean.fail_first {
        let project_root = beans_dir
            .parent()
            .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;
        let verify_cmd = bean.verify.as_ref().unwrap();

        eprintln!("Running verify before claim: {}", verify_cmd);
        let passed = run_verify_check(verify_cmd, project_root)?;

        if passed {
            return Err(anyhow!(
                "Cannot claim bean {}: verify already passes\n\n\
                 The verify command succeeded before any work was done.\n\
                 This means either the test is bogus or the work is already complete.\n\n\
                 Use --force to override.",
                id
            ));
        }

        // Verify failed — good, this proves the test is meaningful
        bean.fail_first = true;
        bean.checkpoint = git_head_sha(project_root);
    }

    // Resolve identity: explicit --by > resolved identity > "anonymous"
    let resolved_by = by.or_else(|| resolve_identity(beans_dir));

    let now = Utc::now();
    bean.status = Status::InProgress;
    bean.claimed_by = resolved_by.clone();
    bean.claimed_at = Some(now);
    bean.updated_at = now;

    // Start a new attempt in the attempt log (for memory system tracking)
    let attempt_num = bean.attempt_log.len() as u32 + 1;
    bean.attempt_log.push(AttemptRecord {
        num: attempt_num,
        outcome: AttemptOutcome::Abandoned, // default until close/release updates it
        notes: None,
        agent: resolved_by.clone(),
        started_at: Some(now),
        finished_at: None,
    });

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    let claimer = resolved_by.as_deref().unwrap_or("anonymous");
    println!("Claimed bean {}: {} (by {})", id, bean.title, claimer);

    // Rebuild index
    let index = Index::build(beans_dir).with_context(|| "Failed to rebuild index")?;
    index
        .save(beans_dir)
        .with_context(|| "Failed to save index")?;

    Ok(())
}

/// Release a claim on a bean.
///
/// Clears claimed_by/claimed_at and sets status back to Open.
pub fn cmd_release(beans_dir: &Path, id: &str) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id).map_err(|_| anyhow!("Bean not found: {}", id))?;

    let mut bean =
        Bean::from_file(&bean_path).with_context(|| format!("Failed to load bean: {}", id))?;

    let now = Utc::now();

    // Finalize the current attempt as abandoned (if one is in progress)
    if let Some(attempt) = bean.attempt_log.last_mut() {
        if attempt.finished_at.is_none() {
            attempt.outcome = AttemptOutcome::Abandoned;
            attempt.finished_at = Some(now);
        }
    }

    bean.claimed_by = None;
    bean.claimed_at = None;
    bean.status = Status::Open;
    bean.updated_at = now;

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    println!("Released claim on bean {}: {}", id, bean.title);

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
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_claim_open_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", Some("alice".to_string()), true).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert_eq!(updated.claimed_by, Some("alice".to_string()));
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn test_claim_without_by() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", None, true).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        // When no --by is given, identity is auto-resolved from config/git.
        // claimed_by may be Some(...) or None depending on environment.
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn test_claim_non_open_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_claim(&beans_dir, "1", Some("bob".to_string()), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_closed_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::Closed;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_claim(&beans_dir, "1", Some("bob".to_string()), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_nonexistent_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_claim(&beans_dir, "99", Some("alice".to_string()), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_release_claimed_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.claimed_by = Some("alice".to_string());
        bean.claimed_at = Some(Utc::now());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_release(&beans_dir, "1").unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.claimed_by, None);
        assert_eq!(updated.claimed_at, None);
    }

    #[test]
    fn test_release_nonexistent_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_release(&beans_dir, "99");
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", Some("alice".to_string()), true).unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        let entry = &index.beans[0];
        assert_eq!(entry.status, Status::InProgress);
    }

    #[test]
    fn test_release_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_release(&beans_dir, "1").unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        let entry = &index.beans[0];
        assert_eq!(entry.status, Status::Open);
    }

    #[test]
    fn test_claim_bean_without_verify_succeeds_with_warning() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Create bean without verify (this is a GOAL, not a SPEC)
        let bean = Bean::new("1", "Add authentication");
        // bean.verify is None by default
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Claim should succeed (warning is printed but doesn't block)
        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), true);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert_eq!(updated.claimed_by, Some("alice".to_string()));
    }

    #[test]
    fn test_claim_bean_with_verify_succeeds() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Create bean with verify (this is a SPEC)
        let mut bean = Bean::new("1", "Add login endpoint");
        bean.verify = Some("cargo test login".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Claim should succeed without warning
        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), true);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
    }

    #[test]
    fn test_claim_bean_with_empty_verify_warns() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Create bean with empty verify string (should be treated as no verify)
        let mut bean = Bean::new("1", "Vague task");
        bean.verify = Some("   ".to_string()); // whitespace only
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Claim should succeed (warning is printed but doesn't block)
        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), true);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
    }

    // =================================================================
    // verify_on_claim tests
    // =================================================================

    #[test]
    fn verify_on_claim_passing_verify_rejected() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Bean with verify that passes immediately ("true" exits 0)
        let mut bean = Bean::new("1", "Already done");
        bean.verify = Some("true".to_string());
        bean.fail_first = true; // created without -p, enforces fail-first
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Claim without force — should be rejected because verify passes
        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), false);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("verify already passes"));
        assert!(err_msg.contains("--force"));

        // Bean should still be open (claim was rejected)
        let unchanged = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(unchanged.status, Status::Open);
    }

    #[test]
    fn verify_on_claim_failing_verify_succeeds() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Bean with verify that fails ("false" exits 1)
        let mut bean = Bean::new("1", "Real work needed");
        bean.verify = Some("false".to_string());
        bean.fail_first = true; // created without -p, enforces fail-first
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Claim without force — should succeed because verify fails
        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), false);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert_eq!(updated.claimed_by, Some("alice".to_string()));
        assert!(
            updated.fail_first,
            "fail_first should be set when verify fails at claim time"
        );
    }

    #[test]
    fn verify_on_claim_force_overrides() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Bean with verify that passes immediately
        let mut bean = Bean::new("1", "Force claim");
        bean.verify = Some("true".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Claim with force — should succeed even though verify passes
        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), true);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert_eq!(updated.claimed_by, Some("alice".to_string()));
    }

    #[test]
    fn verify_on_claim_checkpoint_sha_stored() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Bean with verify that fails
        let mut bean = Bean::new("1", "Checkpoint test");
        bean.verify = Some("false".to_string());
        bean.fail_first = true; // created without -p, enforces fail-first
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Initialize a git repo in the temp dir so we get a real SHA
        let project_root = beans_dir.parent().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(project_root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(project_root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init", "--allow-empty"])
            .current_dir(project_root)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), false);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert!(
            updated.checkpoint.is_some(),
            "checkpoint SHA should be stored"
        );
        let sha = updated.checkpoint.unwrap();
        assert_eq!(sha.len(), 40, "SHA should be 40 hex chars, got: {}", sha);
        assert!(
            sha.chars().all(|c| c.is_ascii_hexdigit()),
            "SHA should be hex"
        );
    }

    #[test]
    fn verify_on_claim_no_verify_skips_check() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Bean without verify — should not run verify check
        let bean = Bean::new("1", "No verify");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_claim(&beans_dir, "1", Some("alice".to_string()), false);
        assert!(result.is_ok());

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert!(
            !updated.fail_first,
            "fail_first should not be set without verify"
        );
        assert!(updated.checkpoint.is_none(), "no checkpoint without verify");
    }

    // =====================================================================
    // Attempt Tracking Tests
    // =====================================================================

    #[test]
    fn claim_starts_attempt() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", Some("agent-1".to_string()), true).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.attempt_log.len(), 1);
        assert_eq!(updated.attempt_log[0].num, 1);
        assert_eq!(updated.attempt_log[0].agent, Some("agent-1".to_string()));
        assert!(updated.attempt_log[0].started_at.is_some());
        assert!(updated.attempt_log[0].finished_at.is_none());
    }

    #[test]
    fn release_marks_attempt_abandoned() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.claimed_by = Some("agent-1".to_string());
        bean.attempt_log.push(AttemptRecord {
            num: 1,
            outcome: AttemptOutcome::Abandoned,
            notes: None,
            agent: Some("agent-1".to_string()),
            started_at: Some(Utc::now()),
            finished_at: None,
        });
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_release(&beans_dir, "1").unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.attempt_log.len(), 1);
        assert_eq!(updated.attempt_log[0].outcome, AttemptOutcome::Abandoned);
        assert!(updated.attempt_log[0].finished_at.is_some());
    }

    #[test]
    fn multiple_claims_accumulate_attempts() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // First claim
        cmd_claim(&beans_dir, "1", Some("agent-1".to_string()), true).unwrap();
        // Release
        cmd_release(&beans_dir, "1").unwrap();
        // Second claim
        cmd_claim(&beans_dir, "1", Some("agent-2".to_string()), true).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.attempt_log.len(), 2);
        assert_eq!(updated.attempt_log[0].num, 1);
        assert_eq!(updated.attempt_log[0].outcome, AttemptOutcome::Abandoned);
        assert!(updated.attempt_log[0].finished_at.is_some());
        assert_eq!(updated.attempt_log[1].num, 2);
        assert_eq!(updated.attempt_log[1].agent, Some("agent-2".to_string()));
        assert!(updated.attempt_log[1].finished_at.is_none());
    }
}
