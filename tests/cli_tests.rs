//! Integration tests for the `bn create` command validation.

use std::fs;

use bn::commands::create::{cmd_create, CreateArgs};
use bn::config::Config;
use tempfile::TempDir;

/// Setup a test environment with a .beans directory and config.
fn setup_test_env() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let beans_dir = dir.path().join(".beans");
    fs::create_dir(&beans_dir).unwrap();

    let config = Config {
        project: "test-cli".to_string(),
        next_id: 1,
        auto_close_parent: true,
        max_tokens: 30000,
        run: None,
    };
    config.save(&beans_dir).unwrap();

    (dir, beans_dir)
}

#[test]
fn create_claim_without_criteria_shows_error() {
    let (_dir, beans_dir) = setup_test_env();

    let args = CreateArgs {
        title: "Bad claimed bean".to_string(),
        description: None,
        acceptance: None,
        notes: None,
        design: None,
        verify: None,
        priority: None,
        labels: None,
        assignee: None,
        deps: None,
        parent: None,
        produces: None,
        requires: None,
        pass_ok: true,
        claim: true,
        by: Some("agent-1".to_string()),
    };

    let result = cmd_create(&beans_dir, args);
    assert!(result.is_err(), "PASS: --claim without criteria rejected");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("validation criteria"),
        "PASS: error mentions validation criteria"
    );
    assert!(
        err_msg.contains("--acceptance or --verify"),
        "PASS: error mentions --acceptance or --verify"
    );
}

#[test]
fn create_claim_with_acceptance_succeeds() {
    let (_dir, beans_dir) = setup_test_env();

    let args = CreateArgs {
        title: "Claimed with acceptance".to_string(),
        description: None,
        acceptance: Some("Feature works".to_string()),
        notes: None,
        design: None,
        verify: None,
        priority: None,
        labels: None,
        assignee: None,
        deps: None,
        parent: None,
        produces: None,
        requires: None,
        pass_ok: true,
        claim: true,
        by: None,
    };

    let result = cmd_create(&beans_dir, args);
    assert!(result.is_ok(), "PASS: --claim with --acceptance accepted");
}

#[test]
fn create_claim_with_verify_succeeds() {
    let (_dir, beans_dir) = setup_test_env();

    let args = CreateArgs {
        title: "Claimed with verify".to_string(),
        description: None,
        acceptance: None,
        notes: None,
        design: None,
        verify: Some("cargo test".to_string()),
        priority: None,
        labels: None,
        assignee: None,
        deps: None,
        parent: None,
        produces: None,
        requires: None,
        pass_ok: true,
        claim: true,
        by: None,
    };

    let result = cmd_create(&beans_dir, args);
    assert!(result.is_ok(), "PASS: --claim with --verify accepted");
}

#[test]
fn create_without_claim_no_criteria_succeeds() {
    let (_dir, beans_dir) = setup_test_env();

    let args = CreateArgs {
        title: "Goal bean".to_string(),
        description: None,
        acceptance: None,
        notes: None,
        design: None,
        verify: None,
        priority: None,
        labels: None,
        assignee: None,
        deps: None,
        parent: None,
        produces: None,
        requires: None,
        pass_ok: true,
        claim: false,
        by: None,
    };

    let result = cmd_create(&beans_dir, args);
    assert!(result.is_ok(), "PASS: create without --claim needs no criteria");
}

#[test]
fn create_claim_with_parent_no_criteria_succeeds() {
    let (_dir, beans_dir) = setup_test_env();

    // Create parent first
    let parent_args = CreateArgs {
        title: "Parent".to_string(),
        description: None,
        acceptance: Some("Children done".to_string()),
        notes: None,
        design: None,
        verify: None,
        priority: None,
        labels: None,
        assignee: None,
        deps: None,
        parent: None,
        produces: None,
        requires: None,
        pass_ok: true,
        claim: false,
        by: None,
    };
    cmd_create(&beans_dir, parent_args).unwrap();

    // Create child with --claim but no criteria — exempt because --parent
    let child_args = CreateArgs {
        title: "Child claimed".to_string(),
        description: None,
        acceptance: None,
        notes: None,
        design: None,
        verify: None,
        priority: None,
        labels: None,
        assignee: None,
        deps: None,
        parent: Some("1".to_string()),
        produces: None,
        requires: None,
        pass_ok: true,
        claim: true,
        by: Some("agent-2".to_string()),
    };

    let result = cmd_create(&beans_dir, child_args);
    assert!(result.is_ok(), "PASS: --claim --parent exempt from criteria check");
}
