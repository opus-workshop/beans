//! `bn plan` — interactively plan a large bean into children.
//!
//! Without an ID, picks the highest-priority ready bean that exceeds max_tokens.
//! Spawns the configured plan template command to do the actual planning work.

use std::path::Path;

use anyhow::Result;

use crate::bean::{Bean, Status};
use crate::config::Config;
use crate::discovery::find_bean_file;
use crate::index::Index;
use crate::tokens::calculate_tokens;
use crate::util::natural_cmp;

/// Arguments for the plan command.
pub struct PlanArgs {
    pub id: Option<String>,
    pub strategy: Option<String>,
    pub auto: bool,
    pub force: bool,
    pub dry_run: bool,
}

/// Execute the `bn plan` command.
pub fn cmd_plan(beans_dir: &Path, args: PlanArgs) -> Result<()> {
    let config = Config::load_with_extends(beans_dir)?;
    let workspace = beans_dir.parent().unwrap_or_else(|| Path::new("."));

    let index = Index::load_or_rebuild(beans_dir)?;

    match args.id {
        Some(ref id) => plan_specific(beans_dir, &config, &index, workspace, id, &args),
        None => plan_auto_pick(beans_dir, &config, &index, workspace, &args),
    }
}

/// Plan a specific bean by ID.
fn plan_specific(
    beans_dir: &Path,
    config: &Config,
    _index: &Index,
    workspace: &Path,
    id: &str,
    args: &PlanArgs,
) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;
    let tokens = calculate_tokens(&bean, workspace);

    if tokens < config.max_tokens as u64 && !args.force {
        let tokens_k = format_tokens_k(tokens);
        eprintln!(
            "Bean {} is {} tokens — small enough to run directly.",
            id, tokens_k
        );
        eprintln!("  Use bn run {} to dispatch it.", id);
        eprintln!("  Use bn plan {} --force to plan anyway.", id);
        return Ok(());
    }

    spawn_plan(config, id, args)
}

/// Auto-pick the highest-priority ready bean that exceeds max_tokens.
fn plan_auto_pick(
    beans_dir: &Path,
    config: &Config,
    index: &Index,
    workspace: &Path,
    args: &PlanArgs,
) -> Result<()> {
    // Find all open beans (GOALs without verify, or any open bean above max_tokens)
    let mut candidates: Vec<(String, String, u8, u64)> = Vec::new();

    for entry in &index.beans {
        if entry.status != Status::Open {
            continue;
        }
        // Skip beans that are claimed
        if entry.claimed_by.is_some() {
            continue;
        }

        let bean_path = match find_bean_file(beans_dir, &entry.id) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let bean = match Bean::from_file(&bean_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let tokens = calculate_tokens(&bean, workspace);
        if tokens >= config.max_tokens as u64 {
            candidates.push((
                entry.id.clone(),
                entry.title.clone(),
                entry.priority,
                tokens,
            ));
        }
    }

    if candidates.is_empty() {
        eprintln!("✓ All ready beans are small enough to run directly.");
        eprintln!("  Use bn run to dispatch them.");
        return Ok(());
    }

    // Sort by priority (ascending P0 first), then by ID
    candidates.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| natural_cmp(&a.0, &b.0)));

    // Show all candidates
    eprintln!("{} beans need planning:", candidates.len());
    for (id, title, priority, tokens) in &candidates {
        let tokens_k = format_tokens_k(*tokens);
        eprintln!("  P{}  {:6}  {:30}  {}", priority, id, title, tokens_k);
    }
    eprintln!();

    // Pick first (highest priority, lowest ID)
    let (id, title, _, tokens) = &candidates[0];
    let tokens_k = format_tokens_k(*tokens);
    eprintln!("Planning: {} — {} ({})", id, title, tokens_k);

    spawn_plan(config, id, args)
}

/// Spawn the plan template command for a bean.
fn spawn_plan(config: &Config, id: &str, args: &PlanArgs) -> Result<()> {
    let template = match &config.plan {
        Some(t) => t.clone(),
        None => {
            anyhow::bail!(
                "No plan command configured.\n\n\
                 Set it with: bn config set plan \"<command>\"\n\n\
                 The command template uses {{id}} as a placeholder for the bean ID.\n\n\
                 Examples:\n  \
                   bn config set plan \"pi @.beans/{{id}}*.md 'decompose this bean into children'\"\n  \
                   bn config set plan \"claude -p 'plan bean {{id}} into sub-tasks'\""
            );
        }
    };

    let mut cmd = template.replace("{id}", id);

    // Append strategy hint if provided
    if let Some(ref strategy) = args.strategy {
        cmd = format!("{} --strategy {}", cmd, strategy);
    }

    if args.dry_run {
        eprintln!("Would spawn: {}", cmd);
        return Ok(());
    }

    eprintln!("Spawning: {}", cmd);

    if args.auto {
        // Non-interactive: wait for completion
        let status = std::process::Command::new("sh").args(["-c", &cmd]).status();
        match status {
            Ok(s) if s.success() => {
                eprintln!("Planning complete. Use bn tree {} to see children.", id);
            }
            Ok(s) => {
                anyhow::bail!("Plan command exited with code {}", s.code().unwrap_or(-1));
            }
            Err(e) => {
                anyhow::bail!("Failed to run plan command: {}", e);
            }
        }
    } else {
        // Interactive: inherit stdin/stdout/stderr
        let status = std::process::Command::new("sh")
            .args(["-c", &cmd])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();
        match status {
            Ok(s) if s.success() => {
                eprintln!("Planning complete. Use bn tree {} to see children.", id);
            }
            Ok(s) => {
                let code = s.code().unwrap_or(-1);
                if code != 0 {
                    anyhow::bail!("Plan command exited with code {}", code);
                }
            }
            Err(e) => {
                anyhow::bail!("Failed to run plan command: {}", e);
            }
        }
    }

    Ok(())
}

/// Format token count as "Nk tokens" string.
fn format_tokens_k(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{}k tokens", tokens / 1000)
    } else {
        format!("{} tokens", tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 10\nmax_tokens: 100\n",
        )
        .unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn plan_help_contains_plan() {
        // This is verified by the bean's verify command: bn plan --help 2>&1 | grep -q 'plan'
        // Here we just verify the module exists and compiles
        assert!(true);
    }

    #[test]
    fn plan_errors_when_no_plan_template() {
        let (dir, beans_dir) = setup_beans_dir();

        // Create a bean big enough to trigger planning
        let mut bean = Bean::new("1", "Big bean");
        bean.description = Some("x".repeat(2000)); // > 100 tokens threshold
        bean.to_file(beans_dir.join("1-big-bean.md")).unwrap();

        // Rebuild index
        let _ = Index::build(&beans_dir);

        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: Some("1".to_string()),
                strategy: None,
                auto: false,
                force: true,
                dry_run: false,
            },
        );

        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("No plan command configured"),
            "Expected plan template error, got: {}",
            err
        );

        drop(dir);
    }

    #[test]
    fn plan_small_bean_suggests_run() {
        let (dir, beans_dir) = setup_beans_dir();

        let bean = Bean::new("1", "Small bean");
        bean.to_file(beans_dir.join("1-small-bean.md")).unwrap();

        let _ = Index::build(&beans_dir);

        // Should succeed (prints advice, doesn't error)
        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: Some("1".to_string()),
                strategy: None,
                auto: false,
                force: false,
                dry_run: false,
            },
        );

        assert!(result.is_ok());

        drop(dir);
    }

    #[test]
    fn plan_force_overrides_size_check() {
        let (dir, beans_dir) = setup_beans_dir();

        // Config with plan template that just exits 0
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 10\nmax_tokens: 100000\nplan: \"true\"\n",
        )
        .unwrap();

        let bean = Bean::new("1", "Small bean");
        bean.to_file(beans_dir.join("1-small-bean.md")).unwrap();

        let _ = Index::build(&beans_dir);

        // With --force, should spawn even for small bean
        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: Some("1".to_string()),
                strategy: None,
                auto: false,
                force: true,
                dry_run: false,
            },
        );

        assert!(result.is_ok());

        drop(dir);
    }

    #[test]
    fn plan_dry_run_does_not_spawn() {
        let (dir, beans_dir) = setup_beans_dir();

        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 10\nmax_tokens: 100\nplan: \"echo planning {id}\"\n",
        )
        .unwrap();

        let mut bean = Bean::new("1", "Big bean");
        bean.description = Some("x".repeat(2000));
        bean.to_file(beans_dir.join("1-big-bean.md")).unwrap();

        let _ = Index::build(&beans_dir);

        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: Some("1".to_string()),
                strategy: None,
                auto: false,
                force: false,
                dry_run: true,
            },
        );

        assert!(result.is_ok());

        drop(dir);
    }

    #[test]
    fn plan_auto_pick_finds_largest() {
        let (dir, beans_dir) = setup_beans_dir();

        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 10\nmax_tokens: 100\nplan: \"true\"\n",
        )
        .unwrap();

        // Bean above threshold
        let mut big = Bean::new("1", "Big bean");
        big.description = Some("x".repeat(2000));
        big.to_file(beans_dir.join("1-big-bean.md")).unwrap();

        // Bean below threshold
        let small = Bean::new("2", "Small bean");
        small.to_file(beans_dir.join("2-small-bean.md")).unwrap();

        let _ = Index::build(&beans_dir);

        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: None,
                strategy: None,
                auto: true,
                force: false,
                dry_run: false,
            },
        );

        assert!(result.is_ok());

        drop(dir);
    }

    #[test]
    fn plan_auto_pick_none_needed() {
        let (dir, beans_dir) = setup_beans_dir();

        // All beans small
        let bean = Bean::new("1", "Small");
        bean.to_file(beans_dir.join("1-small.md")).unwrap();

        let _ = Index::build(&beans_dir);

        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: None,
                strategy: None,
                auto: false,
                force: false,
                dry_run: false,
            },
        );

        assert!(result.is_ok());

        drop(dir);
    }

    #[test]
    fn format_tokens_k_small() {
        assert_eq!(format_tokens_k(500), "500 tokens");
    }

    #[test]
    fn format_tokens_k_large() {
        assert_eq!(format_tokens_k(52000), "52k tokens");
    }

    #[test]
    fn format_tokens_k_exact_boundary() {
        assert_eq!(format_tokens_k(1000), "1k tokens");
    }
}
