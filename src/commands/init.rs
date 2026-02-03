use std::fs;
use std::env;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;

/// Initialize a .beans/ directory with a config.yaml file.
///
/// If `path` is provided, use it. Otherwise, use the current directory.
/// If `project_name` is provided, use it. Otherwise, auto-detect from the directory name.
pub fn cmd_init(path: Option<&Path>, project_name: Option<String>) -> Result<()> {
    let cwd = if let Some(p) = path {
        p.to_path_buf()
    } else {
        env::current_dir()?
    };
    let beans_dir = cwd.join(".beans");

    // Create .beans/ directory if it doesn't exist
    if !beans_dir.exists() {
        fs::create_dir(&beans_dir)
            .with_context(|| format!("Failed to create .beans directory at {}", beans_dir.display()))?;
    } else if !beans_dir.is_dir() {
        anyhow::bail!(".beans exists but is not a directory");
    }

    // Determine project name
    let project = if let Some(name) = project_name {
        name
    } else {
        // Auto-detect from directory name
        cwd.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project".to_string())
    };

    // Create config
    let config = Config {
        project: project.clone(),
        next_id: 1,
        auto_close_parent: true,
    };

    config.save(&beans_dir)?;

    println!("Initialized beans in .beans/");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn init_creates_beans_dir() {
        let dir = TempDir::new().unwrap();
        let result = cmd_init(Some(dir.path()), None);

        assert!(result.is_ok());
        assert!(dir.path().join(".beans").exists());
        assert!(dir.path().join(".beans").is_dir());
    }

    #[test]
    fn init_creates_config_with_explicit_name() {
        let dir = TempDir::new().unwrap();
        let result = cmd_init(Some(dir.path()), Some("my-project".to_string()));

        assert!(result.is_ok());

        let config = Config::load(&dir.path().join(".beans")).unwrap();
        assert_eq!(config.project, "my-project");
        assert_eq!(config.next_id, 1);
    }

    #[test]
    fn init_auto_detects_project_name_from_dir() {
        let dir = TempDir::new().unwrap();
        let result = cmd_init(Some(dir.path()), None);

        assert!(result.is_ok());

        let config = Config::load(&dir.path().join(".beans")).unwrap();
        // The auto-detected name should match the temp directory's name
        let dir_name = dir.path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");
        assert_eq!(config.project, dir_name);
    }

    #[test]
    fn init_idempotent() {
        let dir = TempDir::new().unwrap();

        // First init
        let result1 = cmd_init(Some(dir.path()), Some("test-project".to_string()));
        assert!(result1.is_ok());

        // Second init — should succeed without error
        let result2 = cmd_init(Some(dir.path()), Some("test-project".to_string()));
        assert!(result2.is_ok());

        // Check config
        let config = Config::load(&dir.path().join(".beans")).unwrap();
        assert_eq!(config.project, "test-project");
    }

    #[test]
    fn init_config_is_valid_yaml() {
        let dir = TempDir::new().unwrap();
        let result = cmd_init(Some(dir.path()), Some("yaml-test".to_string()));

        assert!(result.is_ok());

        let config_path = dir.path().join(".beans").join("config.yaml");
        assert!(config_path.exists());

        let contents = fs::read_to_string(&config_path).unwrap();
        assert!(contents.contains("project: yaml-test"));
        assert!(contents.contains("next_id: 1"));
    }
}
