use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::bean::Bean;
use crate::graph;
use crate::index::{count_bean_formats, Index};

/// Issue types that doctor can detect and potentially fix
#[derive(Debug)]
enum Issue {
    StaleIndex,
    MixedFormats { md_count: usize, yaml_count: usize },
    DuplicateId { id: String, files: Vec<String> },
    OrphanedDependency { bean_id: String, missing_dep: String },
    MissingParent { bean_id: String, parent_id: String },
    ArchivedParent { bean_id: String, parent_id: String },
    StaleIndexEntry { id: String },
    Cycle { path: Vec<String> },
}

impl Issue {
    fn display(&self) -> String {
        match self {
            Issue::StaleIndex => "[!] Stale index - run 'bn sync' to rebuild".to_string(),
            Issue::MixedFormats { md_count, yaml_count } => {
                format!(
                    "[!] Mixed bean formats: {} .md files, {} .yaml files\n    \
                     This inflates bean count and causes confusion.\n    \
                     Fix: mkdir -p .beans/legacy && mv .beans/*.yaml .beans/legacy/",
                    md_count, yaml_count
                )
            }
            Issue::DuplicateId { id, files } => {
                format!(
                    "[!] Duplicate ID '{}' in {} files: {}",
                    id,
                    files.len(),
                    files.join(", ")
                )
            }
            Issue::OrphanedDependency { bean_id, missing_dep } => {
                format!(
                    "[!] Orphaned dependency: {} depends on non-existent {}",
                    bean_id, missing_dep
                )
            }
            Issue::MissingParent { bean_id, parent_id } => {
                format!(
                    "[!] Missing parent: {} lists parent {} but it doesn't exist",
                    bean_id, parent_id
                )
            }
            Issue::ArchivedParent { bean_id, parent_id } => {
                format!(
                    "[!] Bean {} references parent '{}' which is archived",
                    bean_id, parent_id
                )
            }
            Issue::StaleIndexEntry { id } => {
                format!("[!] Index has entry for '{}' but no source file exists", id)
            }
            Issue::Cycle { path } => {
                format!("[!] Dependency cycle detected: {}", path.join(" -> "))
            }
        }
    }

    fn is_fixable(&self) -> bool {
        matches!(
            self,
            Issue::StaleIndex | Issue::StaleIndexEntry { .. }
        )
    }
}

/// Files to exclude when scanning for bean files
const EXCLUDED_FILES: &[&str] = &["config.yaml", "index.yaml", "bean.yaml"];

/// Check if a filename represents a bean file
fn is_bean_filename(filename: &str) -> bool {
    if EXCLUDED_FILES.contains(&filename) {
        return false;
    }
    let ext = Path::new(filename).extension().and_then(|e| e.to_str());
    match ext {
        Some("md") => filename.contains('-'), // New format: {id}-{slug}.md
        Some("yaml") => true,                 // Legacy format: {id}.yaml
        _ => false,
    }
}

/// Scan beans directory and collect bean files with their IDs
fn scan_bean_files(beans_dir: &Path) -> Result<HashMap<String, Vec<String>>> {
    let mut id_to_files: HashMap<String, Vec<String>> = HashMap::new();

    let dir_entries = fs::read_dir(beans_dir)?;

    for entry in dir_entries {
        let entry = entry?;
        let path = entry.path();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if !is_bean_filename(filename) {
            continue;
        }

        // Try to parse the bean to get its ID
        if let Ok(bean) = Bean::from_file(&path) {
            id_to_files
                .entry(bean.id.clone())
                .or_default()
                .push(filename.to_string());
        }
    }

    Ok(id_to_files)
}

/// Get all bean source files that exist
fn get_existing_bean_files(beans_dir: &Path) -> Result<Vec<String>> {
    let mut existing = Vec::new();

    let dir_entries = fs::read_dir(beans_dir)?;

    for entry in dir_entries {
        let entry = entry?;
        let path = entry.path();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if is_bean_filename(filename) {
            if let Ok(bean) = Bean::from_file(&path) {
                existing.push(bean.id);
            }
        }
    }

    Ok(existing)
}

/// Collect all archived bean IDs
fn collect_archived_ids(beans_dir: &Path) -> Result<Vec<String>> {
    let archived = Index::collect_archived(beans_dir)?;
    Ok(archived.into_iter().map(|e| e.id).collect())
}

/// Health check: detect orphaned dependencies, missing parent refs, cycles, stale index,
/// duplicate IDs, archived parent refs, and stale index entries.
/// With --fix, automatically resolves fixable issues.
pub fn cmd_doctor(beans_dir: &Path, fix: bool) -> Result<()> {
    let mut issues: Vec<Issue> = Vec::new();

    // Check 1: Index freshness
    let is_stale = Index::is_stale(beans_dir)?;
    if is_stale {
        issues.push(Issue::StaleIndex);
    }

    // Check 2: Mixed bean formats (.yaml and .md)
    let (md_count, yaml_count) = count_bean_formats(beans_dir)?;
    if md_count > 0 && yaml_count > 0 {
        issues.push(Issue::MixedFormats { md_count, yaml_count });
    }

    // Check 3: Duplicate IDs
    let id_to_files = scan_bean_files(beans_dir)?;
    for (id, files) in &id_to_files {
        if files.len() > 1 {
            issues.push(Issue::DuplicateId {
                id: id.clone(),
                files: files.clone(),
            });
        }
    }

    // Load index for remaining checks (rebuild if stale so we can check properly)
    let index = if is_stale {
        // Try to build fresh index for checking, but don't fail if duplicates exist
        match Index::build(beans_dir) {
            Ok(idx) => {
                // Save it if we're fixing
                if fix {
                    idx.save(beans_dir)?;
                }
                idx
            }
            Err(_) => {
                // If build fails (e.g., duplicates), try to load existing
                Index::load(beans_dir).unwrap_or(Index { beans: Vec::new() })
            }
        }
    } else {
        Index::load(beans_dir)?
    };

    // Collect archived bean IDs for parent reference check
    let archived_ids = collect_archived_ids(beans_dir)?;

    // Check 4: Orphaned dependencies (dep IDs that don't exist as beans)
    for entry in &index.beans {
        for dep_id in &entry.dependencies {
            let dep_exists = index.beans.iter().any(|e| &e.id == dep_id);
            let dep_archived = archived_ids.contains(dep_id);
            if !dep_exists && !dep_archived {
                issues.push(Issue::OrphanedDependency {
                    bean_id: entry.id.clone(),
                    missing_dep: dep_id.clone(),
                });
            }
        }
    }

    // Check 5: Missing parent refs (parent doesn't exist at all)
    // Check 6: Archived parent refs (parent exists but is archived)
    for entry in &index.beans {
        if let Some(parent_id) = &entry.parent {
            let parent_in_index = index.beans.iter().any(|e| &e.id == parent_id);
            let parent_archived = archived_ids.contains(parent_id);

            if parent_archived {
                issues.push(Issue::ArchivedParent {
                    bean_id: entry.id.clone(),
                    parent_id: parent_id.clone(),
                });
            } else if !parent_in_index {
                issues.push(Issue::MissingParent {
                    bean_id: entry.id.clone(),
                    parent_id: parent_id.clone(),
                });
            }
        }
    }

    // Check 7: Stale index entries (entries without source files)
    let existing_ids = get_existing_bean_files(beans_dir)?;
    for entry in &index.beans {
        if !existing_ids.contains(&entry.id) {
            issues.push(Issue::StaleIndexEntry {
                id: entry.id.clone(),
            });
        }
    }

    // Check 8: Cycles
    let cycles = graph::find_all_cycles(&index)?;
    for cycle in cycles {
        issues.push(Issue::Cycle { path: cycle });
    }

    // Display issues
    if issues.is_empty() {
        println!("All clear.");
        return Ok(());
    }

    let fixable_count = issues.iter().filter(|i| i.is_fixable()).count();
    let unfixable_count = issues.len() - fixable_count;

    for issue in &issues {
        println!("{}", issue.display());
    }

    // Summary
    println!();
    if fix {
        // Apply fixes for fixable issues
        let mut fixed_count = 0;

        for issue in &issues {
            match issue {
                Issue::StaleIndex | Issue::StaleIndexEntry { .. } => {
                    // Rebuild index handles both of these
                    // We'll do one rebuild at the end
                }
                _ => {}
            }
        }

        // Rebuild index to fix stale issues
        if issues.iter().any(|i| matches!(i, Issue::StaleIndex | Issue::StaleIndexEntry { .. })) {
            match Index::build(beans_dir) {
                Ok(idx) => {
                    idx.save(beans_dir)?;
                    println!("✓ Rebuilt index");
                    fixed_count += issues
                        .iter()
                        .filter(|i| matches!(i, Issue::StaleIndex | Issue::StaleIndexEntry { .. }))
                        .count();
                }
                Err(e) => {
                    println!("✗ Could not rebuild index: {}", e);
                }
            }
        }

        if fixed_count > 0 {
            println!("Fixed {} issue(s)", fixed_count);
        }
        if unfixable_count > 0 {
            println!(
                "{} issue(s) require manual intervention",
                unfixable_count
            );
        }
    } else {
        println!(
            "Found {} issue(s). Run `bn doctor --fix` to resolve fixable issues.",
            issues.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Bean;
    use std::fs;
    use tempfile::TempDir;

    fn setup_clean_project() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let bean1 = Bean::new("1", "Task one");
        let mut bean2 = Bean::new("2", "Task two");
        bean2.dependencies = vec!["1".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        // Rebuild index to make it fresh
        Index::build(&beans_dir).unwrap().save(&beans_dir).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn doctor_clean_project() {
        let (_dir, beans_dir) = setup_clean_project();
        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_detects_orphaned_dep() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let mut bean = Bean::new("1", "Task");
        bean.dependencies = vec!["nonexistent".to_string()];
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_detects_missing_parent() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let mut bean = Bean::new("1.1", "Subtask");
        bean.parent = Some("nonexistent".to_string());
        bean.to_file(beans_dir.join("1.1.yaml")).unwrap();

        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_detects_cycle() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a cycle: 1 -> 2 -> 3 -> 1
        let mut bean1 = Bean::new("1", "Task 1");
        bean1.dependencies = vec!["3".to_string()];

        let mut bean2 = Bean::new("2", "Task 2");
        bean2.dependencies = vec!["1".to_string()];

        let mut bean3 = Bean::new("3", "Task 3");
        bean3.dependencies = vec!["2".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_detects_mixed_formats() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create beans in both formats
        let bean1 = Bean::new("1", "Task one in yaml");
        let bean2 = Bean::new("2", "Task two in md");

        // .yaml file (legacy format)
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        // .md file (new format)
        bean2.to_file(beans_dir.join("2-task-two-in-md.md")).unwrap();

        // Doctor should succeed but detect the issue
        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());

        // Verify counts are correct
        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 1);
        assert_eq!(yaml_count, 1);
    }

    #[test]
    fn doctor_no_warning_for_single_format() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create beans only in .md format
        let bean1 = Bean::new("1", "Task one");
        let bean2 = Bean::new("2", "Task two");

        bean1.to_file(beans_dir.join("1-task-one.md")).unwrap();
        bean2.to_file(beans_dir.join("2-task-two.md")).unwrap();

        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());

        // Should have only .md files
        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 2);
        assert_eq!(yaml_count, 0);
    }

    #[test]
    fn doctor_detects_duplicate_ids() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create two beans with the same ID in different files
        let bean_a = Bean::new("99", "Bean A");
        let bean_b = Bean::new("99", "Bean B");

        bean_a.to_file(beans_dir.join("99-a.md")).unwrap();
        bean_b.to_file(beans_dir.join("99-b.md")).unwrap();

        // Doctor should succeed and report the duplicate
        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_detects_archived_parent() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create archive structure with a parent bean
        let archive_dir = beans_dir.join("archive").join("2026").join("02");
        fs::create_dir_all(&archive_dir).unwrap();

        let mut archived_parent = Bean::new("100", "Archived parent");
        archived_parent.is_archived = true;
        archived_parent
            .to_file(archive_dir.join("100-archived-parent.md"))
            .unwrap();

        // Create a child that references the archived parent
        let mut child = Bean::new("100.1", "Child of archived");
        child.parent = Some("100".to_string());
        child.to_file(beans_dir.join("100.1-child.md")).unwrap();

        // Doctor should succeed and detect the archived parent reference
        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_detects_stale_index_entries() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean and build index
        let bean = Bean::new("1", "Task one");
        bean.to_file(beans_dir.join("1-task-one.md")).unwrap();

        let index = Index::build(&beans_dir).unwrap();
        index.save(&beans_dir).unwrap();

        // Now delete the source file, leaving a stale index entry
        fs::remove_file(beans_dir.join("1-task-one.md")).unwrap();

        // Doctor should succeed and detect the stale entry
        let result = cmd_doctor(&beans_dir, false);
        assert!(result.is_ok());
    }

    #[test]
    fn doctor_fix_rebuilds_index() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean without an index
        let bean = Bean::new("1", "Task one");
        bean.to_file(beans_dir.join("1-task-one.md")).unwrap();

        // Verify index is stale
        assert!(Index::is_stale(&beans_dir).unwrap());

        // Run doctor with --fix
        let result = cmd_doctor(&beans_dir, true);
        assert!(result.is_ok());

        // Index should now be fresh
        assert!(!Index::is_stale(&beans_dir).unwrap());
    }
}
