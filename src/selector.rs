use anyhow::{anyhow, Result};

use crate::bean::Status;
use crate::index::Index;

// ---------------------------------------------------------------------------
// SelectorType
// ---------------------------------------------------------------------------

/// Represents a selector for dynamic bean resolution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorType {
    /// @latest - resolves to the most recently updated bean
    Latest,
    /// @blocked - resolves to open beans with unresolved dependencies
    Blocked,
    /// @parent - resolves to the parent bean of the current bean
    Parent,
    /// @me - resolves to the current bean
    Me,
}

// ---------------------------------------------------------------------------
// SelectionContext
// ---------------------------------------------------------------------------

/// Context for selector resolution
#[derive(Debug, Clone)]
pub struct SelectionContext<'a> {
    /// Reference to the bean index for lookups
    pub index: &'a Index,
    /// ID of the current bean (for @me and @parent)
    pub current_bean_id: Option<String>,
    /// Current user context (for future extensions)
    pub current_user: Option<String>,
}

// ---------------------------------------------------------------------------
// parse_selector
// ---------------------------------------------------------------------------

/// Parse a selector string and return its type.
///
/// # Arguments
/// * `input` - A string that may or may not be a selector
///
/// # Returns
/// * `Ok(SelectorType)` if input starts with @ and matches a known selector
/// * `Err` if input doesn't start with @ or matches an unknown selector
///
/// # Examples
/// ```ignore
/// assert!(matches!(parse_selector("@latest")?, SelectorType::Latest));
/// assert!(matches!(parse_selector("@blocked")?, SelectorType::Blocked));
/// assert!(matches!(parse_selector("@parent")?, SelectorType::Parent));
/// assert!(matches!(parse_selector("@me")?, SelectorType::Me));
/// assert!(parse_selector("not_a_selector").is_err());
/// ```
pub fn parse_selector(input: &str) -> Result<SelectorType> {
    // Check if input starts with @
    if !input.starts_with('@') {
        return Err(anyhow!("Input does not start with @: '{}'", input));
    }

    // Extract the keyword after @
    let keyword = &input[1..];

    match keyword {
        "latest" => Ok(SelectorType::Latest),
        "blocked" => Ok(SelectorType::Blocked),
        "parent" => Ok(SelectorType::Parent),
        "me" => Ok(SelectorType::Me),
        _ => Err(anyhow!("Unknown selector keyword: '@{}'", keyword)),
    }
}

// ---------------------------------------------------------------------------
// resolve_latest
// ---------------------------------------------------------------------------

/// Resolve @latest to the bean with the most recent updated_at timestamp.
///
/// # Arguments
/// * `context` - Selection context containing the index
///
/// # Returns
/// * `Ok(bean_id)` for the most recently updated bean
/// * `Err` if the index is empty
///
/// # Examples
/// ```ignore
/// let context = SelectionContext {
///     index: &my_index,
///     current_bean_id: None,
///     current_user: None,
/// };
/// let latest_id = resolve_latest(&context)?;
/// ```
pub fn resolve_latest(context: &SelectionContext) -> Result<String> {
    if context.index.beans.is_empty() {
        return Err(anyhow!("Cannot resolve @latest: index is empty"));
    }

    // Find the bean with the maximum updated_at timestamp
    let latest = context
        .index
        .beans
        .iter()
        .max_by_key(|entry| entry.updated_at)
        .ok_or_else(|| anyhow!("Failed to find latest bean"))?;

    Ok(latest.id.clone())
}

// ---------------------------------------------------------------------------
// resolve_blocked
// ---------------------------------------------------------------------------

/// Resolve @blocked to a list of open beans with unresolved dependencies.
///
/// A bean is considered "blocked" if:
/// - Its status is Open
/// - It has at least one dependency that references a bean not in the index
///   OR a bean in the index that is still open
///
/// # Arguments
/// * `context` - Selection context containing the index
///
/// # Returns
/// * `Ok(vec_of_ids)` - list of blocked bean IDs (may be empty)
/// * Err - only on internal errors (e.g., missing index data)
///
/// # Examples
/// ```ignore
/// let context = SelectionContext {
///     index: &my_index,
///     current_bean_id: None,
///     current_user: None,
/// };
/// let blocked_ids = resolve_blocked(&context)?;
/// ```
pub fn resolve_blocked(context: &SelectionContext) -> Result<Vec<String>> {
    let mut blocked = Vec::new();

    // Create a map of bean IDs to their status for quick lookup
    let bean_status_map: std::collections::HashMap<&str, Status> = context
        .index
        .beans
        .iter()
        .map(|entry| (entry.id.as_str(), entry.status))
        .collect();

    // Iterate through all beans
    for entry in &context.index.beans {
        // Only consider open beans
        if entry.status != Status::Open {
            continue;
        }

        // Check if any dependencies are unresolved
        let has_unresolved_deps = entry.dependencies.iter().any(|dep| {
            // A dependency is unresolved if:
            // 1. The dependency bean doesn't exist in the index, OR
            // 2. The dependency bean exists but is not Closed (i.e., Open or InProgress)
            match bean_status_map.get(dep.as_str()) {
                None => true,                              // Dependency not in index
                Some(status) => *status != Status::Closed, // Dependency not closed
            }
        });

        if has_unresolved_deps {
            blocked.push(entry.id.clone());
        }
    }

    Ok(blocked)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // Helper to create a test index
    fn create_test_index(beans_data: Vec<(&str, Status, Vec<&str>)>) -> Index {
        use crate::index::IndexEntry;

        let beans = beans_data
            .into_iter()
            .map(|(id, status, deps)| IndexEntry {
                id: id.to_string(),
                title: format!("Bean {}", id),
                status,
                priority: 2,
                parent: None,
                dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
                labels: vec![],
                updated_at: Utc::now(),
            })
            .collect();

        Index { beans }
    }

    // ---------------------------------------------------------------------------
    // parse_selector tests
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_selector_latest() {
        let result = parse_selector("@latest");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SelectorType::Latest);
    }

    #[test]
    fn parse_selector_blocked() {
        let result = parse_selector("@blocked");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SelectorType::Blocked);
    }

    #[test]
    fn parse_selector_parent() {
        let result = parse_selector("@parent");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SelectorType::Parent);
    }

    #[test]
    fn parse_selector_me() {
        let result = parse_selector("@me");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SelectorType::Me);
    }

    #[test]
    fn parse_selector_rejects_no_at() {
        let result = parse_selector("latest");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not start with @"));
    }

    #[test]
    fn parse_selector_rejects_unknown_keyword() {
        let result = parse_selector("@unknown");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown selector"));
    }

    #[test]
    fn parse_selector_rejects_empty_keyword() {
        let result = parse_selector("@");
        assert!(result.is_err());
    }

    #[test]
    fn parse_selector_case_sensitive() {
        let result = parse_selector("@Latest");
        assert!(result.is_err());
    }

    // ---------------------------------------------------------------------------
    // resolve_latest tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_latest_single_bean() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_latest(&context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "1");
    }

    #[test]
    fn resolve_latest_multiple_beans() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec![]),
            ("3", Status::Open, vec![]),
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_latest(&context);
        assert!(result.is_ok());
        // Should return one of the beans (the one with max updated_at)
        // All have the same updated_at, so any is valid; we'll just verify it's in the index
        let bean_id = result.unwrap();
        assert!(index.beans.iter().any(|e| e.id == bean_id));
    }

    #[test]
    fn resolve_latest_empty_index() {
        let index = create_test_index(vec![]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_latest(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn resolve_latest_with_different_timestamps() {
        use crate::index::IndexEntry;
        use std::time::Duration;

        let now = Utc::now();
        let earlier = now - Duration::from_secs(100);
        let later = now + Duration::from_secs(100);

        let beans = vec![
            IndexEntry {
                id: "1".to_string(),
                title: "Bean 1".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                updated_at: earlier,
            },
            IndexEntry {
                id: "2".to_string(),
                title: "Bean 2".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                updated_at: later,
            },
            IndexEntry {
                id: "3".to_string(),
                title: "Bean 3".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                updated_at: now,
            },
        ];

        let index = Index { beans };
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_latest(&context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "2"); // Bean 2 has the latest timestamp
    }

    // ---------------------------------------------------------------------------
    // resolve_blocked tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_blocked_no_blocked_beans() {
        let index = create_test_index(vec![
            ("1", Status::Closed, vec![]),
            ("2", Status::Open, vec![]),
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked: Vec<String> = result.unwrap();
        assert!(blocked.is_empty());
    }

    #[test]
    fn resolve_blocked_bean_with_open_dependency() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec!["1"]), // Blocked by 1 (which is open)
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked: Vec<String> = result.unwrap();
        assert_eq!(blocked, vec!["2".to_string()]);
    }

    #[test]
    fn resolve_blocked_bean_with_closed_dependency() {
        let index = create_test_index(vec![
            ("1", Status::Closed, vec![]),
            ("2", Status::Open, vec!["1"]), // Not blocked: 1 is closed
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked: Vec<String> = result.unwrap();
        assert!(blocked.is_empty());
    }

    #[test]
    fn resolve_blocked_missing_dependency() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec!["999"]), // Blocked: 999 doesn't exist
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked: Vec<String> = result.unwrap();
        assert_eq!(blocked, vec!["2".to_string()]);
    }

    #[test]
    fn resolve_blocked_multiple_open_dependencies() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec![]),
            ("3", Status::Open, vec!["1", "2"]), // Blocked by 1 and 2 (both open)
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked = result.unwrap();
        assert_eq!(blocked, vec!["3"]);
    }

    #[test]
    fn resolve_blocked_complex_dependency_tree() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec!["1"]),       // Blocked by 1
            ("3", Status::Closed, vec![]),
            ("4", Status::Open, vec!["3"]),       // Not blocked: 3 is closed
            ("5", Status::Open, vec!["4", "999"]), // Blocked: 999 missing
            ("6", Status::Closed, vec!["1"]),     // Not blocked: status is closed
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked = result.unwrap();
        assert_eq!(blocked.len(), 2);
        assert!(blocked.contains(&"2".to_string()));
        assert!(blocked.contains(&"5".to_string()));
    }

    #[test]
    fn resolve_blocked_empty_index() {
        let index = create_test_index(vec![]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked: Vec<String> = result.unwrap();
        assert!(blocked.is_empty());
    }

    #[test]
    fn resolve_blocked_bean_with_no_dependencies() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec![]),
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked: Vec<String> = result.unwrap();
        assert!(blocked.is_empty());
    }

    #[test]
    fn resolve_blocked_self_dependency() {
        let index = create_test_index(vec![("1", Status::Open, vec!["1"])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked = result.unwrap();
        assert_eq!(blocked, vec!["1"]); // Bean 1 is blocked by itself
    }

    #[test]
    fn resolve_blocked_in_progress_dependency() {
        let index = create_test_index(vec![
            ("1", Status::InProgress, vec![]),
            ("2", Status::Open, vec!["1"]), // Blocked: 1 is in_progress (open)
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_blocked(&context);
        assert!(result.is_ok());
        let blocked = result.unwrap();
        assert_eq!(blocked, vec!["2"]);
    }
}
