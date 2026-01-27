use std::path::Path;

use anyhow::Result;

use crate::bean::Bean;

/// Handle `bn show <id>` command
/// - Default: print raw YAML
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
        // Default: raw YAML
        let yaml_contents = std::fs::read_to_string(&bean_path)?;
        println!("{}", yaml_contents);
    }

    Ok(())
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
    fn show_raw_yaml_default() {
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
}
