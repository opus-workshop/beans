//! Utility functions for bean ID parsing and status conversion.

use crate::bean::Status;
use anyhow::Result;
use std::str::FromStr;

/// Validate a bean ID to prevent path traversal attacks.
///
/// Valid IDs match the pattern: ^[a-zA-Z0-9._-]+$
/// This prevents directory escape attacks like "../../../etc/passwd".
///
/// # Examples
/// - "1" ✓ (valid)
/// - "3.2.1" ✓ (valid)
/// - "my-task" ✓ (valid)
/// - "task_v1.0" ✓ (valid)
/// - "../etc/passwd" ✗ (invalid)
/// - "task/../escape" ✗ (invalid)
pub fn validate_bean_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(anyhow::anyhow!("Bean ID cannot be empty"));
    }

    if id.len() > 255 {
        return Err(anyhow::anyhow!("Bean ID too long (max 255 characters)"));
    }

    // Check that ID only contains safe characters: alphanumeric, dots, underscores, hyphens
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') {
        return Err(anyhow::anyhow!(
            "Invalid bean ID '{}': must contain only alphanumeric characters, dots, underscores, and hyphens",
            id
        ));
    }

    // Ensure no path traversal sequences
    if id.contains("..") {
        return Err(anyhow::anyhow!(
            "Invalid bean ID '{}': cannot contain '..' (path traversal protection)",
            id
        ));
    }

    Ok(())
}

/// Compare two bean IDs using natural ordering.
/// Parses IDs as dot-separated numeric segments and compares lexicographically.
///
/// # Examples
/// - "1" < "2" (numeric comparison)
/// - "1" < "10" (numeric comparison, not string comparison)
/// - "3.1" < "3.2" (multi-level comparison)
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let sa = parse_id_segments(a);
    let sb = parse_id_segments(b);
    sa.cmp(&sb)
}

/// Parse a dot-separated ID into numeric segments.
///
/// Each segment is parsed as u64. Non-numeric segments are skipped.
/// Used for natural ID comparison.
///
/// # Examples
/// - "1" → [1]
/// - "3.1" → [3, 1]
/// - "3.2.1" → [3, 2, 1]
fn parse_id_segments(id: &str) -> Vec<u64> {
    id.split('.')
        .filter_map(|seg| seg.parse::<u64>().ok())
        .collect()
}

/// Convert a status string to a Status enum, or None if invalid.
///
/// Valid inputs: "open", "in_progress", "closed"
pub fn parse_status(s: &str) -> Option<Status> {
    match s {
        "open" => Some(Status::Open),
        "in_progress" => Some(Status::InProgress),
        "closed" => Some(Status::Closed),
        _ => None,
    }
}

/// Implement FromStr for Status to support standard parsing.
impl FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_status(s).ok_or_else(|| format!("Invalid status: {}", s))
    }
}

/// Convert a bean title into a URL-safe kebab-case slug for use in filenames.
///
/// Algorithm:
/// 1. Trim whitespace
/// 2. Lowercase all characters
/// 3. Replace spaces with hyphens
/// 4. Remove non-alphanumeric characters except hyphens
/// 5. Collapse consecutive hyphens into single hyphen
/// 6. Remove leading/trailing hyphens
/// 7. Truncate to 50 characters
/// 8. Return "unnamed" if empty
///
/// # Examples
/// - "My Task" → "my-task"
/// - "Build API v2.0" → "build-api-v20"
/// - "Foo   Bar" → "foo-bar"
/// - "Implement `bn show` to render Markdown" → "implement-bn-show-to-render-markdown"
/// - "Update Bean parser to read .md + YAML frontmatter" → "update-bean-parser-to-read-md-yaml-frontmatter"
/// - "My-Task!!!" → "my-task"
/// - "   Spaces   " → "spaces"
/// - "" (empty) → "unnamed"
/// - "a" (single char) → "a"
pub fn title_to_slug(title: &str) -> String {
    // Step 1: Trim whitespace
    let trimmed = title.trim();
    
    // Step 2: Lowercase all characters
    let lowercased = trimmed.to_lowercase();
    
    // Step 3 & 4: Replace spaces with hyphens and remove non-alphanumeric (except hyphens)
    let mut slug = String::new();
    for c in lowercased.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if c.is_whitespace() {
            slug.push('-');
        } else if c == '-' {
            slug.push('-');
        }
        // Skip all other characters (special chars, punctuation, etc.)
    }
    
    // Step 5: Collapse consecutive hyphens into single hyphen
    let slug = slug
        .chars()
        .fold(String::new(), |mut acc, c| {
            if c == '-' && acc.ends_with('-') {
                acc
            } else {
                acc.push(c);
                acc
            }
        });
    
    // Step 6: Remove leading/trailing hyphens
    let slug = slug.trim_matches('-').to_string();
    
    // Step 7: Truncate to 50 characters and re-trim hyphens
    let slug = if slug.len() > 50 {
        slug.chars().take(50).collect::<String>().trim_end_matches('-').to_string()
    } else {
        slug
    };

    // Step 8: Return "unnamed" if empty
    if slug.is_empty() {
        "unnamed".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- title_to_slug tests ----------

    #[test]
    fn title_to_slug_simple_case() {
        assert_eq!(title_to_slug("My Task"), "my-task");
    }

    #[test]
    fn title_to_slug_with_numbers_and_dots() {
        assert_eq!(title_to_slug("Build API v2.0"), "build-api-v20");
    }

    #[test]
    fn title_to_slug_multiple_spaces() {
        assert_eq!(title_to_slug("Foo   Bar"), "foo-bar");
    }

    #[test]
    fn title_to_slug_with_backticks() {
        assert_eq!(
            title_to_slug("Implement `bn show` to render Markdown"),
            "implement-bn-show-to-render-markdown"
        );
    }

    #[test]
    fn title_to_slug_with_special_chars() {
        assert_eq!(
            title_to_slug("Update Bean parser to read .md + YAML frontmatter"),
            "update-bean-parser-to-read-md-yaml-frontmatter"
        );
    }

    #[test]
    fn title_to_slug_with_exclamation() {
        assert_eq!(title_to_slug("My-Task!!!"), "my-task");
    }

    #[test]
    fn title_to_slug_leading_trailing_spaces() {
        assert_eq!(title_to_slug("   Spaces   "), "spaces");
    }

    #[test]
    fn title_to_slug_empty_string() {
        assert_eq!(title_to_slug(""), "unnamed");
    }

    #[test]
    fn title_to_slug_single_character() {
        assert_eq!(title_to_slug("a"), "a");
        assert_eq!(title_to_slug("Z"), "z");
    }

    #[test]
    fn title_to_slug_only_spaces() {
        assert_eq!(title_to_slug("   "), "unnamed");
    }

    #[test]
    fn title_to_slug_only_special_chars() {
        assert_eq!(title_to_slug("!!!@@@###"), "unnamed");
    }

    #[test]
    fn title_to_slug_truncate_50_chars() {
        let long_title = "a".repeat(60);
        let result = title_to_slug(&long_title);
        assert_eq!(result, "a".repeat(50));
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn title_to_slug_truncate_with_hyphens() {
        let title = "word ".repeat(20); // Creates long string with hyphens after truncation
        let result = title_to_slug(&title);
        assert!(result.len() <= 50);
    }

    #[test]
    fn title_to_slug_mixed_case() {
        assert_eq!(title_to_slug("ThIs Is A MiXeD CaSe TiTle"), "this-is-a-mixed-case-title");
    }

    #[test]
    fn title_to_slug_numbers_preserved() {
        assert_eq!(title_to_slug("Task 123 Version 4.5.6"), "task-123-version-456");
    }

    #[test]
    fn title_to_slug_consecutive_hyphens() {
        assert_eq!(title_to_slug("foo---bar"), "foo-bar");
        assert_eq!(title_to_slug("foo - - bar"), "foo-bar");
    }

    #[test]
    fn title_to_slug_unicode_removed() {
        // Unicode characters are not ASCII alphanumeric, so they get removed
        assert_eq!(title_to_slug("café"), "caf");
        assert_eq!(title_to_slug("naïve"), "nave");
    }

    #[test]
    fn title_to_slug_all_whitespace_types() {
        assert_eq!(title_to_slug("foo\tbar\nbaz"), "foo-bar-baz");
    }

    #[test]
    fn title_to_slug_exactly_50_chars() {
        let title = "a".repeat(50);
        assert_eq!(title_to_slug(&title), title);
    }

    // ---------- natural_cmp tests ----------

    #[test]
    fn natural_cmp_single_digit() {
        assert_eq!(natural_cmp("1", "2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("2", "1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("1", "1"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_multi_digit() {
        assert_eq!(natural_cmp("1", "10"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("10", "1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("10", "10"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_multi_level() {
        assert_eq!(natural_cmp("3.1", "3.2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("3.2", "3.1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("3.1", "3.1"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_three_level() {
        assert_eq!(natural_cmp("3.2.1", "3.2.2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("3.2.2", "3.2.1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("3.2.1", "3.2.1"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_different_prefix() {
        assert_eq!(natural_cmp("2.1", "3.1"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("10.5", "9.99"), std::cmp::Ordering::Greater);
    }

    // ---------- parse_id_segments tests ----------

    #[test]
    fn parse_id_segments_single() {
        assert_eq!(parse_id_segments("1"), vec![1]);
        assert_eq!(parse_id_segments("42"), vec![42]);
    }

    #[test]
    fn parse_id_segments_multi_level() {
        assert_eq!(parse_id_segments("1.2"), vec![1, 2]);
        assert_eq!(parse_id_segments("3.2.1"), vec![3, 2, 1]);
    }

    #[test]
    fn parse_id_segments_leading_zeros() {
        // Leading zeros are parsed as decimal, not octal
        assert_eq!(parse_id_segments("01"), vec![1]);
        assert_eq!(parse_id_segments("03.02"), vec![3, 2]);
    }

    #[test]
    fn parse_id_segments_non_numeric_skipped() {
        let empty: Vec<u64> = vec![];
        assert_eq!(parse_id_segments("abc"), empty);
        assert_eq!(parse_id_segments("1.abc.2"), vec![1, 2]);
    }

    // ---------- parse_status tests ----------

    #[test]
    fn parse_status_valid_open() {
        assert_eq!(parse_status("open"), Some(Status::Open));
    }

    #[test]
    fn parse_status_valid_in_progress() {
        assert_eq!(parse_status("in_progress"), Some(Status::InProgress));
    }

    #[test]
    fn parse_status_valid_closed() {
        assert_eq!(parse_status("closed"), Some(Status::Closed));
    }

    #[test]
    fn parse_status_invalid() {
        assert_eq!(parse_status("invalid"), None);
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("OPEN"), None);
        assert_eq!(parse_status("Closed"), None);
    }

    #[test]
    fn parse_status_whitespace() {
        assert_eq!(parse_status("open "), None);
        assert_eq!(parse_status(" open"), None);
    }

    // ---------- Status::FromStr tests ----------

    #[test]
    fn status_from_str_open() {
        assert_eq!("open".parse::<Status>(), Ok(Status::Open));
    }

    #[test]
    fn status_from_str_in_progress() {
        assert_eq!("in_progress".parse::<Status>(), Ok(Status::InProgress));
    }

    #[test]
    fn status_from_str_closed() {
        assert_eq!("closed".parse::<Status>(), Ok(Status::Closed));
    }

    #[test]
    fn status_from_str_invalid() {
        assert!("invalid".parse::<Status>().is_err());
        assert!("".parse::<Status>().is_err());
    }

    // ---------- validate_bean_id tests ----------

    #[test]
    fn validate_bean_id_simple_numeric() {
        assert!(validate_bean_id("1").is_ok());
        assert!(validate_bean_id("42").is_ok());
        assert!(validate_bean_id("999").is_ok());
    }

    #[test]
    fn validate_bean_id_dotted() {
        assert!(validate_bean_id("3.1").is_ok());
        assert!(validate_bean_id("3.2.1").is_ok());
        assert!(validate_bean_id("1.2.3.4.5").is_ok());
    }

    #[test]
    fn validate_bean_id_with_underscores() {
        assert!(validate_bean_id("task_1").is_ok());
        assert!(validate_bean_id("my_task_v1").is_ok());
    }

    #[test]
    fn validate_bean_id_with_hyphens() {
        assert!(validate_bean_id("my-task").is_ok());
        assert!(validate_bean_id("task-v1-0").is_ok());
    }

    #[test]
    fn validate_bean_id_alphanumeric() {
        assert!(validate_bean_id("abc123def").is_ok());
        assert!(validate_bean_id("Task1").is_ok());
    }

    #[test]
    fn validate_bean_id_empty_fails() {
        assert!(validate_bean_id("").is_err());
    }

    #[test]
    fn validate_bean_id_path_traversal_fails() {
        assert!(validate_bean_id("../etc/passwd").is_err());
        assert!(validate_bean_id("..").is_err());
        assert!(validate_bean_id("foo/../bar").is_err());
        assert!(validate_bean_id("task..escape").is_err());
    }

    #[test]
    fn validate_bean_id_absolute_path_fails() {
        assert!(validate_bean_id("/etc/passwd").is_err());
    }

    #[test]
    fn validate_bean_id_spaces_fail() {
        assert!(validate_bean_id("my task").is_err());
        assert!(validate_bean_id(" 1").is_err());
        assert!(validate_bean_id("1 ").is_err());
    }

    #[test]
    fn validate_bean_id_special_chars_fail() {
        assert!(validate_bean_id("task@home").is_err());
        assert!(validate_bean_id("task#1").is_err());
        assert!(validate_bean_id("task$money").is_err());
        assert!(validate_bean_id("task%complete").is_err());
        assert!(validate_bean_id("task&friend").is_err());
        assert!(validate_bean_id("task*star").is_err());
        assert!(validate_bean_id("task(paren").is_err());
        assert!(validate_bean_id("task)close").is_err());
        assert!(validate_bean_id("task+plus").is_err());
        assert!(validate_bean_id("task=equals").is_err());
        assert!(validate_bean_id("task[bracket").is_err());
        assert!(validate_bean_id("task]close").is_err());
        assert!(validate_bean_id("task{brace").is_err());
        assert!(validate_bean_id("task}close").is_err());
        assert!(validate_bean_id("task|pipe").is_err());
        assert!(validate_bean_id("task;semicolon").is_err());
        assert!(validate_bean_id("task:colon").is_err());
        assert!(validate_bean_id("task\"quote").is_err());
        assert!(validate_bean_id("task'apostrophe").is_err());
        assert!(validate_bean_id("task<less").is_err());
        assert!(validate_bean_id("task>greater").is_err());
        assert!(validate_bean_id("task,comma").is_err());
        assert!(validate_bean_id("task?question").is_err());
    }

    #[test]
    fn validate_bean_id_too_long() {
        let long_id = "a".repeat(256);
        assert!(validate_bean_id(&long_id).is_err());

        let max_id = "a".repeat(255);
        assert!(validate_bean_id(&max_id).is_ok());
    }
}
