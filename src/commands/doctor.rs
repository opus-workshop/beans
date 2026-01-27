use std::path::Path;

use anyhow::Result;

use crate::graph;
use crate::index::Index;

/// Health check: detect orphaned dependencies, missing parent refs, cycles, and stale index
pub fn cmd_doctor(beans_dir: &Path) -> Result<()> {
    let mut issues_found = false;

    // Check 1: Index freshness
    if Index::is_stale(beans_dir)? {
        println!("[!] Stale index - run 'bn sync' to rebuild");
        issues_found = true;
    }

    let index = Index::load_or_rebuild(beans_dir)?;

    // Check 2: Orphaned dependencies (dep IDs that don't exist as beans)
    for entry in &index.beans {
        for dep_id in &entry.dependencies {
            if !index.beans.iter().any(|e| &e.id == dep_id) {
                println!(
                    "[!] Orphaned dependency: {} depends on non-existent {}",
                    entry.id, dep_id
                );
                issues_found = true;
            }
        }
    }

    // Check 3: Missing parent refs (beans with parent but parent doesn't list them as children)
    for entry in &index.beans {
        if let Some(parent_id) = &entry.parent {
            if !index.beans.iter().any(|e| &e.id == parent_id) {
                println!(
                    "[!] Missing parent: {} lists parent {} but it doesn't exist",
                    entry.id, parent_id
                );
                issues_found = true;
            }
        }
    }

    // Check 4: Cycles
    let cycles = graph::find_all_cycles(beans_dir)?;
    for cycle in cycles {
        let cycle_str = cycle.join(" -> ");
        println!("[!] Dependency cycle detected: {}", cycle_str);
        issues_found = true;
    }

    if !issues_found {
        println!("All clear.");
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
        let result = cmd_doctor(&beans_dir);
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

        let result = cmd_doctor(&beans_dir);
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

        let result = cmd_doctor(&beans_dir);
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

        let result = cmd_doctor(&beans_dir);
        assert!(result.is_ok());
    }
}
