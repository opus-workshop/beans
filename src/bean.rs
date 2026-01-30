use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::util::validate_bean_id;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    InProgress,
    Closed,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Open => write!(f, "open"),
            Status::InProgress => write!(f, "in_progress"),
            Status::Closed => write!(f, "closed"),
        }
    }
}

// ---------------------------------------------------------------------------
// Priority Validation
// ---------------------------------------------------------------------------

/// Validate that priority is in the valid range (0-4, P0-P4).
pub fn validate_priority(priority: u8) -> Result<()> {
    if priority > 4 {
        return Err(anyhow::anyhow!(
            "Invalid priority: {}. Priority must be in range 0-4 (P0-P4)",
            priority
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bean
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bean {
    pub id: String,
    pub title: String,
    pub status: Status,
    #[serde(default = "default_priority")]
    pub priority: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,

    // -- verification & claim fields --

    /// Shell command that must exit 0 to close the bean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
    /// How many times the verify command has been run.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u32,
    /// Maximum verify attempts before escalation (default 3).
    #[serde(default = "default_max_attempts", skip_serializing_if = "is_default_max_attempts")]
    pub max_attempts: u32,
    /// Agent or user currently holding a claim on this bean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    /// When the claim was acquired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<DateTime<Utc>>,
}

fn default_priority() -> u8 {
    2
}

fn default_max_attempts() -> u32 {
    3
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn is_default_max_attempts(v: &u32) -> bool {
    *v == 3
}

impl Bean {
    /// Create a new bean with sensible defaults.
    /// Validates the ID to prevent path traversal attacks.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        let id_str = id.into();
        // Validate the ID format. We panic if invalid since this is a constructor precondition.
        if let Err(e) = validate_bean_id(&id_str) {
            panic!("Invalid bean ID: {}", e);
        }

        let now = Utc::now();
        Self {
            id: id_str,
            title: title.into(),
            status: Status::Open,
            priority: 2,
            created_at: now,
            updated_at: now,
            description: None,
            acceptance: None,
            notes: None,
            design: None,
            labels: Vec::new(),
            assignee: None,
            closed_at: None,
            close_reason: None,
            parent: None,
            dependencies: Vec::new(),
            verify: None,
            attempts: 0,
            max_attempts: 3,
            claimed_by: None,
            claimed_at: None,
        }
    }

    /// Read a bean from a YAML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())?;
        let bean: Bean = serde_yaml::from_str(&contents)?;
        Ok(bean)
    }

    /// Write this bean to a YAML file.
    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(path.as_ref(), yaml)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn round_trip_minimal_bean() {
        let bean = Bean::new("1", "My first bean");

        // Serialize
        let yaml = serde_yaml::to_string(&bean).unwrap();

        // Deserialize
        let restored: Bean = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(bean, restored);
    }

    #[test]
    fn round_trip_full_bean() {
        let now = Utc::now();
        let bean = Bean {
            id: "3.2.1".to_string(),
            title: "Implement parser".to_string(),
            status: Status::InProgress,
            priority: 1,
            created_at: now,
            updated_at: now,
            description: Some("Build a robust YAML parser".to_string()),
            acceptance: Some("All tests pass".to_string()),
            notes: Some("Watch out for edge cases".to_string()),
            design: Some("Use serde_yaml".to_string()),
            labels: vec!["backend".to_string(), "core".to_string()],
            assignee: Some("alice".to_string()),
            closed_at: Some(now),
            close_reason: Some("Done".to_string()),
            parent: Some("3.2".to_string()),
            dependencies: vec!["3.1".to_string()],
            verify: Some("cargo test".to_string()),
            attempts: 1,
            max_attempts: 5,
            claimed_by: Some("agent-7".to_string()),
            claimed_at: Some(now),
        };

        let yaml = serde_yaml::to_string(&bean).unwrap();
        let restored: Bean = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(bean, restored);
    }

    #[test]
    fn status_serializes_as_lowercase() {
        let open = serde_yaml::to_string(&Status::Open).unwrap();
        let in_progress = serde_yaml::to_string(&Status::InProgress).unwrap();
        let closed = serde_yaml::to_string(&Status::Closed).unwrap();

        assert_eq!(open.trim(), "open");
        assert_eq!(in_progress.trim(), "in_progress");
        assert_eq!(closed.trim(), "closed");
    }

    #[test]
    fn optional_fields_omitted_when_none() {
        let bean = Bean::new("1", "Minimal");
        let yaml = serde_yaml::to_string(&bean).unwrap();

        assert!(!yaml.contains("description:"));
        assert!(!yaml.contains("acceptance:"));
        assert!(!yaml.contains("notes:"));
        assert!(!yaml.contains("design:"));
        assert!(!yaml.contains("assignee:"));
        assert!(!yaml.contains("closed_at:"));
        assert!(!yaml.contains("close_reason:"));
        assert!(!yaml.contains("parent:"));
        assert!(!yaml.contains("labels:"));
        assert!(!yaml.contains("dependencies:"));
        assert!(!yaml.contains("verify:"));
        assert!(!yaml.contains("attempts:"));
        assert!(!yaml.contains("max_attempts:"));
        assert!(!yaml.contains("claimed_by:"));
        assert!(!yaml.contains("claimed_at:"));
    }

    #[test]
    fn timestamps_serialize_as_iso8601() {
        let bean = Bean::new("1", "Check timestamps");
        let yaml = serde_yaml::to_string(&bean).unwrap();

        // ISO 8601 timestamps contain 'T' between date and time
        for line in yaml.lines() {
            if line.starts_with("created_at:") || line.starts_with("updated_at:") {
                let value = line.split_once(':').unwrap().1.trim();
                assert!(
                    value.contains('T'),
                    "timestamp should be ISO 8601: {value}"
                );
            }
        }
    }

    #[test]
    fn file_round_trip() {
        let bean = Bean::new("42", "File I/O test");

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        // Write
        bean.to_file(&path).unwrap();

        // Read back
        let restored = Bean::from_file(&path).unwrap();
        assert_eq!(bean, restored);

        // Verify the file is valid YAML we can also read raw
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("id: '42'") || raw.contains("id: \"42\""));
        assert!(raw.contains("title: File I/O test") || raw.contains("title: 'File I/O test'"));
        drop(tmp);
    }

    #[test]
    fn defaults_are_correct() {
        let bean = Bean::new("1", "Defaults");
        assert_eq!(bean.status, Status::Open);
        assert_eq!(bean.priority, 2);
        assert!(bean.labels.is_empty());
        assert!(bean.dependencies.is_empty());
        assert!(bean.description.is_none());
    }

    #[test]
    fn deserialize_with_missing_optional_fields() {
        let yaml = r#"
id: "5"
title: Sparse bean
status: open
priority: 3
created_at: "2025-01-01T00:00:00Z"
updated_at: "2025-01-01T00:00:00Z"
"#;
        let bean: Bean = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(bean.id, "5");
        assert_eq!(bean.priority, 3);
        assert!(bean.description.is_none());
        assert!(bean.labels.is_empty());
    }

    #[test]
    fn validate_priority_accepts_valid_range() {
        for priority in 0..=4 {
            assert!(validate_priority(priority).is_ok(), "Priority {} should be valid", priority);
        }
    }

    #[test]
    fn validate_priority_rejects_out_of_range() {
        assert!(validate_priority(5).is_err());
        assert!(validate_priority(10).is_err());
        assert!(validate_priority(255).is_err());
    }
}
