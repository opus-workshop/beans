use std::path::Path;

use anyhow::Result;

use crate::bean::Status;
use crate::index::Index;
use crate::util::natural_cmp;

/// Show hierarchical tree of beans with status indicators
/// If id provided: show subtree rooted at that bean
/// If no id: show full project tree
pub fn cmd_tree(beans_dir: &Path, id: Option<&str>) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    if let Some(bean_id) = id {
        // Show subtree rooted at bean_id
        print_subtree(&index, bean_id)?;
    } else {
        // Show full project tree
        print_full_tree(&index);
    }

    Ok(())
}

fn print_full_tree(index: &Index) {
    // Find root beans (those with no parent)
    let root_beans: Vec<_> = index
        .beans
        .iter()
        .filter(|e| e.parent.is_none())
        .collect();

    if root_beans.is_empty() {
        println!("No beans found.");
        return;
    }

    let mut visited = std::collections::HashSet::new();
    for root in root_beans {
        print_tree_node(index, &root.id, "", &mut visited);
    }
}

fn print_subtree(index: &Index, bean_id: &str) -> Result<()> {
    let _entry = index
        .beans
        .iter()
        .find(|e| e.id == bean_id)
        .ok_or_else(|| anyhow::anyhow!("Bean {} not found", bean_id))?;

    let mut visited = std::collections::HashSet::new();
    print_tree_node(index, bean_id, "", &mut visited);

    Ok(())
}

fn print_tree_node(
    index: &Index,
    bean_id: &str,
    prefix: &str,
    visited: &mut std::collections::HashSet<String>,
) {
    if visited.contains(bean_id) {
        return;
    }
    visited.insert(bean_id.to_string());

    // Find the bean
    if let Some(entry) = index.beans.iter().find(|e| e.id == bean_id) {
        let status_indicator = match entry.status {
            Status::Open => "[ ]",
            Status::InProgress => "[-]",
            Status::Closed => "[x]",
        };

        println!("{}{} {} {}", prefix, status_indicator, entry.id, entry.title);
    } else {
        println!("{}[!] {}", prefix, bean_id);
        return;
    }

    // Find children (beans with this bean as parent)
    let children: Vec<_> = index
        .beans
        .iter()
        .filter(|e| e.parent.as_ref() == Some(&bean_id.to_string()))
        .collect();

    // Also find dependents (beans that depend on this one)
    let dependents: Vec<_> = index
        .beans
        .iter()
        .filter(|e| e.dependencies.contains(&bean_id.to_string()))
        .collect();

    // Combine and deduplicate
    let mut all_children = children;
    for dep in dependents {
        if !all_children.iter().any(|e| e.id == dep.id) {
            all_children.push(dep);
        }
    }

    // Sort by natural order
    all_children.sort_by(|a, b| natural_cmp(&a.id, &b.id));

    for (i, child) in all_children.iter().enumerate() {
        let is_last_child = i == all_children.len() - 1;
        let connector = if is_last_child { "└── " } else { "├── " };
        let new_prefix = if is_last_child {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        print!("{}{}", prefix, connector);
        print_tree_node(index, &child.id, &new_prefix, visited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Bean;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a hierarchy:
        // 1 (root)
        // ├── 1.1 (child)
        // ├── 1.2 (child)
        // 2 (root)
        // 3 (depends on 1)

        let bean1 = Bean::new("1", "Root task");
        let mut bean1_1 = Bean::new("1.1", "Subtask");
        bean1_1.parent = Some("1".to_string());
        let mut bean1_2 = Bean::new("1.2", "Another subtask");
        bean1_2.parent = Some("1".to_string());
        let bean2 = Bean::new("2", "Another root");
        let mut bean3 = Bean::new("3", "Depends on 1");
        bean3.dependencies = vec!["1".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean1_1.to_file(beans_dir.join("1.1.yaml")).unwrap();
        bean1_2.to_file(beans_dir.join("1.2.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn full_tree_displays() {
        let (_dir, beans_dir) = setup_test_beans();
        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        // Just verify no panic
        print_full_tree(&index);
    }

    #[test]
    fn subtree_works() {
        let (_dir, beans_dir) = setup_test_beans();
        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        // Just verify no panic
        let _ = print_subtree(&index, "1");
    }

    #[test]
    fn subtree_not_found() {
        let (_dir, beans_dir) = setup_test_beans();
        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        let result = print_subtree(&index, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn status_indicators() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let b1 = Bean::new("1", "Open task");
        let mut b2 = Bean::new("2", "In progress");
        b2.status = crate::bean::Status::InProgress;
        let mut b3 = Bean::new("3", "Closed");
        b3.status = crate::bean::Status::Closed;

        b1.to_file(beans_dir.join("1.yaml")).unwrap();
        b2.to_file(beans_dir.join("2.yaml")).unwrap();
        b3.to_file(beans_dir.join("3.yaml")).unwrap();

        let index = Index::load_or_rebuild(&beans_dir).unwrap();
        print_full_tree(&index);
    }
}
