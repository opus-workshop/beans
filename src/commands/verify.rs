use std::path::Path;
use std::process::Command as ShellCommand;

use anyhow::{anyhow, Context, Result};

use crate::bean::Bean;

/// Run the verify command for a bean without closing it.
///
/// Returns `Ok(true)` if the command exits 0, `Ok(false)` if non-zero.
/// If no verify command is set, prints a message and returns `Ok(true)`.
pub fn cmd_verify(beans_dir: &Path, id: &str) -> Result<bool> {
    let bean_path = beans_dir.join(format!("{}.yaml", id));
    if !bean_path.exists() {
        return Err(anyhow!("Bean not found: {}", id));
    }

    let bean = Bean::from_file(&bean_path)
        .with_context(|| format!("Failed to load bean: {}", id))?;

    let verify_cmd = match &bean.verify {
        Some(cmd) => cmd.clone(),
        None => {
            println!("no verify command set for bean {}", id);
            return Ok(true);
        }
    };

    // Run in the project root (parent of .beans/)
    let project_root = beans_dir
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine project root from beans dir"))?;

    println!("Running: {}", verify_cmd);

    let status = ShellCommand::new("sh")
        .args(["-c", &verify_cmd])
        .current_dir(project_root)
        .status()
        .with_context(|| format!("Failed to execute verify command: {}", verify_cmd))?;

    if status.success() {
        println!("Verify passed for bean {}", id);
        Ok(true)
    } else {
        println!("Verify failed for bean {}", id);
        Ok(false)
    }
}
