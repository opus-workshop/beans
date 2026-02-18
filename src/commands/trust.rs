use std::path::Path;
use anyhow::{anyhow, Result};

use crate::hooks::{is_trusted, create_trust, revoke_trust};

/// Manage hook trust status.
///
/// By default, hooks are disabled (not trusted). Users must explicitly run
/// `bn trust` to enable hook execution. This is a security measure to ensure
/// users review .beans/hooks/ scripts before allowing execution.
///
/// # Arguments
///
/// * `beans_dir` - The .beans/ directory path
/// * `revoke` - If true, disable hooks (remove trust file)
/// * `check` - If true, display current trust status without changing it
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err` if file operations fail
pub fn cmd_trust(beans_dir: &Path, revoke: bool, check: bool) -> Result<()> {
    // hooks functions expect the project root (parent of .beans/)
    let project_dir = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    // If --check: print current status
    if check {
        if is_trusted(project_dir) {
            println!("Hooks are enabled");
        } else {
            println!("Hooks are disabled");
        }
        return Ok(());
    }

    // If --revoke: disable hooks
    if revoke {
        revoke_trust(project_dir)?;
        println!("Hooks disabled");
        return Ok(());
    }

    // Otherwise: enable hooks
    create_trust(project_dir)?;
    println!("Hooks enabled. Review .beans/hooks before running commands");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn create_test_dir() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn test_cmd_trust_enables_hooks() {
        let temp_dir = create_test_dir();
        let project_dir = temp_dir.path();
        let beans_dir = project_dir.join(".beans");

        // Ensure .beans directory exists
        fs::create_dir_all(&beans_dir).unwrap();

        // Trust is not enabled by default
        assert!(!is_trusted(project_dir));

        // Enable trust (cmd_trust receives .beans/ path, like main.rs)
        cmd_trust(&beans_dir, false, false).unwrap();

        // Verify trust is now enabled
        assert!(is_trusted(project_dir));
    }

    #[test]
    fn test_cmd_trust_check_reports_disabled() {
        let temp_dir = create_test_dir();
        let project_dir = temp_dir.path();
        let beans_dir = project_dir.join(".beans");

        // Ensure .beans directory exists
        fs::create_dir_all(&beans_dir).unwrap();

        // Check status when disabled - should not error
        let result = cmd_trust(&beans_dir, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cmd_trust_check_reports_enabled() {
        let temp_dir = create_test_dir();
        let project_dir = temp_dir.path();
        let beans_dir = project_dir.join(".beans");

        // Ensure .beans directory exists
        fs::create_dir_all(&beans_dir).unwrap();

        // Enable trust first
        cmd_trust(&beans_dir, false, false).unwrap();

        // Check status when enabled - should not error
        let result = cmd_trust(&beans_dir, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cmd_trust_revoke_disables_hooks() {
        let temp_dir = create_test_dir();
        let project_dir = temp_dir.path();
        let beans_dir = project_dir.join(".beans");

        // Ensure .beans directory exists
        fs::create_dir_all(&beans_dir).unwrap();

        // Enable trust first
        cmd_trust(&beans_dir, false, false).unwrap();
        assert!(is_trusted(project_dir));

        // Revoke trust
        cmd_trust(&beans_dir, true, false).unwrap();

        // Verify trust is disabled
        assert!(!is_trusted(project_dir));
    }

    #[test]
    fn test_cmd_trust_revoke_with_check() {
        let temp_dir = create_test_dir();
        let project_dir = temp_dir.path();
        let beans_dir = project_dir.join(".beans");

        // Ensure .beans directory exists
        fs::create_dir_all(&beans_dir).unwrap();

        // Enable trust first
        cmd_trust(&beans_dir, false, false).unwrap();

        // Revoke with check - should report disabled
        let result = cmd_trust(&beans_dir, true, true);
        assert!(result.is_ok());
    }
}
