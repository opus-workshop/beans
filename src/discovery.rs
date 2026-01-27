use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

/// Walk up from `start` looking for a `.beans/` directory.
/// Returns the path to the `.beans/` directory if found.
/// Errors if no `.beans/` directory exists in any ancestor.
pub fn find_beans_dir(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(".beans");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !current.pop() {
            bail!("No .beans/ directory found. Run `bn init` first.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_beans_in_current_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".beans")).unwrap();

        let result = find_beans_dir(dir.path()).unwrap();
        assert_eq!(result, dir.path().join(".beans"));
    }

    #[test]
    fn finds_beans_in_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".beans")).unwrap();
        let child = dir.path().join("src");
        fs::create_dir(&child).unwrap();

        let result = find_beans_dir(&child).unwrap();
        assert_eq!(result, dir.path().join(".beans"));
    }

    #[test]
    fn finds_beans_in_grandparent_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".beans")).unwrap();
        let child = dir.path().join("src").join("deep");
        fs::create_dir_all(&child).unwrap();

        let result = find_beans_dir(&child).unwrap();
        assert_eq!(result, dir.path().join(".beans"));
    }

    #[test]
    fn returns_error_when_no_beans_exists() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("some").join("nested").join("dir");
        fs::create_dir_all(&child).unwrap();

        let result = find_beans_dir(&child);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No .beans/ directory found"),
            "Error message was: {}",
            err_msg
        );
    }

    #[test]
    fn prefers_closest_beans_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Parent has .beans
        fs::create_dir(dir.path().join(".beans")).unwrap();
        // Child also has .beans
        let child = dir.path().join("subproject");
        fs::create_dir(&child).unwrap();
        fs::create_dir(child.join(".beans")).unwrap();

        let result = find_beans_dir(&child).unwrap();
        assert_eq!(result, child.join(".beans"));
    }
}
