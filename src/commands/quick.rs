use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::{Bean, Status, validate_priority};
use crate::config::Config;
use crate::hooks::{execute_hook, HookEvent};
use crate::index::Index;
use crate::project::suggest_verify_command;
use crate::util::title_to_slug;

/// Arguments for quick-create command.
pub struct QuickArgs {
    pub title: String,
    pub description: Option<String>,
    pub acceptance: Option<String>,
    pub notes: Option<String>,
    pub verify: Option<String>,
    pub priority: Option<u8>,
    pub by: Option<String>,
    pub produces: Option<String>,
    pub requires: Option<String>,
    /// Skip fail-first check (allow verify to already pass)
    pub pass_ok: bool,
}

/// Quick-create: create a bean and immediately claim it.
///
/// This is a convenience command that combines `bn create` + `bn claim`
/// in a single operation. Useful for agents starting immediate work.
pub fn cmd_quick(beans_dir: &Path, args: QuickArgs) -> Result<()> {
    // Validate priority if provided
    if let Some(priority) = args.priority {
        validate_priority(priority)?;
    }

    // Require at least acceptance or verify criteria
    if args.acceptance.is_none() && args.verify.is_none() {
        anyhow::bail!(
            "Bean must have validation criteria: provide --acceptance or --verify (or both)"
        );
    }

    // Fail-first check (default): verify command must FAIL before bean can be created
    // This prevents "cheating tests" like `assert True` that always pass
    // Use --pass-ok / -p to skip this check
    if !args.pass_ok {
        if let Some(verify_cmd) = args.verify.as_ref() {
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
                     - The test is a no-op (assert True)\n\n\
                     Use --pass-ok / -p to skip this check."
                );
            }
            
            println!("✓ Verify failed as expected - test is real");
        }
    }

    // Load config and get next ID
    let mut config = Config::load(beans_dir)?;
    let bean_id = config.increment_id().to_string();
    config.save(beans_dir)?;

    // Generate slug from title
    let slug = title_to_slug(&args.title);

    // Track if verify was provided for suggestion later
    let has_verify = args.verify.is_some();

    // Create the bean with InProgress status (already claimed)
    let now = Utc::now();
    let mut bean = Bean::new(&bean_id, &args.title);
    bean.slug = Some(slug.clone());
    bean.status = Status::InProgress;
    bean.claimed_by = args.by.clone();
    bean.claimed_at = Some(now);

    if let Some(desc) = args.description {
        bean.description = Some(desc);
    }
    if let Some(acceptance) = args.acceptance {
        bean.acceptance = Some(acceptance);
    }
    if let Some(notes) = args.notes {
        bean.notes = Some(notes);
    }
    let has_fail_first = !args.pass_ok && args.verify.is_some();
    if let Some(verify) = args.verify {
        bean.verify = Some(verify);
    }
    if has_fail_first {
        bean.fail_first = true;
    }
    if let Some(priority) = args.priority {
        bean.priority = priority;
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

    // Write the bean file with naming convention: {id}-{slug}.md
    let bean_path = beans_dir.join(format!("{}-{}.md", bean_id, slug));
    bean.to_file(&bean_path)?;

    // Update the index by rebuilding from disk
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    let claimer = args.by.as_deref().unwrap_or("anonymous");
    println!("Created and claimed bean {}: {} (by {})", bean_id, args.title, claimer);

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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn quick_creates_and_claims_bean() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Quick task".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
            notes: None,
            verify: None,
            priority: None,
            by: Some("agent-1".to_string()),
            produces: None,
            requires: None,
            pass_ok: true,
        };

        cmd_quick(&beans_dir, args).unwrap();

        // Check the bean file exists
        let bean_path = beans_dir.join("1-quick-task.md");
        assert!(bean_path.exists());

        // Verify content
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.id, "1");
        assert_eq!(bean.title, "Quick task");
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.claimed_by, Some("agent-1".to_string()));
        assert!(bean.claimed_at.is_some());
    }

    #[test]
    fn quick_works_without_by() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Anonymous task".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            verify: Some("cargo test".to_string()),
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: true,
        };

        cmd_quick(&beans_dir, args).unwrap();

        let bean_path = beans_dir.join("1-anonymous-task.md");
        let bean = Bean::from_file(&bean_path).unwrap();
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.claimed_by, None);
        assert!(bean.claimed_at.is_some());
    }

    #[test]
    fn quick_rejects_missing_validation_criteria() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "No criteria".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            verify: None,
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: true,
        };

        let result = cmd_quick(&beans_dir, args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("validation criteria"));
    }

    #[test]
    fn quick_increments_id() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        // Create first bean
        let args1 = QuickArgs {
            title: "First".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
            notes: None,
            verify: None,
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: true,
        };
        cmd_quick(&beans_dir, args1).unwrap();

        // Create second bean
        let args2 = QuickArgs {
            title: "Second".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            verify: Some("true".to_string()),
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: true,
        };
        cmd_quick(&beans_dir, args2).unwrap();

        // Verify both exist with correct IDs
        let bean1 = Bean::from_file(&beans_dir.join("1-first.md")).unwrap();
        let bean2 = Bean::from_file(&beans_dir.join("2-second.md")).unwrap();
        assert_eq!(bean1.id, "1");
        assert_eq!(bean2.id, "2");
    }

    #[test]
    fn quick_updates_index() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Indexed bean".to_string(),
            description: None,
            acceptance: Some("Indexed correctly".to_string()),
            notes: None,
            verify: None,
            priority: None,
            by: Some("tester".to_string()),
            produces: None,
            requires: None,
            pass_ok: true,
        };

        cmd_quick(&beans_dir, args).unwrap();

        // Load and check index
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        assert_eq!(index.beans[0].id, "1");
        assert_eq!(index.beans[0].title, "Indexed bean");
        assert_eq!(index.beans[0].status, Status::InProgress);
    }

    #[test]
    fn quick_with_all_fields() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Full bean".to_string(),
            description: Some("A description".to_string()),
            acceptance: Some("All tests pass".to_string()),
            notes: Some("Some notes".to_string()),
            verify: Some("cargo test".to_string()),
            priority: Some(1),
            by: Some("agent-x".to_string()),
            produces: Some("FooStruct,bar_function".to_string()),
            requires: Some("BazTrait".to_string()),
            pass_ok: true,
        };

        cmd_quick(&beans_dir, args).unwrap();

        let bean = Bean::from_file(&beans_dir.join("1-full-bean.md")).unwrap();
        assert_eq!(bean.title, "Full bean");
        assert_eq!(bean.description, Some("A description".to_string()));
        assert_eq!(bean.acceptance, Some("All tests pass".to_string()));
        assert_eq!(bean.notes, Some("Some notes".to_string()));
        assert_eq!(bean.verify, Some("cargo test".to_string()));
        assert_eq!(bean.priority, 1);
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.claimed_by, Some("agent-x".to_string()));
    }

    #[test]
    fn default_rejects_passing_verify() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Cheating test".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            verify: Some("true".to_string()), // always passes
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: false, // default: fail-first enforced
        };

        let result = cmd_quick(&beans_dir, args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("verify command already passes"));
    }

    #[test]
    fn default_accepts_failing_verify() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Real test".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            verify: Some("false".to_string()), // always fails
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: false, // default: fail-first enforced
        };

        let result = cmd_quick(&beans_dir, args);
        assert!(result.is_ok());

        // Bean should be created
        let bean_path = beans_dir.join("1-real-test.md");
        assert!(bean_path.exists());

        // Should have fail_first set in the bean
        let bean = Bean::from_file(&bean_path).unwrap();
        assert!(bean.fail_first);
    }

    #[test]
    fn pass_ok_skips_fail_first_check() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "Passing verify ok".to_string(),
            description: None,
            acceptance: None,
            notes: None,
            verify: Some("true".to_string()), // always passes — allowed with --pass-ok
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: true,
        };

        let result = cmd_quick(&beans_dir, args);
        assert!(result.is_ok());

        // Bean should be created
        let bean_path = beans_dir.join("1-passing-verify-ok.md");
        assert!(bean_path.exists());

        // Should NOT have fail_first set
        let bean = Bean::from_file(&bean_path).unwrap();
        assert!(!bean.fail_first);
    }

    #[test]
    fn no_verify_skips_fail_first_check() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let args = QuickArgs {
            title: "No verify".to_string(),
            description: None,
            acceptance: Some("Done".to_string()),
            notes: None,
            verify: None, // no verify command — fail-first not applicable
            priority: None,
            by: None,
            produces: None,
            requires: None,
            pass_ok: false,
        };

        let result = cmd_quick(&beans_dir, args);
        assert!(result.is_ok());

        // Should NOT have fail_first set (no verify)
        let bean_path = beans_dir.join("1-no-verify.md");
        let bean = Bean::from_file(&bean_path).unwrap();
        assert!(!bean.fail_first);
    }
}
