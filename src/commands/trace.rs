use std::collections::HashSet;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::Serialize;

use crate::bean::{AttemptOutcome, Bean, Status};
use crate::discovery::find_bean_file;
use crate::index::Index;

// ---------------------------------------------------------------------------
// Output types (text + JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TraceOutput {
    pub bean: BeanSummary,
    pub parent_chain: Vec<BeanSummary>,
    pub children: Vec<BeanSummary>,
    pub dependencies: Vec<BeanSummary>,
    pub dependents: Vec<BeanSummary>,
    pub produces: Vec<String>,
    pub requires: Vec<String>,
    pub attempts: AttemptSummary,
}

#[derive(Debug, Serialize)]
pub struct BeanSummary {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct AttemptSummary {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
    pub abandoned: usize,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Handle `bn trace <id>` command.
///
/// Walks the bean graph from the given bean: parent chain up to root,
/// direct children, dependencies (what this bean waits on), dependents
/// (what waits on this bean), produces/requires artifacts, and attempt history.
pub fn cmd_trace(id: &str, json: bool, beans_dir: &Path) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let entry = index
        .beans
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow!("Bean {} not found", id))?;

    // Load full bean for attempt log and tokens
    let bean_path = find_bean_file(beans_dir, id)?;
    let bean = Bean::from_file(&bean_path)?;

    // Build reverse graph: dep_id -> list of bean IDs that depend on it
    let mut dependents_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for e in &index.beans {
        for dep in &e.dependencies {
            dependents_map
                .entry(dep.clone())
                .or_default()
                .push(e.id.clone());
        }
    }

    // --- Parent chain (up to root, cycle-safe) ---
    let parent_chain = collect_parent_chain(&index, &entry.parent, &mut HashSet::new());

    // --- Direct children ---
    let children: Vec<BeanSummary> = index
        .beans
        .iter()
        .filter(|e| e.parent.as_deref() == Some(id))
        .map(|e| bean_summary(e.id.clone(), e.title.clone(), &e.status))
        .collect();

    // --- Dependencies (what this bean waits on) ---
    let dependencies: Vec<BeanSummary> = entry
        .dependencies
        .iter()
        .filter_map(|dep_id| {
            index
                .beans
                .iter()
                .find(|e| &e.id == dep_id)
                .map(|e| bean_summary(e.id.clone(), e.title.clone(), &e.status))
        })
        .collect();

    // --- Dependents (what waits on this bean) ---
    let dependents: Vec<BeanSummary> = dependents_map
        .get(id)
        .map(|ids| {
            ids.iter()
                .filter_map(|dep_id| {
                    index
                        .beans
                        .iter()
                        .find(|e| &e.id == dep_id)
                        .map(|e| bean_summary(e.id.clone(), e.title.clone(), &e.status))
                })
                .collect()
        })
        .unwrap_or_default();

    // --- Attempt summary ---
    let attempts = build_attempt_summary(&bean);

    // --- Build output ---
    let this_summary = bean_summary(entry.id.clone(), entry.title.clone(), &entry.status);

    let output = TraceOutput {
        bean: this_summary,
        parent_chain,
        children,
        dependencies,
        dependents,
        produces: entry.produces.clone(),
        requires: entry.requires.clone(),
        attempts,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_trace(&output);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_parent_chain(
    index: &Index,
    parent_id: &Option<String>,
    visited: &mut HashSet<String>,
) -> Vec<BeanSummary> {
    let Some(pid) = parent_id else {
        return vec![];
    };

    // Guard against circular references (shouldn't happen but don't crash)
    if visited.contains(pid) {
        return vec![];
    }
    visited.insert(pid.clone());

    if let Some(entry) = index.beans.iter().find(|e| &e.id == pid) {
        let mut chain = vec![bean_summary(
            entry.id.clone(),
            entry.title.clone(),
            &entry.status,
        )];
        chain.extend(collect_parent_chain(index, &entry.parent, visited));
        chain
    } else {
        vec![]
    }
}

fn bean_summary(id: String, title: String, status: &Status) -> BeanSummary {
    BeanSummary {
        id,
        title,
        status: status.to_string(),
    }
}

fn build_attempt_summary(bean: &Bean) -> AttemptSummary {
    let total = bean.attempt_log.len();
    let successful = bean
        .attempt_log
        .iter()
        .filter(|a| matches!(a.outcome, AttemptOutcome::Success))
        .count();
    let failed = bean
        .attempt_log
        .iter()
        .filter(|a| matches!(a.outcome, AttemptOutcome::Failed))
        .count();
    let abandoned = bean
        .attempt_log
        .iter()
        .filter(|a| matches!(a.outcome, AttemptOutcome::Abandoned))
        .count();

    AttemptSummary {
        total,
        successful,
        failed,
        abandoned,
    }
}

fn status_indicator(status: &str) -> &str {
    match status {
        "closed" => "✓",
        "in_progress" => "⚡",
        _ => "○",
    }
}

fn print_trace(output: &TraceOutput) {
    let b = &output.bean;
    println!("Bean {}: \"{}\" [{}]", b.id, b.title, b.status);

    // Parent chain
    if output.parent_chain.is_empty() {
        println!("  Parent: (root)");
    } else {
        let mut indent = "  ".to_string();
        for parent in &output.parent_chain {
            println!(
                "{}Parent: {} {} \"{}\" [{}]",
                indent,
                status_indicator(&parent.status),
                parent.id,
                parent.title,
                parent.status
            );
            indent.push_str("  ");
        }
        println!("{}Parent: (root)", indent);
    }

    // Children
    if !output.children.is_empty() {
        println!("  Children:");
        for child in &output.children {
            println!(
                "    {} {} \"{}\" [{}]",
                status_indicator(&child.status),
                child.id,
                child.title,
                child.status
            );
        }
    }

    // Dependencies
    if output.dependencies.is_empty() {
        println!("  Dependencies: (none)");
    } else {
        println!("  Dependencies:");
        for dep in &output.dependencies {
            println!(
                "    → {} {} \"{}\" [{}]",
                status_indicator(&dep.status),
                dep.id,
                dep.title,
                dep.status
            );
        }
    }

    // Dependents
    if output.dependents.is_empty() {
        println!("  Dependents: (none)");
    } else {
        println!("  Dependents:");
        for dep in &output.dependents {
            println!(
                "    ← {} {} \"{}\" [{}]",
                status_indicator(&dep.status),
                dep.id,
                dep.title,
                dep.status
            );
        }
    }

    // Produces / Requires
    if output.produces.is_empty() {
        println!("  Produces: (none)");
    } else {
        println!("  Produces: {}", output.produces.join(", "));
    }

    if output.requires.is_empty() {
        println!("  Requires: (none)");
    } else {
        println!("  Requires: {}", output.requires.join(", "));
    }

    // Attempts
    let a = &output.attempts;
    if a.total == 0 {
        println!("  Attempts: (none)");
    } else {
        println!(
            "  Attempts: {} total ({} success, {} failed, {} abandoned)",
            a.total, a.successful, a.failed, a.abandoned
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::{AttemptOutcome, AttemptRecord, Bean};
    use tempfile::TempDir;

    /// Write a bean as a legacy `.yaml` file so `find_bean_file` can locate it.
    fn write_bean(beans_dir: &Path, bean: &Bean) {
        let path = beans_dir.join(format!("{}.yaml", bean.id));
        bean.to_file(&path).expect("write bean file");
    }

    #[test]
    fn test_trace_no_parent_no_deps() {
        let tmp = TempDir::new().unwrap();
        let beans_dir = tmp.path();

        let mut bean = Bean::new("42", "test bean");
        bean.produces = vec!["artifact-a".to_string()];
        bean.attempt_log = vec![AttemptRecord {
            num: 1,
            outcome: AttemptOutcome::Abandoned,
            notes: None,
            agent: None,
            started_at: None,
            finished_at: None,
        }];
        write_bean(beans_dir, &bean);

        // Index is rebuilt from bean files
        let result = cmd_trace("42", false, beans_dir);
        assert!(result.is_ok(), "cmd_trace failed: {:?}", result);
    }

    #[test]
    fn test_trace_json_output() {
        let tmp = TempDir::new().unwrap();
        let beans_dir = tmp.path();

        let bean = Bean::new("1", "root bean");
        write_bean(beans_dir, &bean);

        let result = cmd_trace("1", true, beans_dir);
        assert!(result.is_ok(), "cmd_trace --json failed: {:?}", result);
    }

    #[test]
    fn test_trace_with_parent_and_deps() {
        let tmp = TempDir::new().unwrap();
        let beans_dir = tmp.path();

        // Parent bean
        let parent_bean = Bean::new("10", "parent task");
        write_bean(beans_dir, &parent_bean);

        // Dependency bean
        let mut dep_bean = Bean::new("11", "dep task");
        dep_bean.status = Status::Closed;
        write_bean(beans_dir, &dep_bean);

        // Main bean with parent, deps, produces, requires, attempts
        let mut main_bean = Bean::new("12", "main task");
        main_bean.parent = Some("10".to_string());
        main_bean.dependencies = vec!["11".to_string()];
        main_bean.produces = vec!["api.rs".to_string()];
        main_bean.requires = vec!["Config".to_string()];
        main_bean.attempt_log = vec![
            AttemptRecord {
                num: 1,
                outcome: AttemptOutcome::Failed,
                notes: None,
                agent: None,
                started_at: None,
                finished_at: None,
            },
            AttemptRecord {
                num: 2,
                outcome: AttemptOutcome::Success,
                notes: None,
                agent: None,
                started_at: None,
                finished_at: None,
            },
        ];
        write_bean(beans_dir, &main_bean);

        let result = cmd_trace("12", false, beans_dir);
        assert!(
            result.is_ok(),
            "cmd_trace with parent/deps failed: {:?}",
            result
        );
    }

    #[test]
    fn test_trace_not_found() {
        let tmp = TempDir::new().unwrap();
        let beans_dir = tmp.path();

        // Empty directory — no beans
        let result = cmd_trace("999", false, beans_dir);
        assert!(result.is_err(), "Should error for missing bean");
    }
}
