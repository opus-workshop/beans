use regex::Regex;

/// Extracts file paths from a bean description using regex pattern matching.
///
/// Matches relative file paths with the following extensions:
/// .rs, .ts, .py, .md, .json, .toml, .yaml, .sh, .go, .java
///
/// Examples:
/// - "Modify src/main.rs" → ["src/main.rs"]
/// - "See src/foo.rs and tests/bar.rs" → ["src/foo.rs", "tests/bar.rs"]
/// - "File: src/main.rs." → ["src/main.rs"]
///
/// # Arguments
/// * `description` - The description text to search for file paths
///
/// # Returns
/// A Vec of deduplicated file paths in order of appearance
pub fn extract_paths(description: &str) -> Vec<String> {
    // Simple pattern: match file paths with supported extensions
    // Start with alphanumeric, underscore, or dot (NOT /)
    // Can contain slashes, hyphens, dots, underscores
    // Must end with a supported extension
    let pattern = r"([a-zA-Z0-9_.][a-zA-Z0-9_./\-]*\.(rs|ts|py|md|json|toml|yaml|sh|go|java))\b";

    if let Ok(regex) = Regex::new(pattern) {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for cap in regex.captures_iter(description) {
            if let Some(path) = cap.get(1) {
                let path_str = path.as_str();
                let path_start = path.start();

                // Filter out absolute paths: if preceded directly by /
                if path_start > 0 && description.chars().nth(path_start - 1) == Some('/') {
                    continue;
                }

                // Filter out URLs (check if preceded by :// in the description)
                if path_start >= 3 {
                    let before = &description[path_start.saturating_sub(3)..path_start];
                    if before.ends_with("://") {
                        continue;
                    }
                }

                // Deduplicate and add to result
                if seen.insert(path_str.to_string()) {
                    result.push(path_str.to_string());
                }
            }
        }

        result
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_path() {
        let result = extract_paths("Modify src/main.rs");
        assert_eq!(result, vec!["src/main.rs"]);
    }

    #[test]
    fn test_multiple_paths() {
        let result = extract_paths("See src/foo.rs and tests/bar.rs");
        assert_eq!(result, vec!["src/foo.rs", "tests/bar.rs"]);
    }

    #[test]
    fn test_deduplicate_paths() {
        let result = extract_paths("Update src/main.rs to fix src/main.rs");
        assert_eq!(result, vec!["src/main.rs"]);
    }

    #[test]
    fn test_with_punctuation() {
        let result = extract_paths("File: src/main.rs.");
        assert_eq!(result, vec!["src/main.rs"]);
    }

    #[test]
    fn test_no_paths() {
        let result = extract_paths("No files mentioned here");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_various_extensions() {
        let description = "Check src/config.rs, tests/test.ts, docs/guide.md, package.json, and Cargo.toml";
        let result = extract_paths(description);
        assert_eq!(result, vec!["src/config.rs", "tests/test.ts", "docs/guide.md", "package.json", "Cargo.toml"]);
    }

    #[test]
    fn test_paths_with_hyphens() {
        let result = extract_paths("See src/my-module.rs and tests/integration-test.rs");
        assert_eq!(result, vec!["src/my-module.rs", "tests/integration-test.rs"]);
    }

    #[test]
    fn test_paths_with_underscores() {
        let result = extract_paths("Update src/my_module.rs in tests/my_test.rs");
        assert_eq!(result, vec!["src/my_module.rs", "tests/my_test.rs"]);
    }

    #[test]
    fn test_deeply_nested_paths() {
        let result = extract_paths("Modify deeply/nested/path/to/src/main.rs");
        assert_eq!(result, vec!["deeply/nested/path/to/src/main.rs"]);
    }

    #[test]
    fn test_ignores_absolute_paths() {
        // Absolute paths starting with / should not match
        let result = extract_paths("Do not match /absolute/path/file.rs");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_ignores_urls() {
        // URLs should not match due to :// and domain patterns
        let result = extract_paths("See https://example.com/file.rs for details");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_mixed_valid_and_invalid() {
        let description = "Check src/main.rs at https://example.com/file.rs and tests/test.ts";
        let result = extract_paths(description);
        assert_eq!(result, vec!["src/main.rs", "tests/test.ts"]);
    }

    #[test]
    fn test_order_of_appearance() {
        let description = "Start with z/file.rs, then a/file.rs, then m/file.rs";
        let result = extract_paths(description);
        assert_eq!(result, vec!["z/file.rs", "a/file.rs", "m/file.rs"]);
    }

    #[test]
    fn test_yaml_and_json_extensions() {
        let result = extract_paths("Update config.yaml and settings.json");
        assert_eq!(result, vec!["config.yaml", "settings.json"]);
    }

    #[test]
    fn test_go_and_java_extensions() {
        let result = extract_paths("Implement src/main.go and src/Main.java");
        assert_eq!(result, vec!["src/main.go", "src/Main.java"]);
    }

    #[test]
    fn test_shell_script_extension() {
        let result = extract_paths("Run scripts/deploy.sh for deployment");
        assert_eq!(result, vec!["scripts/deploy.sh"]);
    }

    #[test]
    fn test_empty_string() {
        let result = extract_paths("");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_path_in_middle_of_sentence() {
        let result = extract_paths("The file src/config.rs needs updating because reasons");
        assert_eq!(result, vec!["src/config.rs"]);
    }

    #[test]
    fn test_path_at_start_of_string() {
        let result = extract_paths("src/main.rs is the entry point");
        assert_eq!(result, vec!["src/main.rs"]);
    }

    #[test]
    fn test_path_at_end_of_string() {
        let result = extract_paths("Please modify src/main.rs");
        assert_eq!(result, vec!["src/main.rs"]);
    }

    #[test]
    fn test_adjacent_paths() {
        let result = extract_paths("src/foo.rs src/bar.rs");
        assert_eq!(result, vec!["src/foo.rs", "src/bar.rs"]);
    }

    #[test]
    fn test_paths_with_numbers() {
        let result = extract_paths("Update src/v2/main.rs and test_1.rs");
        assert_eq!(result, vec!["src/v2/main.rs", "test_1.rs"]);
    }
}
