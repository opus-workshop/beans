use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::bean::{Bean, Status};
use crate::discovery::find_bean_file;
use crate::index::Index;

/// Claim a bean for work.
///
/// Sets status to InProgress, records who claimed it and when.
/// The bean must be in Open status to be claimed.
pub fn cmd_claim(beans_dir: &Path, id: &str, by: Option<String>) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)
        .map_err(|_| anyhow!("Bean not found: {}", id))?;

    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    if bean.status != Status::Open {
        return Err(anyhow!(
            "Bean {} is {} -- only open beans can be claimed",
            id,
            bean.status
        ));
    }

    let now = Utc::now();
    bean.status = Status::InProgress;
    bean.claimed_by = by.clone();
    bean.claimed_at = Some(now);
    bean.updated_at = now;

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    let claimer = by.as_deref().unwrap_or("anonymous");
    println!("Claimed bean {}: {} (by {})", id, bean.title, claimer);

    // Rebuild index
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    Ok(())
}

/// Release a claim on a bean.
///
/// Clears claimed_by/claimed_at and sets status back to Open.
pub fn cmd_release(beans_dir: &Path, id: &str) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)
        .map_err(|_| anyhow!("Bean not found: {}", id))?;

    let mut bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    let now = Utc::now();
    bean.claimed_by = None;
    bean.claimed_at = None;
    bean.status = Status::Open;
    bean.updated_at = now;

    bean.to_file(&bean_path)
        .with_context(|| format!("Failed to save bean: {}", id))?;

    println!("Released claim on bean {}: {}", id, bean.title);

    // Rebuild index
    let index = Index::build(beans_dir)
        .with_context(|| "Failed to rebuild index")?;
    index.save(beans_dir)
        .with_context(|| "Failed to save index")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn test_claim_open_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", Some("alice".to_string())).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert_eq!(updated.claimed_by, Some("alice".to_string()));
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn test_claim_without_by() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", None).unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::InProgress);
        assert_eq!(updated.claimed_by, None);
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn test_claim_non_open_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_claim(&beans_dir, "1", Some("bob".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_closed_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::Closed;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        let result = cmd_claim(&beans_dir, "1", Some("bob".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_nonexistent_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_claim(&beans_dir, "99", Some("alice".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_release_claimed_bean() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.claimed_by = Some("alice".to_string());
        bean.claimed_at = Some(Utc::now());
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_release(&beans_dir, "1").unwrap();

        let updated = Bean::from_file(beans_dir.join("1.yaml")).unwrap();
        assert_eq!(updated.status, Status::Open);
        assert_eq!(updated.claimed_by, None);
        assert_eq!(updated.claimed_at, None);
    }

    #[test]
    fn test_release_nonexistent_bean_fails() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let result = cmd_release(&beans_dir, "99");
        assert!(result.is_err());
    }

    #[test]
    fn test_claim_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let bean = Bean::new("1", "Task");
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_claim(&beans_dir, "1", Some("alice".to_string())).unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        let entry = &index.beans[0];
        assert_eq!(entry.status, Status::InProgress);
    }

    #[test]
    fn test_release_rebuilds_index() {
        let (_dir, beans_dir) = setup_test_beans_dir();
        let mut bean = Bean::new("1", "Task");
        bean.status = Status::InProgress;
        bean.to_file(beans_dir.join("1.yaml")).unwrap();

        cmd_release(&beans_dir, "1").unwrap();

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 1);
        let entry = &index.beans[0];
        assert_eq!(entry.status, Status::Open);
    }
}
