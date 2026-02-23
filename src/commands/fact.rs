use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};

use crate::bean::Bean;
use crate::commands::create::{cmd_create, CreateArgs};
use crate::discovery::find_bean_file;
use crate::index::Index;

/// Default TTL for facts: 30 days.
const DEFAULT_TTL_DAYS: i64 = 30;

/// Create a verified fact (convenience wrapper around create with bean_type=fact).
///
/// Facts require a verify command — that's the point. If you can't write a
/// verify command, the knowledge belongs in agents.md, not in `bn fact`.
pub fn cmd_fact(
    beans_dir: &Path,
    title: String,
    verify: String,
    description: Option<String>,
    paths: Option<String>,
    ttl_days: Option<i64>,
    pass_ok: bool,
) -> Result<String> {
    if verify.trim().is_empty() {
        return Err(anyhow!(
            "Facts require a verify command. If you can't write one, \
             this belongs in agents.md, not bn fact."
        ));
    }

    // Create the bean via normal create flow
    let bean_id = cmd_create(
        beans_dir,
        CreateArgs {
            title,
            description,
            acceptance: None,
            notes: None,
            design: None,
            verify: Some(verify),
            priority: Some(3), // facts are lower priority than tasks
            labels: Some("fact".to_string()),
            assignee: None,
            deps: None,
            parent: None,
            produces: None,
            requires: None,
            on_fail: None,
            pass_ok,
            claim: false,
            by: None,
        },
    )?;

    // Now patch the bean to set fact-specific fields
    let bean_path = find_bean_file(beans_dir, &bean_id)?;
    let mut bean = Bean::from_file(&bean_path)?;

    bean.bean_type = "fact".to_string();

    // Set TTL
    let ttl = ttl_days.unwrap_or(DEFAULT_TTL_DAYS);
    bean.stale_after = Some(Utc::now() + Duration::days(ttl));

    // Set paths for relevance matching
    if let Some(paths_str) = paths {
        bean.paths = paths_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    bean.to_file(&bean_path)?;

    // Rebuild index
    let index = Index::build(beans_dir)?;
    index.save(beans_dir)?;

    eprintln!("Created fact {}: {}", bean_id, bean.title);
    Ok(bean_id)
}

/// Verify all facts and report staleness.
///
/// Re-runs verify commands for all beans with bean_type=fact.
/// Reports which facts are stale (past their stale_after date)
/// and which have failing verify commands.
pub fn cmd_verify_facts(beans_dir: &Path) -> Result<()> {
    use std::process::Command as ShellCommand;

    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    // Find all fact beans (both active and archived)
    let index = Index::load_or_rebuild(beans_dir)?;
    let archived = Index::collect_archived(beans_dir).unwrap_or_default();

    let now = Utc::now();
    let mut stale_count = 0;
    let mut failing_count = 0;
    let mut verified_count = 0;
    let mut total_facts = 0;

    // Check active beans
    for entry in index.beans.iter().chain(archived.iter()) {
        let bean_path = if entry.status == crate::bean::Status::Closed {
            crate::discovery::find_archived_bean(beans_dir, &entry.id).ok()
        } else {
            find_bean_file(beans_dir, &entry.id).ok()
        };

        let bean_path = match bean_path {
            Some(p) => p,
            None => continue,
        };

        let mut bean = match Bean::from_file(&bean_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if bean.bean_type != "fact" {
            continue;
        }

        total_facts += 1;

        // Check staleness
        let is_stale = bean
            .stale_after
            .map(|sa| now > sa)
            .unwrap_or(false);

        if is_stale {
            stale_count += 1;
            eprintln!(
                "⚠ STALE: [{}] \"{}\"",
                bean.id, bean.title
            );
        }

        // Re-run verify command
        if let Some(ref verify_cmd) = bean.verify {
            let output = ShellCommand::new("sh")
                .args(["-c", verify_cmd])
                .current_dir(project_root)
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    verified_count += 1;
                    bean.last_verified = Some(now);
                    // Reset stale_after from now
                    if bean.stale_after.is_some() {
                        bean.stale_after = Some(now + Duration::days(DEFAULT_TTL_DAYS));
                    }
                    bean.to_file(&bean_path)?;
                    println!("  ✓ [{}] \"{}\"", bean.id, bean.title);
                }
                Ok(_) => {
                    failing_count += 1;
                    eprintln!(
                        "  ✗ FAILING: [{}] \"{}\" — verify command returned non-zero",
                        bean.id, bean.title
                    );
                }
                Err(e) => {
                    failing_count += 1;
                    eprintln!(
                        "  ✗ ERROR: [{}] \"{}\" — {}",
                        bean.id, bean.title, e
                    );
                }
            }
        }
    }

    println!();
    println!(
        "Facts: {} total, {} verified, {} stale, {} failing",
        total_facts, verified_count, stale_count, failing_count
    );

    if failing_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
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
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
        };
        config.save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn create_fact_sets_bean_type() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let id = cmd_fact(
            &beans_dir,
            "Auth uses RS256".to_string(),
            "grep -q RS256 src/auth.rs".to_string(),
            None,
            None,
            None,
            true, // pass_ok since file doesn't exist
        )
        .unwrap();

        let bean_path = find_bean_file(&beans_dir, &id).unwrap();
        let bean = Bean::from_file(&bean_path).unwrap();

        assert_eq!(bean.bean_type, "fact");
        assert!(bean.labels.contains(&"fact".to_string()));
        assert!(bean.stale_after.is_some());
        assert!(bean.verify.is_some());
    }

    #[test]
    fn create_fact_with_paths() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let id = cmd_fact(
            &beans_dir,
            "Config file format".to_string(),
            "true".to_string(),
            None,
            Some("src/config.rs, src/main.rs".to_string()),
            None,
            true,
        )
        .unwrap();

        let bean_path = find_bean_file(&beans_dir, &id).unwrap();
        let bean = Bean::from_file(&bean_path).unwrap();

        assert_eq!(bean.paths, vec!["src/config.rs", "src/main.rs"]);
    }

    #[test]
    fn create_fact_with_custom_ttl() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let id = cmd_fact(
            &beans_dir,
            "Short-lived fact".to_string(),
            "true".to_string(),
            None,
            None,
            Some(7), // 7 days
            true,
        )
        .unwrap();

        let bean_path = find_bean_file(&beans_dir, &id).unwrap();
        let bean = Bean::from_file(&bean_path).unwrap();

        // stale_after should be ~7 days from now
        let stale = bean.stale_after.unwrap();
        let diff = stale - Utc::now();
        assert!(diff.num_days() >= 6 && diff.num_days() <= 7);
    }

    #[test]
    fn create_fact_requires_verify() {
        let (_dir, beans_dir) = setup_beans_dir_with_config();

        let result = cmd_fact(
            &beans_dir,
            "No verify fact".to_string(),
            "  ".to_string(), // empty verify
            None,
            None,
            None,
            true,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("verify command"));
    }
}
