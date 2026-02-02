use std::path::Path;

use anyhow::Result;
use termimad::MadSkin;

use crate::bean::Bean;
use crate::discovery::find_bean_file;

/// Handle `bn show <id>` command
/// - Default: render beautifully with metadata header and markdown formatting
/// - --json: deserialize and re-serialize as JSON
/// - --short: one-line summary "{id}. {title} [{status}]"
pub fn cmd_show(id: &str, json: bool, short: bool, beans_dir: &Path) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)?;

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
    let skin = MadSkin::default();

    // Print metadata header
    println!("{}", render_metadata_header(bean));

    // Print title as emphasized header
    println!("\n*{}*\n", bean.title);

    // Print description with markdown formatting if it exists
    if let Some(description) = &bean.description {
        let formatted = skin.term_text(description);
        println!("{}", formatted);
    }

    // Print acceptance criteria
    if let Some(acceptance) = &bean.acceptance {
        println!("\n**Acceptance Criteria**");
        let formatted = skin.term_text(acceptance);
        println!("{}", formatted);
    }

    // Print verify command
    if let Some(verify) = &bean.verify {
        println!("\n**Verify Command**");
        println!("```");
        println!("{}", verify);
        println!("```");
    }

    // Print design notes
    if let Some(design) = &bean.design {
        println!("\n**Design**");
        let formatted = skin.term_text(design);
        println!("{}", formatted);
    }

    // Print notes
    if let Some(notes) = &bean.notes {
        println!("\n**Notes**");
        let formatted = skin.term_text(notes);
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

    // Show claim information
    if let Some(claimed_by) = &bean.claimed_by {
        details.push(format!("Claimed by: {}", claimed_by));
    }
    if let Some(claimed_at) = bean.claimed_at {
        let claimed = claimed_at.format("%Y-%m-%d %H:%M:%S UTC");
        details.push(format!("Claimed at: {}", claimed));
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
    use crate::util::title_to_slug;

    #[test]
    fn show_renders_beautifully_default() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();
        
        let bean = Bean::new("1", "Test bean");
        let slug = title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        // Capture stdout
        let result = cmd_show("1", false, false, &beans_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn show_json() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();
        
        let bean = Bean::new("1", "Test bean");
        let slug = title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("1", true, false, &beans_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn show_short() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();
        
        let bean = Bean::new("1", "Test bean");
        let slug = title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("1", false, true, &beans_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn show_not_found() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();
        
        let result = cmd_show("999", false, false, &beans_dir);
        assert!(result.is_err());
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
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();
        
        let mut bean = Bean::new("1", "Test bean");
        bean.description = Some("# Description\n\nThis is test markdown.".to_string());
        let slug = title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("1", false, false, &beans_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn show_works_with_hierarchical_ids() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        std::fs::create_dir(&beans_dir).unwrap();
        
        let bean = Bean::new("11.1", "Hierarchical bean");
        let slug = title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("11.1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_show("11.1", false, false, &beans_dir);
        assert!(result.is_ok());
    }
}
