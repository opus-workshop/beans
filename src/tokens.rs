//! Token calculation for bean context sizing.
//!
//! Estimates token count from bean content + referenced files.
//! Used to determine if a bean needs decomposition vs implementation.

use std::fs;
use std::path::Path;

use regex::Regex;

use crate::bean::Bean;

/// File extensions to recognize as source/config files.
const VALID_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "md", "toml", "yaml", "yml", "json", "sql",
];

/// Extract file paths from text content.
///
/// Recognizes patterns like:
/// - `src/foo/bar.rs`
/// - `path/to/file.ts` (in backticks)
/// - `./relative/path.py`
/// - ~/path/to/file.rs
pub fn extract_file_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Pattern: matches file paths with known extensions
    // Handles: src/foo.rs, ./foo.rs, ~/foo.rs, `foo.rs`, path/to/file.ext
    let extensions = VALID_EXTENSIONS.join("|");
    let pattern = format!(
        r"(?:^|[\s`\(\[])([~.]?/?(?:[\w.-]+/)*[\w.-]+\.(?:{}))\b",
        extensions
    );

    if let Ok(re) = Regex::new(&pattern) {
        for cap in re.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                let path = m.as_str().to_string();
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }

    paths
}

/// Estimate token count from character count.
/// Rule of thumb: ~4 characters per token for code.
fn chars_to_tokens(chars: usize) -> u64 {
    (chars / 4) as u64
}

/// Calculate estimated token count for a bean's context.
///
/// Algorithm:
/// 1. Get bean text (title, description, acceptance, notes)
/// 2. Extract file paths from description using regex
/// 3. Read referenced files that exist
/// 4. Estimate: (total_chars) / 4
///
/// Returns estimated token count.
pub fn calculate_tokens(bean: &Bean, workspace: &Path) -> u64 {
    let mut total_chars = 0;

    // Count bean content
    total_chars += bean.title.len();

    if let Some(ref desc) = bean.description {
        total_chars += desc.len();
    }
    if let Some(ref acceptance) = bean.acceptance {
        total_chars += acceptance.len();
    }
    if let Some(ref notes) = bean.notes {
        total_chars += notes.len();
    }
    if let Some(ref design) = bean.design {
        total_chars += design.len();
    }

    // Extract and read referenced files
    let description = bean.description.as_deref().unwrap_or("");
    let file_paths = extract_file_paths(description);

    for file_path in file_paths {
        // Expand ~ to home directory
        let expanded = if file_path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(&file_path[2..])
            } else {
                workspace.join(&file_path)
            }
        } else if file_path.starts_with('/') {
            std::path::PathBuf::from(&file_path)
        } else {
            workspace.join(&file_path)
        };

        // Try to read the file, skip if it doesn't exist or can't be read
        if let Ok(content) = fs::read_to_string(&expanded) {
            total_chars += content.len();
        }
    }

    chars_to_tokens(total_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_file_paths_basic() {
        let text = "Check src/main.rs and src/lib.rs for details.";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn extract_file_paths_backticks() {
        let text = "Edit `src/commands/create.rs` to add the feature.";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"src/commands/create.rs".to_string()));
    }

    #[test]
    fn extract_file_paths_relative() {
        let text = "See ./config.toml and ./src/util.rs";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"./config.toml".to_string()));
        assert!(paths.contains(&"./src/util.rs".to_string()));
    }

    #[test]
    fn extract_file_paths_home() {
        let text = "Check ~/beans/src/main.rs";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"~/beans/src/main.rs".to_string()));
    }

    #[test]
    fn extract_file_paths_multiple_extensions() {
        let text = r#"
            Files:
            - src/types.ts
            - src/api.py
            - config.yaml
            - schema.json
            - query.sql
        "#;
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"src/types.ts".to_string()));
        assert!(paths.contains(&"src/api.py".to_string()));
        assert!(paths.contains(&"config.yaml".to_string()));
        assert!(paths.contains(&"schema.json".to_string()));
        assert!(paths.contains(&"query.sql".to_string()));
    }

    #[test]
    fn extract_file_paths_no_duplicates() {
        let text = "Check src/main.rs and also src/main.rs again.";
        let paths = extract_file_paths(text);
        assert_eq!(paths.iter().filter(|p| *p == "src/main.rs").count(), 1);
    }

    #[test]
    fn extract_file_paths_ignores_non_file_text() {
        let text = "This is a description without any file paths.";
        let paths = extract_file_paths(text);
        assert!(paths.is_empty());
    }

    #[test]
    fn chars_to_tokens_basic() {
        assert_eq!(chars_to_tokens(400), 100);
        assert_eq!(chars_to_tokens(0), 0);
        assert_eq!(chars_to_tokens(3), 0); // rounding down
        assert_eq!(chars_to_tokens(4), 1);
    }

    #[test]
    fn calculate_tokens_basic() {
        let bean = Bean::new("1", "Test bean");
        let workspace = Path::new("/tmp");
        let tokens = calculate_tokens(&bean, workspace);
        // "Test bean" = 9 chars -> 2 tokens
        assert!(tokens > 0);
    }

    #[test]
    fn calculate_tokens_with_description() {
        use tempfile::TempDir;
        use std::fs;

        let dir = TempDir::new().unwrap();
        let test_file = dir.path().join("test.rs");
        fs::write(&test_file, "fn main() { println!(\"hello\"); }").unwrap();

        let mut bean = Bean::new("1", "Test bean");
        bean.description = Some(format!("Check {}", test_file.to_string_lossy()));

        let tokens = calculate_tokens(&bean, dir.path());
        // Should include both description text and file content
        assert!(tokens > 0);
    }
}
