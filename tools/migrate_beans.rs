use std::fs;
use std::path::Path;
use std::process::Command;
use serde_yaml::{Value, Mapping};

fn main() -> anyhow::Result<()> {
    println!("Starting bean migration from YAML to Markdown format...\n");

    // Find all deleted .beans/*.yaml files in git history
    let yaml_beans = find_yaml_beans_in_git()?;
    println!("Found {} YAML beans in git history\n", yaml_beans.len());

    // Create .beans directory if it doesn't exist
    let beans_dir = Path::new(".beans");
    fs::create_dir_all(beans_dir)?;

    let mut migrated = 0;
    let mut failed = 0;

    for bean_id in yaml_beans {
        match migrate_bean(&bean_id) {
            Ok(filename) => {
                println!("✓ Bean {} -> {}", bean_id, filename);
                migrated += 1;
            }
            Err(e) => {
                println!("✗ Bean {} failed: {}", bean_id, e);
                failed += 1;
            }
        }
    }

    println!("\n--- Migration Summary ---");
    println!("Migrated: {}", migrated);
    println!("Failed: {}", failed);
    println!("Total: {}", migrated + failed);

    Ok(())
}

/// Find all YAML bean files that were in git history
fn find_yaml_beans_in_git() -> anyhow::Result<Vec<String>> {
    // Get all .beans/*.yaml files ever in git
    let output = Command::new("git")
        .args(&[
            "log",
            "--all",
            "--name-only",
            "--pretty=",
            "--",
            ".beans/*.yaml",
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("git log failed");
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut beans: Vec<String> = stdout
        .lines()
        .filter(|line| line.ends_with(".yaml"))
        .filter_map(|line| {
            // Extract ID from .beans/ID.yaml format
            let path = Path::new(line);
            path.file_stem()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Deduplicate and sort numerically
    beans.sort_unstable();
    beans.dedup();

    Ok(beans)
}

/// Migrate a single bean from YAML to Markdown format
fn migrate_bean(bean_id: &str) -> anyhow::Result<String> {
    // Reconstruct the bean from git
    let yaml_content = reconstruct_bean_from_git(bean_id)?;

    // Parse YAML
    let bean_data: Value = serde_yaml::from_str(&yaml_content)?;

    let mapping = bean_data
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("Bean {} is not a valid YAML mapping", bean_id))?;

    // Get title for slug generation
    let title = mapping
        .get(&Value::String("title".to_string()))
        .and_then(|v| v.as_str())
        .unwrap_or(bean_id);

    let slug = generate_slug(title);

    // Extract description (becomes the markdown body)
    let description = mapping
        .get(&Value::String("description".to_string()))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    // Build frontmatter (all fields except description)
    let mut frontmatter = Mapping::new();
    for (key, value) in mapping.iter() {
        if let Value::String(key_str) = key {
            if key_str != "description" {
                frontmatter.insert(key.clone(), value.clone());
            }
        }
    }

    // Generate markdown content with YAML frontmatter
    let markdown_content = format_markdown_with_frontmatter(&frontmatter, &description)?;

    // Write to file: {id}-{slug}.md
    let filename = format!("{}-{}.md", bean_id, slug);
    let filepath = Path::new(".beans").join(&filename);

    fs::write(&filepath, &markdown_content)?;

    Ok(filename)
}

/// Reconstruct bean YAML from git history
fn reconstruct_bean_from_git(bean_id: &str) -> anyhow::Result<String> {
    let yaml_path = format!(".beans/{}.yaml", bean_id);

    // Search through git history to find this file
    let log_output = Command::new("git")
        .args(&["log", "--all", "--oneline", "--", &yaml_path])
        .output()?;

    if !log_output.status.success() {
        anyhow::bail!("Could not find bean {} in git history", bean_id);
    }

    let log = String::from_utf8(log_output.stdout)?;

    // Try the most recent commit first
    for line in log.lines() {
        let commit = line.split_whitespace().next().unwrap_or("HEAD");
        let output = Command::new("git")
            .args(&["show", &format!("{}:{}", commit, yaml_path)])
            .output()?;

        if output.status.success() {
            return Ok(String::from_utf8(output.stdout)?);
        }
    }

    anyhow::bail!("Could not retrieve bean {} from git", bean_id)
}

/// Generate a kebab-case slug from a title
fn generate_slug(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_string()
            } else if c.is_whitespace() || c == '_' {
                "-".to_string()
            } else if "./:".contains(c) {
                String::new() // Remove these characters
            } else {
                "-".to_string()
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Format markdown with YAML frontmatter
fn format_markdown_with_frontmatter(
    frontmatter: &Mapping,
    body: &str,
) -> anyhow::Result<String> {
    let mut output = String::from("---\n");

    // Write frontmatter YAML
    for (key, value) in frontmatter.iter() {
        let key_str = key
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Non-string key in frontmatter"))?;

        match value {
            Value::String(s) => {
                // Check if value needs quoting
                if s.contains('\n') || s.contains(':') {
                    // Use literal block scalar for multiline values
                    output.push_str(&format!("{}: |-\n", key_str));
                    for line in s.lines() {
                        output.push_str(&format!("  {}\n", line));
                    }
                } else {
                    output.push_str(&format!("{}: {}\n", key_str, s));
                }
            }
            Value::Number(n) => {
                output.push_str(&format!("{}: {}\n", key_str, n));
            }
            Value::Bool(b) => {
                output.push_str(&format!("{}: {}\n", key_str, b));
            }
            Value::Null => {
                output.push_str(&format!("{}: null\n", key_str));
            }
            _ => {
                // For complex types, use serde_yaml to serialize
                let val_str = serde_yaml::to_string(value)?;
                output.push_str(&format!("{}: {}", key_str, val_str));
            }
        }
    }

    output.push_str("---\n\n");
    output.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}
