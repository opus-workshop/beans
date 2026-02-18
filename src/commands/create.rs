use std::fs;
use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};

use chrono::Utc;

use crate::bean::{Bean, validate_priority};
use crate::commands::claim::cmd_claim;
use crate::config::Config;
use crate::hooks::{execute_hook, HookEvent};
use crate::index::Index;
use crate::project::suggest_verify_command;
use crate::tokens::calculate_tokens;
use crate::util::title_to_slug;

/// Create arguments structure for organizing all the parameters passed to create.
pub struct CreateArgs {
    pub title: String,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub notes: Option<String>,
    pub design: Option<String>,
    pub verify: Option<String>,
    pub priority: Option<u8>,
    pub labels: Option<String>,
    pub assignee: Option<String>,
    pub deps: Option<String>,
    pub parent: Option<String>,
    pub produces: Option<String>,
    pub requires: Option<String>,
    /// Require verify to fail first (enforced TDD)
    pub fail_first: bool,
    /// Claim the bean immediately after creation
    pub claim: bool,
    /// Who is claiming (used with claim)
    pub by: Option<String>,
}

/// Assign a child ID for a parent bean.
/// Scans .beans/ for {parent_id}.{N}-*.md, finds highest N, returns "{parent_id}.{N+1}".
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

    Ok(format!("{}.{}", parent_id, max_child + 1))
}

/// Create a new bean.
///
/// If `args.parent` is given, assign a child ID ({parent_id}.{next_child}).
/// Otherwise, use the next sequential ID from config and increment it.
/// Returns the created bean ID on success.
pub fn cmd_create(beans_dir: &Path, args: CreateArgs) -> Result<String> {
    // Validate priority if provided
    if let Some(priority) = args.priority {
        validate_priority(priority)?;
    }

    // Note: acceptance and verify are optional. Parent/goal beans may have neither.
    // The real gate is on close: bn close checks verify. Creating without verify
    // just means the bean can't be auto-verified on close.

    // Fail-first check: verify command must FAIL before bean can be created
    // This prevents "cheating tests" like `assert True` that always pass
    if args.fail_first {
        let verify_cmd = args.verify.as_ref()
            .ok_or_else(|| anyhow!("--fail-first requires --verify"))?;
        
        let project_root = beans_dir.parent()
            .ok_or_else(|| anyhow!("Cannot determine project root"))?;
        
        println!("Running verify (must fail): {}", verify_cmd);
        
        let status = ShellCommand::new("sh")
            .args(["-c", verify_cmd])
            .current_dir(project_root)
            .status()
            .with_context(|| format!("Failed to execute verify command: {}", verify_cmd))?;
        
        if status.success() {
            anyhow::bail!(
                "Cannot create bean: verify command already passes!\n\n\
                 The test must FAIL on current code to prove it tests something real.\n\
                 Either:\n\
                 - The test doesn't actually test the new behavior\n\
                 - The feature is already implemented\n\
                 - The test is a no-op (assert True)"
            );
        }
        
        println!("✓ Verify failed as expected - test is real");
    }

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

    // Generate slug from title
    let slug = title_to_slug(&args.title);

    // Track if verify was provided for suggestion later
    let has_verify = args.verify.is_some();

    // Create the bean
    let mut bean = Bean::new(&bean_id, &args.title);
    bean.slug = Some(slug.clone());

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
    if let Some(verify) = args.verify {
        bean.verify = Some(verify);
    }
    if args.fail_first {
        bean.fail_first = true;
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

    // Parse produces
    if let Some(produces_str) = args.produces {
        bean.produces = produces_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
    }

    // Parse requires
    if let Some(requires_str) = args.requires {
        bean.requires = requires_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
    }

    // Get the project directory (parent of beans_dir which is .beans)
    let project_dir = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Failed to determine project directory"))?;

    // Call pre-create hook (blocking - abort if it fails)
    let pre_passed = execute_hook(HookEvent::PreCreate, &bean, project_dir, None)
        .context("Pre-create hook execution failed")?;

    if !pre_passed {
        return Err(anyhow!("Pre-create hook rejected bean creation"));
    }

    // Calculate and store token count
    let tokens = calculate_tokens(&bean, project_dir);
    bean.tokens = Some(tokens);
    bean.tokens_updated = Some(Utc::now());

    // Write the bean file with new naming convention: {id}-{slug}.md
    let bean_path = beans_dir.join(format!("{}-{}.md", bean_id, slug));
    bean.to_file(&bean_path)?;

    // Update the index by rebuilding from disk (includes the bean we just wrote)
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    // Show token size feedback with assessment
    let max_tokens = config.max_tokens;
    if tokens <= max_tokens as u64 {
        println!("Created bean {}: {} ({}k tokens ✓)", bean_id, args.title, tokens / 1000);
    } else {
        println!(
            "Created bean {}: {} ({}k tokens ⚠️ exceeds {}k limit)",
            bean_id,
            args.title,
            tokens / 1000,
            max_tokens / 1000
        );
        println!(
            "  This appears to be a GOAL. Create child SPECS with --parent {}",
            bean_id
        );
    }

    // Suggest verify command if none was provided
    if !has_verify {
        if let Some(suggested) = suggest_verify_command(project_dir) {
            println!("Tip: Consider adding a verify command: --verify \"{}\"", suggested);
        }
    }

    // Call post-create hook (non-blocking - log warning if it fails)
    if let Err(e) = execute_hook(HookEvent::PostCreate, &bean, project_dir, None) {
        eprintln!("Warning: post-create hook failed: {}", e);
    }

    // If --claim was passed, claim the bean immediately
    if args.claim {
        cmd_claim(beans_dir, &bean_id, args.by)?;
    }

    Ok(bean_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Status;
    use tempfile::TempDir;

    fn setup_beans_dir_with_config() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
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
            verify: Some("cargo test".to_string()),
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        // Check the bean file exists with new naming convention
        let bean_path = beans_dir.join("1-first-task.md");
        assert!(bean_path.exists());

        // Verify content
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.id, "1");
        assert_eq!(bean.title, "First task");
        assert_eq!(bean.slug, Some("first-task".to_string()));
    }

    #[test]
    fn create_allows_bean_without_verify_or_acceptance() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Goal bean".to_string(),
            description: Some("A parent/goal bean with no verify".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        let result = cmd_create(&beans_dir, args);
        assert!(result.is_ok(), "Should allow bean without verify or acceptance");

        let bean_path = beans_dir.join("1-goal-bean.md");
        assert!(bean_path.exists());
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.title, "Goal bean");
        assert!(bean.verify.is_none());
        assert!(bean.acceptance.is_none());
    }

    #[test]
    fn create_increments_id() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create first bean
        let args1 = CreateArgs {
            title: "First".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };
        cmd_create(&beans_dir, args1).unwrap();

        // Create second bean
        let args2 = CreateArgs {
            title: "Second".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some("true".to_string()),
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
        };
        cmd_create(&beans_dir, args2).unwrap();

        // Verify both exist with correct IDs and new filenames
        let bean1 = Bean::from_file(&beans_dir.join("1-first.md")).unwrap();
        let bean2 = Bean::from_file(&beans_dir.join("2-second.md")).unwrap();
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
            acceptance: Some("Children complete".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };
        cmd_create(&beans_dir, parent_args).unwrap();

        // Create child bean
        let child_args = CreateArgs {
            title: "Child 1".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some("cargo test".to_string()),
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: Some("1".to_string()),
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
        };
        cmd_create(&beans_dir, child_args).unwrap();

        // Verify child ID is 1.1 with new filename
        let bean = Bean::from_file(&beans_dir.join("1.1-child-1.md")).unwrap();
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
            acceptance: Some("All children complete".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
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
                verify: Some("cargo test".to_string()),
                priority: None,
                labels: None,
                assignee: None,
                deps: None,
                parent: Some("1".to_string()),
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
            };
            cmd_create(&beans_dir, child_args).unwrap();
        }

        // Verify all children exist with new naming
        for i in 1..=3 {
            let expected_id = format!("1.{}", i);
            let expected_slug = format!("child-{}", i);
            let path = beans_dir.join(format!("{}-{}.md", expected_id, expected_slug));
            assert!(path.exists(), "Child {} should exist at {:?}", i, path);

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
            verify: None,
            priority: Some(1),
            labels: Some("bug,critical".to_string()),
            assignee: Some("alice".to_string()),
            deps: Some("2,3".to_string()),
            parent: None,
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        let bean = Bean::from_file(&beans_dir.join("1-complex-bean.md")).unwrap();
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
            acceptance: Some("Indexed correctly".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
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

        // Create some child files with new naming convention
        let bean1 = Bean::new("parent.1", "Child 1");
        let bean2 = Bean::new("parent.2", "Child 2");
        let bean5 = Bean::new("parent.5", "Child 5");

        bean1.to_file(&beans_dir.join("parent.1-child-1.md")).unwrap();
        bean2.to_file(&beans_dir.join("parent.2-child-2.md")).unwrap();
        bean5.to_file(&beans_dir.join("parent.5-child-5.md")).unwrap();

        let id = assign_child_id(&beans_dir, "parent").unwrap();
        assert_eq!(id, "parent.6");
    }

    #[test]
    fn create_rejects_priority_too_high() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Invalid priority bean".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
            notes: None,
            design: None,
            verify: None,
            priority: Some(5),
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
        };

        let result = cmd_create(&beans_dir, args);
        assert!(result.is_err(), "Should reject priority > 4");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("priority"), "Error should mention priority");
    }

    #[test]
    fn create_accepts_valid_priorities() {
        for priority in 0..=4 {
            let (_dir, beans_dir) = setup_beans_dir_with_config();

            let args = CreateArgs {
                title: format!("Bean with priority {}", priority),
                description: None,
                acceptance: Some("Done".to_string()),
                notes: None,
                design: None,
                verify: None,
                priority: Some(priority),
                labels: None,
                assignee: None,
                deps: None,
                parent: None,
            produces: None,
            requires: None,
            fail_first: false,
            claim: false,
            by: None,
            };

            let result = cmd_create(&beans_dir, args);
            assert!(result.is_ok(), "Priority {} should be valid", priority);
        }
    }

    // =========================================================================
    // Hook Integration Tests
    // =========================================================================

    #[test]
    fn pre_create_hook_accepts_bean_creation() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, beans_dir) = setup_beans_dir_with_config();
        let project_dir = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust and create a pre-create hook that succeeds
        crate::hooks::create_trust(project_dir).unwrap();

        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 0").unwrap();

        #[cfg(unix)]
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let args = CreateArgs {
            title: "Bean with accepting hook".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        // Bean should be created
        let result = cmd_create(&beans_dir, args);
        assert!(result.is_ok(), "Creation should succeed with accepting pre-create hook");

        // Verify bean was created
        let bean_path = beans_dir.join("1-bean-with-accepting-hook.md");
        assert!(bean_path.exists(), "Bean file should exist");
    }

    #[test]
    fn pre_create_hook_rejects_bean_creation() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, beans_dir) = setup_beans_dir_with_config();
        let project_dir = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust and create a pre-create hook that fails
        crate::hooks::create_trust(project_dir).unwrap();

        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let args = CreateArgs {
            title: "Bean with rejecting hook".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        // Bean creation should fail
        let result = cmd_create(&beans_dir, args);
        assert!(result.is_err(), "Creation should fail with rejecting pre-create hook");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Pre-create hook rejected"),
            "Error should indicate hook rejection"
        );

        // Verify bean was NOT created
        let bean_path = beans_dir.join("1-bean-with-rejecting-hook.md");
        assert!(
            !bean_path.exists(),
            "Bean file should NOT exist when pre-create hook rejects"
        );
    }

    #[test]
    fn post_create_hook_runs_after_creation() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, beans_dir) = setup_beans_dir_with_config();
        let project_dir = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust and create a post-create hook that writes to a file
        crate::hooks::create_trust(project_dir).unwrap();

        let hook_path = hooks_dir.join("post-create");
        let marker_file = project_dir.join("hook-executed.txt");
        let marker_file_str = marker_file.to_string_lossy().to_string();

        // Create hook that writes to marker file
        let hook_script = format!("#!/bin/bash\necho 'post-create executed' >> '{}'\nexit 0", marker_file_str);
        fs::write(&hook_path, hook_script).unwrap();

        #[cfg(unix)]
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let args = CreateArgs {
            title: "Bean with post-create hook".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        // Create bean
        let result = cmd_create(&beans_dir, args);
        assert!(result.is_ok(), "Creation should succeed");

        // Verify bean was created
        let bean_path = beans_dir.join("1-bean-with-post-create-hook.md");
        assert!(bean_path.exists(), "Bean file should exist");

        // Verify post-create hook ran (marker file exists)
        assert!(marker_file.exists(), "Post-create hook should have run and created marker file");
    }

    #[test]
    fn post_create_hook_failure_does_not_break_creation() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, beans_dir) = setup_beans_dir_with_config();
        let project_dir = dir.path();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Enable trust and create a post-create hook that fails
        crate::hooks::create_trust(project_dir).unwrap();

        let hook_path = hooks_dir.join("post-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let args = CreateArgs {
            title: "Bean with failing post-create hook".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        // Bean creation should STILL succeed (post-create failures are non-blocking)
        let result = cmd_create(&beans_dir, args);
        assert!(result.is_ok(), "Creation should succeed even if post-create hook fails");

        // Verify bean WAS created
        let bean_path = beans_dir.join("1-bean-with-failing-post-create-hook.md");
        assert!(bean_path.exists(), "Bean file should exist even when post-create hook fails");
    }

    #[test]
    fn untrusted_hooks_are_silently_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, beans_dir) = setup_beans_dir_with_config();
        let hooks_dir = beans_dir.join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // DO NOT enable trust - hooks should be skipped

        let hook_path = hooks_dir.join("pre-create");
        fs::write(&hook_path, "#!/bin/bash\nexit 1").unwrap();

        #[cfg(unix)]
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let args = CreateArgs {
            title: "Bean with untrusted hook".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        // Bean creation should succeed (untrusted hooks are skipped)
        let result = cmd_create(&beans_dir, args);
        assert!(result.is_ok(), "Creation should succeed when hooks are untrusted");

        // Verify bean WAS created
        let bean_path = beans_dir.join("1-bean-with-untrusted-hook.md");
        assert!(bean_path.exists(), "Bean file should exist when hooks are untrusted");
    }

    #[test]
    fn fail_first_rejects_passing_verify() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Cheating test".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some("true".to_string()), // always passes
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: true,
            claim: false,
            by: None,
        };

        let result = cmd_create(&beans_dir, args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("verify command already passes"));
    }

    #[test]
    fn fail_first_accepts_failing_verify() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Real test".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some("false".to_string()), // always fails
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: true,
            claim: false,
            by: None,
        };

        let result = cmd_create(&beans_dir, args);
        assert!(result.is_ok());

        // Bean should be created
        let bean_path = beans_dir.join("1-real-test.md");
        assert!(bean_path.exists());
    }

    #[test]
    fn fail_first_requires_verify() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "No verify".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
            notes: None,
            design: None,
            verify: None, // no verify command
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: true,
            claim: false,
            by: None,
        };

        let result = cmd_create(&beans_dir, args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("--fail-first requires --verify"));
    }

    // =========================================================================
    // --claim Flag Tests
    // =========================================================================

    #[test]
    fn create_with_claim_sets_in_progress() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Claimed task".to_string(),
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
            fail_first: false,
            claim: true,
            by: Some("agent-1".to_string()),
        };

        cmd_create(&beans_dir, args).unwrap();

        let bean_path = beans_dir.join("1-claimed-task.md");
        assert!(bean_path.exists());

        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.id, "1");
        assert_eq!(bean.title, "Claimed task");
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.claimed_by, Some("agent-1".to_string()));
        assert!(bean.claimed_at.is_some());
    }

    #[test]
    fn create_with_claim_without_by() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Anon claimed".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some("true".to_string()),
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            fail_first: false,
            claim: true,
            by: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        let bean_path = beans_dir.join("1-anon-claimed.md");
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.claimed_by, None);
        assert!(bean.claimed_at.is_some());
    }

    #[test]
    fn create_without_claim_stays_open() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = CreateArgs {
            title: "Unclaimed task".to_string(),
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
            fail_first: false,
            claim: false,
            by: None,
        };

        cmd_create(&beans_dir, args).unwrap();

        let bean_path = beans_dir.join("1-unclaimed-task.md");
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.status, Status::Open);
        assert_eq!(bean.claimed_by, None);
        assert_eq!(bean.claimed_at, None);
    }

    #[test]
    fn create_with_claim_and_parent() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

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
            fail_first: false,
            claim: false,
            by: None,
        };
        cmd_create(&beans_dir, parent_args).unwrap();

        // Create child with --claim
        let child_args = CreateArgs {
            title: "Child claimed".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some("cargo test".to_string()),
            priority: None,
            labels: None,
            assignee: None,
            deps: None,
            parent: Some("1".to_string()),
            produces: None,
            requires: None,
            fail_first: false,
            claim: true,
            by: Some("agent-2".to_string()),
        };
        cmd_create(&beans_dir, child_args).unwrap();

        let bean_path = beans_dir.join("1.1-child-claimed.md");
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.id, "1.1");
        assert_eq!(bean.parent, Some("1".to_string()));
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.claimed_by, Some("agent-2".to_string()));
    }
}
