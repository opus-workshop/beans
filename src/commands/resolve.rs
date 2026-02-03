//! Resolve command for manual conflict resolution.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::bean::{Bean, ConflictResolution};
use crate::discovery::find_bean_file;
use crate::index::Index;

/// Resolve a field conflict on a bean by choosing one of the versions.
///
/// # Arguments
/// * `beans_dir` - Path to the .beans directory
/// * `id` - Bean ID
/// * `field` - Field name with conflict
/// * `choice` - Index of the version to keep (0, 1, ...)
///
/// # Returns
/// * `Ok(())` on success
/// * `Err` if bean not found, no conflict on field, or invalid choice
pub fn cmd_resolve(
    beans_dir: &Path,
    id: &str,
    field: &str,
    choice: usize,
) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)
        .with_context(|| format!("Bean not found: {}", id))?;
    
    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    // Find conflict for this field
    let conflict_idx = bean.conflicts.iter()
        .position(|c| c.field == field)
        .ok_or_else(|| anyhow!("No conflict for field: {}", field))?;

    let conflict = &bean.conflicts[conflict_idx];

    if choice >= conflict.versions.len() {
        return Err(anyhow!(
            "Invalid choice: {}. Available: 0-{}",
            choice,
            conflict.versions.len() - 1
        ));
    }

    // Get chosen value
    let chosen_value = conflict.versions[choice].value.clone();

    // Apply to bean
    bean.apply_value(field, &chosen_value)
        .with_context(|| format!("Failed to apply value to field: {}", field))?;

    // Mark resolved and remove from conflicts list
    bean.conflicts[conflict_idx].resolution = ConflictResolution::Resolved;
    bean.conflicts.retain(|c| c.resolution == ConflictResolution::Pending);

    // Save
    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    // Rebuild index
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    println!("✓ Resolved conflict for bean {} field '{}'", id, field);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::{FieldConflict, ConflictVersion, Status};
    use crate::util::title_to_slug;
    use chrono::Utc;
    use tempfile::TempDir;
    use std::fs;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_resolve_conflict_basic() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "title".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"Version A\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "\"Version B\"".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Resolve by choosing version 1 (Version B)
        cmd_resolve(&beans_dir, "1", "title", 1).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.title, "Version B");
        assert!(updated.conflicts.is_empty()); // Conflict removed
    }

    #[test]
    fn test_resolve_conflict_choose_first_version() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Original Title");
        bean.conflicts = vec![
            FieldConflict {
                field: "title".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"First Choice\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "\"Second Choice\"".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Resolve by choosing version 0 (First Choice)
        cmd_resolve(&beans_dir, "1", "title", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.title, "First Choice");
    }

    #[test]
    fn test_resolve_invalid_choice() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "title".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"Version A\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Choice 5 is invalid (only 1 version)
        let result = cmd_resolve(&beans_dir, "1", "title", 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid choice"));
    }

    #[test]
    fn test_resolve_no_conflict_on_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "title".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"Version A\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Try to resolve a field that has no conflict
        let result = cmd_resolve(&beans_dir, "1", "status", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No conflict for field"));
    }

    #[test]
    fn test_resolve_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let result = cmd_resolve(&beans_dir, "99", "title", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Bean not found"));
    }

    #[test]
    fn test_resolve_updates_timestamp() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        let original_updated_at = bean.updated_at;
        bean.conflicts = vec![
            FieldConflict {
                field: "title".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"New Title\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        cmd_resolve(&beans_dir, "1", "title", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert!(updated.updated_at > original_updated_at);
    }

    #[test]
    fn test_resolve_status_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "status".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"in_progress\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "\"closed\"".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_resolve(&beans_dir, "1", "status", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.status, Status::InProgress);
    }

    #[test]
    fn test_resolve_priority_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "priority".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "1".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "4".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_resolve(&beans_dir, "1", "priority", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.priority, 1);
    }

    #[test]
    fn test_resolve_description_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "description".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"Description A\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "null".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_resolve(&beans_dir, "1", "description", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.description, Some("Description A".to_string()));
    }

    #[test]
    fn test_resolve_labels_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "labels".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "[\"bug\",\"urgent\"]".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "[\"feature\"]".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_resolve(&beans_dir, "1", "labels", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.labels, vec!["bug".to_string(), "urgent".to_string()]);
    }

    #[test]
    fn test_resolve_multiple_conflicts_one_at_a_time() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "title".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"New Title\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
            FieldConflict {
                field: "priority".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "1".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        // Resolve title first
        cmd_resolve(&beans_dir, "1", "title", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.title, "New Title");
        assert_eq!(updated.conflicts.len(), 1); // One conflict remaining
        assert_eq!(updated.conflicts[0].field, "priority");

        // Now resolve priority
        cmd_resolve(&beans_dir, "1", "priority", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.priority, 1);
        assert!(updated.conflicts.is_empty()); // All conflicts resolved
    }

    #[test]
    fn test_resolve_dependencies_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "dependencies".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "[\"2\",\"3\"]".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "[\"4\"]".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_resolve(&beans_dir, "1", "dependencies", 0).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.dependencies, vec!["2".to_string(), "3".to_string()]);
    }

    #[test]
    fn test_resolve_assignee_field() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        
        let mut bean = Bean::new("1", "Test Bean");
        bean.conflicts = vec![
            FieldConflict {
                field: "assignee".to_string(),
                versions: vec![
                    ConflictVersion {
                        value: "\"alice\"".to_string(),
                        agent: "agent1".to_string(),
                        timestamp: Utc::now(),
                    },
                    ConflictVersion {
                        value: "\"bob\"".to_string(),
                        agent: "agent2".to_string(),
                        timestamp: Utc::now(),
                    },
                ],
                resolution: ConflictResolution::Pending,
            },
        ];
        let slug = title_to_slug(&bean.title);
        bean.to_file(beans_dir.join(format!("1-{}.md", slug))).unwrap();

        cmd_resolve(&beans_dir, "1", "assignee", 1).unwrap();

        let updated = Bean::from_file(crate::discovery::find_bean_file(&beans_dir, "1").unwrap()).unwrap();
        assert_eq!(updated.assignee, Some("bob".to_string()));
    }
}
