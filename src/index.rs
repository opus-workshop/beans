use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::bean::{Bean, Status};
use crate::util::natural_cmp;

// ---------------------------------------------------------------------------
// IndexEntry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub priority: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// Artifacts this bean produces (for smart dependency inference)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<String>,
    /// Artifacts this bean requires (for smart dependency inference)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}

impl From<&Bean> for IndexEntry {
    fn from(bean: &Bean) -> Self {
        Self {
            id: bean.id.clone(),
            title: bean.title.clone(),
            status: bean.status,
            priority: bean.priority,
            parent: bean.parent.clone(),
            dependencies: bean.dependencies.clone(),
            labels: bean.labels.clone(),
            assignee: bean.assignee.clone(),
            updated_at: bean.updated_at,
            produces: bean.produces.clone(),
            requires: bean.requires.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub beans: Vec<IndexEntry>,
}

// Files to exclude when scanning for bean YAMLs.
const EXCLUDED_FILES: &[&str] = &["config.yaml", "index.yaml", "bean.yaml"];

/// Check if a filename represents a bean file (not a config/index/template file).
fn is_bean_filename(filename: &str) -> bool {
    if EXCLUDED_FILES.contains(&filename) {
        return false;
    }
    let ext = std::path::Path::new(filename).extension().and_then(|e| e.to_str());
    match ext {
        Some("md") => filename.contains('-'),  // New format: {id}-{slug}.md
        Some("yaml") => true,                  // Legacy format: {id}.yaml
        _ => false,
    }
}

/// Count bean files by format in the beans directory.
/// Returns (md_count, yaml_count) tuple.
pub fn count_bean_formats(beans_dir: &Path) -> Result<(usize, usize)> {
    let mut md_count = 0;
    let mut yaml_count = 0;

    let dir_entries = fs::read_dir(beans_dir)
        .with_context(|| format!("Failed to read directory: {}", beans_dir.display()))?;

    for entry in dir_entries {
        let entry = entry?;
        let path = entry.path();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if !is_bean_filename(filename) {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str());
        match ext {
            Some("md") => md_count += 1,
            Some("yaml") => yaml_count += 1,
            _ => {}
        }
    }

    Ok((md_count, yaml_count))
}

impl Index {
    /// Build the index by reading all bean files from the beans directory.
    /// Supports both new format ({id}-{slug}.md) and legacy format ({id}.yaml).
    /// Excludes config.yaml, index.yaml, and bean.yaml.
    /// Sorts entries by ID using natural ordering.
    /// Returns an error if duplicate bean IDs are detected.
    pub fn build(beans_dir: &Path) -> Result<Self> {
        let mut entries = Vec::new();
        // Track which files define each ID to detect duplicates
        let mut id_to_files: HashMap<String, Vec<String>> = HashMap::new();

        let dir_entries = fs::read_dir(beans_dir)
            .with_context(|| format!("Failed to read directory: {}", beans_dir.display()))?;

        for entry in dir_entries {
            let entry = entry?;
            let path = entry.path();

            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            if !is_bean_filename(filename) {
                continue;
            }

            let bean = Bean::from_file(&path)
                .with_context(|| format!("Failed to parse bean: {}", path.display()))?;
            
            // Track this ID's file for duplicate detection
            id_to_files
                .entry(bean.id.clone())
                .or_default()
                .push(filename.to_string());
            
            entries.push(IndexEntry::from(&bean));
        }

        // Check for duplicate IDs
        let duplicates: Vec<_> = id_to_files
            .iter()
            .filter(|(_, files)| files.len() > 1)
            .collect();

        if !duplicates.is_empty() {
            let mut msg = String::from("Duplicate bean IDs detected:\n");
            for (id, files) in duplicates {
                msg.push_str(&format!("  ID '{}' defined in: {}\n", id, files.join(", ")));
            }
            return Err(anyhow!(msg));
        }

        entries.sort_by(|a, b| natural_cmp(&a.id, &b.id));

        Ok(Index { beans: entries })
    }

    /// Check whether the cached index is stale.
    /// Returns true if the index file is missing or if any bean file (.md or .yaml)
    /// in the beans directory has been modified after the index was last written.
    pub fn is_stale(beans_dir: &Path) -> Result<bool> {
        let index_path = beans_dir.join("index.yaml");

        // If index doesn't exist, it's stale
        if !index_path.exists() {
            return Ok(true);
        }

        let index_mtime = fs::metadata(&index_path)
            .with_context(|| "Failed to read index.yaml metadata")?
            .modified()
            .with_context(|| "Failed to get index.yaml mtime")?;

        let dir_entries = fs::read_dir(beans_dir)
            .with_context(|| format!("Failed to read directory: {}", beans_dir.display()))?;

        for entry in dir_entries {
            let entry = entry?;
            let path = entry.path();

            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            if !is_bean_filename(filename) {
                continue;
            }

            let file_mtime = fs::metadata(&path)
                .with_context(|| format!("Failed to read metadata: {}", path.display()))?
                .modified()
                .with_context(|| format!("Failed to get mtime: {}", path.display()))?;

            if file_mtime > index_mtime {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Load the cached index or rebuild it if stale.
    /// This is the main entry point for read-heavy commands.
    pub fn load_or_rebuild(beans_dir: &Path) -> Result<Self> {
        if Self::is_stale(beans_dir)? {
            let index = Self::build(beans_dir)?;
            index.save(beans_dir)?;
            Ok(index)
        } else {
            Self::load(beans_dir)
        }
    }

    /// Load the index from the cached index.yaml file.
    pub fn load(beans_dir: &Path) -> Result<Self> {
        let index_path = beans_dir.join("index.yaml");
        let contents = fs::read_to_string(&index_path)
            .with_context(|| format!("Failed to read {}", index_path.display()))?;
        let index: Index = serde_yaml::from_str(&contents)
            .with_context(|| "Failed to parse index.yaml")?;
        Ok(index)
    }

    /// Save the index to .beans/index.yaml.
    pub fn save(&self, beans_dir: &Path) -> Result<()> {
        let index_path = beans_dir.join("index.yaml");
        let yaml = serde_yaml::to_string(self)
            .with_context(|| "Failed to serialize index")?;
        fs::write(&index_path, yaml)
            .with_context(|| format!("Failed to write {}", index_path.display()))?;
        Ok(())
    }

    /// Collect all archived beans from .beans/archive/ directory.
    /// Walks through year/month subdirectories and loads all bean files.
    /// Returns IndexEntry items for archived beans.
    pub fn collect_archived(beans_dir: &Path) -> Result<Vec<IndexEntry>> {
        let mut entries = Vec::new();
        let archive_dir = beans_dir.join("archive");

        if !archive_dir.is_dir() {
            return Ok(entries);
        }

        // Walk through archive directory recursively
        Self::walk_archive_dir(&archive_dir, &mut entries)?;

        Ok(entries)
    }

    /// Recursively walk archive directory and collect bean entries
    fn walk_archive_dir(dir: &Path, entries: &mut Vec<IndexEntry>) -> Result<()> {
        use crate::bean::Bean;
        
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Recurse into subdirectories (year/month)
                Self::walk_archive_dir(&path, entries)?;
            } else if path.is_file() {
                // Check if it's a bean file
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if is_bean_filename(filename) {
                        if let Ok(bean) = Bean::from_file(&path) {
                            entries.push(IndexEntry::from(&bean));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use std::fs;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Helper: create a .beans directory with some bean YAML files.
    fn setup_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a few beans
        let bean1 = Bean::new("1", "First task");
        let bean2 = Bean::new("2", "Second task");
        let bean10 = Bean::new("10", "Tenth task");
        let mut bean3_1 = Bean::new("3.1", "Subtask");
        bean3_1.parent = Some("3".to_string());
        bean3_1.labels = vec!["backend".to_string()];
        bean3_1.dependencies = vec!["1".to_string()];

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean10.to_file(beans_dir.join("10.yaml")).unwrap();
        bean3_1.to_file(beans_dir.join("3.1.yaml")).unwrap();

        // Create files that should be excluded
        fs::write(beans_dir.join("config.yaml"), "project: test\nnext_id: 11\n").unwrap();

        (dir, beans_dir)
    }

    // -- natural_cmp tests --

    #[test]
    fn natural_sort_basic() {
        assert_eq!(natural_cmp("1", "2"), Ordering::Less);
        assert_eq!(natural_cmp("2", "1"), Ordering::Greater);
        assert_eq!(natural_cmp("1", "1"), Ordering::Equal);
    }

    #[test]
    fn natural_sort_numeric_not_lexicographic() {
        // Lexicographic: "10" < "2", but natural: "10" > "2"
        assert_eq!(natural_cmp("2", "10"), Ordering::Less);
        assert_eq!(natural_cmp("10", "2"), Ordering::Greater);
    }

    #[test]
    fn natural_sort_dotted_ids() {
        assert_eq!(natural_cmp("3", "3.1"), Ordering::Less);
        assert_eq!(natural_cmp("3.1", "3.2"), Ordering::Less);
        assert_eq!(natural_cmp("3.2", "10"), Ordering::Less);
    }

    #[test]
    fn natural_sort_full_sequence() {
        let mut ids = vec!["10", "3.2", "1", "3", "3.1", "2"];
        ids.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(ids, vec!["1", "2", "3", "3.1", "3.2", "10"]);
    }

    // -- build tests --

    #[test]
    fn build_reads_all_beans_and_excludes_config() {
        let (_dir, beans_dir) = setup_beans_dir();
        let index = Index::build(&beans_dir).unwrap();

        // Should have 4 beans: 1, 2, 3.1, 10
        assert_eq!(index.beans.len(), 4);

        // Should be naturally sorted
        let ids: Vec<&str> = index.beans.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "3.1", "10"]);
    }

    #[test]
    fn build_extracts_fields_correctly() {
        let (_dir, beans_dir) = setup_beans_dir();
        let index = Index::build(&beans_dir).unwrap();

        let entry = index.beans.iter().find(|e| e.id == "3.1").unwrap();
        assert_eq!(entry.title, "Subtask");
        assert_eq!(entry.status, Status::Open);
        assert_eq!(entry.priority, 2);
        assert_eq!(entry.parent, Some("3".to_string()));
        assert_eq!(entry.dependencies, vec!["1".to_string()]);
        assert_eq!(entry.labels, vec!["backend".to_string()]);
    }

    #[test]
    fn build_excludes_index_and_bean_yaml() {
        let (_dir, beans_dir) = setup_beans_dir();

        // Create index.yaml and bean.yaml — these should be excluded
        fs::write(beans_dir.join("index.yaml"), "beans: []\n").unwrap();
        fs::write(beans_dir.join("bean.yaml"), "id: template\ntitle: Template\n").unwrap();

        let index = Index::build(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 4);
        assert!(!index.beans.iter().any(|e| e.id == "template"));
    }

    #[test]
    fn build_detects_duplicate_ids() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create two beans with the same ID in different files
        let bean_a = Bean::new("99", "Bean A");
        let bean_b = Bean::new("99", "Bean B");

        bean_a.to_file(beans_dir.join("99-a.md")).unwrap();
        bean_b.to_file(beans_dir.join("99-b.md")).unwrap();

        let result = Index::build(&beans_dir);
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("Duplicate bean IDs detected"));
        assert!(err.contains("99"));
        assert!(err.contains("99-a.md"));
        assert!(err.contains("99-b.md"));
    }

    #[test]
    fn build_detects_multiple_duplicate_ids() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create duplicates for two different IDs
        Bean::new("1", "First A").to_file(beans_dir.join("1-a.md")).unwrap();
        Bean::new("1", "First B").to_file(beans_dir.join("1-b.md")).unwrap();
        Bean::new("2", "Second A").to_file(beans_dir.join("2-a.md")).unwrap();
        Bean::new("2", "Second B").to_file(beans_dir.join("2-b.md")).unwrap();

        let result = Index::build(&beans_dir);
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("ID '1'"));
        assert!(err.contains("ID '2'"));
    }

    // -- is_stale tests --

    #[test]
    fn is_stale_when_index_missing() {
        let (_dir, beans_dir) = setup_beans_dir();
        assert!(Index::is_stale(&beans_dir).unwrap());
    }

    #[test]
    fn is_stale_when_yaml_newer_than_index() {
        let (_dir, beans_dir) = setup_beans_dir();

        // Build and save the index first
        let index = Index::build(&beans_dir).unwrap();
        index.save(&beans_dir).unwrap();

        // Wait a moment to ensure distinct mtimes
        thread::sleep(Duration::from_millis(50));

        // Modify a bean file — this makes it newer than the index
        let bean = Bean::new("1", "Modified first task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        assert!(Index::is_stale(&beans_dir).unwrap());
    }

    #[test]
    fn not_stale_when_index_is_fresh() {
        let (_dir, beans_dir) = setup_beans_dir();

        // Build and save
        let index = Index::build(&beans_dir).unwrap();
        index.save(&beans_dir).unwrap();

        // The index was just written, so it should not be stale
        // (index.yaml mtime >= all other yaml mtimes)
        assert!(!Index::is_stale(&beans_dir).unwrap());
    }

    // -- load_or_rebuild tests --

    #[test]
    fn load_or_rebuild_builds_when_no_index() {
        let (_dir, beans_dir) = setup_beans_dir();

        let index = Index::load_or_rebuild(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 4);

        // Should have created index.yaml
        assert!(beans_dir.join("index.yaml").exists());
    }

    #[test]
    fn load_or_rebuild_loads_when_fresh() {
        let (_dir, beans_dir) = setup_beans_dir();

        // Build + save
        let original = Index::build(&beans_dir).unwrap();
        original.save(&beans_dir).unwrap();

        // load_or_rebuild should load without rebuilding
        let loaded = Index::load_or_rebuild(&beans_dir).unwrap();
        assert_eq!(original, loaded);
    }

    // -- save / load round-trip --

    #[test]
    fn save_and_load_round_trip() {
        let (_dir, beans_dir) = setup_beans_dir();

        let index = Index::build(&beans_dir).unwrap();
        index.save(&beans_dir).unwrap();

        let loaded = Index::load(&beans_dir).unwrap();
        assert_eq!(index, loaded);
    }

    // -- empty directory --

    #[test]
    fn build_empty_directory() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let index = Index::build(&beans_dir).unwrap();
        assert!(index.beans.is_empty());
    }

    // -- is_stale ignores non-yaml files --

    #[test]
    fn is_stale_ignores_non_yaml() {
        let (_dir, beans_dir) = setup_beans_dir();

        let index = Index::build(&beans_dir).unwrap();
        index.save(&beans_dir).unwrap();

        // Create a non-yaml file after the index
        thread::sleep(Duration::from_millis(50));
        fs::write(beans_dir.join("notes.txt"), "some notes").unwrap();

        // Should NOT be stale — non-yaml files don't count
        assert!(!Index::is_stale(&beans_dir).unwrap());
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn collect_archived_finds_beans() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        
        // Create archive structure
        let archive_dir = beans_dir.join("archive").join("2026").join("02");
        fs::create_dir_all(&archive_dir).unwrap();
        
        // Create an archived bean
        let mut bean = crate::bean::Bean::new("1", "Archived task");
        bean.status = crate::bean::Status::Closed;
        bean.to_file(archive_dir.join("1-archived-task.md")).unwrap();
        
        let archived = Index::collect_archived(&beans_dir).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, "1");
        assert_eq!(archived[0].status, crate::bean::Status::Closed);
    }
    
    #[test]
    fn collect_archived_empty_when_no_archive() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        
        let archived = Index::collect_archived(&beans_dir).unwrap();
        assert!(archived.is_empty());
    }
}

#[cfg(test)]
mod format_count_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn count_bean_formats_only_yaml() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create only yaml files
        let bean1 = crate::bean::Bean::new("1", "Task 1");
        let bean2 = crate::bean::Bean::new("2", "Task 2");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 0);
        assert_eq!(yaml_count, 2);
    }

    #[test]
    fn count_bean_formats_only_md() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create only md files
        let bean1 = crate::bean::Bean::new("1", "Task 1");
        let bean2 = crate::bean::Bean::new("2", "Task 2");
        bean1.to_file(beans_dir.join("1-task-1.md")).unwrap();
        bean2.to_file(beans_dir.join("2-task-2.md")).unwrap();

        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 2);
        assert_eq!(yaml_count, 0);
    }

    #[test]
    fn count_bean_formats_mixed() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create mixed formats
        let bean1 = crate::bean::Bean::new("1", "Task 1");
        let bean2 = crate::bean::Bean::new("2", "Task 2");
        let bean3 = crate::bean::Bean::new("3", "Task 3");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2-task-2.md")).unwrap();
        bean3.to_file(beans_dir.join("3-task-3.md")).unwrap();

        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 2);
        assert_eq!(yaml_count, 1);
    }

    #[test]
    fn count_bean_formats_excludes_config_files() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create excluded yaml files (config.yaml, index.yaml)
        fs::write(beans_dir.join("config.yaml"), "project: test").unwrap();
        fs::write(beans_dir.join("index.yaml"), "beans: []").unwrap();

        // Create one actual bean
        let bean1 = crate::bean::Bean::new("1", "Task 1");
        bean1.to_file(beans_dir.join("1-task-1.md")).unwrap();

        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 1);
        assert_eq!(yaml_count, 0);  // config.yaml and index.yaml are excluded
    }

    #[test]
    fn count_bean_formats_empty_dir() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let (md_count, yaml_count) = count_bean_formats(&beans_dir).unwrap();
        assert_eq!(md_count, 0);
        assert_eq!(yaml_count, 0);
    }
}
