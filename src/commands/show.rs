use std::path::Path;

use anyhow::Result;
use termimad::MadSkin;

use crate::bean::Bean;

/// Handle `bn show <id>` command
/// - Default: render beautifully with metadata header and markdown formatting
/// - --json: deserialize and re-serialize as JSON
/// - --short: one-line summary "{id}. {title} [{status}]"
pub fn cmd_show(id: &str, json: bool, short: bool, beans_dir: &Path) -> Result<()> {
    let bean_path = beans_dir.join(format!("{}.yaml", id));

    if !bean_path.exists() {
        println!("Bean {} not found.", id);
        return Ok(());
    }

    let bean = Bean::from_file(&bean_path)?;

    if short {
        println!("{}", format_short(&bean));
    } else if json {
        let json_str = serde_json::to_string_pretty(&bean)?;
        println!("{}", json_str);
    } else {
        // Default: beautiful markdown rendering
        render_bean(&bean)?;
    }

    Ok(())
}

/// Render a bean beautifully with metadata header and formatted markdown body
fn render_bean(bean: &Bean) -> Result<()> {
    // Print metadata header
    println!("{}", render_metadata_header(bean));

    // Print title as emphasized header
    println!("\n*{}*\n", bean.title);

    // Print description with markdown formatting if it exists
    if let Some(description) = &bean.description {
        let skin = MadSkin::default();
        let formatted = skin.term_text(description);
        println!("{}", formatted);
    }

    Ok(())
}

/// Render metadata header with ID, status, priority, and dates
fn render_metadata_header(bean: &Bean) -> String {
    let separator = "━".repeat(40);
    let status_str = format!("Status: {}", bean.status);
    let priority_str = format!("Priority: P{}", bean.priority);

    let header_line = format!(
        "  ID: {}  |  {}  |  {}",
        bean.id, status_str, priority_str
    );

    // Build metadata details with optional fields
    let mut details = Vec::new();

    if let Some(parent) = &bean.parent {
        details.push(format!("Parent: {}", parent));
    }

    if !bean.dependencies.is_empty() {
        details.push(format!("Dependencies: {}", bean.dependencies.join(", ")));
    }

    if let Some(assignee) = &bean.assignee {
        details.push(format!("Assignee: {}", assignee));
    }

    if !bean.labels.is_empty() {
        details.push(format!("Labels: {}", bean.labels.join(", ")));
    }

    // Format dates nicely
    let created = bean.created_at.format("%Y-%m-%d %H:%M:%S UTC");
    let updated = bean.updated_at.format("%Y-%m-%d %H:%M:%S UTC");
    details.push(format!("Created: {}", created));
    details.push(format!("Updated: {}", updated));

    if let Some(closed_at) = bean.closed_at {
        let closed = closed_at.format("%Y-%m-%d %H:%M:%S UTC");
        details.push(format!("Closed: {}", closed));
    }

    if let Some(reason) = &bean.close_reason {
        details.push(format!("Close reason: {}", reason));
    }

    let mut output = String::new();
    output.push_str(&separator);
    output.push('\n');
    output.push_str(&header_line);
    output.push('\n');
    output.push_str(&separator);

    if !details.is_empty() {
        output.push_str("\n\n");
        output.push_str(&details.join("\n"));
    }

    output
}

/// Format a bean as a one-line summary
fn format_short(bean: &Bean) -> String {
    format!("{}. {} [{}]", bean.id, bean.title, bean.status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn show_renders_beautifully_default() {
        let dir = TempDir::new().unwrap();
        let bean = Bean::new("1", "Test bean");
        let bean_path = dir.path().join("1.yaml");
        bean.to_file(&bean_path).unwrap();

        // Capture stdout
        let result = cmd_show("1", false, false, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn show_json() {
        let dir = TempDir::new().unwrap();
        let bean = Bean::new("1", "Test bean");
        let bean_path = dir.path().join("1.yaml");
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("1", true, false, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn show_short() {
        let dir = TempDir::new().unwrap();
        let bean = Bean::new("1", "Test bean");
        let bean_path = dir.path().join("1.yaml");
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("1", false, true, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn show_not_found() {
        let dir = TempDir::new().unwrap();
        let result = cmd_show("999", false, false, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn format_short_test() {
        let bean = Bean::new("42", "My task");
        let formatted = format_short(&bean);
        assert_eq!(formatted, "42. My task [open]");
    }

    #[test]
    fn metadata_header_includes_id_and_status() {
        let bean = Bean::new("1", "Test");
        let header = render_metadata_header(&bean);
        assert!(header.contains("ID: 1"));
        assert!(header.contains("Status: open"));
    }

    #[test]
    fn metadata_header_includes_parent_when_set() {
        let mut bean = Bean::new("1.1", "Child task");
        bean.parent = Some("1".to_string());
        let header = render_metadata_header(&bean);
        assert!(header.contains("Parent: 1"));
    }

    #[test]
    fn metadata_header_includes_dependencies() {
        let mut bean = Bean::new("2", "Task");
        bean.dependencies = vec!["1".to_string(), "1.1".to_string()];
        let header = render_metadata_header(&bean);
        assert!(header.contains("Dependencies: 1, 1.1"));
    }

    #[test]
    fn render_bean_with_description() {
        let dir = TempDir::new().unwrap();
        let mut bean = Bean::new("1", "Test bean");
        bean.description = Some("# Description\n\nThis is test markdown.".to_string());
        let bean_path = dir.path().join("1.yaml");
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("1", false, false, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn show_works_with_hierarchical_ids() {
        let dir = TempDir::new().unwrap();
        let bean = Bean::new("11.1", "Hierarchical bean");
        let bean_path = dir.path().join("11.1.yaml");
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("11.1", false, false, dir.path());
        assert!(result.is_ok());
    }
}
