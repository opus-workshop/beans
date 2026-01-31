use std::env;
use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use clap::Parser;

use bn::bean::Bean;
use bn::ctx_assembler::{extract_paths, assemble_context};
use bn::discovery::find_beans_dir;

#[derive(Parser)]
#[command(
    name = "bctx",
    about = "Assemble context for a bean from its description and referenced files",
    version
)]
struct Args {
    /// Bean ID or file path to assemble context for
    /// Examples: `bctx 1`, `bctx 14`, `bctx .beans/14-slug.md`
    bean_id_or_path: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let input = &args.bean_id_or_path;

    // Determine if input is a file path or bean ID
    let bean_path = if input.contains('/') || input.contains('.') && input.ends_with(".md") {
        // Looks like a file path
        PathBuf::from(input)
    } else {
        // Treat as bean ID - find the bean file
        let beans_dir = find_beans_dir(&env::current_dir()?)
            .context("Could not find .beans/ directory")?;
        bn::discovery::find_bean_file(&beans_dir, input)
            .context(format!("Could not find bean with ID: {}", input))?
    };

    // Read and parse the bean
    let bean = Bean::from_file(&bean_path)
        .context(format!("Failed to parse bean from: {}", bean_path.display()))?;

    // Get the base directory for resolving relative paths
    // Always resolve from the project root (where .beans/ is located)
    let base_dir_owned = if input.contains('/') || input.contains('.') && input.ends_with(".md") {
        // File path was provided - need to find project root
        let beans_dir = find_beans_dir(&env::current_dir()?)
            .context("Could not find .beans/ directory")?;
        beans_dir.parent().ok_or_else(|| {
            anyhow::anyhow!("Invalid .beans/ path: {}", beans_dir.display())
        })?.to_path_buf()
    } else {
        // Bean ID was provided - use current directory as project root
        env::current_dir()
            .context("Failed to get current directory")?
    };

    // Extract file paths from the bean description
    let description = bean.description.as_deref().unwrap_or("");
    let paths = extract_paths(description);

    // Assemble the context from the extracted files
    let context = assemble_context(paths, &base_dir_owned)
        .context("Failed to assemble context")?;

    // Output the assembled markdown to stdout
    print!("{}", context);

    Ok(())
}
