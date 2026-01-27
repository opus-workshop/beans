use std::collections::{HashMap, HashSet};

use anyhow::anyhow;
use anyhow::Result;

use crate::index::Index;

/// Detect a cycle in the dependency graph.
///
/// Uses DFS from `to_id` to check if `from_id` is reachable.
/// If so, adding the edge from_id -> to_id would create a cycle.
pub fn detect_cycle(index: &Index, from_id: &str, to_id: &str) -> Result<bool> {
    // Quick check: self-dependency
    if from_id == to_id {
        return Ok(true);
    }

    // Build adjacency list from index
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &index.beans {
        graph.insert(entry.id.clone(), entry.dependencies.clone());
    }

    // DFS from to_id: if we reach from_id, there's a cycle
    let mut visited = HashSet::new();
    let mut stack = vec![to_id.to_string()];

    while let Some(current) = stack.pop() {
        if current == from_id {
            return Ok(true);
        }

        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());

        if let Some(deps) = graph.get(&current) {
            for dep in deps {
                if !visited.contains(dep) {
                    stack.push(dep.clone());
                }
            }
        }
    }

    Ok(false)
}

/// Build a dependency tree rooted at `id`.
/// Returns a string representation with box-drawing characters.
pub fn build_dependency_tree(index: &Index, id: &str) -> Result<String> {
    // Find the root bean
    let root_entry = index
        .beans
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow!("Bean {} not found", id))?;

    let mut output = String::new();
    output.push_str(&format!("{} {}\n", root_entry.id, root_entry.title));

    // Build adjacency list
    let graph: HashMap<String, Vec<String>> = index
        .beans
        .iter()
        .map(|e| (e.id.clone(), e.dependencies.clone()))
        .collect();

    // Build reverse graph (dependents)
    let mut reverse_graph: HashMap<String, Vec<String>> = HashMap::new();
    for (id, deps) in &graph {
        for dep in deps {
            reverse_graph
                .entry(dep.clone())
                .or_insert_with(Vec::new)
                .push(id.clone());
        }
    }

    // Create index map
    let id_map: HashMap<String, &crate::index::IndexEntry> =
        index.beans.iter().map(|e| (e.id.clone(), e)).collect();

    // DFS to build tree
    let mut visited = HashSet::new();
    build_tree_recursive(
        &mut output,
        id,
        &reverse_graph,
        &id_map,
        &mut visited,
        "",
    );

    Ok(output)
}

fn build_tree_recursive(
    output: &mut String,
    current_id: &str,
    reverse_graph: &HashMap<String, Vec<String>>,
    id_map: &HashMap<String, &crate::index::IndexEntry>,
    visited: &mut HashSet<String>,
    prefix: &str,
) {
    if visited.contains(current_id) {
        return;
    }
    visited.insert(current_id.to_string());

    if let Some(dependents) = reverse_graph.get(current_id) {
        for (i, dependent_id) in dependents.iter().enumerate() {
            let is_last_dependent = i == dependents.len() - 1;

            let connector = if is_last_dependent { "└── " } else { "├── " };
            output.push_str(prefix);
            output.push_str(connector);

            if let Some(entry) = id_map.get(dependent_id) {
                output.push_str(&format!("{} {}\n", entry.id, entry.title));
            } else {
                output.push_str(&format!("{}\n", dependent_id));
            }

            let new_prefix = if is_last_dependent {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };

            build_tree_recursive(
                output,
                dependent_id,
                reverse_graph,
                id_map,
                visited,
                &new_prefix,
            );
        }
    }
}

/// Build a project-wide dependency graph as a text tree.
/// Shows all dependencies rooted at beans with no parents.
pub fn build_full_graph(index: &Index) -> Result<String> {
    // Find root beans (those with no parent)
    let root_beans: Vec<_> = index
        .beans
        .iter()
        .filter(|e| e.parent.is_none())
        .collect();

    if root_beans.is_empty() {
        return Ok("No beans found.".to_string());
    }

    let mut output = String::new();

    // Build reverse graph (dependents)
    let mut reverse_graph: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &index.beans {
        for dep in &entry.dependencies {
            reverse_graph
                .entry(dep.clone())
                .or_insert_with(Vec::new)
                .push(entry.id.clone());
        }
    }

    // Create index map
    let id_map: HashMap<String, &crate::index::IndexEntry> =
        index.beans.iter().map(|e| (e.id.clone(), e)).collect();

    let mut visited = HashSet::new();
    for root in root_beans {
        output.push_str(&format!("{} {}\n", root.id, root.title));
        build_tree_recursive(
            &mut output,
            &root.id,
            &reverse_graph,
            &id_map,
            &mut visited,
            "",
        );
    }

    Ok(output)
}

/// Find all cycles in the dependency graph.
/// Returns a list of cycle paths.
pub fn find_all_cycles(index: &Index) -> Result<Vec<Vec<String>>> {
    let mut cycles = Vec::new();

    // Build adjacency list
    let graph: HashMap<String, Vec<String>> = index
        .beans
        .iter()
        .map(|e| (e.id.clone(), e.dependencies.clone()))
        .collect();

    // For each node, check if there's a cycle starting from it
    for start_id in graph.keys() {
        let mut visited = HashSet::new();
        let mut path = vec![start_id.clone()];
        if find_cycle_dfs(&graph, start_id, &mut visited, &mut path, &mut cycles) {
            // Cycle found
        }
    }

    Ok(cycles)
}

fn find_cycle_dfs(
    graph: &HashMap<String, Vec<String>>,
    current: &str,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) -> bool {
    if visited.contains(current) {
        // Check if it's in the current path
        if let Some(pos) = path.iter().position(|id| id == current) {
            // Found a cycle
            let cycle = path[pos..].to_vec();
            if !cycles.contains(&cycle) {
                cycles.push(cycle);
            }
            return true;
        }
        return false;
    }

    visited.insert(current.to_string());

    if let Some(deps) = graph.get(current) {
        for dep in deps {
            path.push(dep.clone());
            find_cycle_dfs(graph, dep, visited, path, cycles);
            path.pop();
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Bean;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans(specs: Vec<(&str, Vec<&str>)>) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        for (id, deps) in specs {
            let mut bean = Bean::new(id, &format!("Task {}", id));
            bean.dependencies = deps.iter().map(|s| s.to_string()).collect();
            bean.to_file(beans_dir.join(format!("{}.yaml", id))).unwrap();
        }

        (dir, beans_dir)
    }

    #[test]
    fn detect_self_cycle() {
        let (_dir, beans_dir) = setup_test_beans(vec![("1", vec![])]);
        let index = Index::build(&beans_dir).unwrap();
        assert!(detect_cycle(&index, "1", "1").unwrap());
    }

    #[test]
    fn detect_two_node_cycle() {
        let (_dir, beans_dir) = setup_test_beans(vec![("1", vec!["2"]), ("2", vec![])]);
        let index = Index::build(&beans_dir).unwrap();
        assert!(detect_cycle(&index, "2", "1").unwrap());
        assert!(!detect_cycle(&index, "1", "2").unwrap());
    }

    #[test]
    fn detect_three_node_cycle() {
        let (_dir, beans_dir) =
            setup_test_beans(vec![("1", vec!["2"]), ("2", vec!["3"]), ("3", vec![])]);
        let index = Index::build(&beans_dir).unwrap();
        // If we add 3 -> 1, it creates a cycle
        assert!(detect_cycle(&index, "3", "1").unwrap());
        assert!(!detect_cycle(&index, "1", "3").unwrap());
    }

    #[test]
    fn no_cycle_linear_chain() {
        let (_dir, beans_dir) =
            setup_test_beans(vec![("1", vec!["2"]), ("2", vec!["3"]), ("3", vec![])]);
        let index = Index::build(&beans_dir).unwrap();
        assert!(!detect_cycle(&index, "1", "2").unwrap());
        assert!(!detect_cycle(&index, "2", "3").unwrap());
    }
}
