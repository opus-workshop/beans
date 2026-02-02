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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
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

    /// Whether this bean has been moved to the archive.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_archived: bool,

    /// Artifacts this bean produces (types, functions, files).
    /// Used by decompose skill for dependency inference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<String>,

    /// Artifacts this bean requires from other beans.
    /// Maps to dependencies via sibling produces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
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

fn is_false(v: &bool) -> bool {
    !*v
}

impl Bean {
    /// Create a new bean with sensible defaults.
    /// Returns an error if the ID is invalid.
    pub fn try_new(id: impl Into<String>, title: impl Into<String>) -> Result<Self> {
        let id_str = id.into();
        validate_bean_id(&id_str)?;

        let now = Utc::now();
        Ok(Self {
            id: id_str,
            title: title.into(),
            slug: None,
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
            is_archived: false,
            produces: Vec::new(),
            requires: Vec::new(),
        })
    }

    /// Create a new bean with sensible defaults.
    /// Panics if the ID is invalid. Prefer `try_new` for fallible construction.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::try_new(id, title).expect("Invalid bean ID")
    }

    /// Parse YAML frontmatter and markdown body.
    /// Expects format:
    /// ```text
    /// ---
    /// id: 1
    /// title: Example
    /// status: open
    /// ...
    /// ---
    /// # Markdown body here
    /// ```
    fn parse_frontmatter(content: &str) -> Result<(String, Option<String>)> {
        // Check if content starts with ---
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            // Not frontmatter format, try pure YAML
            return Err(anyhow::anyhow!("Not markdown frontmatter format"));
        }

        // Find the second --- delimiter
        let after_first_delimiter = if let Some(stripped) = content.strip_prefix("---\r\n") {
            stripped
        } else if let Some(stripped) = content.strip_prefix("---\n") {
            stripped
        } else {
            return Err(anyhow::anyhow!("Not markdown frontmatter format"));
        };

        let second_delimiter_pos = after_first_delimiter
            .find("---")
            .ok_or_else(|| anyhow::anyhow!("Markdown frontmatter is missing closing delimiter (---)"))?;
        let frontmatter = &after_first_delimiter[..second_delimiter_pos];

        // Skip the closing --- and any whitespace to get the body
        let body_start = second_delimiter_pos + 3;
        let body_raw = &after_first_delimiter[body_start..];

        // Trim leading newlines but preserve the rest
        let body = body_raw.trim_start_matches(['\n', '\r']);
        let body = (!body.is_empty()).then(|| body.to_string());

        Ok((frontmatter.to_string(), body))
    }

    /// Parse a bean from a string (either YAML or Markdown with YAML frontmatter).
    pub fn from_string(content: &str) -> Result<Self> {
        // Try frontmatter format first
        match Self::parse_frontmatter(content) {
            Ok((frontmatter, body)) => {
                // Parse frontmatter as YAML
                let mut bean: Bean = serde_yaml::from_str(&frontmatter)?;

                // If there's a body and no description yet, set it
                if let Some(markdown_body) = body {
                    if bean.description.is_none() {
                        bean.description = Some(markdown_body);
                    }
                }

                Ok(bean)
            }
            Err(_) => {
                // Fallback: treat entire content as YAML
                let bean: Bean = serde_yaml::from_str(content)?;
                Ok(bean)
            }
        }
    }

    /// Read a bean from a file (supports both YAML and Markdown with YAML frontmatter).
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())?;
        Self::from_string(&contents)
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
            slug: None,
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
            is_archived: false,
            produces: vec!["Parser".to_string()],
            requires: vec!["Lexer".to_string()],
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
        assert!(!yaml.contains("is_archived:"));
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

    // =====================================================================
    // Tests for Markdown Frontmatter Parsing
    // =====================================================================

    #[test]
    fn test_parse_md_frontmatter() {
        let content = r#"---
id: 11.1
title: Test Bean
status: open
priority: 2
created_at: "2026-01-26T15:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
---

# Description

Test markdown body.
"#;
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "11.1");
        assert_eq!(bean.title, "Test Bean");
        assert_eq!(bean.status, Status::Open);
        assert!(bean.description.is_some());
        assert!(bean.description.as_ref().unwrap().contains("# Description"));
        assert!(bean.description.as_ref().unwrap().contains("Test markdown body"));
    }

    #[test]
    fn test_parse_md_frontmatter_preserves_metadata_fields() {
        let content = r#"---
id: "2.5"
title: Complex Bean
status: in_progress
priority: 1
created_at: "2026-01-01T10:00:00Z"
updated_at: "2026-01-26T15:00:00Z"
parent: "2"
labels:
  - backend
  - urgent
dependencies:
  - "2.1"
  - "2.2"
---

## Implementation Notes

This is a complex bean with multiple metadata fields.
"#;
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "2.5");
        assert_eq!(bean.title, "Complex Bean");
        assert_eq!(bean.status, Status::InProgress);
        assert_eq!(bean.priority, 1);
        assert_eq!(bean.parent, Some("2".to_string()));
        assert_eq!(bean.labels, vec!["backend".to_string(), "urgent".to_string()]);
        assert_eq!(
            bean.dependencies,
            vec!["2.1".to_string(), "2.2".to_string()]
        );
        assert!(bean.description.is_some());
    }

    #[test]
    fn test_parse_md_frontmatter_empty_body() {
        let content = r#"---
id: "3"
title: No Body Bean
status: open
priority: 2
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
---
"#;
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "3");
        assert_eq!(bean.title, "No Body Bean");
        assert!(bean.description.is_none());
    }

    #[test]
    fn test_parse_md_frontmatter_with_body_containing_dashes() {
        let content = r#"---
id: "4"
title: Dashes in Body
status: open
priority: 2
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
---

# Section 1

This has --- inside the body, which should not break parsing.

---

More content after a horizontal rule.
"#;
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "4");
        assert!(bean.description.is_some());
        let body = bean.description.as_ref().unwrap();
        assert!(body.contains("---"));
        assert!(body.contains("horizontal rule"));
    }

    #[test]
    fn test_parse_md_frontmatter_with_whitespace_in_body() {
        let content = r#"---
id: "5"
title: Whitespace Test
status: open
priority: 2
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
---


   Leading whitespace preserved after trimming newlines.

"#;
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "5");
        assert!(bean.description.is_some());
        let body = bean.description.as_ref().unwrap();
        // Leading newlines trimmed, but content preserved
        assert!(body.contains("Leading whitespace"));
    }

    #[test]
    fn test_fallback_to_yaml_parsing() {
        let yaml_content = r#"
id: "6"
title: Pure YAML Bean
status: open
priority: 3
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
description: "This is YAML, not markdown"
"#;
        let bean = Bean::from_string(yaml_content).unwrap();
        assert_eq!(bean.id, "6");
        assert_eq!(bean.title, "Pure YAML Bean");
        assert_eq!(
            bean.description,
            Some("This is YAML, not markdown".to_string())
        );
    }

    #[test]
    fn test_file_round_trip_with_markdown() {
        let content = r#"---
id: "7"
title: File Markdown Test
status: open
priority: 2
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
---

# Markdown Body

This is a test of reading markdown from a file.
"#;

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        // Write markdown content
        std::fs::write(&path, content).unwrap();

        // Read back as bean
        let bean = Bean::from_file(&path).unwrap();
        assert_eq!(bean.id, "7");
        assert_eq!(bean.title, "File Markdown Test");
        assert!(bean.description.is_some());
        assert!(bean
            .description
            .as_ref()
            .unwrap()
            .contains("# Markdown Body"));

        drop(tmp);
    }

    #[test]
    fn test_parse_md_frontmatter_missing_closing_delimiter() {
        let bad_content = r#"---
id: "8"
title: Missing Delimiter
status: open
"#;
        let result = Bean::from_string(bad_content);
        // Should fail because no closing ---
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_md_frontmatter_multiline_fields() {
        let content = r#"---
id: "9"
title: Multiline Test
status: open
priority: 2
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
acceptance: |
  - Criterion 1
  - Criterion 2
  - Criterion 3
---

# Implementation

Start implementing...
"#;
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "9");
        assert!(bean.acceptance.is_some());
        let acceptance = bean.acceptance.as_ref().unwrap();
        assert!(acceptance.contains("Criterion 1"));
        assert!(acceptance.contains("Criterion 2"));
        assert!(bean.description.is_some());
    }

    #[test]
    fn test_parse_md_with_crlf_line_endings() {
        let content = "---\r\nid: \"10\"\r\ntitle: CRLF Test\r\nstatus: open\r\npriority: 2\r\ncreated_at: \"2026-01-01T00:00:00Z\"\r\nupdated_at: \"2026-01-01T00:00:00Z\"\r\n---\r\n\r\n# Body\r\n\r\nCRLF line endings.";
        let bean = Bean::from_string(content).unwrap();
        assert_eq!(bean.id, "10");
        assert_eq!(bean.title, "CRLF Test");
        assert!(bean.description.is_some());
    }

    #[test]
    fn test_parse_md_description_does_not_override_yaml_description() {
        let content = r#"---
id: "11"
title: Override Test
status: open
priority: 2
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
description: "From YAML metadata"
---

# From Markdown Body

This should not override.
"#;
        let bean = Bean::from_string(content).unwrap();
        // Description from YAML should take precedence
        assert_eq!(
            bean.description,
            Some("From YAML metadata".to_string())
        );
    }
}
