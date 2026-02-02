use std::path::Path;

use anyhow::{Context, Result};

use crate::bean::Bean;
use crate::ctx_assembler::{extract_paths, assemble_context};
use crate::discovery::find_bean_file;

/// Assemble context for a bean from its description and referenced files.
///
/// Extracts file paths mentioned in the bean's description and outputs
/// the content of those files in a markdown format suitable for LLM prompts.
pub fn cmd_context(beans_dir: &Path, id: &str) -> Result<()> {
    let bean_path = find_bean_file(beans_dir, id)
        .context(format!("Could not find bean with ID: {}", id))?;

    let bean = Bean::from_file(&bean_path)
        .context(format!("Failed to parse bean from: {}", bean_path.display()))?;

    // Get the project directory (parent of beans_dir which is .beans)
    let project_dir = beans_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid .beans/ path: {}", beans_dir.display()))?;

    // Extract file paths from the bean description
    let description = bean.description.as_deref().unwrap_or("");
    let paths = extract_paths(description);

    if paths.is_empty() {
        eprintln!("No file paths found in bean description.");
        eprintln!("Tip: Reference files in description with paths like 'src/foo.rs' or 'src/commands/bar.rs'");
        return Ok(());
    }

    // Assemble the context from the extracted files
    let context = assemble_context(paths, project_dir)
        .context("Failed to assemble context")?;

    // Output the assembled markdown to stdout
    print!("{}", context);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();
        (dir, beans_dir)
    }

    #[test]
    fn context_with_no_paths_in_description() {
        let (dir, beans_dir) = setup_test_env();

        // Create a bean with no file paths in description
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.description = Some("A description with no file paths".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        // Should succeed but print a tip
        let result = cmd_context(&beans_dir, "1");
        assert!(result.is_ok());
    }

    #[test]
    fn context_with_paths_in_description() {
        let (dir, beans_dir) = setup_test_env();
        let project_dir = dir.path();

        // Create a source file
        let src_dir = project_dir.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("foo.rs"), "fn main() {}").unwrap();

        // Create a bean referencing the file
        let mut bean = crate::bean::Bean::new("1", "Test bean");
        bean.description = Some("Check src/foo.rs for implementation".to_string());
        let slug = crate::util::title_to_slug(&bean.title);
        let bean_path = beans_dir.join(format!("1-{}.md", slug));
        bean.to_file(&bean_path).unwrap();

        let result = cmd_context(&beans_dir, "1");
        assert!(result.is_ok());
    }

    #[test]
    fn context_bean_not_found() {
        let (_dir, beans_dir) = setup_test_env();

        let result = cmd_context(&beans_dir, "999");
        assert!(result.is_err());
    }
}
