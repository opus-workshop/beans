//! Integration tests for the `bn adopt` command.
//!
//! These tests verify the public API for adopting existing beans as children
//! of a parent bean.

use std::fs;

use bn::bean::Bean;
use bn::commands::cmd_adopt;
use bn::config::Config;
use bn::index::Index;
use tempfile::TempDir;

/// Setup a test environment with a .beans directory and config.
fn setup_test_env() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let beans_dir = dir.path().join(".beans");
    fs::create_dir(&beans_dir).unwrap();

    let config = Config {
        project: "test-adopt".to_string(),
        next_id: 100,
        auto_close_parent: true,
    };
    config.save(&beans_dir).unwrap();

    (dir, beans_dir)
}

/// Helper to create a bean with standard fields.
fn create_bean(beans_dir: &std::path::Path, id: &str, title: &str, is_parent: bool) {
    let mut bean = Bean::new(id, title);
    let slug = title.to_lowercase().replace(' ', "-");
    bean.slug = Some(slug.clone());

    if is_parent {
        bean.acceptance = Some("All children complete".to_string());
    } else {
        bean.verify = Some("true".to_string());
    }

    let filename = format!("{}-{}.md", id, slug);
    bean.to_file(beans_dir.join(filename)).unwrap();
}

#[test]
fn test_adopt_basic_single() {
    let (_dir, beans_dir) = setup_test_env();

    // Create parent (100) and child to adopt (101)
    create_bean(&beans_dir, "100", "Parent Task", true);
    create_bean(&beans_dir, "101", "Child Task", false);

    // Adopt: 101 should become 100.1
    let result = cmd_adopt(&beans_dir, "100", &["101".to_string()]).unwrap();

    // Verify the ID mapping
    assert_eq!(result.get("101"), Some(&"100.1".to_string()));

    // Old file should be gone
    assert!(!beans_dir.join("101-child-task.md").exists());

    // New file should exist
    assert!(beans_dir.join("100.1-child-task.md").exists());

    // Verify bean content
    let adopted = Bean::from_file(beans_dir.join("100.1-child-task.md")).unwrap();
    assert_eq!(adopted.id, "100.1");
    assert_eq!(adopted.parent, Some("100".to_string()));
    assert_eq!(adopted.title, "Child Task");
}

#[test]
fn test_adopt_multiple_children() {
    let (_dir, beans_dir) = setup_test_env();

    // Create parent (100) and three children to adopt (101, 102, 103)
    create_bean(&beans_dir, "100", "Parent", true);
    create_bean(&beans_dir, "101", "First", false);
    create_bean(&beans_dir, "102", "Second", false);
    create_bean(&beans_dir, "103", "Third", false);

    // Adopt all three: they should become 100.1, 100.2, 100.3
    let result = cmd_adopt(
        &beans_dir,
        "100",
        &["101".to_string(), "102".to_string(), "103".to_string()],
    )
    .unwrap();

    // Verify sequential numbering
    assert_eq!(result.get("101"), Some(&"100.1".to_string()));
    assert_eq!(result.get("102"), Some(&"100.2".to_string()));
    assert_eq!(result.get("103"), Some(&"100.3".to_string()));

    // All new files should exist
    assert!(beans_dir.join("100.1-first.md").exists());
    assert!(beans_dir.join("100.2-second.md").exists());
    assert!(beans_dir.join("100.3-third.md").exists());

    // All old files should be removed
    assert!(!beans_dir.join("101-first.md").exists());
    assert!(!beans_dir.join("102-second.md").exists());
    assert!(!beans_dir.join("103-third.md").exists());
}

#[test]
fn test_adopt_files_renamed_correctly() {
    let (_dir, beans_dir) = setup_test_env();

    create_bean(&beans_dir, "100", "Parent", true);

    // Create a bean with a specific slug
    let mut bean = Bean::new("101", "My Complex Task Name");
    bean.slug = Some("my-complex-task-name".to_string());
    bean.verify = Some("echo ok".to_string());
    bean.to_file(beans_dir.join("101-my-complex-task-name.md"))
        .unwrap();

    cmd_adopt(&beans_dir, "100", &["101".to_string()]).unwrap();

    // Verify new filename preserves the slug
    assert!(beans_dir.join("100.1-my-complex-task-name.md").exists());

    // Verify content is preserved
    let adopted = Bean::from_file(beans_dir.join("100.1-my-complex-task-name.md")).unwrap();
    assert_eq!(adopted.slug, Some("my-complex-task-name".to_string()));
    assert_eq!(adopted.verify, Some("echo ok".to_string()));
}

#[test]
fn test_adopt_updates_dependency_references() {
    let (_dir, beans_dir) = setup_test_env();

    // Create parent
    create_bean(&beans_dir, "100", "Parent", true);

    // Create bean to adopt (101)
    create_bean(&beans_dir, "101", "Task A", false);

    // Create bean that depends on 101
    let mut dependent = Bean::new("102", "Task B");
    dependent.slug = Some("task-b".to_string());
    dependent.verify = Some("true".to_string());
    dependent.dependencies = vec!["101".to_string()];
    dependent.to_file(beans_dir.join("102-task-b.md")).unwrap();

    // Adopt 101 under 100
    cmd_adopt(&beans_dir, "100", &["101".to_string()]).unwrap();

    // Dependency in bean 102 should now point to 100.1
    let updated = Bean::from_file(beans_dir.join("102-task-b.md")).unwrap();
    assert_eq!(updated.dependencies, vec!["100.1".to_string()]);
}

#[test]
fn test_adopt_updates_index() {
    let (_dir, beans_dir) = setup_test_env();

    create_bean(&beans_dir, "100", "Parent", true);
    create_bean(&beans_dir, "101", "Child", false);

    cmd_adopt(&beans_dir, "100", &["101".to_string()]).unwrap();

    // Load and verify the index
    let index = Index::load(&beans_dir).unwrap();

    // Should have 2 beans: parent and adopted child
    assert_eq!(index.beans.len(), 2);

    // Adopted bean should have new ID in index
    let adopted = index.beans.iter().find(|b| b.id == "100.1");
    assert!(adopted.is_some());
    assert_eq!(adopted.unwrap().parent, Some("100".to_string()));

    // Old ID should not exist in index
    assert!(index.beans.iter().find(|b| b.id == "101").is_none());
}

#[test]
fn test_adopt_error_missing_parent() {
    let (_dir, beans_dir) = setup_test_env();

    // Only create the child, no parent
    create_bean(&beans_dir, "101", "Orphan", false);

    // Try to adopt under non-existent parent
    let result = cmd_adopt(&beans_dir, "999", &["101".to_string()]);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Parent bean '999' not found"),
        "Error should mention missing parent, got: {}",
        err_msg
    );
}

#[test]
fn test_adopt_error_missing_child() {
    let (_dir, beans_dir) = setup_test_env();

    // Only create the parent, no child
    create_bean(&beans_dir, "100", "Parent", true);

    // Try to adopt non-existent child
    let result = cmd_adopt(&beans_dir, "100", &["999".to_string()]);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Child bean '999' not found"),
        "Error should mention missing child, got: {}",
        err_msg
    );
}

#[test]
fn test_adopt_continues_numbering_after_existing_children() {
    let (_dir, beans_dir) = setup_test_env();

    // Create parent with existing children
    create_bean(&beans_dir, "100", "Parent", true);

    let mut child1 = Bean::new("100.1", "Existing Child 1");
    child1.slug = Some("existing-child-1".to_string());
    child1.parent = Some("100".to_string());
    child1.verify = Some("true".to_string());
    child1
        .to_file(beans_dir.join("100.1-existing-child-1.md"))
        .unwrap();

    let mut child2 = Bean::new("100.2", "Existing Child 2");
    child2.slug = Some("existing-child-2".to_string());
    child2.parent = Some("100".to_string());
    child2.verify = Some("true".to_string());
    child2
        .to_file(beans_dir.join("100.2-existing-child-2.md"))
        .unwrap();

    // Create new bean to adopt
    create_bean(&beans_dir, "103", "New Child", false);

    // Adopt - should become 100.3, not 100.1
    let result = cmd_adopt(&beans_dir, "100", &["103".to_string()]).unwrap();

    assert_eq!(result.get("103"), Some(&"100.3".to_string()));
    assert!(beans_dir.join("100.3-new-child.md").exists());
}

#[test]
fn test_adopt_bean_already_has_parent() {
    let (_dir, beans_dir) = setup_test_env();

    // Create two potential parent beans
    create_bean(&beans_dir, "100", "Parent A", true);
    create_bean(&beans_dir, "200", "Parent B", true);

    // Create a child that already belongs to Parent A
    let mut child = Bean::new("100.1", "Existing Child");
    child.slug = Some("existing-child".to_string());
    child.parent = Some("100".to_string());
    child.verify = Some("true".to_string());
    child
        .to_file(beans_dir.join("100.1-existing-child.md"))
        .unwrap();

    // Adopt this child under Parent B - this re-parents the bean
    let result = cmd_adopt(&beans_dir, "200", &["100.1".to_string()]).unwrap();

    // Bean should be re-parented to 200.1
    assert_eq!(result.get("100.1"), Some(&"200.1".to_string()));

    // Verify the new parent is set
    let reparented = Bean::from_file(beans_dir.join("200.1-existing-child.md")).unwrap();
    assert_eq!(reparented.parent, Some("200".to_string()));
    assert_eq!(reparented.id, "200.1");

    // Old file should be gone
    assert!(!beans_dir.join("100.1-existing-child.md").exists());
}

#[test]
fn test_adopt_preserves_bean_fields() {
    let (_dir, beans_dir) = setup_test_env();

    create_bean(&beans_dir, "100", "Parent", true);

    // Create a bean with lots of fields
    let mut bean = Bean::new("101", "Complex Bean");
    bean.slug = Some("complex-bean".to_string());
    bean.description = Some("A detailed description".to_string());
    bean.acceptance = Some("All criteria met".to_string());
    bean.verify = Some("cargo test".to_string());
    bean.dependencies = vec![];
    bean.priority = 1;
    bean.to_file(beans_dir.join("101-complex-bean.md")).unwrap();

    cmd_adopt(&beans_dir, "100", &["101".to_string()]).unwrap();

    // Verify all fields are preserved
    let adopted = Bean::from_file(beans_dir.join("100.1-complex-bean.md")).unwrap();
    assert_eq!(adopted.title, "Complex Bean");
    assert_eq!(adopted.description, Some("A detailed description".to_string()));
    assert_eq!(adopted.acceptance, Some("All criteria met".to_string()));
    assert_eq!(adopted.verify, Some("cargo test".to_string()));
    assert_eq!(adopted.priority, 1);
}
