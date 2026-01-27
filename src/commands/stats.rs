use std::path::Path;

use anyhow::Result;

use crate::bean::Status;
use crate::index::Index;

/// Show project statistics: counts by status, priority, and completion percentage
pub fn cmd_stats(beans_dir: &Path) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    // Count by status
    let total = index.beans.len();
    let open = index.beans.iter().filter(|e| e.status == Status::Open).count();
    let in_progress = index
        .beans
        .iter()
        .filter(|e| e.status == Status::InProgress)
        .count();
    let closed = index.beans.iter().filter(|e| e.status == Status::Closed).count();

    // Count blocked (open with unresolved dependencies)
    let blocked = index
        .beans
        .iter()
        .filter(|e| {
            if e.status != Status::Open {
                return false;
            }
            // Check if any dependencies are not closed
            for dep_id in &e.dependencies {
                if let Some(dep) = index.beans.iter().find(|d| &d.id == dep_id) {
                    if dep.status != Status::Closed {
                        return true;
                    }
                } else {
                    // Dependency doesn't exist, consider it blocking
                    return true;
                }
            }
            false
        })
        .count();

    // Count by priority
    let mut priority_counts = [0usize; 5]; // P0-P4
    for entry in &index.beans {
        if (entry.priority as usize) < 5 {
            priority_counts[entry.priority as usize] += 1;
        }
    }

    // Calculate completion percentage
    let completion_pct = if total > 0 {
        (closed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    println!("=== Bean Statistics ===");
    println!();
    println!("Total:        {}", total);
    println!("Open:         {}", open);
    println!("In Progress:  {}", in_progress);
    println!("Closed:       {}", closed);
    println!("Blocked:      {}", blocked);
    println!();
    println!("Completion:   {:.1}%", completion_pct);
    println!();
    println!("By Priority:");
    println!("  P0: {}", priority_counts[0]);
    println!("  P1: {}", priority_counts[1]);
    println!("  P2: {}", priority_counts[2]);
    println!("  P3: {}", priority_counts[3]);
    println!("  P4: {}", priority_counts[4]);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Bean;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create beans with different statuses and priorities
        let mut b1 = Bean::new("1", "Open P0");
        b1.priority = 0;

        let mut b2 = Bean::new("2", "In Progress P1");
        b2.status = Status::InProgress;
        b2.priority = 1;

        let mut b3 = Bean::new("3", "Closed P2");
        b3.status = Status::Closed;
        b3.priority = 2;

        let mut b4 = Bean::new("4", "Open P3");
        b4.priority = 3;

        let mut b5 = Bean::new("5", "Open depends on 1");
        b5.dependencies = vec!["1".to_string()];

        b1.to_file(beans_dir.join("1.yaml")).unwrap();
        b2.to_file(beans_dir.join("2.yaml")).unwrap();
        b3.to_file(beans_dir.join("3.yaml")).unwrap();
        b4.to_file(beans_dir.join("4.yaml")).unwrap();
        b5.to_file(beans_dir.join("5.yaml")).unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn stats_calculates_counts() {
        let (_dir, beans_dir) = setup_test_beans();
        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        // Verify counts
        assert_eq!(
            index.beans.iter().filter(|e| e.status == Status::Open).count(),
            3
        ); // 1, 4, 5
        assert_eq!(
            index.beans.iter().filter(|e| e.status == Status::InProgress).count(),
            1
        ); // 2
        assert_eq!(
            index.beans.iter().filter(|e| e.status == Status::Closed).count(),
            1
        ); // 3
    }

    #[test]
    fn stats_command_works() {
        let (_dir, beans_dir) = setup_test_beans();
        let result = cmd_stats(&beans_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn empty_project() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let result = cmd_stats(&beans_dir);
        assert!(result.is_ok());
    }
}
