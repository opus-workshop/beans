use std::fs;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;

use crate::bean::Bean;
use crate::index::Index;

/// Delete a bean and clean up all references to it in other beans' dependencies.
///
/// 1. Load the bean to get its title (for printing)
/// 2. Delete the bean file
/// 3. Scan all remaining beans and remove deleted_id from their dependencies
/// 4. Rebuild the index
pub fn cmd_delete(beans_dir: &Path, id: &str) -> Result<()> {
    let bean_path = beans_dir.join(format!("{}.yaml", id));

    // Load the bean to get title before deleting
    let bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;
    let title = bean.title.clone();

    // Delete the bean file
    fs::remove_file(&bean_path)
        .with_context(|| format!("Failed to delete bean file: {}", id))?;

    // Clean up dependency references
    cleanup_dep_references(beans_dir, id)
        .with_context(|| format!("Failed to clean up dependency references for: {}", id))?;

    // Rebuild index
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    println!("Deleted bean {}: {}", id, title);
    Ok(())
}

/// Helper: scan all beans and remove deleted_id from their dependencies lists.
fn cleanup_dep_references(beans_dir: &Path, deleted_id: &str) -> Result<()> {
    let dir_entries = fs::read_dir(beans_dir)
        .with_context(|| format!("Failed to read directory: {}", beans_dir.display()))?;

    for entry in dir_entries {
        let entry = entry?;
        let path = entry.path();

        // Only process .yaml files
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("yaml") {
            continue;
        }

        // Skip excluded files
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if filename == "index.yaml" || filename == "config.yaml" || filename == "bean.yaml" {
            continue;
        }

        // Load the bean
        if let Ok(mut bean) = Bean::from_file(&path) {
            // Remove deleted_id from dependencies if present
            let original_len = bean.dependencies.len();
            bean.dependencies.retain(|dep| dep != deleted_id);

            // Only write if we actually removed something
            if bean.dependencies.len() < original_len {
                bean.to_file(&path)?;
            }
        }
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
    fn test_delete_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task to delete");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        assert!(beans_dir.join("1.yaml").exists());

        cmd_delete(&beans_dir, "1").unwrap();

        assert!(!beans_dir.join("1.yaml").exists());
    }

    #[test]
    fn test_delete_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_delete(&beans_dir, "99");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_cleans_dependencies() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Create beans with dependencies
        let bean1 = Bean::new("1", "Task 1");
        let mut bean2 = Bean::new("2", "Task 2");
        let mut bean3 = Bean::new("3", "Task 3");

        bean2.dependencies = vec!["1".to_string()];
        bean3.dependencies = vec!["1".to_string(), "2".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        // Delete bean 1
        cmd_delete(&beans_dir, "1").unwrap();

        // Verify bean 2 no longer depends on 1
        let bean2_updated = Bean::from_file(beans_dir.join("2.yaml")).unwrap();
        assert!(!bean2_updated.dependencies.contains(&"1".to_string()));

        // Verify bean 3 no longer depends on 1, but still depends on 2
        let bean3_updated = Bean::from_file(beans_dir.join("3.yaml")).unwrap();
        assert!(!bean3_updated.dependencies.contains(&"1".to_string()));
        assert!(bean3_updated.dependencies.contains(&"2".to_string()));
    }

    #[test]
    fn test_delete_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        cmd_delete(&beans_dir, "1").unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].id, "2");
    }

    #[test]
    fn test_cleanup_does_not_modify_unrelated_beans() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Create beans where only some depend on 1
        let bean1 = Bean::new("1", "Task 1");
        let mut bean2 = Bean::new("2", "Task 2");
        let bean3 = Bean::new("3", "Task 3"); // No dependencies

        bean2.dependencies = vec!["1".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        cmd_delete(&beans_dir, "1").unwrap();

        let bean3_check = Bean::from_file(beans_dir.join("3.yaml")).unwrap();
        assert!(bean3_check.dependencies.is_empty());
    }

    #[test]
    fn test_delete_with_complex_dependency_graph() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        // Create a diamond dependency: 4 <- [2, 3], 2 <- 1, 3 <- 1
        let bean1 = Bean::new("1", "Root");
        let mut bean2 = Bean::new("2", "Middle left");
        let mut bean3 = Bean::new("3", "Middle right");
        let mut bean4 = Bean::new("4", "Bottom");

        bean2.dependencies = vec!["1".to_string()];
        bean3.dependencies = vec!["1".to_string()];
        bean4.dependencies = vec!["2".to_string(), "3".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();
        bean4.to_file(beans_dir.join("4.yaml")).unwrap();

        // Delete node 1
        cmd_delete(&beans_dir, "1").unwrap();

        // Verify cleanup
        let bean2_updated = Bean::from_file(beans_dir.join("2.yaml")).unwrap();
        let bean3_updated = Bean::from_file(beans_dir.join("3.yaml")).unwrap();
        let bean4_updated = Bean::from_file(beans_dir.join("4.yaml")).unwrap();

        assert!(!bean2_updated.dependencies.contains(&"1".to_string()));
        assert!(!bean3_updated.dependencies.contains(&"1".to_string()));
        assert!(!bean4_updated.dependencies.contains(&"1".to_string()));
        assert!(bean4_updated.dependencies.contains(&"2".to_string()));
        assert!(bean4_updated.dependencies.contains(&"3".to_string()));
    }

    #[test]
    fn test_delete_ignores_excluded_files() {
        let (_dir, beans_dir) = setup_test_beans_dir();

        let bean1 = Bean::new("1", "Task 1");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();

        // Create config.yaml with a fake reference to "1"
        fs::write(
            beans_dir.join("config.yaml"),
            "next_id: 2\nproject_name: test\n",
        )
        .unwrap();

        // This should not fail even though config.yaml exists
        cmd_delete(&beans_dir, "1").unwrap();
        assert!(!beans_dir.join("1.yaml").exists());
    }
}
