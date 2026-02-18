//! Three-way merge logic for bean fields.
//!
//! Implements field-level merging for resolving non-overlapping changes:
//! - Scalar fields: title, priority, status, assignee, description, etc.
//! - Append fields: notes (always append both with timestamps)
//! - Collection fields: labels, dependencies (set union for adds)

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::collections::HashSet;

use crate::bean::{Bean, ConflictResolution, ConflictVersion, FieldConflict, Status};

// ---------------------------------------------------------------------------
// MergeResult
// ---------------------------------------------------------------------------

/// Result of a merge operation
#[derive(Debug, Clone, Default)]
pub struct MergeResult {
    /// Fields that had conflicts (could not auto-merge)
    pub conflicts: Vec<String>,
    /// Fields that were auto-merged successfully
    pub merged: Vec<String>,
}

impl MergeResult {
    /// Returns true if there were no conflicts
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Three-way merge implementation
// ---------------------------------------------------------------------------

impl Bean {
    /// Three-way merge: self is left (ours), base is original, right is theirs.
    /// Modifies self in place with merged values.
    /// Returns MergeResult with list of conflicting and merged field names.
    pub fn merge(&mut self, base: &Bean, right: &Bean) -> Result<MergeResult> {
        let mut result = MergeResult::default();

        // Scalar fields
        self.merge_scalar_string("title", &base.title, &right.title, &mut result)?;
        self.merge_scalar_option("slug", &base.slug, &right.slug, &mut result)?;
        self.merge_scalar("status", &base.status, &right.status, &mut result)?;
        self.merge_scalar("priority", &base.priority, &right.priority, &mut result)?;
        self.merge_scalar_option("description", &base.description, &right.description, &mut result)?;
        self.merge_scalar_option("acceptance", &base.acceptance, &right.acceptance, &mut result)?;
        self.merge_scalar_option("design", &base.design, &right.design, &mut result)?;
        self.merge_scalar_option("assignee", &base.assignee, &right.assignee, &mut result)?;
        self.merge_scalar_option("parent", &base.parent, &right.parent, &mut result)?;
        self.merge_scalar_option("verify", &base.verify, &right.verify, &mut result)?;
        self.merge_scalar("fail_first", &base.fail_first, &right.fail_first, &mut result)?;
        self.merge_scalar("max_attempts", &base.max_attempts, &right.max_attempts, &mut result)?;
        self.merge_scalar_option("close_reason", &base.close_reason, &right.close_reason, &mut result)?;

        // Append fields (notes always appends, no conflicts)
        self.merge_append_notes(&base.notes, &right.notes, &mut result);

        // Collection fields (set union for adds)
        self.merge_collection("labels", &base.labels, &right.labels, &mut result)?;
        self.merge_collection("dependencies", &base.dependencies, &right.dependencies, &mut result)?;
        self.merge_collection("produces", &base.produces, &right.produces, &mut result)?;
        self.merge_collection("requires", &base.requires, &right.requires, &mut result)?;

        // Update timestamp
        self.updated_at = Utc::now();

        Ok(result)
    }

    /// Merge a scalar field using 3-way merge rules.
    /// - If base == left: take right (only right changed)
    /// - If base == right: take left (only left changed)
    /// - If left == right: take left (both same)
    /// - If base != left && base != right && left != right: CONFLICT
    fn merge_scalar<T>(
        &mut self,
        field: &str,
        base: &T,
        right: &T,
        result: &mut MergeResult,
    ) -> Result<()>
    where
        T: PartialEq + Clone + Serialize + 'static,
    {
        let left = self.get_field::<T>(field)?;

        if base == &left {
            // Only right changed, take right
            if base != right {
                self.set_field(field, right.clone())?;
                result.merged.push(field.to_string());
            }
            // else: no changes at all
        } else if base == right {
            // Only left changed, keep left (already in self)
            if base != &left {
                result.merged.push(field.to_string());
            }
        } else if &left == right {
            // Both changed to same value, keep it
            result.merged.push(field.to_string());
        } else {
            // Both changed to different values - CONFLICT
            self.record_conflict(field, &left, right)?;
            result.conflicts.push(field.to_string());
        }

        Ok(())
    }

    /// Merge an optional scalar field
    fn merge_scalar_option<T>(
        &mut self,
        field: &str,
        base: &Option<T>,
        right: &Option<T>,
        result: &mut MergeResult,
    ) -> Result<()>
    where
        T: PartialEq + Clone + Serialize + 'static,
    {
        let left = self.get_field_option::<T>(field)?;

        if base == &left {
            // Only right changed, take right
            if base != right {
                self.set_field_option(field, right.clone())?;
                result.merged.push(field.to_string());
            }
        } else if base == right {
            // Only left changed, keep left
            if base != &left {
                result.merged.push(field.to_string());
            }
        } else if &left == right {
            // Both changed to same value
            result.merged.push(field.to_string());
        } else {
            // Both changed to different values - CONFLICT
            self.record_conflict_option(field, &left, right)?;
            result.conflicts.push(field.to_string());
        }

        Ok(())
    }

    /// Merge a required String field (title)
    fn merge_scalar_string(
        &mut self,
        field: &str,
        base: &str,
        right: &str,
        result: &mut MergeResult,
    ) -> Result<()> {
        let left = self.title.clone(); // title is the only required string

        if base == left {
            // Only right changed
            if base != right {
                self.title = right.to_string();
                result.merged.push(field.to_string());
            }
        } else if base == right {
            // Only left changed, keep left
            if base != left {
                result.merged.push(field.to_string());
            }
        } else if left == right {
            // Both same
            result.merged.push(field.to_string());
        } else {
            // CONFLICT
            self.record_conflict(field, &left, &right.to_string())?;
            result.conflicts.push(field.to_string());
        }

        Ok(())
    }

    /// Merge notes field - always appends both sides (no conflicts).
    /// Entries are timestamped to preserve history.
    fn merge_append_notes(
        &mut self,
        base_notes: &Option<String>,
        right_notes: &Option<String>,
        result: &mut MergeResult,
    ) {
        let left_notes = self.notes.clone();

        // Calculate what was added on each side
        let base = base_notes.as_deref().unwrap_or("");
        let left = left_notes.as_deref().unwrap_or("");
        let right = right_notes.as_deref().unwrap_or("");

        // If neither side changed, nothing to do
        if left == base && right == base {
            return;
        }

        // If only one side changed, take that side
        if left == base && right != base {
            self.notes = right_notes.clone();
            result.merged.push("notes".to_string());
            return;
        }
        if right == base && left != base {
            // Keep left (already in self)
            result.merged.push("notes".to_string());
            return;
        }

        // Both sides changed - append both additions
        let left_addition = left.strip_prefix(base).unwrap_or(left);
        let right_addition = right.strip_prefix(base).unwrap_or(right);

        // Build merged notes
        let mut merged = base.to_string();
        if !left_addition.is_empty() {
            if !merged.is_empty() && !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(left_addition.trim_start_matches('\n'));
        }
        if !right_addition.is_empty() {
            if !merged.is_empty() && !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(right_addition.trim_start_matches('\n'));
        }

        self.notes = if merged.is_empty() { None } else { Some(merged) };
        result.merged.push("notes".to_string());
    }

    /// Merge a collection field (Vec<String>) using set semantics.
    /// - Adds from both sides are included (union)
    /// - Removes from both sides are applied
    /// - Conflict if one side removes what the other side kept/added
    fn merge_collection(
        &mut self,
        field: &str,
        base: &[String],
        right: &[String],
        result: &mut MergeResult,
    ) -> Result<()> {
        let left = self.get_field_vec(field)?;

        let base_set: HashSet<&String> = base.iter().collect();
        let left_set: HashSet<&String> = left.iter().collect();
        let right_set: HashSet<&String> = right.iter().collect();

        // Items added on each side
        let left_added: HashSet<&String> = left_set.difference(&base_set).copied().collect();
        let right_added: HashSet<&String> = right_set.difference(&base_set).copied().collect();

        // Items removed on each side
        let left_removed: HashSet<&String> = base_set.difference(&left_set).copied().collect();
        let right_removed: HashSet<&String> = base_set.difference(&right_set).copied().collect();

        // Check for conflict: one side removed what the other side has
        // (either kept from base or added)
        let left_kept_or_added: HashSet<&String> = left_set.iter().copied().collect();
        let right_kept_or_added: HashSet<&String> = right_set.iter().copied().collect();

        let conflict_items: Vec<&String> = left_removed
            .intersection(&right_kept_or_added)
            .chain(right_removed.intersection(&left_kept_or_added))
            .copied()
            .collect();

        if !conflict_items.is_empty() {
            // Record conflict with the divergent values
            let right_vec: Vec<String> = right.to_vec();
            self.record_conflict(field, &left, &right_vec)?;
            result.conflicts.push(field.to_string());
            return Ok(());
        }

        // No conflict - compute merged set
        // Start with base, apply both sides' changes
        let mut merged_set: HashSet<String> = base_set.iter().map(|s| (*s).clone()).collect();

        // Apply removes from both sides
        for item in left_removed.iter().chain(right_removed.iter()) {
            merged_set.remove(*item);
        }

        // Apply adds from both sides
        for item in left_added.iter().chain(right_added.iter()) {
            merged_set.insert((*item).clone());
        }

        // Convert back to sorted vec for determinism
        let mut merged: Vec<String> = merged_set.into_iter().collect();
        merged.sort();

        // Check if anything changed
        let left_sorted = {
            let mut v = left.clone();
            v.sort();
            v
        };
        if merged != left_sorted {
            self.set_field_vec(field, merged)?;
            result.merged.push(field.to_string());
        } else if left_added.is_empty() && right_added.is_empty() 
               && left_removed.is_empty() && right_removed.is_empty() {
            // No changes at all
        } else {
            result.merged.push(field.to_string());
        }

        Ok(())
    }

    /// Record a conflict for a field
    fn record_conflict<T: Serialize>(&mut self, field: &str, left: &T, right: &T) -> Result<()> {
        self.conflicts.push(FieldConflict {
            field: field.to_string(),
            versions: vec![
                ConflictVersion {
                    value: serde_json::to_string(left)?,
                    agent: "left".to_string(),
                    timestamp: Utc::now(),
                },
                ConflictVersion {
                    value: serde_json::to_string(right)?,
                    agent: "right".to_string(),
                    timestamp: Utc::now(),
                },
            ],
            resolution: ConflictResolution::Pending,
        });
        Ok(())
    }

    /// Record a conflict for an optional field
    fn record_conflict_option<T: Serialize>(
        &mut self,
        field: &str,
        left: &Option<T>,
        right: &Option<T>,
    ) -> Result<()> {
        self.conflicts.push(FieldConflict {
            field: field.to_string(),
            versions: vec![
                ConflictVersion {
                    value: serde_json::to_string(left)?,
                    agent: "left".to_string(),
                    timestamp: Utc::now(),
                },
                ConflictVersion {
                    value: serde_json::to_string(right)?,
                    agent: "right".to_string(),
                    timestamp: Utc::now(),
                },
            ],
            resolution: ConflictResolution::Pending,
        });
        Ok(())
    }

    // --- Field accessor helpers ---

    fn get_field<T: Clone + 'static>(&self, field: &str) -> Result<T> {
        use std::any::Any;
        let val: Box<dyn Any> = match field {
            "status" => Box::new(self.status),
            "priority" => Box::new(self.priority),
            "fail_first" => Box::new(self.fail_first),
            "max_attempts" => Box::new(self.max_attempts),
            _ => return Err(anyhow::anyhow!("Unknown scalar field: {}", field)),
        };
        val.downcast::<T>()
            .map(|v| *v)
            .map_err(|_| anyhow::anyhow!("Type mismatch for field '{}'", field))
    }

    fn set_field<T: Clone + 'static>(&mut self, field: &str, value: T) -> Result<()> {
        use std::any::Any;
        let val: Box<dyn Any> = Box::new(value);
        match field {
            "status" => {
                self.status = *val.downcast::<Status>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'status'"))?;
            }
            "priority" => {
                self.priority = *val.downcast::<u8>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'priority'"))?;
            }
            "fail_first" => {
                self.fail_first = *val.downcast::<bool>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'fail_first'"))?;
            }
            "max_attempts" => {
                self.max_attempts = *val.downcast::<u32>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'max_attempts'"))?;
            }
            _ => return Err(anyhow::anyhow!("Unknown scalar field: {}", field)),
        }
        Ok(())
    }

    fn get_field_option<T: Clone + 'static>(&self, field: &str) -> Result<Option<T>> {
        use std::any::Any;
        let val: Box<dyn Any> = match field {
            "slug" => Box::new(self.slug.clone()),
            "description" => Box::new(self.description.clone()),
            "acceptance" => Box::new(self.acceptance.clone()),
            "design" => Box::new(self.design.clone()),
            "assignee" => Box::new(self.assignee.clone()),
            "parent" => Box::new(self.parent.clone()),
            "verify" => Box::new(self.verify.clone()),
            "close_reason" => Box::new(self.close_reason.clone()),
            _ => return Err(anyhow::anyhow!("Unknown optional field: {}", field)),
        };
        val.downcast::<Option<T>>()
            .map(|v| *v)
            .map_err(|_| anyhow::anyhow!("Type mismatch for field '{}'", field))
    }

    fn set_field_option<T: Clone + 'static>(&mut self, field: &str, value: Option<T>) -> Result<()> {
        use std::any::Any;
        let val: Box<dyn Any> = Box::new(value);
        match field {
            "slug" => {
                self.slug = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'slug'"))?;
            }
            "description" => {
                self.description = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'description'"))?;
            }
            "acceptance" => {
                self.acceptance = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'acceptance'"))?;
            }
            "design" => {
                self.design = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'design'"))?;
            }
            "assignee" => {
                self.assignee = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'assignee'"))?;
            }
            "parent" => {
                self.parent = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'parent'"))?;
            }
            "verify" => {
                self.verify = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'verify'"))?;
            }
            "close_reason" => {
                self.close_reason = *val.downcast::<Option<String>>()
                    .map_err(|_| anyhow::anyhow!("Type mismatch for field 'close_reason'"))?;
            }
            _ => return Err(anyhow::anyhow!("Unknown optional field: {}", field)),
        }
        Ok(())
    }

    fn get_field_vec(&self, field: &str) -> Result<Vec<String>> {
        match field {
            "labels" => Ok(self.labels.clone()),
            "dependencies" => Ok(self.dependencies.clone()),
            "produces" => Ok(self.produces.clone()),
            "requires" => Ok(self.requires.clone()),
            _ => Err(anyhow::anyhow!("Unknown collection field: {}", field)),
        }
    }

    fn set_field_vec(&mut self, field: &str, value: Vec<String>) -> Result<()> {
        match field {
            "labels" => self.labels = value,
            "dependencies" => self.dependencies = value,
            "produces" => self.produces = value,
            "requires" => self.requires = value,
            _ => return Err(anyhow::anyhow!("Unknown collection field: {}", field)),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::bean::Status;

    fn make_bean(id: &str, title: &str) -> Bean {
        Bean::new(id, title)
    }

    #[test]
    fn test_merge_no_changes() {
        let base = make_bean("1", "Test");
        let mut left = base.clone();
        let right = base.clone();

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert!(result.merged.is_empty());
    }

    #[test]
    fn test_merge_only_left_changed_scalar() {
        let base = make_bean("1", "Original");
        let mut left = base.clone();
        left.title = "Modified by left".to_string();
        let right = base.clone();

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.title, "Modified by left");
        assert!(result.merged.contains(&"title".to_string()));
    }

    #[test]
    fn test_merge_only_right_changed_scalar() {
        let base = make_bean("1", "Original");
        let mut left = base.clone();
        let mut right = base.clone();
        right.title = "Modified by right".to_string();

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.title, "Modified by right");
        assert!(result.merged.contains(&"title".to_string()));
    }

    #[test]
    fn test_merge_both_same_change() {
        let base = make_bean("1", "Original");
        let mut left = base.clone();
        left.title = "Same change".to_string();
        let mut right = base.clone();
        right.title = "Same change".to_string();

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.title, "Same change");
    }

    #[test]
    fn test_merge_conflict_different_changes() {
        let base = make_bean("1", "Original");
        let mut left = base.clone();
        left.title = "Left change".to_string();
        let mut right = base.clone();
        right.title = "Right change".to_string();

        let result = left.merge(&base, &right).unwrap();
        assert!(!result.is_clean());
        assert!(result.conflicts.contains(&"title".to_string()));
        assert!(!left.conflicts.is_empty());
        assert_eq!(left.conflicts[0].field, "title");
        assert_eq!(left.conflicts[0].resolution, ConflictResolution::Pending);
    }

    #[test]
    fn test_merge_status_changes() {
        let base = make_bean("1", "Test");
        let mut left = base.clone();
        left.status = Status::InProgress;
        let right = base.clone();

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.status, Status::InProgress);
    }

    #[test]
    fn test_merge_priority_conflict() {
        let base = make_bean("1", "Test");
        let mut left = base.clone();
        left.priority = 1;
        let mut right = base.clone();
        right.priority = 3;

        let result = left.merge(&base, &right).unwrap();
        assert!(!result.is_clean());
        assert!(result.conflicts.contains(&"priority".to_string()));
    }

    #[test]
    fn test_merge_optional_field_set_by_right() {
        let base = make_bean("1", "Test");
        let mut left = base.clone();
        let mut right = base.clone();
        right.description = Some("Added by right".to_string());

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.description, Some("Added by right".to_string()));
    }

    #[test]
    fn test_merge_optional_field_conflict() {
        let base = make_bean("1", "Test");
        let mut left = base.clone();
        left.description = Some("Left desc".to_string());
        let mut right = base.clone();
        right.description = Some("Right desc".to_string());

        let result = left.merge(&base, &right).unwrap();
        assert!(!result.is_clean());
        assert!(result.conflicts.contains(&"description".to_string()));
    }

    #[test]
    fn test_merge_notes_append() {
        let mut base = make_bean("1", "Test");
        base.notes = Some("Initial notes".to_string());

        let mut left = base.clone();
        left.notes = Some("Initial notes\nLeft addition".to_string());

        let mut right = base.clone();
        right.notes = Some("Initial notes\nRight addition".to_string());

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean()); // Notes never conflict
        let notes = left.notes.as_ref().unwrap();
        assert!(notes.contains("Initial notes"));
        assert!(notes.contains("Left addition"));
        assert!(notes.contains("Right addition"));
    }

    #[test]
    fn test_merge_notes_only_right() {
        let mut base = make_bean("1", "Test");
        base.notes = Some("Base".to_string());
        let left = base.clone();
        let mut right = base.clone();
        right.notes = Some("Base\nRight added".to_string());

        let mut left = left.clone();
        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.notes, Some("Base\nRight added".to_string()));
    }

    #[test]
    fn test_merge_labels_union() {
        let mut base = make_bean("1", "Test");
        base.labels = vec!["a".to_string()];

        let mut left = base.clone();
        left.labels = vec!["a".to_string(), "b".to_string()];

        let mut right = base.clone();
        right.labels = vec!["a".to_string(), "c".to_string()];

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert!(left.labels.contains(&"a".to_string()));
        assert!(left.labels.contains(&"b".to_string()));
        assert!(left.labels.contains(&"c".to_string()));
    }

    #[test]
    fn test_merge_labels_both_remove() {
        let mut base = make_bean("1", "Test");
        base.labels = vec!["a".to_string(), "b".to_string()];

        let mut left = base.clone();
        left.labels = vec!["a".to_string()]; // removed b

        let mut right = base.clone();
        right.labels = vec!["a".to_string()]; // also removed b

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.labels, vec!["a".to_string()]);
    }

    #[test]
    fn test_merge_labels_conflict_one_removes() {
        let mut base = make_bean("1", "Test");
        base.labels = vec!["a".to_string(), "b".to_string()];

        let mut left = base.clone();
        left.labels = vec!["a".to_string()]; // removed b

        let mut right = base.clone();
        right.labels = vec!["a".to_string(), "b".to_string(), "c".to_string()]; // kept b, added c

        let result = left.merge(&base, &right).unwrap();
        assert!(!result.is_clean()); // Conflict: left removed b, right kept b
        assert!(result.conflicts.contains(&"labels".to_string()));
    }

    #[test]
    fn test_merge_dependencies_union() {
        let mut base = make_bean("1", "Test");
        base.dependencies = vec!["1.1".to_string()];

        let mut left = base.clone();
        left.dependencies = vec!["1.1".to_string(), "1.2".to_string()];

        let mut right = base.clone();
        right.dependencies = vec!["1.1".to_string(), "1.3".to_string()];

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert!(left.dependencies.contains(&"1.1".to_string()));
        assert!(left.dependencies.contains(&"1.2".to_string()));
        assert!(left.dependencies.contains(&"1.3".to_string()));
    }

    #[test]
    fn test_merge_result_is_clean() {
        let result = MergeResult::default();
        assert!(result.is_clean());

        let result_with_conflicts = MergeResult {
            conflicts: vec!["title".to_string()],
            merged: vec![],
        };
        assert!(!result_with_conflicts.is_clean());
    }

    #[test]
    fn test_merge_multiple_fields() {
        let base = make_bean("1", "Original");
        let mut left = base.clone();
        left.priority = 1;
        left.labels = vec!["backend".to_string()];

        let mut right = base.clone();
        right.description = Some("Added desc".to_string());
        right.labels = vec!["frontend".to_string()];

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert_eq!(left.priority, 1);
        assert_eq!(left.description, Some("Added desc".to_string()));
        assert!(left.labels.contains(&"backend".to_string()));
        assert!(left.labels.contains(&"frontend".to_string()));
    }

    #[test]
    fn test_conflict_records_both_versions() {
        let base = make_bean("1", "Original");
        let mut left = base.clone();
        left.description = Some("Left version".to_string());
        let mut right = base.clone();
        right.description = Some("Right version".to_string());

        let _ = left.merge(&base, &right).unwrap();

        assert_eq!(left.conflicts.len(), 1);
        let conflict = &left.conflicts[0];
        assert_eq!(conflict.field, "description");
        assert_eq!(conflict.versions.len(), 2);
        assert!(conflict.versions[0].value.contains("Left version"));
        assert_eq!(conflict.versions[0].agent, "left");
        assert!(conflict.versions[1].value.contains("Right version"));
        assert_eq!(conflict.versions[1].agent, "right");
    }

    #[test]
    fn test_merge_produces_requires() {
        let mut base = make_bean("1", "Test");
        base.produces = vec!["TypeA".to_string()];
        base.requires = vec!["TypeB".to_string()];

        let mut left = base.clone();
        left.produces = vec!["TypeA".to_string(), "TypeC".to_string()];

        let mut right = base.clone();
        right.requires = vec!["TypeB".to_string(), "TypeD".to_string()];

        let result = left.merge(&base, &right).unwrap();
        assert!(result.is_clean());
        assert!(left.produces.contains(&"TypeA".to_string()));
        assert!(left.produces.contains(&"TypeC".to_string()));
        assert!(left.requires.contains(&"TypeB".to_string()));
        assert!(left.requires.contains(&"TypeD".to_string()));
    }
}
