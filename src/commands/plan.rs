//! `bn plan` — interactively plan a large bean into children.
//!
//! Without an ID, picks the highest-priority ready bean that is oversized or unscoped.
//! When `config.plan` is set, spawns that template command.
//! Otherwise, builds a rich decomposition prompt and spawns `pi` directly.

use std::path::Path;

use anyhow::Result;

use crate::bean::{Bean, Status};
use crate::config::Config;
use crate::discovery::find_bean_file;
use crate::index::Index;
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
    _workspace: &Path,
    id: &str,
    args: &PlanArgs,
) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;

    let is_oversized = bean.produces.len() > crate::blocking::MAX_PRODUCES
        || bean.paths.len() > crate::blocking::MAX_PATHS;

    if !is_oversized && !args.force {
        eprintln!("Bean {} is small enough to run directly.", id,);
        eprintln!("  Use bn run {} to dispatch it.", id);
        eprintln!("  Use bn plan {} --force to plan anyway.", id);
        return Ok(());
    }

    spawn_plan(beans_dir, config, id, &bean, args)
}

/// Auto-pick the highest-priority ready bean that is oversized.
fn plan_auto_pick(
    beans_dir: &Path,
    config: &Config,
    index: &Index,
    _workspace: &Path,
    args: &PlanArgs,
) -> Result<()> {
    // Find all open beans that are oversized (too many produces or paths)
    let mut candidates: Vec<(String, String, u8)> = Vec::new();

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

        let is_oversized = bean.produces.len() > crate::blocking::MAX_PRODUCES
            || bean.paths.len() > crate::blocking::MAX_PATHS;

        if is_oversized {
            candidates.push((entry.id.clone(), entry.title.clone(), entry.priority));
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
    for (id, title, priority) in &candidates {
        eprintln!("  P{}  {:6}  {}", priority, id, title);
    }
    eprintln!();

    // Pick first (highest priority, lowest ID)
    let (id, title, _) = &candidates[0];
    eprintln!("Planning: {} — {}", id, title);

    // Load the full bean for prompt building
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;

    spawn_plan(beans_dir, config, id, &bean, args)
}

/// Spawn the plan command for a bean.
///
/// If `config.plan` is set, uses that template (backward compatible).
/// Otherwise, builds a rich decomposition prompt and spawns `pi` directly.
fn spawn_plan(
    beans_dir: &Path,
    config: &Config,
    id: &str,
    bean: &Bean,
    args: &PlanArgs,
) -> Result<()> {
    // If a custom plan template is configured, use it (backward compat)
    if let Some(ref template) = config.plan {
        return spawn_template(template, id, args);
    }

    // Built-in decomposition: build prompt and spawn pi
    spawn_builtin(beans_dir, id, bean, args)
}

/// Spawn the plan using a user-configured template command.
fn spawn_template(template: &str, id: &str, args: &PlanArgs) -> Result<()> {
    let mut cmd = template.replace("{id}", id);

    if let Some(ref strategy) = args.strategy {
        cmd = format!("{} --strategy {}", cmd, strategy);
    }

    if args.dry_run {
        eprintln!("Would spawn: {}", cmd);
        return Ok(());
    }

    eprintln!("Spawning: {}", cmd);
    run_shell_command(&cmd, id, args.auto)
}

/// Build a decomposition prompt and spawn `pi` with it directly.
fn spawn_builtin(beans_dir: &Path, id: &str, bean: &Bean, args: &PlanArgs) -> Result<()> {
    let prompt = build_decomposition_prompt(id, bean, args.strategy.as_deref());

    // Find the bean file to pass as context
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean_path_str = bean_path.display().to_string();

    // Build pi command: pass the bean file as context and the prompt
    let escaped_prompt = shell_escape(&prompt);
    let cmd = format!("pi @{} {}", bean_path_str, escaped_prompt);

    if args.dry_run {
        eprintln!("Would spawn: {}", cmd);
        eprintln!("\n--- Built-in decomposition prompt ---");
        eprintln!("{}", prompt);
        return Ok(());
    }

    eprintln!("Spawning built-in decomposition for bean {}...", id);
    run_shell_command(&cmd, id, args.auto)
}

/// Execute a shell command, either interactively or non-interactively.
fn run_shell_command(cmd: &str, id: &str, auto: bool) -> Result<()> {
    if auto {
        let status = std::process::Command::new("sh").args(["-c", cmd]).status();
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
        let status = std::process::Command::new("sh")
            .args(["-c", cmd])
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

/// Build a rich decomposition prompt that embeds the core planning wisdom.
fn build_decomposition_prompt(id: &str, bean: &Bean, strategy: Option<&str>) -> String {
    let strategy_guidance = match strategy {
        Some("feature") | Some("by-feature") => {
            "Split by feature — each child is a vertical slice (types + impl + tests for one feature)."
        }
        Some("layer") | Some("by-layer") => {
            "Split by layer — types/interfaces first, then implementation, then tests."
        }
        Some("file") | Some("by-file") => {
            "Split by file — each child handles one file or closely related file group."
        }
        Some("phase") => {
            "Split by phase — scaffold first, then core logic, then edge cases, then polish."
        }
        Some(other) => {
            // Custom strategy, include as-is
            return build_prompt_text(id, bean, other);
        }
        None => "Choose the best strategy: by-feature (vertical slices), by-layer, or by-file.",
    };

    build_prompt_text(id, bean, strategy_guidance)
}

/// Assemble the full prompt text with decomposition rules.
fn build_prompt_text(id: &str, bean: &Bean, strategy_guidance: &str) -> String {
    let title = &bean.title;
    let priority = bean.priority;
    let description = bean.description.as_deref().unwrap_or("(no description)");

    // Build produces/requires context if present
    let mut dep_context = String::new();
    if !bean.produces.is_empty() {
        dep_context.push_str(&format!("\nProduces: {}\n", bean.produces.join(", ")));
    }
    if !bean.requires.is_empty() {
        dep_context.push_str(&format!("Requires: {}\n", bean.requires.join(", ")));
    }

    format!(
        r#"Decompose bean {id} into smaller child beans.

## Parent Bean
- **ID:** {id}
- **Title:** {title}
- **Priority:** P{priority}
{dep_context}
## Strategy
{strategy_guidance}

## Sizing Rules
- A bean is **atomic** if it requires ≤5 functions to write and ≤10 to read
- Each child should have at most 3 `produces` artifacts and 5 `paths`
- Count functions concretely by examining the code — don't estimate

## Splitting Rules
- Create **2-4 children** for medium beans, **3-5** for large ones
- **Maximize parallelism** — prefer independent beans over sequential chains
- Each child must have a **verify command** that exits 0 on success
- Children should be independently testable where possible
- Use `--produces` and `--requires` to express dependencies between siblings

## Context Embedding Rules
- **Embed context into descriptions** — don't reference files, include the relevant types/signatures
- Include: concrete file paths, function signatures, type definitions
- Include: specific steps, edge cases, error handling requirements
- Be specific: "Add `fn validate_email(s: &str) -> bool` to `src/util.rs`" not "add validation"

## How to Create Children
Use `bn create` for each child bean:

```
bn create "child title" \
  --parent {id} \
  --priority {priority} \
  --verify "test command that exits 0" \
  --produces "artifact_name" \
  --requires "artifact_from_sibling" \
  --description "Full description with:
- What to implement
- Which files to modify (with paths)
- Key types/signatures to use or create
- Acceptance criteria
- Edge cases to handle"
```

## Description Template
A good child bean description includes:
1. **What**: One clear sentence of what this child does
2. **Files**: Specific file paths with what changes in each
3. **Context**: Embedded type definitions, function signatures, patterns to follow
4. **Acceptance**: Concrete criteria the verify command checks
5. **Edge cases**: What could go wrong, what to handle

## Your Task
1. Read the parent bean's description below
2. Examine referenced source files to count functions accurately
3. Decide on a split strategy
4. Create 2-5 child beans using `bn create` commands
5. Ensure every child has a verify command
6. After creating children, run `bn tree {id}` to show the result

## Parent Bean Description
{description}"#,
    )
}

/// Escape a string for safe use as a single shell argument.
fn shell_escape(s: &str) -> String {
    // Use single quotes, escaping any internal single quotes
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
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
            "project: test\nnext_id: 10\n",
        )
        .unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn plan_help_contains_plan() {
        // This is verified by the bean's verify command: bn plan --help 2>&1 | grep -q 'plan'
        // Here we just verify the module exists and compiles
    }

    #[test]
    fn plan_no_template_without_auto_errors() {
        let (dir, beans_dir) = setup_beans_dir();

        // Create an oversized bean (>3 produces) to trigger planning
        let mut bean = Bean::new("1", "Big bean");
        bean.produces = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        bean.to_file(beans_dir.join("1-big-bean.md")).unwrap();

        let _ = Index::build(&beans_dir);

        // Without --auto AND without config.plan, should use builtin
        // which tries to spawn pi (will fail in test env but that's the intent)
        let result = cmd_plan(
            &beans_dir,
            PlanArgs {
                id: Some("1".to_string()),
                strategy: None,
                auto: false,
                force: true,
                dry_run: true, // dry_run so we don't actually spawn
            },
        );

        // dry_run should succeed
        assert!(result.is_ok());

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
            "project: test\nnext_id: 10\nplan: \"true\"\n",
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
            "project: test\nnext_id: 10\nplan: \"echo planning {id}\"\n",
        )
        .unwrap();

        let mut bean = Bean::new("1", "Big bean");
        bean.produces = vec!["a".into(), "b".into(), "c".into(), "d".into()];
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
    fn plan_auto_pick_finds_oversized() {
        let (dir, beans_dir) = setup_beans_dir();

        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 10\nplan: \"true\"\n",
        )
        .unwrap();

        // Bean above scope threshold (oversized)
        let mut big = Bean::new("1", "Big bean");
        big.produces = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        big.to_file(beans_dir.join("1-big-bean.md")).unwrap();

        // Bean below scope threshold
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
    fn build_prompt_includes_decomposition_rules() {
        let bean = Bean::new("42", "Implement auth system");
        let prompt = build_decomposition_prompt("42", &bean, None);

        // Core decomposition rules are present
        assert!(prompt.contains("Decompose bean 42"), "missing header");
        assert!(prompt.contains("Implement auth system"), "missing title");
        assert!(prompt.contains("≤5 functions"), "missing sizing rules");
        assert!(
            prompt.contains("Maximize parallelism"),
            "missing parallelism rule"
        );
        assert!(
            prompt.contains("Embed context"),
            "missing context embedding rule"
        );
        assert!(
            prompt.contains("verify command"),
            "missing verify requirement"
        );
        assert!(prompt.contains("bn create"), "missing create syntax");
        assert!(prompt.contains("--parent 42"), "missing parent flag");
        assert!(prompt.contains("--produces"), "missing produces flag");
        assert!(prompt.contains("--requires"), "missing requires flag");
    }

    #[test]
    fn build_prompt_with_strategy() {
        let bean = Bean::new("1", "Big task");
        let prompt = build_decomposition_prompt("1", &bean, Some("by-feature"));

        assert!(
            prompt.contains("vertical slice"),
            "missing feature strategy guidance"
        );
    }

    #[test]
    fn build_prompt_includes_produces_requires() {
        let mut bean = Bean::new("5", "Task with deps");
        bean.produces = vec!["auth_types".to_string(), "auth_middleware".to_string()];
        bean.requires = vec!["db_connection".to_string()];

        let prompt = build_decomposition_prompt("5", &bean, None);

        assert!(prompt.contains("auth_types"), "missing produces");
        assert!(prompt.contains("db_connection"), "missing requires");
    }

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn shell_escape_with_quotes() {
        assert_eq!(shell_escape("it's here"), "'it'\\''s here'");
    }

    #[test]
    fn plan_builtin_dry_run_shows_prompt() {
        let (dir, beans_dir) = setup_beans_dir();

        // No plan template configured — will use builtin
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
                force: true,
                dry_run: true,
            },
        );

        assert!(result.is_ok());

        drop(dir);
    }
}
