use std::fmt;

use crate::bean::Status;
use crate::index::{Index, IndexEntry};

// ---------------------------------------------------------------------------
// Scope thresholds
// ---------------------------------------------------------------------------

/// Maximum number of `produces` artifacts before a bean is considered oversized.
pub const MAX_PRODUCES: usize = 3;

/// Maximum number of `paths` before a bean is considered oversized.
pub const MAX_PATHS: usize = 5;

// ---------------------------------------------------------------------------
// BlockReason
// ---------------------------------------------------------------------------

/// Why a bean cannot be dispatched right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    /// One or more dependency beans are not yet closed.
    WaitingOn(Vec<String>),
    /// Scope is too large: `produces > MAX_PRODUCES` or `paths > MAX_PATHS`.
    Oversized,
    /// No scope defined: both `produces` and `paths` are empty.
    Unscoped,
}

impl fmt::Display for BlockReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockReason::WaitingOn(ids) => {
                write!(f, "waiting on {}", ids.join(", "))
            }
            BlockReason::Oversized => write!(f, "oversized"),
            BlockReason::Unscoped => write!(f, "unscoped"),
        }
    }
}

// ---------------------------------------------------------------------------
// Unified blocking check
// ---------------------------------------------------------------------------

/// Check whether `entry` is blocked, returning the reason if so.
///
/// Checks in priority order:
/// 1. **Explicit dependencies** — any dep that isn't closed (or doesn't exist).
/// 2. **Requires/produces** — sibling beans that produce a required artifact
///    but aren't closed yet.
/// 3. **Oversized** — `produces > 3` or `paths > 5`.
/// 4. **Unscoped** — `produces == 0` and `paths == 0`.
pub fn check_blocked(entry: &IndexEntry, index: &Index) -> Option<BlockReason> {
    let mut waiting_on = Vec::new();

    // Explicit dependencies
    for dep_id in &entry.dependencies {
        match index.beans.iter().find(|e| e.id == *dep_id) {
            Some(dep) if dep.status == Status::Closed => {}
            _ => waiting_on.push(dep_id.clone()),
        }
    }

    // Smart dependencies: requires → sibling produces
    for required in &entry.requires {
        if let Some(producer) = index.beans.iter().find(|e| {
            e.id != entry.id && e.parent == entry.parent && e.produces.contains(required)
        }) {
            if producer.status != Status::Closed && !waiting_on.contains(&producer.id) {
                waiting_on.push(producer.id.clone());
            }
        }
    }

    if !waiting_on.is_empty() {
        return Some(BlockReason::WaitingOn(waiting_on));
    }

    // Scope checks
    if entry.produces.len() > MAX_PRODUCES || entry.paths.len() > MAX_PATHS {
        return Some(BlockReason::Oversized);
    }

    if entry.produces.is_empty() && entry.paths.is_empty() {
        return Some(BlockReason::Unscoped);
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_entry(id: &str) -> IndexEntry {
        IndexEntry {
            id: id.to_string(),
            title: format!("Bean {}", id),
            status: Status::Open,
            priority: 2,
            parent: None,
            dependencies: vec![],
            labels: vec![],
            assignee: None,
            updated_at: Utc::now(),
            produces: vec![],
            requires: vec![],
            has_verify: true,
            claimed_by: None,
            attempts: 0,
            paths: vec![],
        }
    }

    fn make_index(entries: Vec<IndexEntry>) -> Index {
        Index { beans: entries }
    }

    // -- WaitingOn: explicit deps --

    #[test]
    fn blocking_not_blocked_when_deps_closed() {
        let mut dep = make_entry("1");
        dep.status = Status::Closed;

        let mut entry = make_entry("2");
        entry.dependencies = vec!["1".into()];
        entry.produces = vec!["Foo".into()];
        entry.paths = vec!["src/foo.rs".into()];

        let index = make_index(vec![dep, entry.clone()]);
        assert_eq!(check_blocked(&entry, &index), None);
    }

    #[test]
    fn blocking_waiting_on_open_dep() {
        let dep = make_entry("1"); // open

        let mut entry = make_entry("2");
        entry.dependencies = vec!["1".into()];
        entry.produces = vec!["Foo".into()];
        entry.paths = vec!["src/foo.rs".into()];

        let index = make_index(vec![dep, entry.clone()]);
        assert_eq!(
            check_blocked(&entry, &index),
            Some(BlockReason::WaitingOn(vec!["1".into()]))
        );
    }

    #[test]
    fn blocking_waiting_on_missing_dep() {
        let mut entry = make_entry("2");
        entry.dependencies = vec!["999".into()]; // doesn't exist
        entry.produces = vec!["Foo".into()];
        entry.paths = vec!["src/foo.rs".into()];

        let index = make_index(vec![entry.clone()]);
        assert_eq!(
            check_blocked(&entry, &index),
            Some(BlockReason::WaitingOn(vec!["999".into()]))
        );
    }

    #[test]
    fn blocking_waiting_on_multiple_deps() {
        let dep_a = make_entry("1"); // open
        let dep_b = make_entry("3"); // open

        let mut entry = make_entry("2");
        entry.dependencies = vec!["1".into(), "3".into()];
        entry.produces = vec!["Foo".into()];
        entry.paths = vec!["src/foo.rs".into()];

        let index = make_index(vec![dep_a, entry.clone(), dep_b]);
        assert_eq!(
            check_blocked(&entry, &index),
            Some(BlockReason::WaitingOn(vec!["1".into(), "3".into()]))
        );
    }

    // -- WaitingOn: requires/produces --

    #[test]
    fn blocking_waiting_on_sibling_producer() {
        let mut producer = make_entry("5.1");
        producer.parent = Some("5".into());
        producer.produces = vec!["UserType".into()];

        let mut consumer = make_entry("5.2");
        consumer.parent = Some("5".into());
        consumer.requires = vec!["UserType".into()];
        consumer.produces = vec!["UserAPI".into()];
        consumer.paths = vec!["src/api.rs".into()];

        let index = make_index(vec![producer, consumer.clone()]);
        assert_eq!(
            check_blocked(&consumer, &index),
            Some(BlockReason::WaitingOn(vec!["5.1".into()]))
        );
    }

    #[test]
    fn blocking_not_blocked_when_producer_closed() {
        let mut producer = make_entry("5.1");
        producer.parent = Some("5".into());
        producer.produces = vec!["UserType".into()];
        producer.status = Status::Closed;

        let mut consumer = make_entry("5.2");
        consumer.parent = Some("5".into());
        consumer.requires = vec!["UserType".into()];
        consumer.produces = vec!["UserAPI".into()];
        consumer.paths = vec!["src/api.rs".into()];

        let index = make_index(vec![producer, consumer.clone()]);
        assert_eq!(check_blocked(&consumer, &index), None);
    }

    #[test]
    fn blocking_no_duplicate_when_dep_and_requires_overlap() {
        let mut producer = make_entry("5.1");
        producer.parent = Some("5".into());
        producer.produces = vec!["UserType".into()];

        let mut consumer = make_entry("5.2");
        consumer.parent = Some("5".into());
        consumer.dependencies = vec!["5.1".into()]; // explicit dep
        consumer.requires = vec!["UserType".into()]; // also requires from same bean
        consumer.produces = vec!["UserAPI".into()];
        consumer.paths = vec!["src/api.rs".into()];

        let index = make_index(vec![producer, consumer.clone()]);
        if let Some(BlockReason::WaitingOn(ids)) = check_blocked(&consumer, &index) {
            // 5.1 should appear only once even though it's both an explicit dep and a producer
            assert_eq!(ids, vec!["5.1".to_string()]);
        } else {
            panic!("Expected WaitingOn");
        }
    }

    // -- Oversized --

    #[test]
    fn blocking_oversized_too_many_produces() {
        let mut entry = make_entry("1");
        entry.produces = vec!["A".into(), "B".into(), "C".into(), "D".into()]; // 4 > MAX_PRODUCES
        entry.paths = vec!["src/a.rs".into()];

        let index = make_index(vec![entry.clone()]);
        assert_eq!(check_blocked(&entry, &index), Some(BlockReason::Oversized));
    }

    #[test]
    fn blocking_oversized_too_many_paths() {
        let mut entry = make_entry("1");
        entry.produces = vec!["A".into()];
        entry.paths = vec![
            "a.rs".into(),
            "b.rs".into(),
            "c.rs".into(),
            "d.rs".into(),
            "e.rs".into(),
            "f.rs".into(),
        ]; // 6 > MAX_PATHS

        let index = make_index(vec![entry.clone()]);
        assert_eq!(check_blocked(&entry, &index), Some(BlockReason::Oversized));
    }

    #[test]
    fn blocking_not_oversized_at_threshold() {
        let mut entry = make_entry("1");
        entry.produces = vec!["A".into(), "B".into(), "C".into()]; // exactly MAX_PRODUCES
        entry.paths = vec![
            "a.rs".into(),
            "b.rs".into(),
            "c.rs".into(),
            "d.rs".into(),
            "e.rs".into(),
        ]; // exactly MAX_PATHS

        let index = make_index(vec![entry.clone()]);
        assert_eq!(check_blocked(&entry, &index), None);
    }

    // -- Unscoped --

    #[test]
    fn blocking_unscoped_empty_produces_and_paths() {
        let entry = make_entry("1"); // produces=[], paths=[]

        let index = make_index(vec![entry.clone()]);
        assert_eq!(check_blocked(&entry, &index), Some(BlockReason::Unscoped));
    }

    #[test]
    fn blocking_not_unscoped_with_produces() {
        let mut entry = make_entry("1");
        entry.produces = vec!["SomeType".into()];

        let index = make_index(vec![entry.clone()]);
        // produces is NOT empty → not unscoped; 1 <= MAX_PRODUCES → not oversized
        assert_eq!(check_blocked(&entry, &index), None);
    }

    #[test]
    fn blocking_not_unscoped_with_paths() {
        let mut entry = make_entry("1");
        entry.paths = vec!["src/main.rs".into()];

        let index = make_index(vec![entry.clone()]);
        // paths is NOT empty → not unscoped; 1 <= MAX_PATHS → not oversized
        assert_eq!(check_blocked(&entry, &index), None);
    }

    // -- Display --

    #[test]
    fn blocking_display_waiting_on() {
        let reason = BlockReason::WaitingOn(vec!["3.1".into(), "3.2".into()]);
        assert_eq!(format!("{}", reason), "waiting on 3.1, 3.2");
    }

    #[test]
    fn blocking_display_oversized() {
        assert_eq!(format!("{}", BlockReason::Oversized), "oversized");
    }

    #[test]
    fn blocking_display_unscoped() {
        assert_eq!(format!("{}", BlockReason::Unscoped), "unscoped");
    }

    // -- Priority: deps checked before scope --

    #[test]
    fn blocking_deps_take_priority_over_oversized() {
        let dep = make_entry("1"); // open

        let mut entry = make_entry("2");
        entry.dependencies = vec!["1".into()];
        entry.produces = vec!["A".into(), "B".into(), "C".into(), "D".into()]; // oversized
        entry.paths = vec!["a.rs".into()];

        let index = make_index(vec![dep, entry.clone()]);
        // Should return WaitingOn, not Oversized
        assert!(matches!(
            check_blocked(&entry, &index),
            Some(BlockReason::WaitingOn(_))
        ));
    }

    #[test]
    fn blocking_deps_take_priority_over_unscoped() {
        let dep = make_entry("1"); // open

        let mut entry = make_entry("2");
        entry.dependencies = vec!["1".into()];
        // produces=[], paths=[] → would be unscoped

        let index = make_index(vec![dep, entry.clone()]);
        // Should return WaitingOn, not Unscoped
        assert!(matches!(
            check_blocked(&entry, &index),
            Some(BlockReason::WaitingOn(_))
        ));
    }
}
