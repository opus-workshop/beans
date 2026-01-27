use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::bean::Bean;
use crate::config::Config;
use crate::index::Index;

/// Create arguments structure for organizing all the parameters passed to create.
pub struct CreateArgs {
    pub title: String,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub notes: Option<String>,
    pub design: Option<String>,
    pub priority: Option<u8>,
    pub labels: Option<String>,
    pub assignee: Option<String>,
    pub deps: Option<String>,
    pub parent: Option<String>,
}

/// Assign a child ID for a parent bean.
/// Scans .beans/ for {parent_id}.*.yaml, finds highest N, returns "{parent_id}.{N+1}".
fn assign_child_id(beans_dir: &Path, parent_id: &str) -> Result<String> {
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

        // Look for files matching "{parent_id}.{N}.yaml"
        if let Some(name_without_ext) = filename.strip_suffix(".yaml") {
            if let Some(name_without_parent) = name_without_ext.strip_prefix(parent_id) {
                if name_without_parent.starts_with('.') {
                    if let Ok(child_num) = name_without_parent[1..].parse::<u32>() {
                        if child_num > max_child {
                            max_child = child_num;
                        }
                    }
                }
            }
        }
    }

    Ok(format!("{}.{}", parent_id, max_child + 1))
}

/// Create a new bean.
///
/// If `args.parent` is given, assign a child ID ({parent_id}.{next_child}).
/// Otherwise, use the next sequential ID from config and increment it.
pub fn cmd_create(beans_dir: &Path, args: CreateArgs) -> Result<()> {
    // Load config
    let mut config = Config::load(beans_dir)?;

    // Determine the bean ID
    let bean_id = if let Some(parent_id) = &args.parent {
        assign_child_id(beans_dir, parent_id)?
    } else {
        let id = config.increment_id();
        config.save(beans_dir)?;
        id.to_string()
    };

    // Create the bean
    let mut bean = Bean::new(&bean_id, &args.title);

    if let Some(desc) = args.description {
        bean.description = Some(desc);
    }
    if let Some(acceptance) = args.acceptance {
        bean.acceptance = Some(acceptance);
    }
    if let Some(notes) = args.notes {
        bean.notes = Some(notes);
    }
    if let Some(design) = args.design {
        bean.design = Some(design);
    }
    if let Some(priority) = args.priority {
        bean.priority = priority;
    }
    if let Some(assignee) = args.assignee {
        bean.assignee = Some(assignee);
    }
    if let Some(parent) = args.parent {
        bean.parent = Some(parent);
    }

    // Parse labels
    if let Some(labels_str) = args.labels {
        bean.labels = labels_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
    }

    // Parse dependencies
    if let Some(deps_str) = args.deps {
        bean.dependencies = deps_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
    }

    // Write the bean file
    let bean_path = beans_dir.join(format!("{}.yaml", bean_id));
    bean.to_file(&bean_path)?;

    // Update the index by rebuilding from disk (includes the bean we just wrote)
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    println!("Created bean {}: {}", bean_id, args.title);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_beans_dir_with_config() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let config = Config {
            project: "test".to_string(),
            next_id: 1,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn create_minimal_bean() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "First task".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        // Check the bean file exists
        let bean_path = beans_dir.join("1.yaml");
        assert!(bean_path.exists());

        // Verify content
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.id, "1");
        assert_eq!(bean.title, "First task");
    }

    #[test]
    fn create_increments_id() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create first bean
        let args1 = CreateArgs {
            title: "First".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
        };
        cmd_create(&beans_dir, args1).unwrap();

        // Create second bean
        let args2 = CreateArgs {
            title: "Second".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
        };
        cmd_create(&beans_dir, args2).unwrap();

        // Verify both exist with correct IDs
        let bean1 = Bean::from_file(&beans_dir.join("1.yaml")).unwrap();
        let bean2 = Bean::from_file(&beans_dir.join("2.yaml")).unwrap();
        assert_eq!(bean1.id, "1");
        assert_eq!(bean2.id, "2");
    }

    #[test]
    fn create_with_parent_assigns_child_id() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent bean
        let parent_args = CreateArgs {
            title: "Parent".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
        };
        cmd_create(&beans_dir, parent_args).unwrap();

        // Create child bean
        let child_args = CreateArgs {
            title: "Child 1".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: Some("1".to_string()),
        };
        cmd_create(&beans_dir, child_args).unwrap();

        // Verify child ID is 1.1
        let bean = Bean::from_file(&beans_dir.join("1.1.yaml")).unwrap();
        assert_eq!(bean.id, "1.1");
        assert_eq!(bean.parent, Some("1".to_string()));
    }

    #[test]
    fn create_multiple_children() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create parent
        let parent_args = CreateArgs {
            title: "Parent".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
        };
        cmd_create(&beans_dir, parent_args).unwrap();

        // Create multiple children
        for i in 1..=3 {
            let child_args = CreateArgs {
                title: format!("Child {}", i),
                description: None,
                acceptance: None,
                notes: None,
                design: None,
                priority: None,
                labels: None,
                assignee: None,
                deps: None,
                parent: Some("1".to_string()),
            };
            cmd_create(&beans_dir, child_args).unwrap();
        }

        // Verify all children exist
        for i in 1..=3 {
            let expected_id = format!("1.{}", i);
            let path = beans_dir.join(format!("{}.yaml", expected_id));
            assert!(path.exists(), "Child {} should exist", i);

            let bean = Bean::from_file(&path).unwrap();
            assert_eq!(bean.id, expected_id);
        }
    }

    #[test]
    fn create_with_all_fields() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Complex bean".to_string(),
            description: Some("A description".to_string()),
            acceptance: Some("All tests pass".to_string()),
            notes: Some("Some notes".to_string()),
            design: Some("Design decision".to_string()),
            priority: Some(1),
            labels: Some("bug,critical".to_string()),
            assignee: Some("alice".to_string()),
            deps: Some("2,3".to_string()),
            parent: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        let bean = Bean::from_file(&beans_dir.join("1.yaml")).unwrap();
        assert_eq!(bean.title, "Complex bean");
        assert_eq!(bean.description, Some("A description".to_string()));
        assert_eq!(bean.acceptance, Some("All tests pass".to_string()));
        assert_eq!(bean.notes, Some("Some notes".to_string()));
        assert_eq!(bean.design, Some("Design decision".to_string()));
        assert_eq!(bean.priority, 1);
        assert_eq!(bean.labels, vec!["bug", "critical"]);
        assert_eq!(bean.assignee, Some("alice".to_string()));
        assert_eq!(bean.dependencies, vec!["2", "3"]);
    }

    #[test]
    fn create_updates_index() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Indexed bean".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        // Load and check index
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].id, "1");
        assert_eq!(index.beans[0].title, "Indexed bean");
    }

    #[test]
    fn assign_child_id_starts_at_1() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let id = assign_child_id(&beans_dir, "parent").unwrap();
        assert_eq!(id, "parent.1");
    }

    #[test]
    fn assign_child_id_finds_existing_children() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create some child files
        let bean1 = Bean::new("parent.1", "Child 1");
        let bean2 = Bean::new("parent.2", "Child 2");
        let bean3 = Bean::new("parent.5", "Child 5");

        bean1.to_file(&beans_dir.join("parent.1.yaml")).unwrap();
        bean2.to_file(&beans_dir.join("parent.2.yaml")).unwrap();
        bean3.to_file(&beans_dir.join("parent.5.yaml")).unwrap();

        let id = assign_child_id(&beans_dir, "parent").unwrap();
        assert_eq!(id, "parent.6");
    }
}
