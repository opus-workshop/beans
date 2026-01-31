use regex::Regex;
use std::path::Path;
use std::fs;
use std::io;

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

/// Reads a file from disk and returns its contents as a string.
///
/// # Arguments
/// * `path` - The file path to read
///
/// # Returns
/// * `Ok(String)` - The file contents
/// * `Err` - If the file doesn't exist, is not readable, or appears to be binary
///
/// # Behavior
/// - Returns an error with descriptive message if file is missing
/// - Detects and skips binary files (checks for null bytes)
/// - Warns to stderr when skipping binary files
pub fn read_file(path: &Path) -> io::Result<String> {
    let content = fs::read_to_string(path)?;

    // Check for binary files by looking for null bytes
    if content.contains('\0') {
        eprintln!("Warning: Skipping binary file: {}", path.display());
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "File appears to be binary (contains null bytes)",
        ));
    }

    Ok(content)
}

/// Detects the programming language from a file extension.
///
/// Supports: rs, ts, tsx, py, go, java, json, yaml, toml, sh, md
fn detect_language(path: &str) -> &str {
    match path.split('.').last() {
        Some("rs") => "rust",
        Some("ts") => "typescript",
        Some("tsx") => "typescript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("json") => "json",
        Some("yaml") | Some("yml") => "yaml",
        Some("toml") => "toml",
        Some("sh") => "sh",
        Some("md") => "markdown",
        _ => "text",
    }
}

/// Formats a file's content as a markdown code block.
///
/// # Arguments
/// * `path` - The file path (used for display and language detection)
/// * `content` - The file contents
///
/// # Returns
/// A markdown-formatted string with the file header and code fence
///
/// # Format
/// ```
/// ## File: {path}
/// ```{lang}
/// {content}
/// ```
/// ```
pub fn format_file_block(path: &str, content: &str) -> String {
    let language = detect_language(path);
    format!("## File: {}\n```{}\n{}\n```\n", path, language, content)
}

/// Assembles context from multiple files into a single markdown document.
///
/// # Arguments
/// * `paths` - Vector of file paths to include
/// * `base_dir` - The base directory to resolve relative paths against
///
/// # Returns
/// * `Ok(String)` - Valid markdown containing all files
/// * `Err` - If no files could be read
///
/// # Behavior
/// - Reads each file using read_file()
/// - Formats each using format_file_block()
/// - Skips files that can't be read (with stderr warning)
/// - Continues even if some files fail
/// - Returns empty string if all files fail
pub fn assemble_context(paths: Vec<String>, base_dir: &Path) -> io::Result<String> {
    let mut output = String::new();

    for path_str in paths {
        let full_path = base_dir.join(&path_str);

        match read_file(&full_path) {
            Ok(content) => {
                output.push_str(&format_file_block(&path_str, &content));
                output.push('\n');
            }
            Err(e) => {
                eprintln!("Warning: Could not read file {}: {}", path_str, e);
            }
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{extract_paths, read_file, format_file_block, assemble_context, detect_language};
    use std::fs;
    use tempfile::TempDir;

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

    // Tests for read_file function
    #[test]
    fn test_read_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        let content = "fn main() {\n    println!(\"Hello\");\n}\n";
        fs::write(&test_file, content).unwrap();

        let result = read_file(&test_file).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_read_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let missing_file = temp_dir.path().join("nonexistent.rs");

        let result = read_file(&missing_file);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_file_binary() {
        let temp_dir = TempDir::new().unwrap();
        let binary_file = temp_dir.path().join("binary.bin");
        let binary_content = vec![0, 1, 2, 3, 0, 255];
        fs::write(&binary_file, binary_content).unwrap();

        let result = read_file(&binary_file);
        assert!(result.is_err());
    }

    // Tests for detect_language function
    #[test]
    fn test_detect_language_rust() {
        assert_eq!(detect_language("src/main.rs"), "rust");
    }

    #[test]
    fn test_detect_language_python() {
        assert_eq!(detect_language("script.py"), "python");
    }

    #[test]
    fn test_detect_language_json() {
        assert_eq!(detect_language("config.json"), "json");
    }

    #[test]
    fn test_detect_language_yaml() {
        assert_eq!(detect_language("config.yaml"), "yaml");
    }

    #[test]
    fn test_detect_language_yml() {
        assert_eq!(detect_language("config.yml"), "yaml");
    }

    #[test]
    fn test_detect_language_typescript() {
        assert_eq!(detect_language("index.ts"), "typescript");
    }

    #[test]
    fn test_detect_language_tsx() {
        assert_eq!(detect_language("component.tsx"), "typescript");
    }

    #[test]
    fn test_detect_language_go() {
        assert_eq!(detect_language("main.go"), "go");
    }

    #[test]
    fn test_detect_language_java() {
        assert_eq!(detect_language("Main.java"), "java");
    }

    #[test]
    fn test_detect_language_shell() {
        assert_eq!(detect_language("deploy.sh"), "sh");
    }

    #[test]
    fn test_detect_language_markdown() {
        assert_eq!(detect_language("README.md"), "markdown");
    }

    #[test]
    fn test_detect_language_toml() {
        assert_eq!(detect_language("Cargo.toml"), "toml");
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(detect_language("file.unknown"), "text");
    }

    // Tests for format_file_block function
    #[test]
    fn test_format_file_block_rust() {
        let path = "src/main.rs";
        let content = "fn main() {}";
        let result = format_file_block(path, content);

        assert!(result.contains("## File: src/main.rs"));
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("```"));
    }

    #[test]
    fn test_format_file_block_python() {
        let path = "script.py";
        let content = "print('hello')";
        let result = format_file_block(path, content);

        assert!(result.contains("## File: script.py"));
        assert!(result.contains("```python"));
        assert!(result.contains("print('hello')"));
    }

    #[test]
    fn test_format_file_block_json() {
        let path = "config.json";
        let content = r#"{"key": "value"}"#;
        let result = format_file_block(path, content);

        assert!(result.contains("## File: config.json"));
        assert!(result.contains("```json"));
        assert!(result.contains(r#"{"key": "value"}"#));
    }

    #[test]
    fn test_format_file_block_multiline() {
        let path = "src/lib.rs";
        let content = "pub fn foo() {\n    // comment\n    return 42;\n}";
        let result = format_file_block(path, content);

        assert!(result.contains("## File: src/lib.rs"));
        assert!(result.contains("```rust"));
        assert!(result.contains("pub fn foo()"));
        assert!(result.contains("// comment"));
        assert!(result.contains("return 42;"));
    }

    // Tests for assemble_context function
    #[test]
    fn test_assemble_context_single_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();

        let result = assemble_context(
            vec!["test.rs".to_string()],
            temp_dir.path(),
        ).unwrap();

        assert!(result.contains("## File: test.rs"));
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main() {}"));
    }

    #[test]
    fn test_assemble_context_multiple_files() {
        let temp_dir = TempDir::new().unwrap();

        let file1 = temp_dir.path().join("file1.rs");
        fs::write(&file1, "// file 1").unwrap();

        let file2 = temp_dir.path().join("file2.py");
        fs::write(&file2, "# file 2").unwrap();

        let result = assemble_context(
            vec!["file1.rs".to_string(), "file2.py".to_string()],
            temp_dir.path(),
        ).unwrap();

        assert!(result.contains("## File: file1.rs"));
        assert!(result.contains("```rust"));
        assert!(result.contains("// file 1"));

        assert!(result.contains("## File: file2.py"));
        assert!(result.contains("```python"));
        assert!(result.contains("# file 2"));
    }

    #[test]
    fn test_assemble_context_skips_missing_files() {
        let temp_dir = TempDir::new().unwrap();

        let existing = temp_dir.path().join("exists.rs");
        fs::write(&existing, "fn hello() {}").unwrap();

        let result = assemble_context(
            vec!["exists.rs".to_string(), "missing.rs".to_string()],
            temp_dir.path(),
        ).unwrap();

        // Should contain existing file
        assert!(result.contains("## File: exists.rs"));
        assert!(result.contains("fn hello() {}"));

        // Should not contain missing file
        assert!(!result.contains("missing.rs"));
    }

    #[test]
    fn test_assemble_context_empty_paths() {
        let temp_dir = TempDir::new().unwrap();

        let result = assemble_context(
            vec![],
            temp_dir.path(),
        ).unwrap();

        assert_eq!(result.trim(), "");
    }

    #[test]
    fn test_assemble_context_preserves_content() {
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("test.json");
        let content = r#"{
  "key": "value",
  "nested": {
    "inner": 42
  }
}"#;
        fs::write(&test_file, content).unwrap();

        let result = assemble_context(
            vec!["test.json".to_string()],
            temp_dir.path(),
        ).unwrap();

        assert!(result.contains(content));
    }
}
