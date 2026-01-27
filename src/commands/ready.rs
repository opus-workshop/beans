use std::path::Path;

use anyhow::Result;

use crate::bean::Status;
use crate::index::{Index, IndexEntry};

/// Show beans ready to work on (status=open AND all dependencies closed)
/// Sorted by priority (P0 first), then by id
pub fn cmd_ready(beans_dir: &Path) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    // Filter: status=open AND all deps closed
    let ready: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|entry| {
            if entry.status != Status::Open {
                return false;
            }
            // Check if all dependencies are closed
            resolve_blocked(entry, &index).is_empty()
        })
        .collect();

    if ready.is_empty() {
        println!("No beans ready to work on.");
    } else {
        // Sort by priority (ascending, so P0 first), then by id
        let mut sorted_ready = ready;
        sorted_ready.sort_by(|a, b| {
            match a.priority.cmp(&b.priority) {
                std::cmp::Ordering::Equal => natural_cmp(&a.id, &b.id),
                other => other,
            }
        });

        for entry in sorted_ready {
            println!("P{}  {}    {}", entry.priority, entry.id, entry.title);
        }
    }

    Ok(())
}

/// Show beans blocked by unresolved dependencies
/// Output: "3.2  title  ← blocked by: 2" format
pub fn cmd_blocked(beans_dir: &Path) -> Result<()> {
    let index = Index::load_or_rebuild(beans_dir)?;

    // Filter: status=open AND any dep not closed
    let blocked: Vec<&IndexEntry> = index
        .beans
        .iter()
        .filter(|entry| {
            if entry.status != Status::Open {
                return false;
            }
            // Check if any dependencies are not closed
            !resolve_blocked(entry, &index).is_empty()
        })
        .collect();

    if blocked.is_empty() {
        println!("No blocked beans.");
    } else {
        // Sort by priority, then id (same as ready)
        let mut sorted_blocked = blocked;
        sorted_blocked.sort_by(|a, b| {
            match a.priority.cmp(&b.priority) {
                std::cmp::Ordering::Equal => natural_cmp(&a.id, &b.id),
                other => other,
            }
        });

        for entry in sorted_blocked {
            let blockers = resolve_blocked(entry, &index);
            let blockers_str = blockers.join(", ");
            println!("{}  {}  ← blocked by: {}", entry.id, entry.title, blockers_str);
        }
    }

    Ok(())
}

/// Return list of dependency IDs that are not closed
/// Only check immediate deps (not transitive)
fn resolve_blocked(entry: &IndexEntry, index: &Index) -> Vec<String> {
    let mut blocked_by = Vec::new();

    for dep_id in &entry.dependencies {
        if let Some(dep_entry) = index.beans.iter().find(|e| &e.id == dep_id) {
            if dep_entry.status != Status::Closed {
                blocked_by.push(dep_id.clone());
            }
        } else {
            // Dependency doesn't exist in index, consider it blocking
            blocked_by.push(dep_id.clone());
        }
    }

    blocked_by
}

/// Compare two bean IDs using natural ordering (same as in index.rs)
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let sa = parse_id_segments(a);
    let sb = parse_id_segments(b);
    sa.cmp(&sb)
}

/// Parse a dot-separated ID into numeric segments
fn parse_id_segments(id: &str) -> Vec<u64> {
    id.split('.')
        .filter_map(|seg| seg.parse::<u64>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_beans() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create beans with various dependency and status states
        let bean1 = crate::bean::Bean::new("1", "Task one");
        let bean2 = crate::bean::Bean::new("2", "Task two");
        let mut bean3 = crate::bean::Bean::new("3", "Task three - depends on 1");
        bean3.dependencies = vec!["1".to_string()];
        let mut bean4 = crate::bean::Bean::new("4", "Task four - depends on 2");
        bean4.dependencies = vec!["2".to_string()];
        let mut bean2_closed = crate::bean::Bean::new("5", "Task five - depends on closed");
        bean2_closed.dependencies = vec!["closed-bean".to_string()];

        let mut closed_bean = crate::bean::Bean::new("closed-bean", "Closed");
        closed_bean.status = Status::Closed;

        bean1.to_file(beans_dir.join("1.yaml")).unwrap();
        bean2.to_file(beans_dir.join("2.yaml")).unwrap();
        bean3.to_file(beans_dir.join("3.yaml")).unwrap();
        bean4.to_file(beans_dir.join("4.yaml")).unwrap();
        bean2_closed.to_file(beans_dir.join("5.yaml")).unwrap();
        closed_bean.to_file(beans_dir.join("closed-bean.yaml")).unwrap();

        fs::write(
            beans_dir.join("config.yaml"),
            "project: test\nnext_id: 6\n",
        )
        .unwrap();

        (dir, beans_dir)
    }

    #[test]
    fn resolve_blocked_no_deps() {
        let index = Index::build(&setup_test_beans().1).unwrap();
        let entry = index.beans.iter().find(|e| e.id == "1").unwrap();
        assert!(resolve_blocked(entry, &index).is_empty());
    }

    #[test]
    fn resolve_blocked_with_open_dep() {
        let index = Index::build(&setup_test_beans().1).unwrap();
        let entry = index.beans.iter().find(|e| e.id == "3").unwrap();
        let blocked = resolve_blocked(entry, &index);
        assert_eq!(blocked, vec!["1".to_string()]);
    }

    #[test]
    fn resolve_blocked_with_closed_dep() {
        let index = Index::build(&setup_test_beans().1).unwrap();
        let entry = index.beans.iter().find(|e| e.id == "5").unwrap();
        let blocked = resolve_blocked(entry, &index);
        assert!(blocked.is_empty());
    }

    #[test]
    fn cmd_ready_filters_open_with_closed_deps() {
        let (_dir, beans_dir) = setup_test_beans();

        // Bean 1 and 2 have no deps, so they're ready
        // Bean 3 depends on 1 (open), so not ready
        // Bean 4 depends on 2 (open), so not ready
        // Bean 5 depends on closed-bean (closed), so it's ready
        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        let ready: Vec<&IndexEntry> = index
            .beans
            .iter()
            .filter(|entry| {
                if entry.status != Status::Open {
                    return false;
                }
                resolve_blocked(entry, &index).is_empty()
            })
            .collect();

        assert_eq!(ready.len(), 3); // 1, 2, 5
        let ids: Vec<&str> = ready.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"1"));
        assert!(ids.contains(&"2"));
        assert!(ids.contains(&"5"));
    }

    #[test]
    fn cmd_blocked_filters_open_with_open_deps() {
        let (_dir, beans_dir) = setup_test_beans();

        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        let blocked: Vec<&IndexEntry> = index
            .beans
            .iter()
            .filter(|entry| {
                if entry.status != Status::Open {
                    return false;
                }
                !resolve_blocked(entry, &index).is_empty()
            })
            .collect();

        assert_eq!(blocked.len(), 2); // 3 and 4
        let ids: Vec<&str> = blocked.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"3"));
        assert!(ids.contains(&"4"));
    }

    #[test]
    fn sort_by_priority_then_id() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let mut b1 = crate::bean::Bean::new("1", "P2 first");
        b1.priority = 2;
        let mut b2 = crate::bean::Bean::new("2", "P1 second");
        b2.priority = 1;
        let mut b3 = crate::bean::Bean::new("3", "P2 third");
        b3.priority = 2;

        b1.to_file(beans_dir.join("1.yaml")).unwrap();
        b2.to_file(beans_dir.join("2.yaml")).unwrap();
        b3.to_file(beans_dir.join("3.yaml")).unwrap();

        let index = Index::load_or_rebuild(&beans_dir).unwrap();

        let mut ready: Vec<&IndexEntry> = index.beans.iter().collect();
        ready.sort_by(|a, b| {
            match a.priority.cmp(&b.priority) {
                std::cmp::Ordering::Equal => natural_cmp(&a.id, &b.id),
                other => other,
            }
        });

        // Should be: 2 (P1), 1 (P2), 3 (P2)
        assert_eq!(ready[0].id, "2");
        assert_eq!(ready[1].id, "1");
        assert_eq!(ready[2].id, "3");
    }

    #[test]
    fn natural_cmp_works() {
        assert_eq!(natural_cmp("1", "2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("10", "2"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("3.1", "3.2"), std::cmp::Ordering::Less);
    }
}
