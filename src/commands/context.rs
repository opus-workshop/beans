use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};

use crate::bean::{AttemptOutcome, Bean};
use crate::config::Config;
use crate::ctx_assembler::{assemble_context, extract_paths, read_file};
use crate::discovery::find_bean_file;
use crate::index::Index;
use crate::prompt::{build_agent_prompt, PromptOptions};

/// Load project rules from the configured rules file.
///
/// Returns `None` if the file doesn't exist or is empty.
/// Warns to stderr if the file is very large (>1000 lines).
fn load_rules(beans_dir: &Path) -> Option<String> {
    let config = Config::load(beans_dir).ok()?;
    let rules_path = config.rules_path(beans_dir);

    let content = std::fs::read_to_string(&rules_path).ok()?;
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return None;
    }

    let line_count = content.lines().count();
    if line_count > 1000 {
        eprintln!(
            "Warning: RULES.md is very large ({} lines). Consider trimming it.",
            line_count
        );
    }

    Some(content)
}

/// Format rules content with delimiters for agent context injection.
fn format_rules_section(rules: &str) -> String {
    format!(
        "═══ PROJECT RULES ═══════════════════════════════════════════\n\
         {}\n\
         ═════════════════════════════════════════════════════════════\n\n",
        rules.trim_end()
    )
}

/// Format the attempt_log and notes field into a "Previous Attempts" section.
///
/// Returns `None` if there are no attempt notes and no bean notes — callers
/// should skip output entirely in that case (no noise).
fn format_attempt_notes_section(bean: &Bean) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Accumulated notes written via `bn update --note`
    if let Some(ref notes) = bean.notes {
        let trimmed = notes.trim();
        if !trimmed.is_empty() {
            parts.push(format!("Bean notes:\n{}", trimmed));
        }
    }

    // Per-attempt notes from the attempt_log
    let attempt_entries: Vec<String> = bean
        .attempt_log
        .iter()
        .filter_map(|a| {
            let notes = a.notes.as_deref()?.trim();
            if notes.is_empty() {
                return None;
            }
            let outcome = match a.outcome {
                AttemptOutcome::Success => "success",
                AttemptOutcome::Failed => "failed",
                AttemptOutcome::Abandoned => "abandoned",
            };
            let agent_str = a
                .agent
                .as_deref()
                .map(|ag| format!(" ({})", ag))
                .unwrap_or_default();
            Some(format!(
                "Attempt #{}{} [{}]: {}",
                a.num, agent_str, outcome, notes
            ))
        })
        .collect();

    if !attempt_entries.is_empty() {
        parts.push(attempt_entries.join("\n"));
    }

    if parts.is_empty() {
        return None;
    }

    Some(format!(
        "═══ Previous Attempts ════════════════════════════════════════\n\
         {}\n\
         ══════════════════════════════════════════════════════════════\n\n",
        parts.join("\n\n").trim_end()
    ))
}

// ─── Structure extraction ────────────────────────────────────────────────────

/// Extract function/type signatures and imports from Rust source.
///
/// Matches top-level declarations: `use`, `fn`, `struct`, `enum`, `trait`,
/// `impl`, `type`, and `const`. Strips trailing `{` from signature lines.
fn extract_rust_structure(content: &str) -> Vec<String> {
    let mut result = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
        {
            continue;
        }

        // Imports
        if trimmed.starts_with("use ") {
            result.push(trimmed.to_string());
            continue;
        }

        // Declarations: check common prefixes
        let is_decl = trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("pub(crate) async fn ")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub(crate) struct ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub(crate) enum ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("pub(crate) trait ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("pub type ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("pub const ")
            || trimmed.starts_with("pub(crate) const ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("pub static ")
            || trimmed.starts_with("static ");

        if is_decl {
            // Take the signature line; strip trailing `{` and whitespace
            let sig = trimmed.trim_end_matches('{').trim_end();
            result.push(sig.to_string());
        }
    }

    result
}

/// Extract function/type signatures and imports from TypeScript/TSX source.
///
/// Matches: `import`, `export function`, `function`, `class`, `interface`,
/// `export type`, `export const`, `export enum`, and their async variants.
fn extract_ts_structure(content: &str) -> Vec<String> {
    let mut result = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
        {
            continue;
        }

        // Imports
        if trimmed.starts_with("import ") {
            result.push(trimmed.to_string());
            continue;
        }

        let is_decl = trimmed.starts_with("export function ")
            || trimmed.starts_with("export async function ")
            || trimmed.starts_with("export default function ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("async function ")
            || trimmed.starts_with("export class ")
            || trimmed.starts_with("export abstract class ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("export interface ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("export type ")
            || trimmed.starts_with("export enum ")
            || trimmed.starts_with("export const ")
            || trimmed.starts_with("export default class ")
            || trimmed.starts_with("export default async function ");

        if is_decl {
            let sig = trimmed.trim_end_matches('{').trim_end();
            result.push(sig.to_string());
        }
    }

    result
}

/// Extract function/class definitions and imports from Python source.
///
/// Matches top-level `def`, `async def`, `class`, `import`, and `from` lines.
/// Strips trailing `:` from definition signatures.
fn extract_python_structure(content: &str) -> Vec<String> {
    let mut result = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Top-level imports (no indentation)
        if line.starts_with("import ") || line.starts_with("from ") {
            result.push(trimmed.to_string());
            continue;
        }

        // Top-level and nested defs/classes — capture the signature line
        if trimmed.starts_with("def ")
            || trimmed.starts_with("async def ")
            || trimmed.starts_with("class ")
        {
            let sig = trimmed.trim_end_matches(':').trim_end();
            result.push(sig.to_string());
        }
    }

    result
}

/// Extract a structural summary (signatures, imports) from file content.
///
/// Dispatches to language-specific extractors based on file extension.
/// Returns `None` for unrecognized file types or when no structure is found.
/// Silently skips unrecognized types — no error is returned.
pub fn extract_file_structure(path: &str, content: &str) -> Option<String> {
    let ext = Path::new(path).extension()?.to_str()?;

    let lines: Vec<String> = match ext {
        "rs" => extract_rust_structure(content),
        "ts" | "tsx" => extract_ts_structure(content),
        "py" => extract_python_structure(content),
        _ => return None,
    };

    if lines.is_empty() {
        return None;
    }

    Some(lines.join("\n"))
}

/// Format multiple file structures into a single "File Structure" section.
///
/// Each entry is `(path, structure_text)`. Returns `None` if the input is empty.
fn format_structure_block(structures: &[(&str, String)]) -> Option<String> {
    if structures.is_empty() {
        return None;
    }

    let mut body = String::new();
    for (path, structure) in structures {
        body.push_str(&format!("### {}\n```\n{}\n```\n\n", path, structure));
    }

    Some(format!(
        "═══ File Structure ═══════════════════════════════════════════\n\
         {}\
         ══════════════════════════════════════════════════════════════\n\n",
        body
    ))
}

// ─── Bean spec formatting ────────────────────────────────────────────────────

/// Format the bean's core spec as the first section of the context output.
fn format_bean_spec_section(bean: &Bean) -> String {
    let mut s = String::new();
    s.push_str("═══ BEAN ════════════════════════════════════════════════════\n");
    s.push_str(&format!("ID: {}\n", bean.id));
    s.push_str(&format!("Title: {}\n", bean.title));
    s.push_str(&format!("Priority: P{}\n", bean.priority));
    s.push_str(&format!("Status: {}\n", bean.status));

    if let Some(ref verify) = bean.verify {
        s.push_str(&format!("Verify: {}\n", verify));
    }

    if !bean.produces.is_empty() {
        s.push_str(&format!("Produces: {}\n", bean.produces.join(", ")));
    }
    if !bean.requires.is_empty() {
        s.push_str(&format!("Requires: {}\n", bean.requires.join(", ")));
    }
    if !bean.dependencies.is_empty() {
        s.push_str(&format!("Dependencies: {}\n", bean.dependencies.join(", ")));
    }
    if let Some(ref parent) = bean.parent {
        s.push_str(&format!("Parent: {}\n", parent));
    }

    if let Some(ref desc) = bean.description {
        s.push_str(&format!("\n## Description\n{}\n", desc));
    }
    if let Some(ref acceptance) = bean.acceptance {
        s.push_str(&format!("\n## Acceptance Criteria\n{}\n", acceptance));
    }

    s.push_str("═════════════════════════════════════════════════════════════\n\n");
    s
}

// ─── Dependency context ──────────────────────────────────────────────────────

/// Information about a sibling bean that produces an artifact this bean requires.
struct DepProvider {
    artifact: String,
    bean_id: String,
    bean_title: String,
    status: String,
    description: Option<String>,
}

/// Resolve dependency context: find sibling beans that produce artifacts
/// this bean requires, and load their descriptions.
fn resolve_dependency_context(beans_dir: &Path, bean: &Bean) -> Vec<DepProvider> {
    if bean.requires.is_empty() {
        return Vec::new();
    }

    let index = match Index::load_or_rebuild(beans_dir) {
        Ok(idx) => idx,
        Err(_) => return Vec::new(),
    };

    let mut providers = Vec::new();

    for required in &bean.requires {
        let producer = index
            .beans
            .iter()
            .find(|e| e.id != bean.id && e.parent == bean.parent && e.produces.contains(required));

        if let Some(entry) = producer {
            let desc = find_bean_file(beans_dir, &entry.id)
                .ok()
                .and_then(|p| Bean::from_file(&p).ok())
                .and_then(|b| b.description.clone());

            providers.push(DepProvider {
                artifact: required.clone(),
                bean_id: entry.id.clone(),
                bean_title: entry.title.clone(),
                status: format!("{}", entry.status),
                description: desc,
            });
        }
    }

    providers
}

/// Format dependency providers into a section for the context output.
fn format_dependency_section(providers: &[DepProvider]) -> Option<String> {
    if providers.is_empty() {
        return None;
    }

    let mut s = String::new();
    s.push_str("═══ DEPENDENCY CONTEXT ══════════════════════════════════════\n");

    for p in providers {
        s.push_str(&format!(
            "Bean {} ({}) produces `{}` [{}]\n",
            p.bean_id, p.bean_title, p.artifact, p.status
        ));
        if let Some(ref desc) = p.description {
            let preview: String = desc.chars().take(500).collect();
            s.push_str(&format!("{}\n", preview));
            if desc.len() > 500 {
                s.push_str("...\n");
            }
        }
        s.push('\n');
    }

    s.push_str("═════════════════════════════════════════════════════════════\n\n");
    Some(s)
}

// ─── Path merging ────────────────────────────────────────────────────────────

/// Merge explicit `bean.paths` with paths regex-extracted from the description.
/// Explicit paths come first, then regex-extracted paths fill gaps.
/// Deduplicates by path string.
fn merge_paths(bean: &Bean) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for p in &bean.paths {
        if seen.insert(p.clone()) {
            result.push(p.clone());
        }
    }

    let description = bean.description.as_deref().unwrap_or("");
    for p in extract_paths(description) {
        if seen.insert(p.clone()) {
            result.push(p);
        }
    }

    result
}

// ─── Command ─────────────────────────────────────────────────────────────────

/// Assemble complete agent context for a bean — the single source of truth.
///
/// Outputs everything an agent needs to implement this bean:
/// 1. Bean spec — ID, title, verify, priority, status, description, acceptance
/// 2. Previous attempts — what was tried and failed
/// 3. Project rules — conventions from RULES.md
/// 4. Dependency context — sibling beans that produce required artifacts
/// 5. File structure — function signatures and imports
/// 6. File contents — full source of referenced files
///
/// File paths are merged from explicit `bean.paths` field (priority) and
/// regex-extracted paths from the description (fills gaps).
///
/// When `structure_only` is true, only structural summaries are emitted.
pub fn cmd_context(beans_dir: &Path, id: &str, json: bool, structure_only: bool, agent_prompt: bool) -> Result<()> {
    let bean_path =
        find_bean_file(beans_dir, id).context(format!("Could not find bean with ID: {}", id))?;

    let bean = Bean::from_file(&bean_path).context(format!(
        "Failed to parse bean from: {}",
        bean_path.display()
    ))?;

    // --agent-prompt: output the full structured prompt that an agent sees during bn run
    if agent_prompt {
        let options = PromptOptions {
            beans_dir: beans_dir.to_path_buf(),
            instructions: None,
            concurrent_overlaps: None,
        };
        let result = build_agent_prompt(&bean, &options)?;
        println!("{}", result.system_prompt);
        return Ok(());
    }

    let project_dir = beans_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid .beans/ path: {}", beans_dir.display()))?;

    // Merge explicit paths with regex-extracted paths from description
    let paths = merge_paths(&bean);

    // Load supplementary context
    let rules = load_rules(beans_dir);
    let attempt_notes = format_attempt_notes_section(&bean);
    let dep_providers = resolve_dependency_context(beans_dir, &bean);

    // Read file contents and extract structure
    struct FileEntry {
        path: String,
        content: Option<String>,
        structure: Option<String>,
    }

    let canonical_base = project_dir
        .canonicalize()
        .context("Cannot canonicalize project dir")?;

    let mut entries: Vec<FileEntry> = Vec::new();
    for path_str in &paths {
        let full_path = project_dir.join(path_str);
        let canonical = full_path.canonicalize().ok();

        let in_bounds = canonical
            .as_ref()
            .map(|c| c.starts_with(&canonical_base))
            .unwrap_or(false);

        let content = if let Some(ref c) = canonical {
            if in_bounds {
                read_file(c).ok()
            } else {
                None
            }
        } else {
            None
        };

        let structure = content
            .as_deref()
            .and_then(|c| extract_file_structure(path_str, c));

        entries.push(FileEntry {
            path: path_str.clone(),
            content,
            structure,
        });
    }

    if json {
        let files: Vec<serde_json::Value> = entries
            .iter()
            .map(|entry| {
                let exists = entry.content.is_some();
                let mut file_obj = serde_json::json!({
                    "path": entry.path,
                    "exists": exists,
                });
                if !structure_only {
                    file_obj["content"] = serde_json::Value::String(
                        entry
                            .content
                            .as_deref()
                            .unwrap_or("(not found)")
                            .to_string(),
                    );
                }
                if let Some(ref s) = entry.structure {
                    file_obj["structure"] = serde_json::Value::String(s.clone());
                }
                file_obj
            })
            .collect();

        let dep_json: Vec<serde_json::Value> = dep_providers
            .iter()
            .map(|p| {
                serde_json::json!({
                    "artifact": p.artifact,
                    "bean_id": p.bean_id,
                    "title": p.bean_title,
                    "status": p.status,
                    "description": p.description,
                })
            })
            .collect();

        let mut obj = serde_json::json!({
            "id": bean.id,
            "title": bean.title,
            "priority": bean.priority,
            "status": format!("{}", bean.status),
            "verify": bean.verify,
            "description": bean.description,
            "acceptance": bean.acceptance,
            "produces": bean.produces,
            "requires": bean.requires,
            "dependencies": bean.dependencies,
            "parent": bean.parent,
            "files": files,
            "dependency_context": dep_json,
        });
        if let Some(ref rules_content) = rules {
            obj["rules"] = serde_json::Value::String(rules_content.clone());
        }
        if let Some(ref notes) = attempt_notes {
            obj["attempt_notes"] = serde_json::Value::String(notes.clone());
        }
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        let mut output = String::new();

        // 1. Bean spec — the most important section
        output.push_str(&format_bean_spec_section(&bean));

        // 2. Previous attempts — what was tried and failed
        if let Some(ref notes) = attempt_notes {
            output.push_str(notes);
        }

        // 3. Project rules
        if let Some(ref rules_content) = rules {
            output.push_str(&format_rules_section(rules_content));
        }

        // 4. Dependency context
        if let Some(dep_section) = format_dependency_section(&dep_providers) {
            output.push_str(&dep_section);
        }

        // 5. Structural summaries
        let structure_pairs: Vec<(&str, String)> = entries
            .iter()
            .filter_map(|e| e.structure.as_ref().map(|s| (e.path.as_str(), s.clone())))
            .collect();

        if let Some(structure_block) = format_structure_block(&structure_pairs) {
            output.push_str(&structure_block);
        }

        // 6. Full file contents (unless --structure-only)
        if !structure_only {
            let file_paths: Vec<String> = paths.clone();
            if !file_paths.is_empty() {
                let context = assemble_context(file_paths, project_dir)
                    .context("Failed to assemble context")?;
                output.push_str(&context);
            }
        }

        print!("{}", output);
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn context_with_no_paths_in_description() {
        let (_dir, beans_dir) = setup_test_env();

        // Create a bean with no file paths in description
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.description = Some("A description with no file paths".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        // Should succeed but print a tip
        let result = cmd_context(&beans_dir, "1", false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn context_with_paths_in_description() {
        let (dir, beans_dir) = setup_test_env();
        let project_dir = dir.path();

        // Create a source file
        let src_dir = project_dir.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("foo.rs"), "fn main() {}").unwrap();

        // Create a bean referencing the file
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.description = Some("Check src/foo.rs for implementation".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_context(&beans_dir, "1", false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn context_bean_not_found() {
        let (_dir, beans_dir) = setup_test_env();

        let result = cmd_context(&beans_dir, "999", false, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn load_rules_returns_none_when_file_missing() {
        let (_dir, beans_dir) = setup_test_env();
        // Write a minimal config so Config::load succeeds
        fs::write(beans_dir.join("config.yaml"), "project: test\nnext_id: 1\n").unwrap();

        let result = load_rules(&beans_dir);
        assert!(result.is_none());
    }

    #[test]
    fn load_rules_returns_none_when_file_empty() {
        let (_dir, beans_dir) = setup_test_env();
        fs::write(beans_dir.join("config.yaml"), "project: test\nnext_id: 1\n").unwrap();
        fs::write(beans_dir.join("RULES.md"), "   \n\n  ").unwrap();

        let result = load_rules(&beans_dir);
        assert!(result.is_none());
    }

    #[test]
    fn load_rules_returns_content_when_present() {
        let (_dir, beans_dir) = setup_test_env();
        fs::write(beans_dir.join("config.yaml"), "project: test\nnext_id: 1\n").unwrap();
        fs::write(beans_dir.join("RULES.md"), "# My Rules\nNo unwrap.\n").unwrap();

        let result = load_rules(&beans_dir);
        assert!(result.is_some());
        assert!(result.unwrap().contains("No unwrap."));
    }

    #[test]
    fn load_rules_uses_custom_rules_file_path() {
        let (_dir, beans_dir) = setup_test_env();
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 1\nrules_file: custom-rules.md\n",
        )
        .unwrap();
        fs::write(beans_dir.join("custom-rules.md"), "Custom rules here").unwrap();

        let result = load_rules(&beans_dir);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Custom rules here"));
    }

    #[test]
    fn format_rules_section_wraps_with_delimiters() {
        let output = format_rules_section("# Rules\nBe nice.\n");
        assert!(output.starts_with("═══ PROJECT RULES"));
        assert!(output.contains("# Rules\nBe nice."));
        assert!(
            output.ends_with("═════════════════════════════════════════════════════════════\n\n")
        );
    }

    // --- attempt notes tests ---

    fn make_bean_with_attempts() -> crate::bean::Bean {
        use crate::bean::{AttemptOutcome, AttemptRecord};
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.attempt_log = vec![
            AttemptRecord {
                num: 1,
                outcome: AttemptOutcome::Abandoned,
                notes: Some("Tried X, hit bug Y".to_string()),
                agent: Some("pi-agent".to_string()),
                started_at: None,
                finished_at: None,
            },
            AttemptRecord {
                num: 2,
                outcome: AttemptOutcome::Failed,
                notes: Some("Fixed Y, now Z fails".to_string()),
                agent: None,
                started_at: None,
                finished_at: None,
            },
        ];
        bean
    }

    #[test]
    fn format_attempt_notes_returns_none_when_no_notes() {
        let bean = crate::bean::Bean::new("1", "Empty bean");
        // No attempt_log, no notes
        let result = format_attempt_notes_section(&bean);
        assert!(result.is_none());
    }

    #[test]
    fn format_attempt_notes_returns_none_when_attempts_have_no_notes() {
        use crate::bean::{AttemptOutcome, AttemptRecord};
        let mut bean = crate::bean::Bean::new("1", "Empty bean");
        bean.attempt_log = vec![AttemptRecord {
            num: 1,
            outcome: AttemptOutcome::Abandoned,
            notes: None,
            agent: None,
            started_at: None,
            finished_at: None,
        }];
        let result = format_attempt_notes_section(&bean);
        assert!(result.is_none());
    }

    #[test]
    fn format_attempt_notes_includes_attempt_log_notes() {
        let bean = make_bean_with_attempts();
        let result = format_attempt_notes_section(&bean).expect("should produce output");
        assert!(
            result.contains("Previous Attempts"),
            "should have section header"
        );
        assert!(result.contains("Attempt #1"), "should include attempt 1");
        assert!(result.contains("pi-agent"), "should include agent name");
        assert!(result.contains("abandoned"), "should include outcome");
        assert!(
            result.contains("Tried X, hit bug Y"),
            "should include notes text"
        );
        assert!(result.contains("Attempt #2"), "should include attempt 2");
        assert!(
            result.contains("Fixed Y, now Z fails"),
            "should include attempt 2 notes"
        );
    }

    #[test]
    fn format_attempt_notes_includes_bean_notes() {
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.notes = Some("Watch out for edge cases".to_string());
        let result = format_attempt_notes_section(&bean).expect("should produce output");
        assert!(result.contains("Watch out for edge cases"));
        assert!(result.contains("Bean notes:"));
    }

    #[test]
    fn format_attempt_notes_skips_empty_notes_strings() {
        use crate::bean::{AttemptOutcome, AttemptRecord};
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.notes = Some("   ".to_string()); // whitespace only
        bean.attempt_log = vec![AttemptRecord {
            num: 1,
            outcome: AttemptOutcome::Abandoned,
            notes: Some("  ".to_string()), // whitespace only
            agent: None,
            started_at: None,
            finished_at: None,
        }];
        let result = format_attempt_notes_section(&bean);
        assert!(
            result.is_none(),
            "whitespace-only notes should produce no output"
        );
    }

    #[test]
    fn context_includes_attempt_notes_in_text_output() {
        let (dir, beans_dir) = setup_test_env();
        let project_dir = dir.path();

        // Create a source file
        let src_dir = project_dir.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("foo.rs"), "fn main() {}").unwrap();

        // Create a bean with attempt notes
        let mut bean = make_bean_with_attempts();
        bean.id = "1".to_string();
        bean.description = Some("Check src/foo.rs for implementation".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        // The function prints to stdout — just verify it runs without error
        let result = cmd_context(&beans_dir, "1", false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn context_includes_attempt_notes_in_json_output() {
        let (dir, beans_dir) = setup_test_env();
        let project_dir = dir.path();

        let src_dir = project_dir.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("foo.rs"), "fn main() {}").unwrap();

        let mut bean = make_bean_with_attempts();
        bean.id = "1".to_string();
        bean.description = Some("Check src/foo.rs for implementation".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_context(&beans_dir, "1", true, false, false);
        assert!(result.is_ok());
    }
}
