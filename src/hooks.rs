use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::bean::Bean;

// ---------------------------------------------------------------------------
// HookEvent Enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreCreate,
    PostCreate,
    PreUpdate,
    PostUpdate,
    PreClose,
}

impl HookEvent {
    /// Convert HookEvent to its string representation for hook file names.
    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::PreCreate => "pre-create",
            HookEvent::PostCreate => "post-create",
            HookEvent::PreUpdate => "pre-update",
            HookEvent::PostUpdate => "post-update",
            HookEvent::PreClose => "pre-close",
        }
    }
}

// ---------------------------------------------------------------------------
// HookPayload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    pub event: String,
    pub bean: Bean,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HookPayload {
    /// Create a new HookPayload for the given event and bean.
    pub fn new(event: HookEvent, bean: Bean, reason: Option<String>) -> Self {
        Self {
            event: event.as_str().to_string(),
            bean,
            reason,
        }
    }

    /// Serialize this payload to JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|e| anyhow!("Failed to serialize payload to JSON: {}", e))
    }
}

// ---------------------------------------------------------------------------
// Hook Path Management
// ---------------------------------------------------------------------------

/// Get the path to a hook script based on the event and beans directory.
pub fn get_hook_path(beans_dir: &Path, event: HookEvent) -> PathBuf {
    beans_dir.join(".beans").join("hooks").join(event.as_str())
}

/// Check if a hook file exists and is executable.
pub fn is_hook_executable(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    // On Unix-like systems, check execute bit
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        if let Ok(metadata) = fs::metadata(path) {
            let mode = metadata.permissions().mode();
            // Check if any execute bit is set (user, group, or other)
            (mode & 0o111) != 0
        } else {
            false
        }
    }

    // On Windows, any executable file extension is considered executable
    #[cfg(windows)]
    {
        let path_str = path.to_string_lossy();
        let exe_extensions = [".exe", ".bat", ".cmd", ".ps1", ".com"];
        exe_extensions.iter().any(|ext| path_str.ends_with(ext))
    }

    // Default to true if we can't determine
    #[cfg(not(any(unix, windows)))]
    true
}

// ---------------------------------------------------------------------------
// Hook Execution
// ---------------------------------------------------------------------------

/// Execute a hook script with the given payload.
///
/// # Arguments
///
/// * `event` - The hook event to trigger
/// * `bean` - The bean to pass to the hook
/// * `beans_dir` - The beans directory (parent of .beans/hooks)
/// * `reason` - Optional reason (used for pre-close hooks)
///
/// # Returns
///
/// * `Ok(true)` - Hook executed successfully and returned exit code 0
/// * `Ok(false)` - Hook executed but returned non-zero exit code
/// * `Err` - Hook not found, not executable, timeout, or I/O error
pub fn execute_hook(
    event: HookEvent,
    bean: &Bean,
    beans_dir: &Path,
    reason: Option<String>,
) -> Result<bool> {
    // Security model: Hooks are disabled by default. Users must explicitly enable them
    // with `bn trust` before any hooks will execute. This ensures users review the
    // hook scripts in .beans/hooks/ before giving them execution rights.
    if !is_trusted(beans_dir) {
        return Ok(true);
    }

    let hook_path = get_hook_path(beans_dir, event);

    // If hook doesn't exist, silently return success
    if !hook_path.exists() {
        return Ok(true);
    }

    // If hook exists but is not executable, return error
    if !is_hook_executable(&hook_path) {
        return Err(anyhow!(
            "Hook {} exists but is not executable",
            hook_path.display()
        ));
    }

    // Create the payload
    let payload = HookPayload::new(event, bean.clone(), reason);
    let json_payload = payload.to_json()?;

    // Spawn the subprocess
    let mut child = Command::new(&hook_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn hook process: {}", e))?;

    // Write JSON to stdin
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("Failed to open stdin"))?;
        use std::io::Write;
        stdin
            .write_all(json_payload.as_bytes())
            .map_err(|e| anyhow!("Failed to write payload to hook stdin: {}", e))?;
        // stdin is dropped here, closing the pipe
    }

    // Wait for the process with a 30-second timeout
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process completed
                return Ok(status.success());
            }
            Ok(None) => {
                // Process still running
                if start.elapsed() > timeout {
                    // Timeout exceeded, kill the process
                    let _ = child.kill();
                    return Err(anyhow!(
                        "Hook execution timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                // Sleep briefly before checking again
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(anyhow!("Failed to wait for hook process: {}", e));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Trust Management
// ---------------------------------------------------------------------------

/// Check if hooks are trusted (enabled).
///
/// Returns true if the .beans/.hooks-trusted file exists, false otherwise.
/// Does not error if the file doesn't exist.
pub fn is_trusted(beans_dir: &Path) -> bool {
    beans_dir.join(".beans").join(".hooks-trusted").exists()
}

/// Enable hook trust by creating the .beans/.hooks-trusted file.
///
/// # Returns
///
/// * `Ok(())` - Trust file created successfully
/// * `Err` - Failed to create trust file
pub fn create_trust(beans_dir: &Path) -> Result<()> {
    let trust_path = beans_dir.join(".beans").join(".hooks-trusted");

    // Ensure .beans directory exists
    std::fs::create_dir_all(
        trust_path
            .parent()
            .ok_or_else(|| anyhow!("Invalid trust path"))?,
    )?;

    // Create the trust file with metadata
    let metadata = format!("Hooks enabled at {}\n", chrono::Utc::now());
    std::fs::write(&trust_path, metadata).map_err(|e| anyhow!("Failed to create trust file: {}", e))
}

/// Revoke hook trust by deleting the .beans/.hooks-trusted file.
///
/// # Returns
///
/// * `Ok(())` - Trust file deleted successfully
/// * `Err` - Trust file doesn't exist or failed to delete
pub fn revoke_trust(beans_dir: &Path) -> Result<()> {
    let trust_path = beans_dir.join(".beans").join(".hooks-trusted");

    if !trust_path.exists() {
        return Err(anyhow!("Trust file does not exist"));
    }

    std::fs::remove_file(&trust_path).map_err(|e| anyhow!("Failed to revoke trust: {}", e))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_bean() -> Bean {
        Bean::new("1", "Test Bean")
    }

    fn create_test_dir() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn test_hook_event_string_representation() {
        assert_eq!(HookEvent::PreCreate.as_str(), "pre-create");
        assert_eq!(HookEvent::PostCreate.as_str(), "post-create");
        assert_eq!(HookEvent::PreUpdate.as_str(), "pre-update");
        assert_eq!(HookEvent::PostUpdate.as_str(), "post-update");
        assert_eq!(HookEvent::PreClose.as_str(), "pre-close");
    }

    #[test]
    fn test_hook_payload_serializes_to_json() {
        let bean = create_test_bean();
        let payload = HookPayload::new(HookEvent::PreCreate, bean.clone(), None);

        let json = payload.to_json().unwrap();
        assert!(json.contains("\"event\":\"pre-create\""));
        assert!(json.contains("\"id\":\"1\""));
        assert!(json.contains("\"title\":\"Test Bean\""));
        assert!(!json.contains("\"reason\"") || json.contains("\"reason\":null"));
    }

    #[test]
    fn test_hook_payload_with_reason() {
        let bean = create_test_bean();
        let payload = HookPayload::new(
            HookEvent::PreClose,
            bean,
            Some("Completed successfully".to_string()),
        );

        let json = payload.to_json().unwrap();
        assert!(json.contains("\"event\":\"pre-close\""));
        assert!(json.contains("\"reason\":\"Completed successfully\""));
    }

    #[test]
    fn test_get_hook_path() {
        let temp_dir = create_test_dir();
        let hook_path = get_hook_path(temp_dir.path(), HookEvent::PreCreate);

        assert!(hook_path.ends_with(".beans/hooks/pre-create"));
    }

    #[test]
    fn test_missing_hook_returns_ok_true() {
        let temp_dir = create_test_dir();
        let bean = create_test_bean();

        // No hook exists
        let result = execute_hook(HookEvent::PreCreate, &bean, temp_dir.path(), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn test_non_executable_hook_returns_error() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hook execution is attempted
        create_trust(beans_dir).unwrap();

        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();
        // File is not executable

        let bean = create_test_bean();
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not executable"));
    }

    #[test]
    fn test_successful_hook_execution() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hook execution is attempted
        create_trust(beans_dir).unwrap();

        let hook_path = hooks_dir.join("pre-create");
        // Use a simple script that just exits successfully, ignoring stdin
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = create_test_bean();
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);

        assert!(result.is_ok(), "Hook execution failed: {:?}", result.err());
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn test_hook_execution_with_failure_exit_code() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hook execution is attempted
        create_trust(beans_dir).unwrap();

        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = create_test_bean();
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);

        assert!(result.is_ok(), "Hook execution failed: {:?}", result.err());
        assert_eq!(result.unwrap(), false);
    }

    #[test]
    fn test_hook_receives_json_payload_on_stdin() {
        // Test that the payload can be serialized to JSON and would be sent to the hook
        let bean = create_test_bean();
        let payload = HookPayload::new(HookEvent::PreCreate, bean, None);

        let json = payload.to_json().unwrap();

        // Verify the JSON contains all expected fields
        assert!(json.contains("\"event\":\"pre-create\""));
        assert!(json.contains("\"bean\":{"));
        assert!(json.contains("\"id\":\"1\""));
        assert!(json.contains("\"title\":\"Test Bean\""));
        assert!(json.contains("\"status\":"));
    }

    #[test]
    #[cfg(unix)]
    fn test_hook_timeout() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust so hook execution is attempted
        create_trust(beans_dir).unwrap();

        let hook_path = hooks_dir.join("pre-create");
        // Script that sleeps for longer than timeout
        fs::write(&hook_path, "#!/bin/bash\nsleep 60\nexit 0").unwrap();

        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let bean = create_test_bean();
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[test]
    fn test_is_hook_executable_with_missing_file() {
        let temp_dir = create_test_dir();
        let hook_path = temp_dir.path().join("nonexistent");

        assert!(!is_hook_executable(&hook_path));
    }

    #[test]
    #[cfg(unix)]
    fn test_is_hook_executable_with_executable_file() {
        let temp_dir = create_test_dir();
        let hook_path = temp_dir.path().join("executable");
        fs::write(&hook_path, "#!/bin/bash\necho test").unwrap();

        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(is_hook_executable(&hook_path));
    }

    #[test]
    #[cfg(unix)]
    fn test_is_hook_executable_with_non_executable_file() {
        let temp_dir = create_test_dir();
        let hook_path = temp_dir.path().join("non-executable");
        fs::write(&hook_path, "#!/bin/bash\necho test").unwrap();

        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(!is_hook_executable(&hook_path));
    }

    #[test]
    fn test_hook_payload_with_all_bean_fields() {
        let mut bean = create_test_bean();
        bean.description = Some("Test description".to_string());
        bean.acceptance = Some("Test acceptance".to_string());
        bean.labels = vec!["test".to_string(), "important".to_string()];

        let payload = HookPayload::new(HookEvent::PostCreate, bean, None);
        let json = payload.to_json().unwrap();

        assert!(json.contains("description"));
        assert!(json.contains("Test description"));
        assert!(json.contains("labels"));
        assert!(json.contains("test"));
    }

    // =====================================================================
    // Trust Management Tests
    // =====================================================================

    #[test]
    fn test_is_trusted_returns_false_when_trust_file_does_not_exist() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();

        // Create .beans directory
        fs::create_dir_all(beans_dir.join(".beans")).unwrap();

        // Trust should not exist
        assert!(!is_trusted(beans_dir));
    }

    #[test]
    fn test_is_trusted_returns_true_when_trust_file_exists() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();

        // Create .beans directory and trust file
        fs::create_dir_all(beans_dir.join(".beans")).unwrap();
        fs::write(beans_dir.join(".beans").join(".hooks-trusted"), "").unwrap();

        // Trust should exist
        assert!(is_trusted(beans_dir));
    }

    #[test]
    fn test_create_trust_creates_trust_file() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();

        // Create .beans directory
        fs::create_dir_all(beans_dir.join(".beans")).unwrap();

        // Trust should not exist yet
        assert!(!is_trusted(beans_dir));

        // Create trust
        let result = create_trust(beans_dir);
        assert!(result.is_ok());

        // Trust should now exist
        assert!(is_trusted(beans_dir));

        // Verify file contains metadata
        let content = fs::read_to_string(beans_dir.join(".beans").join(".hooks-trusted")).unwrap();
        assert!(content.contains("Hooks enabled"));
    }

    #[test]
    fn test_revoke_trust_removes_trust_file() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();

        // Create .beans directory and trust file
        fs::create_dir_all(beans_dir.join(".beans")).unwrap();
        fs::write(beans_dir.join(".beans").join(".hooks-trusted"), "").unwrap();

        // Trust should exist
        assert!(is_trusted(beans_dir));

        // Revoke trust
        let result = revoke_trust(beans_dir);
        assert!(result.is_ok());

        // Trust should no longer exist
        assert!(!is_trusted(beans_dir));
    }

    #[test]
    fn test_revoke_trust_errors_if_file_does_not_exist() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();

        // Create .beans directory
        fs::create_dir_all(beans_dir.join(".beans")).unwrap();

        // Try to revoke non-existent trust
        let result = revoke_trust(beans_dir);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Trust file does not exist"));
    }

    #[test]
    fn test_execute_hook_skips_when_not_trusted() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Create an executable hook
        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = create_test_bean();

        // Hook should NOT execute (returns Ok(true) but doesn't run)
        // If trust is disabled, hook should not even check executability
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true); // Returns true but doesn't execute
    }

    #[test]
    fn test_execute_hook_runs_when_trusted() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust
        create_trust(beans_dir).unwrap();

        // Create an executable hook that succeeds
        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = create_test_bean();

        // Hook should execute successfully
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn test_execute_hook_respects_non_trusted_status() {
        let temp_dir = create_test_dir();
        let beans_dir = temp_dir.path();
        let hooks_dir = beans_dir.join(".beans").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Create a hook but DO NOT enable trust
        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bean = create_test_bean();

        // Hook should NOT execute (returns Ok(true) silently)
        let result = execute_hook(HookEvent::PreCreate, &bean, beans_dir, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }
}
