use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::Bean;
use crate::index::Index;
use crate::util::parse_status;

/// Update a bean's fields based on provided flags.
///
/// - title, description, acceptance, design, priority, assignee, status: replace
/// - notes: append with timestamp separator
/// - labels: add/remove operations
/// - updates updated_at and rebuilds index
pub fn cmd_update(
    beans_dir: &Path,
    id: &str,
    title: Option<String>,
    description: Option<String>,
    acceptance: Option<String>,
    notes: Option<String>,
    design: Option<String>,
    status: Option<String>,
    priority: Option<u8>,
    assignee: Option<String>,
    add_label: Option<String>,
    remove_label: Option<String>,
) -> Result<()> {
    // Validate priority if provided
    if let Some(p) = priority {
        crate::bean::validate_priority(p)?;
    }

    // Load the bean
    let bean_path = beans_dir.join(format!("{}.yaml", id));
    if !bean_path.exists() {
        return Err(anyhow!("Bean not found: {}", id));
    }

    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    // Apply updates
    if let Some(new_title) = title {
        bean.title = new_title;
    }

    if let Some(new_description) = description {
        bean.description = Some(new_description);
    }

    if let Some(new_acceptance) = acceptance {
        bean.acceptance = Some(new_acceptance);
    }

    if let Some(new_notes) = notes {
        // Append notes with timestamp separator
        let timestamp = Utc::now().to_rfc3339();
        if let Some(existing) = bean.notes {
            bean.notes = Some(format!("{}\n\n---\n{}\n{}", existing, timestamp, new_notes));
        } else {
            bean.notes = Some(format!("---\n{}\n{}", timestamp, new_notes));
        }
    }

    if let Some(new_design) = design {
        bean.design = Some(new_design);
    }

    if let Some(new_status) = status {
        bean.status = parse_status(&new_status)
            .ok_or_else(|| anyhow!("Invalid status: {}", new_status))?;
    }

    if let Some(new_priority) = priority {
        bean.priority = new_priority;
    }

    if let Some(new_assignee) = assignee {
        bean.assignee = Some(new_assignee);
    }

    if let Some(label) = add_label {
        if !bean.labels.contains(&label) {
            bean.labels.push(label);
        }
    }

    if let Some(label) = remove_label {
        bean.labels.retain(|l| l != &label);
    }

    // Update timestamp
    bean.updated_at = Utc::now();

    // Write back
    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    // Rebuild index
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    println!("Updated bean {}: {}", id, bean.title);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Status;
    use tempfile::TempDir;
    use std::fs;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_update_title() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Original title");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", Some("New title".to_string()), None, None, None, None, None, None, None, None, None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.title, "New title");
    }

    #[test]
    fn test_update_notes_appends() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Test");
        bean.notes = Some("First note".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", None, None, None, Some("Second note".to_string()), None, None, None, None, None, None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        let notes = updated.notes.unwrap();
        assert!(notes.contains("First note"));
        assert!(notes.contains("Second note"));
        assert!(notes.contains("---"));
    }

    #[test]
    fn test_update_notes_creates_with_timestamp() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", None, None, None, Some("First note".to_string()), None, None, None, None, None, None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        let notes = updated.notes.unwrap();
        assert!(notes.contains("First note"));
        assert!(notes.contains("---"));
        assert!(notes.contains("T")); // ISO 8601 has T for date-time
    }

    #[test]
    fn test_update_status() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", None, None, None, None, None, Some("in_progress".to_string()), None, None, None, None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
    }

    #[test]
    fn test_update_priority() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", None, None, None, None, None, None, Some(1), None, None, None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.priority, 1);
    }

    #[test]
    fn test_update_add_label() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", None, None, None, None, None, None, None, None, Some("urgent".to_string()), None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert!(updated.labels.contains(&"urgent".to_string()));
    }

    #[test]
    fn test_update_remove_label() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Test");
        bean.labels = vec!["urgent".to_string(), "bug".to_string()];
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", None, None, None, None, None, None, None, None, None, Some("urgent".to_string())).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert!(!updated.labels.contains(&"urgent".to_string()));
        assert!(updated.labels.contains(&"bug".to_string()));
    }

    #[test]
    fn test_update_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_update(&beans_dir, "99", Some("Title".to_string()), None, None, None, None, None, None, None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_multiple_fields() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Original");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_update(&beans_dir, "1", Some("New title".to_string()), Some("New desc".to_string()), None, None, None, Some("closed".to_string()), Some(0), None, None, None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.title, "New title");
        assert_eq!(updated.description, Some("New desc".to_string()));
        assert_eq!(updated.status, Status::Closed);
        assert_eq!(updated.priority, 0);
    }

    #[test]
    fn test_update_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Original");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Index doesn't exist yet
        assert!(!beans_dir.join("index.yaml").exists());

        cmd_update(&beans_dir, "1", Some("New title".to_string()), None, None, None, None, None, None, None, None, None).unwrap();

        // Index should be created
        assert!(beans_dir.join("index.yaml").exists());

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].title, "New title");
    }

    #[test]
    fn test_update_rejects_priority_too_high() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Test");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_update(&beans_dir, "1", None, None, None, None, None, None, Some(5), None, None, None);
        assert!(result.is_err(), "Should reject priority > 4");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("priority"), "Error should mention priority");
    }

    #[test]
    fn test_update_accepts_valid_priorities() {
        for priority in 0..=4 {
            let (_dir, beans_dir) = setup_test_beans_dir();
            let bean = Bean::new("1", "Test");
            bean.to_file(beans_dir.join("1.yaml")).unwrap();

            let result = cmd_update(&beans_dir, "1", None, None, None, None, None, None, Some(priority), None, None, None);
            assert!(result.is_ok(), "Priority {} should be valid", priority);

            let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
            assert_eq!(updated.priority, priority);
        }
    }
}
