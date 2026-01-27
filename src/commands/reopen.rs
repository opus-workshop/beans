use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::Bean;
use crate::index::Index;

/// Reopen a closed bean.
///
/// Sets status=open, clears closed_at and close_reason.
/// Updates updated_at and rebuilds index.
pub fn cmd_reopen(beans_dir: &Path, id: &str) -> Result<()> {
    let bean_path = beans_dir.join(format!("{}.yaml", id));
    if !bean_path.exists() {
        return Err(anyhow!("Bean not found: {}", id));
    }

    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    bean.status = crate::bean::Status::Open;
    bean.closed_at = None;
    bean.close_reason = None;
    bean.updated_at = Utc::now();

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    // Rebuild index
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    println!("Reopened bean {}: {}", id, bean.title);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Status;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_reopen_closed_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::Closed;
        bean.closed_at = Some(Utc::now());
        bean.close_reason = Some("Done".to_string());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_reopen(&beans_dir, "1").unwrap();

        let reopened = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(reopened.status, Status::Open);
        assert!(reopened.closed_at.is_none());
        assert!(reopened.close_reason.is_none());
    }

    #[test]
    fn test_reopen_nonexistent_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_reopen(&beans_dir, "99");
        assert!(result.is_err());
    }

    #[test]
    fn test_reopen_updates_updated_at() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::Closed;
        bean.closed_at = Some(Utc::now());
        let original_updated_at = bean.updated_at;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        cmd_reopen(&beans_dir, "1").unwrap();

        let reopened = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert!(reopened.updated_at > original_updated_at);
    }

    #[test]
    fn test_reopen_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::Closed;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_reopen(&beans_dir, "1").unwrap();

        let index = Index::load(&beans_dir).unwrap();
        let entry = index.beans.iter().find(|e| e.id == "1").unwrap();
        assert_eq!(entry.status, Status::Open);
    }

    #[test]
    fn test_reopen_open_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        // Should work fine even if already open
        cmd_reopen(&beans_dir, "1").unwrap();

        let reopened = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(reopened.status, Status::Open);
    }
}
