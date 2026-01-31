//! Editor integration for bn edit command.
//!
//! This module provides low-level file operations for editing beans:
//! - Launching an external editor subprocess
//! - Creating backups before editing
//! - Error handling for common editor failures

use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Open a file in the user's configured editor.
///
/// Reads the $EDITOR environment variable and spawns a subprocess with the file path.
/// Waits for the editor to exit and validates the exit status.
///
/// # Arguments
/// * `path` - Path to the file to edit
///
/// # Returns
/// * Ok(()) if editor exits successfully (status 0)
/// * Err if:
///   - $EDITOR environment variable is not set
///   - Editor executable is not found
///   - Editor process exits with non-zero status
///   - Editor subprocess crashes
///
/// # Examples
/// ```ignore
/// open_editor(Path::new(".beans/1-my-task.md"))?;
/// ```
pub fn open_editor(path: &Path) -> Result<()> {
    // Get EDITOR environment variable
    let editor = env::var("EDITOR")
        .context("$EDITOR environment variable not set. Please set it to your preferred editor (e.g., vim, nano, emacs)")?;

    // Ensure file exists before opening
    if !path.exists() {
        return Err(anyhow!(
            "File does not exist: {}",
            path.display()
        ));
    }

    // Convert path to string for error messages
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("Path contains invalid UTF-8: {}", path.display()))?;

    // Spawn editor subprocess
    let mut cmd = Command::new(&editor);
    cmd.arg(path_str);

    let status = cmd
        .status()
        .with_context(|| {
            anyhow!(
                "Failed to launch editor '{}'. Make sure it is installed and in your PATH",
                editor
            )
        })?;

    // Check exit status
    if !status.success() {
        let exit_code = status.code().unwrap_or(-1);
        return Err(anyhow!(
            "Editor '{}' exited with code {}",
            editor,
            exit_code
        ));
    }

    Ok(())
}

/// Load file content into memory as a backup before editing.
///
/// Reads the entire file content into a byte vector. This is used to detect
/// if the file was actually modified by comparing before/after content.
///
/// # Arguments
/// * `path` - Path to the file to backup
///
/// # Returns
/// * Ok(Vec<u8>) containing the file content
/// * Err if:
///   - File does not exist
///   - Permission denied reading the file
///   - I/O error occurs
///
/// # Examples
/// ```ignore
/// let backup = load_backup(Path::new(".beans/1-my-task.md"))?;
/// ```
pub fn load_backup(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| {
        anyhow!(
            "Failed to read file for backup: {}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_temp_file(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.md");
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        (dir, file_path)
    }

    #[test]
    fn test_load_backup_reads_content() {
        let (_dir, path) = create_temp_file("Hello, World!");
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, b"Hello, World!");
    }

    #[test]
    fn test_load_backup_reads_empty_file() {
        let (_dir, path) = create_temp_file("");
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup.len(), 0);
    }

    #[test]
    fn test_load_backup_reads_multiline_content() {
        let (_dir, path) = create_temp_file("Line 1\nLine 2\nLine 3");
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, b"Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_load_backup_reads_binary_content() {
        let (_dir, path) = create_temp_file("Binary\x00\x01\x02");
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, b"Binary\x00\x01\x02");
    }

    #[test]
    fn test_load_backup_nonexistent_file() {
        let path = Path::new("/nonexistent/path/to/file.md");
        let result = load_backup(path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to read file"));
    }

    #[test]
    fn test_load_backup_large_file() {
        let (_dir, path) = create_temp_file(&"X".repeat(1024 * 1024)); // 1MB file
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup.len(), 1024 * 1024);
    }

    #[test]
    fn test_open_editor_nonexistent_file() {
        env::set_var("EDITOR", "echo");
        let path = Path::new("/nonexistent/path/to/file.md");
        let result = open_editor(path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("does not exist"));
    }

    #[test]
    fn test_open_editor_success_with_echo() {
        // Use 'echo' as a harmless editor that exits successfully
        env::set_var("EDITOR", "echo");
        let (_dir, path) = create_temp_file("test content");
        let result = open_editor(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_editor_success_with_true() {
        // Use 'true' as a harmless editor that always succeeds
        env::set_var("EDITOR", "true");
        let (_dir, path) = create_temp_file("test content");
        let result = open_editor(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_backup_preserves_exact_content() {
        let test_content = "# Bean Title\n\nsome description\n\nstatus: open";
        let (_dir, path) = create_temp_file(test_content);

        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, test_content.as_bytes());
    }

    #[test]
    fn test_backup_backup_before_edit_workflow() {
        let original = "original content";
        let (_dir, path) = create_temp_file(original);

        // Simulate backup before edit
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, original.as_bytes());

        // Simulate file modification
        fs::write(&path, "modified content").unwrap();

        // Verify backup is unchanged
        assert_eq!(backup, original.as_bytes());

        // Verify file is modified
        let current = fs::read(&path).unwrap();
        assert_ne!(current, backup);
    }
}
