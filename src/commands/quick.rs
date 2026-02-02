use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::{Bean, Status, validate_priority};
use crate::config::Config;
use crate::hooks::{execute_hook, HookEvent};
use crate::index::Index;
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

    // Load config and get next ID
    let mut config = Config::load(beans_dir)?;
    let bean_id = config.increment_id().to_string();
    config.save(beans_dir)?;

    // Generate slug from title
    let slug = title_to_slug(&args.title);

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
    if let Some(verify) = args.verify {
        bean.verify = Some(verify);
    }
    if let Some(priority) = args.priority {
        bean.priority = priority;
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
}
