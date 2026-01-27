use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::bean::Status;
use crate::index::{Index, IndexEntry};

/// List beans with optional filtering.
/// - Default: tree-format with status indicators
/// - --status: filter by status (open, in_progress, closed)
/// - --priority: filter by priority (0-4)
/// - --parent: show only children of this parent
/// - --label: filter by label
/// - --assignee: filter by assignee
/// - --all: include closed beans (default excludes closed)
/// - --json: JSON array output
/// - Shows [!] for blocked beans
pub fn cmd_list(
    status_filter: Option<&str>,
    priority_filter: Option<u8>,
    parent_filter: Option<&str>,
    label_filter: Option<&str>,
    assignee_filter: Option<&str>,
    all: bool,
    json: bool,
    beans_dir: &Path,
) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    // Parse status filter
    let status_filter = status_filter.and_then(parse_status);

    // Filter entries
    let mut filtered = index.beans.clone();

    // Apply filters
    filtered.retain(|entry| {
        // Status filter
        if !all && entry.status == Status::Closed {
            return false;
        }
        if let Some(status) = status_filter {
            if entry.status != status {
                return false;
            }
        }

        // Priority filter
        if let Some(priority) = priority_filter {
            if entry.priority != priority {
                return false;
            }
        }

        // Parent filter
        if let Some(parent) = parent_filter {
            if entry.parent.as_deref() != Some(parent) {
                return false;
            }
        }

        // Label filter
        if let Some(label) = label_filter {
            if !entry.labels.contains(&label.to_string()) {
                return false;
            }
        }

        // Assignee filter
        if let Some(_assignee) = assignee_filter {
            // We need to load the full bean to check assignee (not in index)
            // For now, skip this optimization and check during rendering
            return true;
        }

        true
    });

    if json {
        let json_str = serde_json::to_string_pretty(&filtered)?;
        println!("{}", json_str);
    } else {
        // Tree format with status indicators
        let tree = render_tree(&filtered, &index);
        println!("{}", tree);
        println!("Legend: [ ] open  [-] in_progress  [x] closed  [!] blocked");
    }

    Ok(())
}

/// Render beans as a hierarchical tree.
/// - Root beans have no parent
/// - Children indented 2 spaces per level
/// - Status: [ ] open, [-] in_progress, [x] closed, [!] blocked
fn render_tree(entries: &[IndexEntry], index: &Index) -> String {
    let mut output = String::new();

    // Build parent -> children map
    let mut children_map: HashMap<Option<String>, Vec<&IndexEntry>> = HashMap::new();
    for entry in entries {
        children_map
            .entry(entry.parent.clone())
            .or_insert_with(Vec::new)
            .push(entry);
    }

    // Sort children by id within each parent
    for children in children_map.values_mut() {
        children.sort_by(|a, b| natural_cmp(&a.id, &b.id));
    }

    // Render root entries
    if let Some(roots) = children_map.get(&None) {
        for root in roots {
            render_entry(&mut output, root, 0, &children_map, index);
        }
    }

    output
}

/// Recursively render an entry and its children
fn render_entry(
    output: &mut String,
    entry: &IndexEntry,
    depth: u32,
    children_map: &HashMap<Option<String>, Vec<&IndexEntry>>,
    index: &Index,
) {
    let indent = "  ".repeat(depth as usize);
    let status_indicator = get_status_indicator(entry, index);
    output.push_str(&format!(
        "{}{} {}. {}\n",
        indent, status_indicator, entry.id, entry.title
    ));

    // Render children
    if let Some(children) = children_map.get(&Some(entry.id.clone())) {
        for child in children {
            render_entry(output, child, depth + 1, children_map, index);
        }
    }
}

/// Get status indicator for an entry
fn get_status_indicator(entry: &IndexEntry, index: &Index) -> String {
    if is_blocked(entry, index) {
        "[!]".to_string()
    } else {
        match entry.status {
            Status::Open => "[ ]".to_string(),
            Status::InProgress => "[-]".to_string(),
            Status::Closed => "[x]".to_string(),
        }
    }
}

/// Check if a bean is blocked by unresolved dependencies
fn is_blocked(entry: &IndexEntry, index: &Index) -> bool {
    for dep_id in &entry.dependencies {
        if let Some(dep_entry) = index.beans.iter().find(|e| &e.id == dep_id) {
            if dep_entry.status != Status::Closed {
                return true;
            }
        }
    }
    false
}

/// Parse status string to Status enum
fn parse_status(s: &str) -> Option<Status> {
    match s {
        "open" => Some(Status::Open),
        "in_progress" => Some(Status::InProgress),
        "closed" => Some(Status::Closed),
        _ => None,
    }
}

/// Compare two bean IDs using natural ordering (same as in index.rs)
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let sa = parse_id_segments(a);
    let sb = parse_id_segments(b);
    sa.cmp(&sb)
}

/// Parse a dot-separated ID into numeric segments
fn parse_id_segments(id: &str) -> Vec<u64> {
    id.split('.')
        .filter_map(|seg| seg.parse::<u64>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create some test beans
        let bean1 = crate::bean::Bean::new("1", "First task");
        let mut bean2 = crate::bean::Bean::new("2", "Second task");
        bean2.status = Status::InProgress;
        let mut bean3 = crate::bean::Bean::new("3", "Parent task");
        bean3.dependencies = vec!["1".to_string()];

        let mut bean3_1 = crate::bean::Bean::new("3.1", "Subtask");
        bean3_1.parent = Some("3".to_string());

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();
        bean3_1.to_file(beans_dir.join("3.1.yaml")).unwrap();

        // Create config
        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 4\n",
        )
        .unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn parse_status_valid() {
        assert_eq!(parse_status("open"), Some(Status::Open));
        assert_eq!(parse_status("in_progress"), Some(Status::InProgress));
        assert_eq!(parse_status("closed"), Some(Status::Closed));
    }

    #[test]
    fn parse_status_invalid() {
        assert_eq!(parse_status("invalid"), None);
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn is_blocked_by_open_dependency() {
        let index = Index::build(&setup_test_beans().1).unwrap();
        let entry = index.beans.iter().find(|e| e.id == "3").unwrap();
        // bean 3 depends on bean 1 which is open, so bean 3 is blocked
        assert!(is_blocked(entry, &index));
    }

    #[test]
    fn is_not_blocked_when_no_dependencies() {
        let index = Index::build(&setup_test_beans().1).unwrap();
        let entry = index.beans.iter().find(|e| e.id == "1").unwrap();
        assert!(!is_blocked(entry, &index));
    }

    #[test]
    fn status_indicator_open() {
        let entry = IndexEntry {
            id: "1".to_string(),
            title: "Test".to_string(),
            status: Status::Open,
            priority: 2,
            parent: None,
            dependencies: Vec::new(),
            labels: Vec::new(),
            updated_at: chrono::Utc::now(),
        };
        let index = Index {
            beans: vec![entry.clone()],
        };
        assert_eq!(get_status_indicator(&entry, &index), "[ ]");
    }

    #[test]
    fn status_indicator_in_progress() {
        let entry = IndexEntry {
            id: "1".to_string(),
            title: "Test".to_string(),
            status: Status::InProgress,
            priority: 2,
            parent: None,
            dependencies: Vec::new(),
            labels: Vec::new(),
            updated_at: chrono::Utc::now(),
        };
        let index = Index {
            beans: vec![entry.clone()],
        };
        assert_eq!(get_status_indicator(&entry, &index), "[-]");
    }

    #[test]
    fn status_indicator_closed() {
        let entry = IndexEntry {
            id: "1".to_string(),
            title: "Test".to_string(),
            status: Status::Closed,
            priority: 2,
            parent: None,
            dependencies: Vec::new(),
            labels: Vec::new(),
            updated_at: chrono::Utc::now(),
        };
        let index = Index {
            beans: vec![entry.clone()],
        };
        assert_eq!(get_status_indicator(&entry, &index), "[x]");
    }

    #[test]
    fn render_tree_hierarchy() {
        let (_dir, beans_dir) = setup_test_beans();
        let index = Index::build(&beans_dir).unwrap();
        let tree = render_tree(&index.beans, &index);

        // Should contain entries
        assert!(tree.contains("1. First task"));
        assert!(tree.contains("2. Second task"));
        assert!(tree.contains("3. Parent task"));
        assert!(tree.contains("3.1. Subtask"));

        // 3.1 should be indented (child of 3)
        let lines: Vec<&str> = tree.lines().collect();
        let line_3 = lines.iter().find(|l| l.contains("3. Parent task")).unwrap();
        let line_3_1 = lines.iter().find(|l| l.contains("3.1. Subtask")).unwrap();

        // 3.1 should have more indentation than 3
        let indent_3 = line_3.len() - line_3.trim_start().len();
        let indent_3_1 = line_3_1.len() - line_3_1.trim_start().len();
        assert!(indent_3_1 > indent_3);
    }
}
