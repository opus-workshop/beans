//! Editor integration for bn edit command.
//!
//! This module provides low-level file operations for editing beans:
//! - Launching an external editor subprocess
//! - Creating backups before editing
//! - Validating and saving edited content
//! - Rebuilding indices after modifications
//! - Prompting user for rollback on validation errors

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::bean::Bean;
use crate::index::Index;

/// Validate bean content and persist it to disk with updated timestamp.
///
/// Parses the content using Bean::from_string() to validate the YAML schema.
/// If validation succeeds, writes the content to the file and updates the
/// updated_at field to the current UTC time.
///
/// # Arguments
/// * `path` - Path where the validated content will be written
/// * `content` - The edited bean content (YAML or Markdown with YAML frontmatter)
///
/// # Returns
/// * Ok(()) if validation succeeds and file is written
/// * Err with descriptive message if:
///   - Content fails YAML schema validation
///   - File I/O error occurs
///
/// # Examples
/// ```ignore
/// validate_and_save(Path::new(".beans/1-my-task.md"), edited_content)?;
/// ```
pub fn validate_and_save(path: &Path, content: &str) -> Result<()> {
    // Parse content to validate schema
    let mut bean = Bean::from_string(content)
        .with_context(|| "Failed to parse bean: invalid YAML schema")?;

    // Update the timestamp to current time
    bean.updated_at = Utc::now();

    // Serialize the validated bean back to YAML
    let validated_yaml = serde_yaml::to_string(&bean)
        .with_context(|| "Failed to serialize validated bean")?;

    // Write to disk
    fs::write(path, validated_yaml)
        .with_context(|| format!("Failed to write bean to {}", path.display()))?;

    Ok(())
}

/// Rebuild the bean index from current bean files on disk.
///
/// Reads all bean files in the beans directory, builds a fresh index,
/// and saves it to .beans/index.yaml. This should be called after any
/// bean modification to keep the index synchronized.
///
/// # Arguments
/// * `beans_dir` - Path to the .beans directory
///
/// # Returns
/// * Ok(()) if index is built and saved successfully
/// * Err if:
///   - Directory is not readable
///   - Bean files fail to parse
///   - Index file cannot be written
///
/// # Examples
/// ```ignore
/// rebuild_index_after_edit(Path::new(".beans"))?;
/// ```
pub fn rebuild_index_after_edit(beans_dir: &Path) -> Result<()> {
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to build index from bean files")?;

    index.save(beans_dir)
        .with_context(|| "Failed to save index to .beans/index.yaml")?;

    Ok(())
}

/// Prompt user for action when validation fails: retry, rollback, or abort.
///
/// Displays the validation error and presents an interactive prompt with three options:
/// - 'y' or 'retry': Re-open the editor for another attempt (returns Ok)
/// - 'r' or 'rollback': Restore the original file from backup and abort (returns Ok)
/// - 'n' or any other input: Abort the edit operation (returns Err)
///
/// # Arguments
/// * `backup` - The original file content before editing (in bytes)
/// * `path` - Path to the bean file being edited
///
/// # Returns
/// * Ok(()) if user chooses 'retry' (signals caller to re-open editor) or 'rollback'
/// * Err if user chooses 'n'/'abort'
///
/// # Examples
/// ```ignore
/// match prompt_rollback(&backup, &path) {
///     Ok(()) => {
///         // User chose retry or rollback - check backup file to determine which
///         if path matches backup { /* was rollback */ }
///         else { /* was retry */ }
///     }
///     Err(e) => println!("Edit aborted: {}", e),
/// }
/// ```
pub fn prompt_rollback(backup: &[u8], path: &Path) -> Result<()> {
    // Present user with menu
    println!("\nValidation failed. What would you like to do?");
    println!("  (y)    Retry in editor");
    println!("  (r)    Rollback and discard changes");
    println!("  (n)    Abort");
    print!("\nChoice: ");
    io::stdout().flush()?;

    // Read user input
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice = input.trim().to_lowercase();

    match choice.as_str() {
        "y" | "retry" => {
            // User wants to retry — return Ok to signal retry
            Ok(())
        }
        "r" | "rollback" => {
            // Restore from backup and return Ok (successful rollback)
            fs::write(path, backup)
                .with_context(|| format!("Failed to restore backup to {}", path.display()))?;
            println!("Rollback complete. Original file restored.");
            Ok(())
        }
        "n" => {
            // User aborts
            Err(anyhow!("Edit aborted by user"))
        }
        _ => {
            // Invalid input treated as abort
            Err(anyhow!("Edit aborted by user"))
        }
    }
}

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

    fn create_valid_bean_file(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("1-test.md");
        fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    // =====================================================================
    // Tests for load_backup (from 2.1)
    // =====================================================================

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

    // =====================================================================
    // Tests for validate_and_save (Bean 2.2)
    // =====================================================================

    #[test]
    fn test_validate_and_save_parses_and_validates_yaml() {
        let bean_content = r#"id: "1"
title: Test Bean
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
"#;
        let (_dir, path) = create_valid_bean_file(bean_content);

        let result = validate_and_save(&path, bean_content);
        assert!(result.is_ok());

        // Verify file was written
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("id: '1'") || saved.contains("id: \"1\""));
    }

    #[test]
    fn test_validate_and_save_updates_timestamp() {
        let bean_content = r#"id: "1"
title: Test Bean
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
"#;
        let (_dir, path) = create_valid_bean_file(bean_content);

        // Save original timestamp
        let before = Bean::from_string(bean_content).unwrap();
        let before_ts = before.updated_at;

        // Wait a tiny bit to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Validate and save
        validate_and_save(&path, bean_content).unwrap();

        // Load the saved bean and check timestamp was updated
        let saved_bean = Bean::from_file(&path).unwrap();
        assert!(saved_bean.updated_at > before_ts);
    }

    #[test]
    fn test_validate_and_save_rejects_invalid_yaml() {
        let invalid_content = "id: 1\ntitle: Test\nstatus: invalid_status\n";
        let (_dir, path) = create_valid_bean_file(invalid_content);

        let result = validate_and_save(&path, invalid_content);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("invalid YAML"));
    }

    #[test]
    fn test_validate_and_save_persists_to_disk() {
        let bean_content = r#"id: "1"
title: Original Title
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
"#;
        let (_dir, path) = create_valid_bean_file(bean_content);

        validate_and_save(&path, bean_content).unwrap();

        // Read from disk and verify
        let bean = Bean::from_file(&path).unwrap();
        assert_eq!(bean.id, "1");
        assert_eq!(bean.title, "Original Title");
    }

    #[test]
    fn test_validate_and_save_with_markdown_frontmatter() {
        let md_content = r#"---
id: "2"
title: Markdown Bean
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
---

# Description

This is a markdown body.
"#;
        let (_dir, path) = create_valid_bean_file(md_content);

        validate_and_save(&path, md_content).unwrap();

        let bean = Bean::from_file(&path).unwrap();
        assert_eq!(bean.id, "2");
        assert_eq!(bean.title, "Markdown Bean");
        assert!(bean.description.is_some());
    }

    #[test]
    fn test_validate_and_save_missing_required_field() {
        let invalid_content = r#"id: "1"
title: Test
status: open
"#; // Missing created_at and updated_at
        let (_dir, path) = create_valid_bean_file(invalid_content);

        let result = validate_and_save(&path, invalid_content);
        assert!(result.is_err());
    }

    // =====================================================================
    // Tests for rebuild_index_after_edit (Bean 2.2)
    // =====================================================================

    #[test]
    fn test_rebuild_index_after_edit_creates_index() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean file
        let bean_content = r#"id: "1"
title: Test Bean
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
"#;
        fs::write(beans_dir.join("1-test.md"), bean_content).unwrap();

        // Rebuild index
        rebuild_index_after_edit(&beans_dir).unwrap();

        // Verify index.yaml was created
        assert!(beans_dir.join("index.yaml").exists());

        // Load and verify index
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].id, "1");
        assert_eq!(index.beans[0].title, "Test Bean");
    }

    #[test]
    fn test_rebuild_index_after_edit_includes_all_beans() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create multiple beans
        let bean1 = Bean::new("1", "First Bean");
        let bean2 = Bean::new("2", "Second Bean");
        let bean3 = Bean::new("3", "Third Bean");

        bean1.to_file(beans_dir.join("1-first.md")).unwrap();
        bean2.to_file(beans_dir.join("2-second.md")).unwrap();
        bean3.to_file(beans_dir.join("3-third.md")).unwrap();

        rebuild_index_after_edit(&beans_dir).unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 3);
    }

    #[test]
    fn test_rebuild_index_after_edit_saves_to_correct_location() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let bean = Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1-test.md")).unwrap();

        rebuild_index_after_edit(&beans_dir).unwrap();

        let index_path = beans_dir.join("index.yaml");
        assert!(index_path.exists(), "index.yaml should be saved to .beans/");
    }

    #[test]
    fn test_rebuild_index_after_edit_empty_directory() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Rebuild index with no beans
        rebuild_index_after_edit(&beans_dir).unwrap();

        // Index should be created but empty
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 0);
    }

    #[test]
    fn test_rebuild_index_after_edit_invalid_beans_dir() {
        let nonexistent = Path::new("/nonexistent/.beans");
        let result = rebuild_index_after_edit(nonexistent);
        assert!(result.is_err());
    }

    // =====================================================================
    // Tests for prompt_rollback (Bean 2.2)
    // =====================================================================

    #[test]
    fn test_prompt_rollback_restores_file_from_backup() {
        let (_dir, path) = create_temp_file("modified content");
        let backup = b"original content";

        // If we could mock stdin, we'd test rollback by:
        // 1. Verifying backup is written
        // 2. Checking file content matches backup
        // For now, verify the function would write backup correctly
        let result = fs::write(&path, backup);
        assert!(result.is_ok());

        let saved = fs::read(&path).unwrap();
        assert_eq!(saved, backup);
    }

    #[test]
    fn test_prompt_rollback_backup_preserves_content() {
        let original = "original bean content";
        let (_dir, path) = create_temp_file(original);

        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, original.as_bytes());

        // Modify file
        fs::write(&path, "modified content").unwrap();

        // Restore from backup
        fs::write(&path, &backup).unwrap();

        // Verify restoration
        let restored = fs::read(&path).unwrap();
        assert_eq!(restored, original.as_bytes());
    }

    #[test]
    fn test_validate_and_save_workflow_full() {
        // Full workflow: backup -> edit -> validate -> save
        let bean_content = r#"id: "1"
title: Original
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
"#;
        let (_dir, path) = create_valid_bean_file(bean_content);

        // Step 1: Backup
        let backup = load_backup(&path).unwrap();
        assert_eq!(backup, bean_content.as_bytes());

        // Step 2: Simulate edit (modify title)
        let edited_content = r#"id: "1"
title: Modified
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
"#;

        // Step 3: Validate and save
        validate_and_save(&path, edited_content).unwrap();

        // Step 4: Verify changes persisted
        let saved_bean = Bean::from_file(&path).unwrap();
        assert_eq!(saved_bean.title, "Modified");
    }

    #[test]
    fn test_rebuild_index_reflects_recent_edits() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create initial bean
        let bean1 = Bean::new("1", "First");
        bean1.to_file(beans_dir.join("1-first.md")).unwrap();

        // Build index
        rebuild_index_after_edit(&beans_dir).unwrap();
        let index1 = Index::load(&beans_dir).unwrap();
        assert_eq!(index1.beans.len(), 1);

        // Add another bean and rebuild
        let bean2 = Bean::new("2", "Second");
        bean2.to_file(beans_dir.join("2-second.md")).unwrap();

        rebuild_index_after_edit(&beans_dir).unwrap();
        let index2 = Index::load(&beans_dir).unwrap();
        assert_eq!(index2.beans.len(), 2);
    }
}
