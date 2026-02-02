use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::bean::Bean;
use crate::discovery::find_bean_file;
use crate::index::Index;

/// Find the next available child number for a parent.
/// Scans .beans/ for existing children ({parent_id}.{N}-*.md or {parent_id}.{N}.yaml),
/// finds highest N, returns N+1.
fn next_child_number(beans_dir: &Path, parent_id: &str) -> Result<u32> {
    let mut max_child: u32 = 0;

    let dir_entries = fs::read_dir(beans_dir)
        .with_context(|| format!("Failed to read directory: {}", beans_dir.display()))?;

    for entry in dir_entries {
        let entry = entry?;
        let path = entry.path();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        // Look for files matching "{parent_id}.{N}-*.md" (new format)
        if let Some(name_without_ext) = filename.strip_suffix(".md") {
            if let Some(name_without_parent) = name_without_ext.strip_prefix(parent_id) {
                if let Some(after_dot) = name_without_parent.strip_prefix('.') {
                    // Extract the number part before the hyphen
                    let num_part = after_dot.split('-').next().unwrap_or_default();
                    if let Ok(child_num) = num_part.parse::<u32>() {
                        if child_num > max_child {
                            max_child = child_num;
                        }
                    }
                }
            }
        }

        // Also support legacy format for backward compatibility: {parent_id}.{N}.yaml
        if let Some(name_without_ext) = filename.strip_suffix(".yaml") {
            if let Some(name_without_parent) = name_without_ext.strip_prefix(parent_id) {
                if let Some(after_dot) = name_without_parent.strip_prefix('.') {
                    if let Ok(child_num) = after_dot.parse::<u32>() {
                        if child_num > max_child {
                            max_child = child_num;
                        }
                    }
                }
            }
        }
    }

    Ok(max_child + 1)
}

/// Adopt existing beans as children of a parent bean.
///
/// This command:
/// 1. Validates that the parent bean exists
/// 2. For each child ID:
///    - Loads the bean
///    - Assigns a new ID: `{parent_id}.{N}` (where N is sequential)
///    - Sets the bean's `parent` field to `parent_id`
///    - Renames the file to match the new ID
/// 3. Updates all dependency references across ALL beans
/// 4. Rebuilds the index
///
/// # Arguments
/// * `beans_dir` - Path to the `.beans/` directory
/// * `parent_id` - The ID of the parent bean
/// * `child_ids` - List of bean IDs to adopt as children
///
/// # Returns
/// A map of old_id -> new_id for the adopted beans
pub fn cmd_adopt(beans_dir: &Path, parent_id: &str, child_ids: &[String]) -> Result<HashMap<String, String>> {
    // Validate parent exists
    let parent_path = find_bean_file(beans_dir, parent_id)
        .with_context(|| format!("Parent bean '{}' not found", parent_id))?;
    let _parent_bean = Bean::from_file(&parent_path)
        .with_context(|| format!("Failed to load parent bean '{}'", parent_id))?;

    // Track ID mappings: old_id -> new_id
    let mut id_map: HashMap<String, String> = HashMap::new();

    // Find the starting child number
    let mut next_num = next_child_number(beans_dir, parent_id)?;

    // Process each child
    for old_id in child_ids {
        // Load the child bean
        let old_path = find_bean_file(beans_dir, old_id)
            .with_context(|| format!("Child bean '{}' not found", old_id))?;
        let mut bean = Bean::from_file(&old_path)
            .with_context(|| format!("Failed to load child bean '{}'", old_id))?;

        // Compute new ID
        let new_id = format!("{}.{}", parent_id, next_num);
        next_num += 1;

        // Update bean fields
        bean.id = new_id.clone();
        bean.parent = Some(parent_id.to_string());
        bean.updated_at = Utc::now();

        // Compute new file path
        let slug = bean.slug.clone().unwrap_or_else(|| "unnamed".to_string());
        let new_filename = format!("{}-{}.md", new_id, slug);
        let new_path = beans_dir.join(&new_filename);

        // Write the updated bean to the new path
        bean.to_file(&new_path)
            .with_context(|| format!("Failed to write bean to {}", new_path.display()))?;

        // Remove the old file (if it's different from the new path)
        if old_path != new_path {
            fs::remove_file(&old_path)
                .with_context(|| format!("Failed to remove old bean file {}", old_path.display()))?;
        }

        // Track the mapping
        id_map.insert(old_id.clone(), new_id.clone());
        println!("Adopted {} -> {} (under {})", old_id, new_id, parent_id);
    }

    // Update dependencies across all beans
    if !id_map.is_empty() {
        update_all_dependencies(beans_dir, &id_map)?;
    }

    // Rebuild the index
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    Ok(id_map)
}

/// Update dependency references in all beans based on the ID mapping.
///
/// Scans all bean files in the directory and replaces any dependency IDs
/// that appear in the id_map with their new values.
fn update_all_dependencies(beans_dir: &Path, id_map: &HashMap<String, String>) -> Result<()> {
    let dir_entries = fs::read_dir(beans_dir)
        .with_context(|| format!("Failed to read directory: {}", beans_dir.display()))?;

    for entry in dir_entries {
        let entry = entry?;
        let path = entry.path();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        // Only process bean files (.md with hyphen or .yaml)
        let is_bean_file = (filename.ends_with(".md") && filename.contains('-'))
            || (filename.ends_with(".yaml")
                && filename != "config.yaml"
                && filename != "index.yaml"
                && filename != "bean.yaml");

        if !is_bean_file {
            continue;
        }

        // Load the bean
        let mut bean = match Bean::from_file(&path) {
            Ok(b) => b,
            Err(_) => continue, // Skip files that can't be parsed
        };

        // Check if any dependencies need updating
        let mut modified = false;
        let mut new_deps = Vec::new();

        for dep in &bean.dependencies {
            if let Some(new_id) = id_map.get(dep) {
                new_deps.push(new_id.clone());
                modified = true;
            } else {
                new_deps.push(dep.clone());
            }
        }

        // Also check and update the parent field if it was remapped
        if let Some(ref parent) = bean.parent {
            if let Some(new_parent) = id_map.get(parent) {
                bean.parent = Some(new_parent.clone());
                modified = true;
            }
        }

        // Save if modified
        if modified {
            bean.dependencies = new_deps;
            bean.updated_at = Utc::now();
            bean.to_file(&path)
                .with_context(|| format!("Failed to update bean {}", path.display()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::TempDir;

    fn setup_beans_dir_with_config() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let config = Config {
            project: "test".to_string(),
            next_id: 10,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn adopt_single_bean() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent bean
        let mut parent = Bean::new("1", "Parent task");
        parent.slug = Some("parent-task".to_string());
        parent.acceptance = Some("Children complete".to_string());
        parent.to_file(beans_dir.join("1-parent-task.md")).unwrap();

        // Create child bean
        let mut child = Bean::new("2", "Child task");
        child.slug = Some("child-task".to_string());
        child.verify = Some("cargo test".to_string());
        child.to_file(beans_dir.join("2-child-task.md")).unwrap();

        // Adopt
        let result = cmd_adopt(&beans_dir, "1", &["2".to_string()]).unwrap();

        // Verify mapping
        assert_eq!(result.get("2"), Some(&"1.1".to_string()));

        // Verify old file is gone
        assert!(!beans_dir.join("2-child-task.md").exists());

        // Verify new file exists
        assert!(beans_dir.join("1.1-child-task.md").exists());

        // Verify bean content
        let adopted = Bean::from_file(beans_dir.join("1.1-child-task.md")).unwrap();
        assert_eq!(adopted.id, "1.1");
        assert_eq!(adopted.parent, Some("1".to_string()));
        assert_eq!(adopted.title, "Child task");
    }

    #[test]
    fn adopt_multiple_beans() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent
        let mut parent = Bean::new("1", "Parent");
        parent.slug = Some("parent".to_string());
        parent.acceptance = Some("All done".to_string());
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        // Create children
        for i in 2..=4 {
            let mut child = Bean::new(&i.to_string(), &format!("Child {}", i));
            child.slug = Some(format!("child-{}", i));
            child.verify = Some("true".to_string());
            child.to_file(beans_dir.join(format!("{}-child-{}.md", i, i))).unwrap();
        }

        // Adopt all three
        let result = cmd_adopt(&beans_dir, "1", &["2".to_string(), "3".to_string(), "4".to_string()]).unwrap();

        // Verify mappings (should be sequential)
        assert_eq!(result.get("2"), Some(&"1.1".to_string()));
        assert_eq!(result.get("3"), Some(&"1.2".to_string()));
        assert_eq!(result.get("4"), Some(&"1.3".to_string()));

        // Verify files
        assert!(beans_dir.join("1.1-child-2.md").exists());
        assert!(beans_dir.join("1.2-child-3.md").exists());
        assert!(beans_dir.join("1.3-child-4.md").exists());
    }

    #[test]
    fn adopt_with_existing_children() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent with existing child
        let mut parent = Bean::new("1", "Parent");
        parent.slug = Some("parent".to_string());
        parent.acceptance = Some("Done".to_string());
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        let mut existing_child = Bean::new("1.1", "Existing child");
        existing_child.slug = Some("existing-child".to_string());
        existing_child.parent = Some("1".to_string());
        existing_child.verify = Some("true".to_string());
        existing_child.to_file(beans_dir.join("1.1-existing-child.md")).unwrap();

        // Create new bean to adopt
        let mut new_bean = Bean::new("5", "New bean");
        new_bean.slug = Some("new-bean".to_string());
        new_bean.verify = Some("true".to_string());
        new_bean.to_file(beans_dir.join("5-new-bean.md")).unwrap();

        // Adopt - should get 1.2, not 1.1
        let result = cmd_adopt(&beans_dir, "1", &["5".to_string()]).unwrap();

        assert_eq!(result.get("5"), Some(&"1.2".to_string()));
        assert!(beans_dir.join("1.2-new-bean.md").exists());
    }

    #[test]
    fn adopt_updates_dependencies() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent
        let mut parent = Bean::new("1", "Parent");
        parent.slug = Some("parent".to_string());
        parent.acceptance = Some("Done".to_string());
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        // Create bean to adopt
        let mut to_adopt = Bean::new("2", "To adopt");
        to_adopt.slug = Some("to-adopt".to_string());
        to_adopt.verify = Some("true".to_string());
        to_adopt.to_file(beans_dir.join("2-to-adopt.md")).unwrap();

        // Create bean that depends on the one being adopted
        let mut dependent = Bean::new("3", "Dependent");
        dependent.slug = Some("dependent".to_string());
        dependent.verify = Some("true".to_string());
        dependent.dependencies = vec!["2".to_string()];
        dependent.to_file(beans_dir.join("3-dependent.md")).unwrap();

        // Adopt bean 2 under parent 1
        cmd_adopt(&beans_dir, "1", &["2".to_string()]).unwrap();

        // Verify dependent bean's dependencies were updated
        let dependent_updated = Bean::from_file(beans_dir.join("3-dependent.md")).unwrap();
        assert_eq!(dependent_updated.dependencies, vec!["1.1".to_string()]);
    }

    #[test]
    fn adopt_fails_for_missing_parent() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create only the child, no parent
        let mut child = Bean::new("2", "Child");
        child.slug = Some("child".to_string());
        child.verify = Some("true".to_string());
        child.to_file(beans_dir.join("2-child.md")).unwrap();

        // Try to adopt under non-existent parent
        let result = cmd_adopt(&beans_dir, "99", &["2".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Parent bean '99' not found"));
    }

    #[test]
    fn adopt_fails_for_missing_child() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create only the parent
        let mut parent = Bean::new("1", "Parent");
        parent.slug = Some("parent".to_string());
        parent.acceptance = Some("Done".to_string());
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        // Try to adopt non-existent child
        let result = cmd_adopt(&beans_dir, "1", &["99".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Child bean '99' not found"));
    }

    #[test]
    fn adopt_rebuilds_index() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent and child
        let mut parent = Bean::new("1", "Parent");
        parent.slug = Some("parent".to_string());
        parent.acceptance = Some("Done".to_string());
        parent.to_file(beans_dir.join("1-parent.md")).unwrap();

        let mut child = Bean::new("2", "Child");
        child.slug = Some("child".to_string());
        child.verify = Some("true".to_string());
        child.to_file(beans_dir.join("2-child.md")).unwrap();

        // Adopt
        cmd_adopt(&beans_dir, "1", &["2".to_string()]).unwrap();

        // Load index and verify
        let index = Index::load(&beans_dir).unwrap();
        
        // Should have 2 beans: parent (1) and adopted child (1.1)
        assert_eq!(index.beans.len(), 2);
        
        // Find the adopted bean in the index
        let adopted = index.beans.iter().find(|b| b.id == "1.1");
        assert!(adopted.is_some());
        assert_eq!(adopted.unwrap().parent, Some("1".to_string()));

        // Old ID should not be in index
        assert!(index.beans.iter().find(|b| b.id == "2").is_none());
    }

    #[test]
    fn next_child_number_empty() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let num = next_child_number(&beans_dir, "1").unwrap();
        assert_eq!(num, 1);
    }

    #[test]
    fn next_child_number_with_existing() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create existing children
        fs::write(beans_dir.join("1.1-child-one.md"), "test").unwrap();
        fs::write(beans_dir.join("1.2-child-two.md"), "test").unwrap();
        fs::write(beans_dir.join("1.5-child-five.md"), "test").unwrap();

        let num = next_child_number(&beans_dir, "1").unwrap();
        assert_eq!(num, 6); // Next after 5
    }

    #[test]
    fn next_child_number_ignores_other_parents() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create children under different parents
        fs::write(beans_dir.join("1.1-child.md"), "test").unwrap();
        fs::write(beans_dir.join("2.1-child.md"), "test").unwrap();
        fs::write(beans_dir.join("2.2-child.md"), "test").unwrap();

        // Should only count children of parent "1"
        let num = next_child_number(&beans_dir, "1").unwrap();
        assert_eq!(num, 2);
    }
}
