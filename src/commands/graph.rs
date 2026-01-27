use std::path::Path;

use anyhow::Result;

use crate::index::Index;

/// Display dependency graph in Mermaid or DOT format
/// Default format is Mermaid (graph TD syntax)
/// Use --format dot for Graphviz DOT format
pub fn cmd_graph(beans_dir: &Path, format: &str) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    match format {
        "dot" => output_dot_graph(&index)?,
        "mermaid" | _ => output_mermaid_graph(&index)?,
    }

    Ok(())
}

fn output_mermaid_graph(index: &Index) -> Result<()> {
    println!("graph TD");

    // Create a set of all nodes we'll reference
    let mut nodes = std::collections::HashSet::new();

    // Output edges (dependencies)
    for entry in &index.beans {
        for dep_id in &entry.dependencies {
            println!("    {}[{}] --> {}[{}]",
                format_node_id(&entry.id),
                escape_for_mermaid(&entry.title),
                format_node_id(dep_id),
                escape_for_mermaid(
                    index.beans
                        .iter()
                        .find(|e| &e.id == dep_id)
                        .map(|e| e.title.as_str())
                        .unwrap_or(dep_id)
                )
            );
            nodes.insert(entry.id.clone());
            nodes.insert(dep_id.clone());
        }
    }

    // Add isolated nodes (beans with no dependencies and no dependents)
    for entry in &index.beans {
        if entry.dependencies.is_empty()
            && !index.beans.iter().any(|e| e.dependencies.contains(&entry.id))
        {
            if !nodes.contains(&entry.id) {
                println!("    {}[{}]",
                    format_node_id(&entry.id),
                    escape_for_mermaid(&entry.title)
                );
            }
        }
    }

    Ok(())
}

fn output_dot_graph(index: &Index) -> Result<()> {
    println!("digraph {{");
    println!("    rankdir=LR;");

    // Node declarations
    for entry in &index.beans {
        println!(
            "    \"{}\" [label=\"{}\"];",
            entry.id,
            entry.title.replace("\"", "\\\"")
        );
    }

    // Edge declarations
    for entry in &index.beans {
        for dep_id in &entry.dependencies {
            println!("    \"{}\" -> \"{}\";", entry.id, dep_id);
        }
    }

    println!("}}");

    Ok(())
}

/// Format node ID for Mermaid (replace dots with underscores)
fn format_node_id(id: &str) -> String {
    format!("N{}", id.replace('.', "_"))
}

/// Escape text for Mermaid graph labels
fn escape_for_mermaid(text: &str) -> String {
    text.replace("\"", "&quot;")
        .replace("[", "&lsqb;")
        .replace("]", "&rsqb;")
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

        let bean1 = Bean::new("1", "Task one");
        let bean2 = Bean::new("2", "Task two");
        let mut bean3 = Bean::new("3", "Task three");
        bean3.dependencies = vec!["1".to_string(), "2".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn mermaid_output_valid() {
        let (_dir, beans_dir) = setup_test_beans();
        let result = cmd_graph(&beans_dir, "mermaid");
        assert!(result.is_ok());
    }

    #[test]
    fn dot_output_valid() {
        let (_dir, beans_dir) = setup_test_beans();
        let result = cmd_graph(&beans_dir, "dot");
        assert!(result.is_ok());
    }

    #[test]
    fn default_format_is_mermaid() {
        let (_dir, beans_dir) = setup_test_beans();
        let result = cmd_graph(&beans_dir, "");
        assert!(result.is_ok());
    }

    #[test]
    fn escaping_special_chars() {
        let id = "test.id";
        let formatted = format_node_id(id);
        assert_eq!(formatted, "Ntest_id");
    }

    #[test]
    fn mermaid_escape() {
        let text = "Task [with] brackets";
        let escaped = escape_for_mermaid(text);
        assert!(escaped.contains("&lsqb;"));
        assert!(escaped.contains("&rsqb;"));
    }
}
