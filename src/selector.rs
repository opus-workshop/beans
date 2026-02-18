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
    context
        .index
        .beans
        .iter()
        .max_by_key(|entry| entry.updated_at)
        .map(|entry| entry.id.clone())
        .ok_or_else(|| anyhow!("Cannot resolve @latest: index is empty"))
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
// resolve_parent
// ---------------------------------------------------------------------------

/// Resolve @parent to the parent bean ID of the current bean.
///
/// # Arguments
/// * `context` - Selection context containing the current bean ID and index
///
/// # Returns
/// * `Ok(parent_id)` - the parent bean's ID
/// * `Err` if current bean has no parent, is not found, or parent is not in index
///
/// # Examples
/// ```ignore
/// let context = SelectionContext {
///     index: &my_index,
///     current_bean_id: Some("3.1".to_string()),
///     current_user: None,
/// };
/// let parent_id = resolve_parent(&context)?; // Returns "3"
/// ```
pub fn resolve_parent(context: &SelectionContext) -> Result<String> {
    let current_id = context.current_bean_id.as_ref()
        .ok_or_else(|| anyhow!("No current bean ID in context for @parent resolution"))?;

    let current_bean = context.index.beans.iter()
        .find(|entry| entry.id == *current_id)
        .ok_or_else(|| anyhow!("Bean {} not found in index", current_id))?;

    let parent_id = current_bean.parent.as_ref()
        .ok_or_else(|| anyhow!("Bean {} has no parent", current_id))?;

    // Verify parent exists in index
    context.index.beans.iter()
        .find(|entry| entry.id == *parent_id)
        .ok_or_else(|| anyhow!("Parent {} not found in index", parent_id))?;

    Ok(parent_id.clone())
}

// ---------------------------------------------------------------------------
// resolve_me
// ---------------------------------------------------------------------------

/// Resolve @me to the list of open beans assigned to the current user.
///
/// The current user is determined by:
/// 1. The `current_user` field in the context (if provided), OR
/// 2. The `BN_USER` environment variable
///
/// Only open (non-closed) beans are returned.
///
/// # Arguments
/// * `context` - Selection context containing the current user and index
///
/// # Returns
/// * `Ok(vec_of_ids)` - list of open beans assigned to the user (sorted naturally)
/// * `Err` if current user is not set and BN_USER environment variable is not set
///
/// # Examples
/// ```ignore
/// let context = SelectionContext {
///     index: &my_index,
///     current_bean_id: None,
///     current_user: Some("alice".to_string()),
/// };
/// let my_beans = resolve_me(&context)?; // Returns beans assigned to alice
/// ```
pub fn resolve_me(context: &SelectionContext) -> Result<Vec<String>> {
    let current_user = if let Some(user) = &context.current_user {
        user.clone()
    } else {
        std::env::var("BN_USER").map_err(|_| {
            anyhow!("BN_USER environment variable not set. Please set: export BN_USER=myname")
        })?
    };

    let mut my_beans: Vec<String> = context.index.beans.iter()
        .filter(|entry| {
            (entry.assignee.as_ref() == Some(&current_user)
                || entry.claimed_by.as_ref() == Some(&current_user))
                && entry.status != Status::Closed
        })
        .map(|entry| entry.id.clone())
        .collect();

    // Sort by natural order (1, 2, 3.1, 10, etc.)
    my_beans.sort_by(|a, b| crate::util::natural_cmp(a, b));

    Ok(my_beans)
}

// ---------------------------------------------------------------------------
// resolve_selector_full
// ---------------------------------------------------------------------------

/// Unified resolver that converts any selector to a list of bean IDs.
///
/// This function provides a consistent interface for all selector types:
/// - @latest returns a single-element vec
/// - @blocked returns multiple beans or empty vec
/// - @parent returns a single-element vec
/// - @me returns multiple beans or empty vec
///
/// # Arguments
/// * `selector_type` - The type of selector to resolve
/// * `context` - Selection context for resolution
///
/// # Returns
/// * `Ok(vec_of_ids)` - list of resolved bean IDs
/// * `Err` if resolution fails (e.g., missing context, empty index)
///
/// # Examples
/// ```ignore
/// let context = SelectionContext {
///     index: &my_index,
///     current_bean_id: Some("3.1".to_string()),
///     current_user: Some("alice".to_string()),
/// };
/// let ids = resolve_selector_full(SelectorType::Parent, &context)?;
/// assert_eq!(ids.len(), 1); // Parent selector always returns one bean
/// ```
pub fn resolve_selector_full(selector_type: SelectorType, context: &SelectionContext) -> Result<Vec<String>> {
    match selector_type {
        SelectorType::Latest => {
            let id = resolve_latest(context)?;
            Ok(vec![id])
        }
        SelectorType::Blocked => {
            resolve_blocked(context)
        }
        SelectorType::Parent => {
            let id = resolve_parent(context)?;
            Ok(vec![id])
        }
        SelectorType::Me => {
            resolve_me(context)
        }
    }
}

// ---------------------------------------------------------------------------
// resolve_selector_string
// ---------------------------------------------------------------------------

/// Resolve a selector string (e.g., "@latest", "@blocked") to bean IDs.
///
/// This is the main entry point for selector resolution from CLI arguments.
/// It combines parsing and resolution in one step.
///
/// # Arguments
/// * `selector_str` - A selector string starting with @ or a literal bean ID
/// * `context` - Selection context for resolution
///
/// # Returns
/// * `Ok(vec_of_ids)` - list of resolved bean IDs
/// * `Err` if selector parsing fails or resolution fails
///
/// # Examples
/// ```ignore
/// let context = SelectionContext {
///     index: &my_index,
///     current_bean_id: None,
///     current_user: None,
/// };
/// let ids = resolve_selector_string("@latest", &context)?;
/// ```
pub fn resolve_selector_string(selector_str: &str, context: &SelectionContext) -> Result<Vec<String>> {
    // Check if it's a selector (starts with @)
    if !selector_str.starts_with('@') {
        // Not a selector, return as-is (literal bean ID)
        return Ok(vec![selector_str.to_string()]);
    }

    // Parse and resolve the selector
    let selector_type = parse_selector(selector_str)?;
    resolve_selector_full(selector_type, context)
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
                assignee: None,
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
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
                assignee: None,
                updated_at: earlier,
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
            IndexEntry {
                id: "2".to_string(),
                title: "Bean 2".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: None,
                updated_at: later,
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
            IndexEntry {
                id: "3".to_string(),
                title: "Bean 3".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: None,
                updated_at: now,
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
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

    // ---------------------------------------------------------------------------
    // resolve_parent tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_parent_simple() {
        use crate::index::IndexEntry;

        let entry_parent = IndexEntry {
            id: "3".to_string(),
            title: "Parent".to_string(),
            status: Status::Open,
            priority: 2,
            parent: None,
            dependencies: vec![],
            labels: vec![],
            assignee: None,
            updated_at: Utc::now(),
            produces: Vec::new(),
            requires: Vec::new(),
                has_verify: true, claimed_by: None,
        };

        let entry_child = IndexEntry {
            id: "3.1".to_string(),
            title: "Child".to_string(),
            status: Status::Open,
            priority: 2,
            parent: Some("3".to_string()),
            dependencies: vec![],
            labels: vec![],
            assignee: None,
            updated_at: Utc::now(),
            produces: Vec::new(),
            requires: Vec::new(),
                has_verify: true, claimed_by: None,
        };

        let index = Index { beans: vec![entry_parent, entry_child] };
        let context = SelectionContext {
            index: &index,
            current_bean_id: Some("3.1".to_string()),
            current_user: None,
        };

        let result = resolve_parent(&context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "3");
    }

    #[test]
    fn resolve_parent_no_current_bean() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_parent(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No current bean ID"));
    }

    #[test]
    fn resolve_parent_current_bean_not_found() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: Some("999".to_string()),
            current_user: None,
        };

        let result = resolve_parent(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found in index"));
    }

    #[test]
    fn resolve_parent_no_parent() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: Some("1".to_string()),
            current_user: None,
        };

        let result = resolve_parent(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("has no parent"));
    }

    #[test]
    fn resolve_parent_parent_not_in_index() {
        use crate::index::IndexEntry;

        let entry = IndexEntry {
            id: "3.1".to_string(),
            title: "Child".to_string(),
            status: Status::Open,
            priority: 2,
            parent: Some("999".to_string()),
            dependencies: vec![],
            labels: vec![],
            assignee: None,
            updated_at: Utc::now(),
            produces: Vec::new(),
            requires: Vec::new(),
                has_verify: true, claimed_by: None,
        };

        let index = Index { beans: vec![entry] };
        let context = SelectionContext {
            index: &index,
            current_bean_id: Some("3.1".to_string()),
            current_user: None,
        };

        let result = resolve_parent(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Parent 999 not found"));
    }

    // ---------------------------------------------------------------------------
    // resolve_me tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_me_with_current_user() {
        use crate::index::IndexEntry;

        let beans = vec![
            IndexEntry {
                id: "1".to_string(),
                title: "Bean 1".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("alice".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
            IndexEntry {
                id: "2".to_string(),
                title: "Bean 2".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("bob".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
            IndexEntry {
                id: "3".to_string(),
                title: "Bean 3".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("alice".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
        ];

        let index = Index { beans };
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: Some("alice".to_string()),
        };

        let result = resolve_me(&context);
        assert!(result.is_ok());
        let my_beans = result.unwrap();
        assert_eq!(my_beans.len(), 2);
        assert_eq!(my_beans, vec!["1", "3"]);
    }

    #[test]
    fn resolve_me_excludes_closed() {
        use crate::index::IndexEntry;

        let beans = vec![
            IndexEntry {
                id: "1".to_string(),
                title: "Bean 1".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("alice".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
            IndexEntry {
                id: "2".to_string(),
                title: "Bean 2".to_string(),
                status: Status::Closed,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("alice".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
        ];

        let index = Index { beans };
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: Some("alice".to_string()),
        };

        let result = resolve_me(&context);
        assert!(result.is_ok());
        let my_beans = result.unwrap();
        assert_eq!(my_beans.len(), 1);
        assert_eq!(my_beans, vec!["1"]);
    }

    #[test]
    fn resolve_me_includes_claimed_by() {
        use crate::index::IndexEntry;

        let beans = vec![
            IndexEntry {
                id: "1".to_string(),
                title: "Bean 1".to_string(),
                status: Status::InProgress,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: None,
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true,
                claimed_by: Some("alice".to_string()),
            },
            IndexEntry {
                id: "2".to_string(),
                title: "Bean 2".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("alice".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true,
                claimed_by: None,
            },
            IndexEntry {
                id: "3".to_string(),
                title: "Bean 3".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: None,
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true,
                claimed_by: Some("bob".to_string()),
            },
        ];

        let index = Index { beans };
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: Some("alice".to_string()),
        };

        let result = resolve_me(&context);
        assert!(result.is_ok());
        let my_beans = result.unwrap();
        assert_eq!(my_beans.len(), 2);
        assert!(my_beans.contains(&"1".to_string())); // claimed_by alice
        assert!(my_beans.contains(&"2".to_string())); // assignee alice
    }

    #[test]
    fn resolve_me_no_assignee() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: Some("alice".to_string()),
        };

        let result = resolve_me(&context);
        assert!(result.is_ok());
        let my_beans = result.unwrap();
        assert!(my_beans.is_empty());
    }

    #[test]
    fn resolve_me_no_user_env_var_set() {
        // This test requires BN_USER to not be set
        // We temporarily unset it, run the test, then restore
        let original = std::env::var("BN_USER").ok();
        std::env::remove_var("BN_USER");

        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_me(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("BN_USER"));

        // Restore original
        if let Some(user) = original {
            std::env::set_var("BN_USER", user);
        }
    }

    // ---------------------------------------------------------------------------
    // resolve_selector_full tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_selector_full_latest() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec![]),
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_selector_full(SelectorType::Latest, &context);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn resolve_selector_full_blocked() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec!["1"]),
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_selector_full(SelectorType::Blocked, &context);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids, vec!["2"]);
    }

    #[test]
    fn resolve_selector_full_parent() {
        use crate::index::IndexEntry;

        let beans = vec![
            IndexEntry {
                id: "3".to_string(),
                title: "Parent".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: None,
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
            IndexEntry {
                id: "3.1".to_string(),
                title: "Child".to_string(),
                status: Status::Open,
                priority: 2,
                parent: Some("3".to_string()),
                dependencies: vec![],
                labels: vec![],
                assignee: None,
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
        ];

        let index = Index { beans };
        let context = SelectionContext {
            index: &index,
            current_bean_id: Some("3.1".to_string()),
            current_user: None,
        };

        let result = resolve_selector_full(SelectorType::Parent, &context);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids, vec!["3"]);
    }

    #[test]
    fn resolve_selector_full_me() {
        use crate::index::IndexEntry;

        let beans = vec![
            IndexEntry {
                id: "1".to_string(),
                title: "Bean 1".to_string(),
                status: Status::Open,
                priority: 2,
                parent: None,
                dependencies: vec![],
                labels: vec![],
                assignee: Some("alice".to_string()),
                updated_at: Utc::now(),
                produces: Vec::new(),
                requires: Vec::new(),
                has_verify: true, claimed_by: None,
            },
        ];

        let index = Index { beans };
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: Some("alice".to_string()),
        };

        let result = resolve_selector_full(SelectorType::Me, &context);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids, vec!["1"]);
    }

    // ---------------------------------------------------------------------------
    // resolve_selector_string tests
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_selector_string_literal_id() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_selector_string("1", &context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["1"]);
    }

    #[test]
    fn resolve_selector_string_selector() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_selector_string("@latest", &context);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids.len(), 1);
        assert!(index.beans.iter().any(|e| e.id == ids[0]));
    }

    #[test]
    fn resolve_selector_string_blocked_selector() {
        let index = create_test_index(vec![
            ("1", Status::Open, vec![]),
            ("2", Status::Open, vec!["1"]),
        ]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_selector_string("@blocked", &context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["2"]);
    }

    #[test]
    fn resolve_selector_string_invalid_selector() {
        let index = create_test_index(vec![("1", Status::Open, vec![])]);
        let context = SelectionContext {
            index: &index,
            current_bean_id: None,
            current_user: None,
        };

        let result = resolve_selector_string("@invalid", &context);
        assert!(result.is_err());
    }
}
