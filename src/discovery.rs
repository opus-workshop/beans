use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Walk up from `start` looking for a `.beans/` directory.
/// Returns the path to the `.beans/` directory if found.
/// Errors if no `.beans/` directory exists in any ancestor.
pub fn find_beans_dir(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(".beans");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !current.pop() {
            bail!("No .beans/ directory found. Run `bn init` first.");
        }
    }
}

/// Find a bean file by ID, supporting the new {id}-{slug}.md naming convention.
///
/// Given ID "11.1", searches for ".beans/11.1-*.md"
/// Returns the full path if found, with the slug included in the filename.
///
/// The function:
/// 1. Validates the ID to prevent path traversal attacks
/// 2. Globs for `.md` files matching the ID prefix
/// 3. Returns the first match found (there should only be one per ID)
/// 4. Returns an error if no bean is found
///
/// # Examples
/// - `find_bean_file(beans_dir, "1")` → `.beans/1-my-task.md`
/// - `find_bean_file(beans_dir, "11.1")` → `.beans/11.1-refactor-md-parser.md`
///
/// # Arguments
/// * `beans_dir` - Path to the `.beans/` directory
/// * `id` - The bean ID to find (e.g., "1", "11.1", "3.2.1")
///
/// # Errors
/// * If the ID is invalid (empty, contains path traversal, etc.)
/// * If no bean file is found for the given ID
/// * If glob pattern matching fails
pub fn find_bean_file(beans_dir: &Path, id: &str) -> Result<PathBuf> {
    // Validate ID to prevent path traversal attacks
    crate::util::validate_bean_id(id)?;

    // Build glob pattern: {beans_dir}/{id}-*.md
    let pattern = format!("{}/*{}-*.md", beans_dir.display(), id);

    // Use glob to search for matching files
    for entry in glob::glob(&pattern).context("glob pattern failed")? {
        let path = entry?;
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            // Check if filename matches {id}-*.md pattern exactly
            if filename.starts_with(&format!("{}-", id)) && filename.ends_with(".md") {
                return Ok(path);
            }
        }
    }

    Err(anyhow::anyhow!("Bean {} not found", id))
}

/// Compute the archive path for a bean given its ID, slug, and date.
///
/// Returns the path: `.beans/archive/YYYY/MM/<id>-<slug>.md`
///
/// # Arguments
/// * `beans_dir` - Path to the `.beans/` directory
/// * `id` - The bean ID (e.g., "1", "11.1", "3.2.1")
/// * `slug` - The bean slug (derived from title)
/// * `date` - The date to use for year/month subdirectories
///
/// # Returns
/// A PathBuf representing `.beans/archive/YYYY/MM/<id>-<slug>.md`
///
/// # Examples
/// ```ignore
/// let path = archive_path_for_bean(
///     Path::new(".beans"),
///     "12",
///     "bean-archive-system",
///     chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()
/// );
/// // Returns: .beans/archive/2026/01/12-bean-archive-system.md
/// ```
pub fn archive_path_for_bean(
    beans_dir: &Path,
    id: &str,
    slug: &str,
    date: chrono::NaiveDate,
) -> PathBuf {
    let year = date.format("%Y").to_string();
    let month = date.format("%m").to_string();
    let filename = format!("{}-{}.md", id, slug);
    beans_dir.join("archive").join(&year).join(&month).join(filename)
}

/// Find an archived bean by ID within the `.beans/archive/` directory tree.
///
/// Searches recursively through `.beans/archive/**/` for a bean file matching the given ID.
/// Returns the full path to the first matching bean file.
///
/// # Arguments
/// * `beans_dir` - Path to the `.beans/` directory
/// * `id` - The bean ID to search for
///
/// # Returns
/// `Ok(PathBuf)` with the path to the archived bean file if found
/// `Err` if the bean is not found in the archive
///
/// # Examples
/// ```ignore
/// let path = find_archived_bean(Path::new(".beans"), "12")?;
/// // Returns: .beans/archive/2026/01/12-bean-archive-system.md
/// ```
pub fn find_archived_bean(beans_dir: &Path, id: &str) -> Result<PathBuf> {
    // Validate ID to prevent path traversal attacks
    crate::util::validate_bean_id(id)?;

    let archive_dir = beans_dir.join("archive");

    // If archive directory doesn't exist, bean is not archived
    if !archive_dir.is_dir() {
        bail!("Archived bean {} not found (archive directory does not exist)", id);
    }

    // Recursively search through year subdirectories
    for year_entry in std::fs::read_dir(&archive_dir).context("Failed to read archive directory")? {
        let year_path = year_entry?.path();
        if !year_path.is_dir() {
            continue;
        }

        // Search through month subdirectories
        for month_entry in std::fs::read_dir(&year_path).context("Failed to read year directory")? {
            let month_path = month_entry?.path();
            if !month_path.is_dir() {
                continue;
            }

            // Search through bean files in month directory
            for bean_entry in std::fs::read_dir(&month_path).context("Failed to read month directory")? {
                let bean_path = bean_entry?.path();
                if !bean_path.is_file() {
                    continue;
                }

                // Check if filename matches the pattern {id}-*.md
                if let Some(filename) = bean_path.file_name().and_then(|n| n.to_str()) {
                    if filename.starts_with(&format!("{}-", id)) && filename.ends_with(".md") {
                        return Ok(bean_path);
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!("Archived bean {} not found", id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_beans_in_current_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".beans")).unwrap();

        let result = find_beans_dir(dir.path()).unwrap();
        assert_eq!(result, dir.path().join(".beans"));
    }

    #[test]
    fn finds_beans_in_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".beans")).unwrap();
        let child = dir.path().join("src");
        fs::create_dir(&child).unwrap();

        let result = find_beans_dir(&child).unwrap();
        assert_eq!(result, dir.path().join(".beans"));
    }

    #[test]
    fn finds_beans_in_grandparent_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".beans")).unwrap();
        let child = dir.path().join("src").join("deep");
        fs::create_dir_all(&child).unwrap();

        let result = find_beans_dir(&child).unwrap();
        assert_eq!(result, dir.path().join(".beans"));
    }

    #[test]
    fn returns_error_when_no_beans_exists() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("some").join("nested").join("dir");
        fs::create_dir_all(&child).unwrap();

        let result = find_beans_dir(&child);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No .beans/ directory found"),
            "Error message was: {}",
            err_msg
        );
    }

    #[test]
    fn prefers_closest_beans_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Parent has .beans
        fs::create_dir(dir.path().join(".beans")).unwrap();
        // Child also has .beans
        let child = dir.path().join("subproject");
        fs::create_dir(&child).unwrap();
        fs::create_dir(child.join(".beans")).unwrap();

        let result = find_beans_dir(&child).unwrap();
        assert_eq!(result, child.join(".beans"));
    }

    // =====================================================================
    // Tests for find_bean_file()
    // =====================================================================

    #[test]
    fn find_bean_file_simple_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean file with slug
        fs::write(beans_dir.join("1-my-task.md"), "test content").unwrap();

        let result = find_bean_file(&beans_dir, "1").unwrap();
        assert_eq!(result, beans_dir.join("1-my-task.md"));
    }

    #[test]
    fn find_bean_file_hierarchical_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean file with hierarchical ID
        fs::write(beans_dir.join("11.1-refactor-parser.md"), "test content").unwrap();

        let result = find_bean_file(&beans_dir, "11.1").unwrap();
        assert_eq!(result, beans_dir.join("11.1-refactor-parser.md"));
    }

    #[test]
    fn find_bean_file_three_level_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean file with three-level ID
        fs::write(beans_dir.join("3.2.1-deep-task.md"), "test content").unwrap();

        let result = find_bean_file(&beans_dir, "3.2.1").unwrap();
        assert_eq!(result, beans_dir.join("3.2.1-deep-task.md"));
    }

    #[test]
    fn find_bean_file_returns_first_match() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create multiple files that start with the same ID prefix
        // (shouldn't happen in practice, but test the behavior)
        fs::write(beans_dir.join("2-alpha.md"), "first").unwrap();
        fs::write(beans_dir.join("2-beta.md"), "second").unwrap();

        let result = find_bean_file(&beans_dir, "2").unwrap();
        // Should return one of the files (glob order is implementation-dependent)
        assert!(result.ends_with("2-alpha.md") || result.ends_with("2-beta.md"));
        assert!(result.file_name().unwrap().to_str().unwrap().ends_with(".md"));
    }

    #[test]
    fn find_bean_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Try to find a bean that doesn't exist
        let result = find_bean_file(&beans_dir, "999");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Bean 999 not found"));
    }

    #[test]
    fn find_bean_file_validates_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Try to find with invalid ID (path traversal attempt)
        let result = find_bean_file(&beans_dir, "../../../etc/passwd");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid bean ID") || err_msg.contains("path traversal"));
    }

    #[test]
    fn find_bean_file_validates_empty_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Try to find with empty ID
        let result = find_bean_file(&beans_dir, "");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("cannot be empty") || err_msg.contains("invalid"));
    }

    #[test]
    fn find_bean_file_with_long_slug() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean file with a long slug
        let long_slug = "implement-comprehensive-feature-with-full-test-coverage";
        let filename = format!("5-{}.md", long_slug);
        fs::write(beans_dir.join(&filename), "test content").unwrap();

        let result = find_bean_file(&beans_dir, "5").unwrap();
        assert!(result.to_str().unwrap().contains(long_slug));
    }

    #[test]
    fn find_bean_file_ignores_yaml_files() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a .yaml file (should be ignored)
        fs::write(beans_dir.join("7.yaml"), "old format").unwrap();

        // Try to find the bean (should fail since we only look for .md)
        let result = find_bean_file(&beans_dir, "7");
        assert!(result.is_err());
    }

    #[test]
    fn find_bean_file_ignores_files_without_proper_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a file that doesn't match the pattern
        fs::write(beans_dir.join("7-something-else.md"), "wrong file").unwrap();

        // Try to find "8" (which exists as "7-something-else.md")
        let result = find_bean_file(&beans_dir, "8");
        assert!(result.is_err());
    }

    #[test]
    fn find_bean_file_handles_numeric_id_prefix_matching() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create files: "2-task.md" and "20-task.md"
        fs::write(beans_dir.join("2-task.md"), "bean 2").unwrap();
        fs::write(beans_dir.join("20-task.md"), "bean 20").unwrap();

        // Looking for "2" should only match "2-task.md", not "20-task.md"
        let result = find_bean_file(&beans_dir, "2").unwrap();
        assert_eq!(result, beans_dir.join("2-task.md"));
    }

    #[test]
    fn find_bean_file_with_special_chars_in_slug() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create a bean file with hyphens and numbers in slug
        fs::write(beans_dir.join("6-v2-refactor-api.md"), "test").unwrap();

        let result = find_bean_file(&beans_dir, "6").unwrap();
        assert_eq!(result, beans_dir.join("6-v2-refactor-api.md"));
    }

    #[test]
    fn find_bean_file_rejects_special_chars_in_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Try IDs with special characters that should be rejected
        assert!(find_bean_file(&beans_dir, "task@home").is_err());
        assert!(find_bean_file(&beans_dir, "task#1").is_err());
        assert!(find_bean_file(&beans_dir, "task$money").is_err());
    }

    // =====================================================================
    // Tests for archive_path_for_bean()
    // =====================================================================

    #[test]
    fn archive_path_for_bean_basic() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let path = archive_path_for_bean(&beans_dir, "12", "bean-archive-system", date);
        
        // Verify path structure
        assert_eq!(
            path,
            beans_dir.join("archive/2026/01/12-bean-archive-system.md")
        );
    }

    #[test]
    fn archive_path_for_bean_hierarchical_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        let date = chrono::NaiveDate::from_ymd_opt(2025, 12, 15).unwrap();
        let path = archive_path_for_bean(&beans_dir, "11.1", "refactor-parser", date);
        
        assert_eq!(
            path,
            beans_dir.join("archive/2025/12/11.1-refactor-parser.md")
        );
    }

    #[test]
    fn archive_path_for_bean_single_digit_month() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let path = archive_path_for_bean(&beans_dir, "5", "task", date);
        
        // Month should be zero-padded (03, not 3)
        assert_eq!(
            path,
            beans_dir.join("archive/2026/03/5-task.md")
        );
    }

    #[test]
    fn archive_path_for_bean_three_level_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        let date = chrono::NaiveDate::from_ymd_opt(2024, 8, 20).unwrap();
        let path = archive_path_for_bean(&beans_dir, "3.2.1", "deep-task", date);
        
        assert_eq!(
            path,
            beans_dir.join("archive/2024/08/3.2.1-deep-task.md")
        );
    }

    #[test]
    fn archive_path_for_bean_long_slug() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let long_slug = "implement-comprehensive-feature-with-full-test-coverage";
        let path = archive_path_for_bean(&beans_dir, "42", long_slug, date);
        
        assert!(path.to_str().unwrap().contains(long_slug));
        assert_eq!(
            path,
            beans_dir.join("archive/2026/01/42-implement-comprehensive-feature-with-full-test-coverage.md")
        );
    }

    // =====================================================================
    // Tests for find_archived_bean()
    // =====================================================================

    #[test]
    fn find_archived_bean_simple_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        let archive_dir = beans_dir.join("archive/2026/01");
        fs::create_dir_all(&archive_dir).unwrap();

        // Create an archived bean file
        fs::write(archive_dir.join("12-bean-archive.md"), "archived content").unwrap();

        let result = find_archived_bean(&beans_dir, "12").unwrap();
        assert_eq!(result, archive_dir.join("12-bean-archive.md"));
    }

    #[test]
    fn find_archived_bean_hierarchical_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        let archive_dir = beans_dir.join("archive/2025/12");
        fs::create_dir_all(&archive_dir).unwrap();

        // Create an archived bean file
        fs::write(archive_dir.join("11.1-refactor-parser.md"), "archived content").unwrap();

        let result = find_archived_bean(&beans_dir, "11.1").unwrap();
        assert_eq!(result, archive_dir.join("11.1-refactor-parser.md"));
    }

    #[test]
    fn find_archived_bean_multiple_years() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        // Create archive structure with multiple years
        fs::create_dir_all(beans_dir.join("archive/2024/06")).unwrap();
        fs::create_dir_all(beans_dir.join("archive/2025/12")).unwrap();
        fs::create_dir_all(beans_dir.join("archive/2026/01")).unwrap();

        // Create bean in 2024
        fs::write(
            beans_dir.join("archive/2024/06/5-old-task.md"),
            "old content"
        ).unwrap();

        // Create bean in 2026
        fs::write(
            beans_dir.join("archive/2026/01/12-new-task.md"),
            "new content"
        ).unwrap();

        // Should find the bean regardless of year
        let result = find_archived_bean(&beans_dir, "5").unwrap();
        assert!(result.to_str().unwrap().contains("2024/06"));

        let result = find_archived_bean(&beans_dir, "12").unwrap();
        assert!(result.to_str().unwrap().contains("2026/01"));
    }

    #[test]
    fn find_archived_bean_multiple_months() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        
        // Create archive structure with multiple months in same year
        fs::create_dir_all(beans_dir.join("archive/2026/01")).unwrap();
        fs::create_dir_all(beans_dir.join("archive/2026/02")).unwrap();
        fs::create_dir_all(beans_dir.join("archive/2026/03")).unwrap();

        // Create beans in different months
        fs::write(
            beans_dir.join("archive/2026/01/10-january-task.md"),
            "january"
        ).unwrap();

        fs::write(
            beans_dir.join("archive/2026/03/10-march-task.md"),
            "march"
        ).unwrap();

        // Both should be found (returns first match)
        let result = find_archived_bean(&beans_dir, "10").unwrap();
        assert!(result.to_str().unwrap().contains("2026"));
        assert!(result.file_name().unwrap().to_str().unwrap().starts_with("10-"));
    }

    #[test]
    fn find_archived_bean_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        let archive_dir = beans_dir.join("archive/2026/01");
        fs::create_dir_all(&archive_dir).unwrap();

        // Create a different bean
        fs::write(archive_dir.join("12-some-task.md"), "content").unwrap();

        // Try to find a bean that doesn't exist
        let result = find_archived_bean(&beans_dir, "999");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Archived bean 999 not found"));
    }

    #[test]
    fn find_archived_bean_no_archive_dir() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Archive directory doesn't exist
        let result = find_archived_bean(&beans_dir, "12");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Archived bean 12 not found"));
    }

    #[test]
    fn find_archived_bean_validates_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Try with invalid IDs (path traversal)
        let result = find_archived_bean(&beans_dir, "../../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid bean ID"));

        let result = find_archived_bean(&beans_dir, "");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn find_archived_bean_three_level_id() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        let archive_dir = beans_dir.join("archive/2024/08");
        fs::create_dir_all(&archive_dir).unwrap();

        // Create an archived bean with three-level ID
        fs::write(archive_dir.join("3.2.1-deep-task.md"), "archived content").unwrap();

        let result = find_archived_bean(&beans_dir, "3.2.1").unwrap();
        assert_eq!(result, archive_dir.join("3.2.1-deep-task.md"));
    }

    #[test]
    fn find_archived_bean_ignores_non_matching_ids() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        let archive_dir = beans_dir.join("archive/2026/01");
        fs::create_dir_all(&archive_dir).unwrap();

        // Create beans with similar IDs
        fs::write(archive_dir.join("1-first-task.md"), "bean 1").unwrap();
        fs::write(archive_dir.join("10-tenth-task.md"), "bean 10").unwrap();
        fs::write(archive_dir.join("100-hundredth-task.md"), "bean 100").unwrap();

        // Looking for "1" should only match "1-first-task.md", not "10-" or "100-"
        let result = find_archived_bean(&beans_dir, "1").unwrap();
        assert_eq!(result, archive_dir.join("1-first-task.md"));

        // Looking for "10" should only match "10-tenth-task.md"
        let result = find_archived_bean(&beans_dir, "10").unwrap();
        assert_eq!(result, archive_dir.join("10-tenth-task.md"));
    }

    #[test]
    fn find_archived_bean_with_long_slug() {
        let dir = tempfile::tempdir().unwrap();
        let beans_dir = dir.path().join(".beans");
        let archive_dir = beans_dir.join("archive/2026/01");
        fs::create_dir_all(&archive_dir).unwrap();

        let long_slug = "implement-comprehensive-feature-with-full-test-coverage";
        let filename = format!("42-{}.md", long_slug);
        fs::write(archive_dir.join(&filename), "archived").unwrap();

        let result = find_archived_bean(&beans_dir, "42").unwrap();
        assert!(result.to_str().unwrap().contains(long_slug));
    }
}
