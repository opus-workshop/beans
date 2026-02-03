use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub project: String,
    pub next_id: u32,
    /// Auto-close parent beans when all children are closed/archived (default: true)
    #[serde(default = "default_auto_close_parent")]
    pub auto_close_parent: bool,
}

fn default_auto_close_parent() -> bool {
    true
}

impl Config {
    /// Load config from .beans/config.yaml inside the given beans directory.
    pub fn load(beans_dir: &Path) -> Result<Self> {
        let path = beans_dir.join("config.yaml");
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        let config: Config = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to parse config at {}", path.display()))?;
        Ok(config)
    }

    /// Save config to .beans/config.yaml inside the given beans directory.
    pub fn save(&self, beans_dir: &Path) -> Result<()> {
        let path = beans_dir.join("config.yaml");
        let contents = serde_yaml::to_string(self)
            .context("Failed to serialize config")?;
        fs::write(&path, &contents)
            .with_context(|| format!("Failed to write config at {}", path.display()))?;
        Ok(())
    }

    /// Return the current next_id and increment it for the next call.
    pub fn increment_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn config_round_trips_through_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test-project".to_string(),
            next_id: 42,
            auto_close_parent: true,
        };

        config.save(dir.path()).unwrap();
        let loaded = Config::load(dir.path()).unwrap();

        assert_eq!(config, loaded);
    }

    #[test]
    fn increment_id_returns_current_and_bumps() {
        let mut config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
        };

        assert_eq!(config.increment_id(), 1);
        assert_eq!(config.increment_id(), 2);
        assert_eq!(config.increment_id(), 3);
        assert_eq!(config.next_id, 4);
    }

    #[test]
    fn load_returns_error_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = Config::load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_returns_error_for_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.yaml"), "not: [valid: yaml: config").unwrap();
        let result = Config::load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn save_creates_file_that_is_valid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "my-project".to_string(),
            next_id: 100,
            auto_close_parent: true,
        };
        config.save(dir.path()).unwrap();

        let contents = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(contents.contains("project: my-project"));
        assert!(contents.contains("next_id: 100"));
    }

    #[test]
    fn auto_close_parent_defaults_to_true() {
        let dir = tempfile::tempdir().unwrap();
        // Write a config WITHOUT auto_close_parent field
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.auto_close_parent, true);
    }

    #[test]
    fn auto_close_parent_can_be_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.auto_close_parent, false);
    }
}
