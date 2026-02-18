use std::path::Path;

use anyhow::{anyhow, Result};

use crate::config::Config;

/// Get a configuration value by key
pub fn cmd_config_get(beans_dir: &Path, key: &str) -> Result<()> {
    let config = Config::load(beans_dir)?;

    let value = match key {
        "project" => config.project,
        "next_id" => config.next_id.to_string(),
        "auto_close_parent" => config.auto_close_parent.to_string(),
        "max_tokens" => config.max_tokens.to_string(),
        "run" => config.run.unwrap_or_default(),
        _ => return Err(anyhow!("Unknown config key: {}", key)),
    };

    println!("{}", value);
    Ok(())
}

/// Set a configuration value by key
pub fn cmd_config_set(beans_dir: &Path, key: &str, value: &str) -> Result<()> {
    let mut config = Config::load(beans_dir)?;

    match key {
        "project" => {
            config.project = value.to_string();
        }
        "next_id" => {
            config.next_id = value
                .parse()
                .map_err(|_| anyhow!("Invalid value for next_id: {}", value))?;
        }
        "auto_close_parent" => {
            config.auto_close_parent = value
                .parse()
                .map_err(|_| anyhow!("Invalid value for auto_close_parent: {} (expected true/false)", value))?;
        }
        "max_tokens" => {
            config.max_tokens = value
                .parse()
                .map_err(|_| anyhow!("Invalid value for max_tokens: {} (expected positive integer)", value))?;
        }
        "run" => {
            if value.is_empty() || value == "none" || value == "unset" {
                config.run = None;
            } else {
                config.run = Some(value.to_string());
            }
        }
        _ => return Err(anyhow!("Unknown config key: {}", key)),
    }

    config.save(beans_dir)?;
    println!("Set {} = {}", key, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\nauto_close_parent: true\nmax_tokens: 30000\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn get_max_tokens_returns_value() {
        let dir = setup_test_dir();
        // Just verify it doesn't error - output goes to stdout
        let result = cmd_config_get(dir.path(), "max_tokens");
        assert!(result.is_ok());
    }

    #[test]
    fn get_unknown_key_returns_error() {
        let dir = setup_test_dir();
        let result = cmd_config_get(dir.path(), "unknown_key");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown config key"));
    }

    #[test]
    fn set_max_tokens_updates_config() {
        let dir = setup_test_dir();
        cmd_config_set(dir.path(), "max_tokens", "50000").unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.max_tokens, 50000);
    }

    #[test]
    fn set_max_tokens_with_invalid_value_returns_error() {
        let dir = setup_test_dir();
        let result = cmd_config_set(dir.path(), "max_tokens", "not_a_number");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid value"));
    }

    #[test]
    fn set_unknown_key_returns_error() {
        let dir = setup_test_dir();
        let result = cmd_config_set(dir.path(), "unknown_key", "value");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown config key"));
    }

    #[test]
    fn get_run_returns_empty_when_unset() {
        let dir = setup_test_dir();
        let result = cmd_config_get(dir.path(), "run");
        assert!(result.is_ok());
    }

    #[test]
    fn set_run_stores_command_template() {
        let dir = setup_test_dir();
        cmd_config_set(dir.path(), "run", "claude -p 'implement bean {id}'").unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.run, Some("claude -p 'implement bean {id}'".to_string()));
    }

    #[test]
    fn set_run_to_none_clears_it() {
        let dir = setup_test_dir();
        cmd_config_set(dir.path(), "run", "some command").unwrap();
        cmd_config_set(dir.path(), "run", "none").unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.run, None);
    }

    #[test]
    fn set_run_to_empty_clears_it() {
        let dir = setup_test_dir();
        cmd_config_set(dir.path(), "run", "some command").unwrap();
        cmd_config_set(dir.path(), "run", "").unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.run, None);
    }
}
