use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::Bean;
use crate::index::Index;

#[cfg(test)]
use std::fs;

/// Close one or more beans.
///
/// Sets status=closed, closed_at=now, and optionally close_reason.
/// Rebuilds the index.
pub fn cmd_close(
    beans_dir: &Path,
    ids: Vec<String>,
    reason: Option<String>,
) -> Result<()> {
    if ids.is_empty() {
        return Err(anyhow!("At least one bean ID is required"));
    }

    let now = Utc::now();

    for id in ids {
        let bean_path = beans_dir.join(format!("{}.yaml", id));
        if !bean_path.exists() {
            return Err(anyhow!("Bean not found: {}", id));
        }

        let mut bean = Bean::from_file(&bean_path)
            .with_context(|| format!("Failed to load bean: {}", id))?;

        bean.status = crate::bean::Status::Closed;
        bean.closed_at = Some(now);
        bean.close_reason = reason.clone();
        bean.updated_at = now;

        bean.to_file(&bean_path)
            .with_context(|| format!("Failed to save bean: {}", id))?;

        println!("Closed bean {}: {}", id, bean.title);
    }

    // Rebuild index once after all updates
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Status;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_close_single_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert!(updated.closed_at.is_some());
        assert!(updated.close_reason.is_none());
    }

    #[test]
    fn test_close_with_reason() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], Some("Fixed".to_string())).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Closed);
        assert_eq!(updated.close_reason, Some("Fixed".to_string()));
    }

    #[test]
    fn test_close_multiple_beans() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        let bean3 = Bean::new("3", "Task 3");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string(), "2".to_string(), "3".to_string()], None).unwrap();

        for id in &["1", "2", "3"] {
            let bean = Bean::from_file(beans_dir.join(format!("{}.yaml", id))).unwrap();
            assert_eq!(bean.status, Status::Closed);
            assert!(bean.closed_at.is_some());
        }
    }

    #[test]
    fn test_close_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_close(&beans_dir, vec!["99".to_string()], None);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_no_ids() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_close(&beans_dir, vec![], None);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean1 = Bean::new("1", "Task 1");
        let bean2 = Bean::new("2", "Task 2");
        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 2);
        let entry1 = index.beans.iter().find(|e| e.id == "1").unwrap();
        assert_eq!(entry1.status, Status::Closed);
    }

    #[test]
    fn test_close_sets_updated_at() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        let original_updated_at = bean.updated_at;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        cmd_close(&beans_dir, vec!["1".to_string()], None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert!(updated.updated_at > original_updated_at);
    }
}
