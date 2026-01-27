use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::Bean;
use crate::index::Index;
use crate::graph::{detect_cycle, build_dependency_tree, build_full_graph, find_all_cycles};

/// Add a dependency: `bn dep add <id> <depends-on-id>`
/// Sets id.dependencies to include depends-on-id.
/// Checks for cycles before adding.
pub fn cmd_dep_add(beans_dir: &Path, id: &str, depends_on_id: &str) -> Result<()> {
    // Verify both beans exist
    let bean_path = beans_dir.join(format!("{}.yaml", id));
    if !bean_path.exists() {
        return Err(anyhow!("Bean {} not found", id));
    }

    let depends_on_path = beans_dir.join(format!("{}.yaml", depends_on_id));
    if !depends_on_path.exists() {
        return Err(anyhow!("Bean {} not found", depends_on_id));
    }

    // Check for self-dependency
    if id == depends_on_id {
        return Err(anyhow!(
            "Cannot add self-dependency: {} cannot depend on itself",
            id
        ));
    }

    // Load index once for cycle detection
    let index = Index::load_or_rebuild(beans_dir)?;

    // Check for cycles
    if detect_cycle(&index, id, depends_on_id)? {
        return Err(anyhow!(
            "Dependency cycle detected: adding {} -> {} would create a cycle. Edge not added.",
            id,
            depends_on_id
        ));
    }

    // Load the bean and add dependency
    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    // Check if already dependent
    if bean.dependencies.contains(&depends_on_id.to_string()) {
        return Err(anyhow!(
            "Bean {} already depends on {}",
            id,
            depends_on_id
        ));
    }

    bean.dependencies.push(depends_on_id.to_string());
    bean.updated_at = Utc::now();

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    // Rebuild index
    let index = Index::build(beans_dir).with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir).with_context(|| "Failed to save index")?;

    println!("{} now depends on {}", id, depends_on_id);

    Ok(())
}

/// Remove a dependency: `bn dep remove <id> <depends-on-id>`
pub fn cmd_dep_remove(beans_dir: &Path, id: &str, depends_on_id: &str) -> Result<()> {
    let bean_path = beans_dir.join(format!("{}.yaml", id));
    if !bean_path.exists() {
        return Err(anyhow!("Bean {} not found", id));
    }

    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    let original_len = bean.dependencies.len();
    bean.dependencies.retain(|d| d != depends_on_id);

    if bean.dependencies.len() == original_len {
        return Err(anyhow!(
            "Bean {} does not depend on {}",
            id,
            depends_on_id
        ));
    }

    bean.updated_at = Utc::now();
    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    // Rebuild index
    let index = Index::build(beans_dir).with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir).with_context(|| "Failed to save index")?;

    println!("{} no longer depends on {}", id, depends_on_id);

    Ok(())
}

/// List dependencies and dependents: `bn dep list <id>`
pub fn cmd_dep_list(beans_dir: &Path, id: &str) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let entry = index
        .beans
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow!("Bean {} not found", id))?;

    // Create id -> entry map
    let id_map: HashMap<String, &crate::index::IndexEntry> =
        index.beans.iter().map(|e| (e.id.clone(), e)).collect();

    // Print dependencies
    println!("Dependencies ({}):", entry.dependencies.len());
    if entry.dependencies.is_empty() {
        println!("  (none)");
    } else {
        for dep_id in &entry.dependencies {
            if let Some(dep_entry) = id_map.get(dep_id) {
                println!("  {} {}", dep_entry.id, dep_entry.title);
            } else {
                println!("  {} (not found)", dep_id);
            }
        }
    }

    // Find dependents (reverse lookup)
    let dependents: Vec<_> = index
        .beans
        .iter()
        .filter(|e| e.dependencies.contains(&id.to_string()))
        .collect();

    println!("\nDependents ({}):", dependents.len());
    if dependents.is_empty() {
        println!("  (none)");
    } else {
        for dep in dependents {
            println!("  {} {}", dep.id, dep.title);
        }
    }

    Ok(())
}

/// Show dependency tree: `bn dep tree [id]`
/// If id provided, show tree rooted at that bean.
/// If no id, show project-wide DAG.
pub fn cmd_dep_tree(beans_dir: &Path, id: Option<&str>) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    let tree = if let Some(id) = id {
        build_dependency_tree(&index, id)?
    } else {
        build_full_graph(&index)?
    };

    println!("{}", tree);

    Ok(())
}

/// Detect cycles: `bn dep cycles`
pub fn cmd_dep_cycles(beans_dir: &Path) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;
    let cycles = find_all_cycles(&index)?;

    if cycles.is_empty() {
        println!("No cycles detected.");
    } else {
        println!("Dependency cycles detected:");
        for cycle in cycles {
            let cycle_str = cycle.join(" -> ");
            println!("  {} -> {} (repeats)", cycle_str, cycle[0]);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_dep_add_simple() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        cmd_dep_add(&beans_dir, "1", "2").unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.dependencies, vec!["2".to_string()]);
    }

    #[test]
    fn test_dep_add_self_dependency_rejected() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_dep_add(&beans_dir, "1", "1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("self-dependency"));
    }

    #[test]
    fn test_dep_add_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_dep_add(&beans_dir, "1", "999");
        assert!(result.is_err());
    }

    #[test]
    fn test_dep_add_cycle_detection() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        bean1.dependencies = vec!["2".to_string()];
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        // Rebuild index so it's fresh
        Index::build(&beans_dir).unwrap().save(&beans_dir).unwrap();

        // Try to add 2 -> 1, which creates a cycle
        let result = cmd_dep_add(&beans_dir, "2", "1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle"));
    }

    #[test]
    fn test_dep_remove() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        bean1.dependencies = vec!["2".to_string()];
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        cmd_dep_remove(&beans_dir, "1", "2").unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.dependencies, Vec::<String>::new());
    }

    #[test]
    fn test_dep_remove_not_found() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_dep_remove(&beans_dir, "1", "2");
        assert!(result.is_err());
    }

    #[test]
    fn test_dep_list_with_dependencies() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        let mut bean3 = Bean::new("3", "Task 3");
        bean1.dependencies = vec!["2".to_string()];
        bean3.dependencies = vec!["1".to_string()];
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        // This should succeed — just testing that it runs
        let result = cmd_dep_list(&beans_dir, "1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_dep_add_duplicate_rejected() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        bean1.dependencies = vec!["2".to_string()];
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        // Try to add the same dependency again
        let result = cmd_dep_add(&beans_dir, "1", "2");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already depends"));
    }
}
