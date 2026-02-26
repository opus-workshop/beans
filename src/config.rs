use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub project: String,
    pub next_id: u32,
    /// Auto-close parent beans when all children are closed/archived (default: true)
    #[serde(default = "default_auto_close_parent")]
    pub auto_close_parent: bool,
    /// Maximum tokens for bean context (default: 30000)
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Shell command template for `--run`. Use `{id}` as placeholder for bean ID.
    /// Example: `claude -p "implement bean {id} and run bn close {id}"`.
    /// If unset, `--run` will print an error asking the user to configure it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Shell command template for planning large beans. Uses `{id}` placeholder.
    /// If unset, plan operations will print an error asking the user to configure it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Maximum agent loops before stopping (default: 10, 0 = unlimited)
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    /// Maximum parallel agents for `bn run` (default: 4)
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    /// Seconds between polls in --watch mode (default: 30)
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u32,
    /// Paths to parent config files to inherit from (lowest to highest priority).
    /// Supports `~/` for home directory. Paths are relative to the project root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<String>,
    /// Path to project rules file, relative to .beans/ directory (default: "RULES.md").
    /// Contents are injected into every `bn context` output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_file: Option<String>,
    /// Enable file locking for concurrent agents (default: false).
    /// When enabled, agents lock files listed in bean `paths` on spawn
    /// and lock-on-write during execution. Prevents concurrent agents
    /// from clobbering the same file.
    #[serde(default, skip_serializing_if = "is_false_bool")]
    pub file_locking: bool,
}

fn default_auto_close_parent() -> bool {
    true
}

fn default_max_tokens() -> u32 {
    30000
}

fn default_max_loops() -> u32 {
    10
}

fn default_max_concurrent() -> u32 {
    4
}

fn default_poll_interval() -> u32 {
    30
}

fn is_false_bool(v: &bool) -> bool {
    !v
}

impl Config {
    /// Load config from .beans/config.yaml inside the given beans directory.
    pub fn load(beans_dir: &Path) -> Result<Self> {
        let path = beans_dir.join("config.yaml");
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        let config: Config = serde_yml::from_str(&contents)
            .with_context(|| format!("Failed to parse config at {}", path.display()))?;
        Ok(config)
    }

    /// Load config with inheritance from extended configs.
    ///
    /// Resolves the `extends` field, loading parent configs and merging
    /// inheritable fields. Local values take precedence over extended values.
    /// Fields `project`, `next_id`, and `extends` are never inherited.
    pub fn load_with_extends(beans_dir: &Path) -> Result<Self> {
        let mut config = Self::load(beans_dir)?;

        if config.extends.is_empty() {
            return Ok(config);
        }

        let mut seen = HashSet::new();
        let mut stack: Vec<String> = config.extends.clone();
        let mut parents: Vec<Config> = Vec::new();

        while let Some(path_str) = stack.pop() {
            let resolved = Self::resolve_extends_path(&path_str, beans_dir)?;

            let canonical = resolved
                .canonicalize()
                .with_context(|| format!("Cannot resolve extends path: {}", path_str))?;

            if !seen.insert(canonical.clone()) {
                continue; // Cycle detection
            }

            let contents = fs::read_to_string(&canonical).with_context(|| {
                format!("Failed to read extends config: {}", canonical.display())
            })?;
            let parent: Config = serde_yml::from_str(&contents).with_context(|| {
                format!("Failed to parse extends config: {}", canonical.display())
            })?;

            for ext in &parent.extends {
                stack.push(ext.clone());
            }

            parents.push(parent);
        }

        // Merge: closest parent first (highest priority among parents).
        // Only override local values that are still at their defaults.
        for parent in &parents {
            if config.max_tokens == default_max_tokens() {
                config.max_tokens = parent.max_tokens;
            }
            if config.run.is_none() {
                config.run = parent.run.clone();
            }
            if config.plan.is_none() {
                config.plan = parent.plan.clone();
            }
            if config.max_loops == default_max_loops() {
                config.max_loops = parent.max_loops;
            }
            if config.max_concurrent == default_max_concurrent() {
                config.max_concurrent = parent.max_concurrent;
            }
            if config.poll_interval == default_poll_interval() {
                config.poll_interval = parent.poll_interval;
            }
            if config.auto_close_parent == default_auto_close_parent() {
                config.auto_close_parent = parent.auto_close_parent;
            }
            if config.rules_file.is_none() {
                config.rules_file = parent.rules_file.clone();
            }
            if !config.file_locking {
                config.file_locking = parent.file_locking;
            }
            // Never inherit: project, next_id, extends
        }

        Ok(config)
    }

    /// Resolve an extends path to an absolute path.
    /// `~/` expands to the home directory; other paths are relative to the project root.
    fn resolve_extends_path(path_str: &str, beans_dir: &Path) -> Result<PathBuf> {
        if let Some(stripped) = path_str.strip_prefix("~/") {
            let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot resolve home directory"))?;
            Ok(home.join(stripped))
        } else {
            // Resolve relative to the project root (parent of .beans/)
            let project_root = beans_dir.parent().unwrap_or(Path::new("."));
            Ok(project_root.join(path_str))
        }
    }

    /// Save config to .beans/config.yaml inside the given beans directory.
    pub fn save(&self, beans_dir: &Path) -> Result<()> {
        let path = beans_dir.join("config.yaml");
        let contents = serde_yml::to_string(self).context("Failed to serialize config")?;
        fs::write(&path, &contents)
            .with_context(|| format!("Failed to write config at {}", path.display()))?;
        Ok(())
    }

    /// Return the path to the project rules file.
    /// Defaults to `.beans/RULES.md` if `rules_file` is not set.
    /// The path is resolved relative to the beans directory.
    pub fn rules_path(&self, beans_dir: &Path) -> PathBuf {
        match &self.rules_file {
            Some(custom) => {
                let p = Path::new(custom);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    beans_dir.join(custom)
                }
            }
            None => beans_dir.join("RULES.md"),
        }
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
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
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
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
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
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
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
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.auto_close_parent, false);
    }

    #[test]
    fn max_tokens_defaults_to_30000() {
        let dir = tempfile::tempdir().unwrap();
        // Write a config WITHOUT max_tokens field
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.max_tokens, 30000);
    }

    #[test]
    fn max_tokens_can_be_customized() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 50000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.max_tokens, 50000);
    }

    #[test]
    fn run_defaults_to_none() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.run, None);
    }

    #[test]
    fn run_can_be_set() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: Some("claude -p 'implement bean {id}'".to_string()),
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(
            loaded.run,
            Some("claude -p 'implement bean {id}'".to_string())
        );
    }

    #[test]
    fn run_not_serialized_when_none() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let contents = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!contents.contains("run:"));
    }

    #[test]
    fn max_loops_defaults_to_10() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.max_loops, 10);
    }

    #[test]
    fn max_loops_can_be_customized() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 25,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.max_loops, 25);
    }

    // --- extends tests ---

    /// Helper: write a YAML config file at the given path.
    fn write_yaml(path: &std::path::Path, yaml: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, yaml).unwrap();
    }

    /// Helper: write a minimal local config inside a beans dir, with extends.
    fn write_local_config(beans_dir: &std::path::Path, extends: &[&str], extra: &str) {
        let extends_yaml: Vec<String> = extends.iter().map(|e| format!("  - \"{}\"", e)).collect();
        let extends_block = if extends.is_empty() {
            String::new()
        } else {
            format!("extends:\n{}\n", extends_yaml.join("\n"))
        };
        let yaml = format!("project: test\nnext_id: 1\n{}{}", extends_block, extra);
        write_yaml(&beans_dir.join("config.yaml"), &yaml);
    }

    #[test]
    fn extends_empty_loads_normally() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();
        write_local_config(&beans_dir, &[], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.project, "test");
        assert_eq!(config.max_tokens, 30000); // default
        assert!(config.run.is_none());
    }

    #[test]
    fn extends_single_merges_fields() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        // Parent config (outside .beans, at project root)
        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: shared\nnext_id: 999\nmax_tokens: 50000\nrun: \"deli spawn {id}\"\nmax_loops: 20\n",
        );

        write_local_config(&beans_dir, &["shared.yaml"], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        // Inherited
        assert_eq!(config.max_tokens, 50000);
        assert_eq!(config.run, Some("deli spawn {id}".to_string()));
        assert_eq!(config.max_loops, 20);
        // Never inherited
        assert_eq!(config.project, "test");
        assert_eq!(config.next_id, 1);
    }

    #[test]
    fn extends_local_overrides_parent() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: shared\nnext_id: 999\nmax_tokens: 50000\nrun: \"parent-run\"\nmax_loops: 20\n",
        );

        // Local config sets its own max_tokens and run
        write_local_config(
            &beans_dir,
            &["shared.yaml"],
            "max_tokens: 60000\nrun: \"local-run\"\nmax_loops: 5\n",
        );

        let config = Config::load_with_extends(&beans_dir).unwrap();
        // Local values win
        assert_eq!(config.max_tokens, 60000);
        assert_eq!(config.run, Some("local-run".to_string()));
        assert_eq!(config.max_loops, 5);
    }

    #[test]
    fn extends_circular_detected_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        // A extends B, B extends A
        let a_path = dir.path().join("a.yaml");
        let b_path = dir.path().join("b.yaml");
        write_yaml(
            &a_path,
            &format!("project: a\nnext_id: 1\nextends:\n  - \"b.yaml\"\nmax_tokens: 40000\n"),
        );
        write_yaml(
            &b_path,
            &format!("project: b\nnext_id: 1\nextends:\n  - \"a.yaml\"\nmax_tokens: 50000\n"),
        );

        write_local_config(&beans_dir, &["a.yaml"], "");

        // Should not infinite loop; loads successfully
        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.project, "test");
        // Gets value from one of the parents
        assert!(config.max_tokens == 40000 || config.max_tokens == 50000);
    }

    #[test]
    fn extends_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        write_local_config(&beans_dir, &["nonexistent.yaml"], "");

        let result = Config::load_with_extends(&beans_dir);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("nonexistent.yaml"),
            "Error should mention the missing file: {}",
            err_msg
        );
    }

    #[test]
    fn extends_recursive_a_extends_b_extends_c() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        // C: base config
        let c_path = dir.path().join("c.yaml");
        write_yaml(
            &c_path,
            "project: c\nnext_id: 1\nmax_tokens: 40000\nrun: \"from-c\"\n",
        );

        // B extends C, overrides max_tokens
        let b_path = dir.path().join("b.yaml");
        write_yaml(
            &b_path,
            "project: b\nnext_id: 1\nextends:\n  - \"c.yaml\"\nmax_tokens: 50000\n",
        );

        // Local extends B
        write_local_config(&beans_dir, &["b.yaml"], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        // B's max_tokens (50000) should apply since it's the direct parent
        assert_eq!(config.max_tokens, 50000);
        // run comes from C (B doesn't set it, but C does)
        assert_eq!(config.run, Some("from-c".to_string()));
    }

    #[test]
    fn extends_project_and_next_id_never_inherited() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: parent-project\nnext_id: 999\nmax_tokens: 50000\n",
        );

        write_local_config(&beans_dir, &["shared.yaml"], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.project, "test");
        assert_eq!(config.next_id, 1);
    }

    #[test]
    fn extends_tilde_resolves_to_home_dir() {
        // We can't fully test ~ expansion without writing to the real home dir,
        // but we can verify the path resolution logic.
        let beans_dir = std::path::Path::new("/tmp/fake-beans");
        let resolved = Config::resolve_extends_path("~/shared/config.yaml", beans_dir).unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolved, home.join("shared/config.yaml"));
    }

    #[test]
    fn extends_not_serialized_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let contents = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!contents.contains("extends"));
    }

    #[test]
    fn extends_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert!(loaded.extends.is_empty());
    }

    // --- plan, max_concurrent, poll_interval tests ---

    #[test]
    fn plan_defaults_to_none() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.plan, None);
    }

    #[test]
    fn plan_can_be_set() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: Some("claude -p 'plan bean {id}'".to_string()),
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.plan, Some("claude -p 'plan bean {id}'".to_string()));
    }

    #[test]
    fn plan_not_serialized_when_none() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let contents = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!contents.contains("plan:"));
    }

    #[test]
    fn max_concurrent_defaults_to_4() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.max_concurrent, 4);
    }

    #[test]
    fn max_concurrent_can_be_customized() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 8,
            poll_interval: 30,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.max_concurrent, 8);
    }

    #[test]
    fn poll_interval_defaults_to_30() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "project: test\nnext_id: 1\n",
        )
        .unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.poll_interval, 30);
    }

    #[test]
    fn poll_interval_can_be_customized() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: None,
            max_loops: 10,
            max_concurrent: 4,
            poll_interval: 60,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };
        config.save(dir.path()).unwrap();

        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.poll_interval, 60);
    }

    #[test]
    fn extends_inherits_plan() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: shared\nnext_id: 999\nplan: \"plan-cmd {id}\"\n",
        );

        write_local_config(&beans_dir, &["shared.yaml"], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.plan, Some("plan-cmd {id}".to_string()));
    }

    #[test]
    fn extends_inherits_max_concurrent() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: shared\nnext_id: 999\nmax_concurrent: 16\n",
        );

        write_local_config(&beans_dir, &["shared.yaml"], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.max_concurrent, 16);
    }

    #[test]
    fn extends_inherits_poll_interval() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: shared\nnext_id: 999\npoll_interval: 120\n",
        );

        write_local_config(&beans_dir, &["shared.yaml"], "");

        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.poll_interval, 120);
    }

    #[test]
    fn extends_local_overrides_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir_all(&beans_dir).unwrap();

        let parent_path = dir.path().join("shared.yaml");
        write_yaml(
            &parent_path,
            "project: shared\nnext_id: 999\nplan: \"parent-plan\"\nmax_concurrent: 16\npoll_interval: 120\n",
        );

        write_local_config(
            &beans_dir,
            &["shared.yaml"],
            "plan: \"local-plan\"\nmax_concurrent: 2\npoll_interval: 10\n",
        );

        let config = Config::load_with_extends(&beans_dir).unwrap();
        assert_eq!(config.plan, Some("local-plan".to_string()));
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.poll_interval, 10);
    }

    #[test]
    fn new_fields_round_trip_through_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            project: "test".to_string(),
            next_id: 1,
            auto_close_parent: true,
            max_tokens: 30000,
            run: None,
            plan: Some("plan {id}".to_string()),
            max_loops: 10,
            max_concurrent: 8,
            poll_interval: 60,
            extends: vec![],
            rules_file: None,
            file_locking: false,
        };

        config.save(dir.path()).unwrap();
        let loaded = Config::load(dir.path()).unwrap();

        assert_eq!(config, loaded);
    }
}
