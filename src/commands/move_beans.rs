use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::bean::Bean;
use crate::config::Config;
use crate::discovery::find_bean_file;
use crate::index::Index;

/// Resolve a path to a `.beans/` directory.
///
/// Accepts either:
/// - A path ending in `.beans/` directly
/// - A project directory containing `.beans/`
fn resolve_beans_dir(path: &Path) -> Result<PathBuf> {
    if path.is_dir() && path.file_name().is_some_and(|n| n == ".beans") {
        return Ok(path.to_path_buf());
    }

    let candidate = path.join(".beans");
    if candidate.is_dir() {
        return Ok(candidate);
    }

    bail!(
        "No .beans/ directory found at '{}'\n\
         Pass the path to a .beans/ directory or the project directory containing it.",
        path.display()
    );
}

/// Move beans between two `.beans/` directories.
///
/// For each bean ID:
/// 1. Loads the bean from the source directory
/// 2. Assigns a new sequential ID in the destination (via `config.next_id`)
/// 3. Writes the bean to the destination with the new ID
/// 4. Removes the bean from the source
/// 5. Updates both source and destination indices
///
/// Returns a map of old_id → new_id.
fn move_beans(
    source_dir: &Path,
    dest_dir: &Path,
    ids: &[String],
) -> Result<HashMap<String, String>> {
    // Prevent moving beans into the same directory
    let source_canonical = source_dir.canonicalize().with_context(|| {
        format!(
            "Failed to resolve source path: {}",
            source_dir.display()
        )
    })?;
    let dest_canonical = dest_dir.canonicalize().with_context(|| {
        format!(
            "Failed to resolve destination path: {}",
            dest_dir.display()
        )
    })?;
    if source_canonical == dest_canonical {
        bail!("Source and destination are the same .beans/ directory");
    }

    // Load destination config to get next_id
    let mut dest_config =
        Config::load(dest_dir).context("Failed to load destination config")?;

    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut source_files_to_remove: Vec<PathBuf> = Vec::new();

    for old_id in ids {
        // Find and load the bean from source
        let source_path = find_bean_file(source_dir, old_id)
            .with_context(|| format!("Bean '{}' not found in {}", old_id, source_dir.display()))?;
        let mut bean = Bean::from_file(&source_path)
            .with_context(|| format!("Failed to load bean '{}' from source", old_id))?;

        // Assign a new ID in the destination
        let new_id = dest_config.increment_id().to_string();

        // Update bean fields
        bean.id = new_id.clone();
        bean.updated_at = Utc::now();

        // Clear source-specific fields that don't transfer cleanly
        bean.parent = None;
        bean.dependencies.clear();
        bean.claimed_by = None;
        bean.claimed_at = None;

        // Write to destination
        let slug = bean.slug.clone().unwrap_or_else(|| "unnamed".to_string());
        let dest_filename = format!("{}-{}.md", new_id, slug);
        let dest_path = dest_dir.join(&dest_filename);
        bean.to_file(&dest_path)
            .with_context(|| format!("Failed to write bean to {}", dest_path.display()))?;

        // Track for removal
        source_files_to_remove.push(source_path);
        id_map.insert(old_id.clone(), new_id.clone());

        eprintln!("Moved {} → {} ({})", old_id, new_id, bean.title);
    }

    // Save updated destination config (with incremented next_id)
    dest_config
        .save(dest_dir)
        .context("Failed to save destination config")?;

    // Remove source files
    for path in &source_files_to_remove {
        fs::remove_file(path)
            .with_context(|| format!("Failed to remove source file: {}", path.display()))?;
    }

    // Rebuild both indices
    let dest_index = Index::build(dest_dir)?;
    dest_index.save(dest_dir)?;

    if source_dir.join("config.yaml").exists() {
        let source_index = Index::build(source_dir)?;
        source_index.save(source_dir)?;
    }

    Ok(id_map)
}

/// Move beans from another `.beans/` directory into the current project.
///
/// `beans_dir` is the current project's `.beans/` (the destination).
pub fn cmd_move_from(
    beans_dir: &Path,
    from: &str,
    ids: &[String],
) -> Result<HashMap<String, String>> {
    let from_path = PathBuf::from(from);
    let source_dir = resolve_beans_dir(&from_path)
        .with_context(|| format!("Failed to resolve --from: {}", from))?;

    let result = move_beans(&source_dir, beans_dir, ids)?;

    eprintln!(
        "\nMoved {} bean{} from {} → {}",
        result.len(),
        if result.len() == 1 { "" } else { "s" },
        source_dir.display(),
        beans_dir.display(),
    );

    Ok(result)
}

/// Move beans from the current project into another `.beans/` directory.
///
/// `beans_dir` is the current project's `.beans/` (the source).
pub fn cmd_move_to(
    beans_dir: &Path,
    to: &str,
    ids: &[String],
) -> Result<HashMap<String, String>> {
    let to_path = PathBuf::from(to);
    let dest_dir = resolve_beans_dir(&to_path)
        .with_context(|| format!("Failed to resolve --to: {}", to))?;

    let result = move_beans(beans_dir, &dest_dir, ids)?;

    eprintln!(
        "\nMoved {} bean{} from {} → {}",
        result.len(),
        if result.len() == 1 { "" } else { "s" },
        beans_dir.display(),
        dest_dir.display(),
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_beans_dir(name: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let config = Config {
            project: name.to_string(),
            next_id: 1,
            auto_close_parent: true,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
            worktree: false,
            on_close: None,
            on_fail: None,
            post_plan: None,
            verify_timeout: None,
            review: None,
            user: None,
            user_email: None,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    fn create_test_bean(beans_dir: &Path, id: &str, title: &str) {
        let mut bean = Bean::new(id, title);
        bean.slug = Some(crate::util::title_to_slug(title));
        bean.verify = Some("true".to_string());
        let slug = bean.slug.clone().unwrap();
        bean.to_file(beans_dir.join(format!("{}-{}.md", id, slug)))
            .unwrap();
    }

    // =====================================================================
    // move_beans (core)
    // =====================================================================

    #[test]
    fn move_single_bean() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        create_test_bean(&src_beans, "1", "Fix login bug");

        let result = move_beans(&src_beans, &dst_beans, &["1".to_string()]).unwrap();

        assert_eq!(result.get("1"), Some(&"1".to_string()));
        assert!(!src_beans.join("1-fix-login-bug.md").exists());
        assert!(dst_beans.join("1-fix-login-bug.md").exists());

        let moved = Bean::from_file(dst_beans.join("1-fix-login-bug.md")).unwrap();
        assert_eq!(moved.id, "1");
        assert_eq!(moved.title, "Fix login bug");
        assert!(moved.parent.is_none());
        assert!(moved.dependencies.is_empty());
    }

    #[test]
    fn move_multiple_beans() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        let mut config = Config::load(&dst_beans).unwrap();
        config.next_id = 10;
        config.save(&dst_beans).unwrap();

        create_test_bean(&src_beans, "1", "Task one");
        create_test_bean(&src_beans, "2", "Task two");
        create_test_bean(&src_beans, "3", "Task three");

        let result = move_beans(
            &src_beans,
            &dst_beans,
            &["1".to_string(), "2".to_string(), "3".to_string()],
        )
        .unwrap();

        assert_eq!(result.get("1"), Some(&"10".to_string()));
        assert_eq!(result.get("2"), Some(&"11".to_string()));
        assert_eq!(result.get("3"), Some(&"12".to_string()));

        assert!(!src_beans.join("1-task-one.md").exists());
        assert!(dst_beans.join("10-task-one.md").exists());
        assert!(dst_beans.join("11-task-two.md").exists());
        assert!(dst_beans.join("12-task-three.md").exists());
    }

    #[test]
    fn move_clears_parent_and_deps() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        let mut bean = Bean::new("1.1", "Child task");
        bean.slug = Some("child-task".to_string());
        bean.verify = Some("true".to_string());
        bean.parent = Some("1".to_string());
        bean.dependencies = vec!["5".to_string(), "6".to_string()];
        bean.claimed_by = Some("agent-1".to_string());
        bean.to_file(src_beans.join("1.1-child-task.md")).unwrap();

        let result =
            move_beans(&src_beans, &dst_beans, &["1.1".to_string()]).unwrap();

        let new_id = result.get("1.1").unwrap();
        let moved = Bean::from_file(dst_beans.join(format!("{}-child-task.md", new_id))).unwrap();

        assert!(moved.parent.is_none());
        assert!(moved.dependencies.is_empty());
        assert!(moved.claimed_by.is_none());
        assert!(moved.claimed_at.is_none());
        assert_eq!(moved.title, "Child task");
        assert_eq!(moved.verify, Some("true".to_string()));
    }

    #[test]
    fn move_preserves_bean_content() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        let mut bean = Bean::new("1", "Complex task");
        bean.slug = Some("complex-task".to_string());
        bean.verify = Some("cargo test auth".to_string());
        bean.description = Some("Do the thing with the stuff".to_string());
        bean.acceptance = Some("All tests pass".to_string());
        bean.notes = Some("Tried X, failed. Avoid Y.".to_string());
        bean.labels = vec!["bug".to_string(), "auth".to_string()];
        bean.priority = 0;
        bean.to_file(src_beans.join("1-complex-task.md")).unwrap();

        let result =
            move_beans(&src_beans, &dst_beans, &["1".to_string()]).unwrap();

        let new_id = result.get("1").unwrap();
        let moved =
            Bean::from_file(dst_beans.join(format!("{}-complex-task.md", new_id))).unwrap();

        assert_eq!(moved.verify, Some("cargo test auth".to_string()));
        assert_eq!(
            moved.description,
            Some("Do the thing with the stuff".to_string())
        );
        assert_eq!(moved.acceptance, Some("All tests pass".to_string()));
        assert_eq!(
            moved.notes,
            Some("Tried X, failed. Avoid Y.".to_string())
        );
        assert_eq!(moved.labels, vec!["bug".to_string(), "auth".to_string()]);
        assert_eq!(moved.priority, 0);
    }

    #[test]
    fn move_fails_for_missing_bean() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        let result = move_beans(&src_beans, &dst_beans, &["999".to_string()]);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Bean '999' not found"));
    }

    #[test]
    fn move_fails_for_same_directory() {
        let (_dir, beans_dir) = setup_beans_dir("same");
        create_test_bean(&beans_dir, "1", "Task");

        let result = move_beans(&beans_dir, &beans_dir, &["1".to_string()]);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Source and destination are the same"));
    }

    #[test]
    fn move_updates_destination_config_next_id() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        create_test_bean(&src_beans, "1", "Task one");
        create_test_bean(&src_beans, "2", "Task two");

        move_beans(
            &src_beans,
            &dst_beans,
            &["1".to_string(), "2".to_string()],
        )
        .unwrap();

        let config = Config::load(&dst_beans).unwrap();
        assert_eq!(config.next_id, 3);
    }

    #[test]
    fn move_rebuilds_both_indices() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        create_test_bean(&src_beans, "1", "Task one");
        create_test_bean(&src_beans, "2", "Task two");

        move_beans(&src_beans, &dst_beans, &["1".to_string()]).unwrap();

        let src_index = Index::load(&src_beans).unwrap();
        assert_eq!(src_index.beans.len(), 1);
        assert_eq!(src_index.beans[0].id, "2");

        let dst_index = Index::load(&dst_beans).unwrap();
        assert_eq!(dst_index.beans.len(), 1);
        assert_eq!(dst_index.beans[0].title, "Task one");
    }

    // =====================================================================
    // cmd_move_from (pull direction)
    // =====================================================================

    #[test]
    fn move_from_with_beans_dir_path() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        create_test_bean(&src_beans, "1", "Some task");

        let result = cmd_move_from(
            &dst_beans,
            src_beans.to_str().unwrap(),
            &["1".to_string()],
        )
        .unwrap();

        assert_eq!(result.len(), 1);
    }

    #[test]
    fn move_from_with_project_dir_path() {
        let (src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        create_test_bean(&src_beans, "1", "Some task");

        let result = cmd_move_from(
            &dst_beans,
            src_dir.path().to_str().unwrap(),
            &["1".to_string()],
        )
        .unwrap();

        assert_eq!(result.len(), 1);
    }

    // =====================================================================
    // cmd_move_to (push direction)
    // =====================================================================

    #[test]
    fn move_to_pushes_beans() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (_dst_dir, dst_beans) = setup_beans_dir("dest");

        let mut config = Config::load(&dst_beans).unwrap();
        config.next_id = 50;
        config.save(&dst_beans).unwrap();

        create_test_bean(&src_beans, "1", "Push me");

        let result = cmd_move_to(
            &src_beans,
            dst_beans.to_str().unwrap(),
            &["1".to_string()],
        )
        .unwrap();

        assert_eq!(result.get("1"), Some(&"50".to_string()));
        assert!(!src_beans.join("1-push-me.md").exists());
        assert!(dst_beans.join("50-push-me.md").exists());
    }

    #[test]
    fn move_to_with_project_dir_path() {
        let (_src_dir, src_beans) = setup_beans_dir("source");
        let (dst_dir, dst_beans) = setup_beans_dir("dest");

        create_test_bean(&src_beans, "1", "Push task");

        let result = cmd_move_to(
            &src_beans,
            dst_dir.path().to_str().unwrap(),
            &["1".to_string()],
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(dst_beans.join("1-push-task.md").exists());
    }

    // =====================================================================
    // resolve_beans_dir
    // =====================================================================

    #[test]
    fn resolve_with_beans_dir() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let result = resolve_beans_dir(&beans_dir).unwrap();
        assert_eq!(result, beans_dir);
    }

    #[test]
    fn resolve_with_project_dir() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let result = resolve_beans_dir(dir.path()).unwrap();
        assert_eq!(result, beans_dir);
    }

    #[test]
    fn resolve_fails_for_no_beans() {
        let dir = TempDir::new().unwrap();
        let result = resolve_beans_dir(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn move_to_fails_for_invalid_dest() {
        let (_dir, src_beans) = setup_beans_dir("source");
        create_test_bean(&src_beans, "1", "Task");

        let result = cmd_move_to(&src_beans, "/nonexistent/path", &["1".to_string()]);
        assert!(result.is_err());
    }
}
