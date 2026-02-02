use std::path::Path;

/// Detected project type based on configuration files
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Ruby,
    Unknown,
}

impl ProjectType {
    /// Get the suggested verify command for this project type
    pub fn suggested_verify(&self) -> Option<&'static str> {
        match self {
            ProjectType::Rust => Some("cargo test"),
            ProjectType::Node => Some("npm test"),
            ProjectType::Python => Some("pytest"),
            ProjectType::Go => Some("go test ./..."),
            ProjectType::Ruby => Some("bundle exec rspec"),
            ProjectType::Unknown => None,
        }
    }

    /// Get all common verify commands for this project type
    pub fn common_verify_commands(&self) -> Vec<&'static str> {
        match self {
            ProjectType::Rust => vec![
                "cargo test",
                "cargo build",
                "cargo clippy",
                "cargo fmt --check",
            ],
            ProjectType::Node => vec![
                "npm test",
                "npm run build",
                "npm run lint",
                "npx tsc --noEmit",
            ],
            ProjectType::Python => vec![
                "pytest",
                "python -m pytest",
                "mypy .",
                "ruff check .",
            ],
            ProjectType::Go => vec![
                "go test ./...",
                "go build ./...",
                "go vet ./...",
            ],
            ProjectType::Ruby => vec![
                "bundle exec rspec",
                "bundle exec rake test",
                "bundle exec rubocop",
            ],
            ProjectType::Unknown => vec![],
        }
    }
}

/// Detect the project type from the project directory
pub fn detect_project_type(project_dir: &Path) -> ProjectType {
    // Check for Rust project
    if project_dir.join("Cargo.toml").exists() {
        return ProjectType::Rust;
    }

    // Check for Node project
    if project_dir.join("package.json").exists() {
        return ProjectType::Node;
    }

    // Check for Python project
    if project_dir.join("pyproject.toml").exists()
        || project_dir.join("setup.py").exists()
        || project_dir.join("requirements.txt").exists()
    {
        return ProjectType::Python;
    }

    // Check for Go project
    if project_dir.join("go.mod").exists() {
        return ProjectType::Go;
    }

    // Check for Ruby project
    if project_dir.join("Gemfile").exists() {
        return ProjectType::Ruby;
    }

    ProjectType::Unknown
}

/// Get suggested verify command for the project at the given directory
pub fn suggest_verify_command(project_dir: &Path) -> Option<&'static str> {
    let project_type = detect_project_type(project_dir);
    project_type.suggested_verify()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detect_rust_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Rust);
    }

    #[test]
    fn detect_node_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Node);
    }

    #[test]
    fn detect_python_project_pyproject() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Python);
    }

    #[test]
    fn detect_python_project_requirements() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask").unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Python);
    }

    #[test]
    fn detect_go_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("go.mod"), "module example").unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Go);
    }

    #[test]
    fn detect_ruby_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Gemfile"), "source 'https://rubygems.org'").unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Ruby);
    }

    #[test]
    fn detect_unknown_project() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_project_type(dir.path()), ProjectType::Unknown);
    }

    #[test]
    fn rust_verify_suggestions() {
        assert_eq!(ProjectType::Rust.suggested_verify(), Some("cargo test"));
        assert!(ProjectType::Rust.common_verify_commands().contains(&"cargo test"));
        assert!(ProjectType::Rust.common_verify_commands().contains(&"cargo build"));
    }

    #[test]
    fn node_verify_suggestions() {
        assert_eq!(ProjectType::Node.suggested_verify(), Some("npm test"));
        assert!(ProjectType::Node.common_verify_commands().contains(&"npm test"));
    }

    #[test]
    fn suggest_verify_returns_command() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(suggest_verify_command(dir.path()), Some("cargo test"));
    }

    #[test]
    fn unknown_has_no_suggestions() {
        assert_eq!(ProjectType::Unknown.suggested_verify(), None);
        assert!(ProjectType::Unknown.common_verify_commands().is_empty());
    }
}
