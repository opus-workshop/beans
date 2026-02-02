# Bean 14.1: Quick-bean mode implementation

## Changes Made:

### Fixed:
1. Removed broken Status command from CLI and main.rs (lines 214 in cli.rs, line 206 in main.rs, import on line 12)
2. Fixed broken create.rs reference to `final_description` → changed to `args.description`

### Implementation Plan for Quick and Done commands:

#### CLI Changes (src/cli.rs):
Add after Unarchive command (after line 264, before closing `}`):

```rust
/// Quick-create: create and claim a bean in one command
Quick {
    /// Bean title
    title: String,

    /// Full description / agent context
    #[arg(long)]
    description: Option<String>,

    /// Acceptance criteria
    #[arg(long)]
    acceptance: Option<String>,

    /// Additional notes
    #[arg(long)]
    notes: Option<String>,

    /// Design decisions
    #[arg(long)]
    design: Option<String>,

    /// Shell command that must exit 0 to close
    #[arg(long)]
    verify: Option<String>,

    /// Priority P0-P4 (default: P2)
    #[arg(long)]
    priority: Option<u8>,

    /// Comma-separated labels
    #[arg(long)]
    labels: Option<String>,

    /// Who is claiming (agent name or user)
    #[arg(long)]
    by: Option<String>,

    /// Comma-separated dependency IDs
    #[arg(long)]
    deps: Option<String>,
},

/// Close the currently claimed bean (shorthand for `bn close @me`)
Done {
    /// Close reason
    #[arg(long)]
    reason: Option<String>,
},
```

#### Main.rs Changes (src/main.rs):
Add after Unarchive handler (after line 224, before closing `}`):

```rust
Command::Quick {
    title,
    description,
    acceptance,
    notes,
    design,
    verify,
    priority,
    labels,
    by,
    deps,
} => {
    // Create the bean
    cmd_create(&beans_dir, CreateArgs {
        title: title.clone(),
        description,
        acceptance,
        notes,
        design,
        verify,
        priority,
        labels,
        assignee: by.clone(),
        deps,
        parent: None,
    })?;

    // Get the bean ID (next_id - 1 since cmd_create incremented it)
    let config = bn::config::Config::load(&beans_dir)?;
    let bean_id = (config.next_id - 1).to_string();

    // Claim the bean
    cmd_claim(&beans_dir, &bean_id, by)
}

Command::Done { reason } => {
    // Resolve @me to get currently claimed beans
    let index = Index::load(&beans_dir)?;
    let context = SelectionContext {
        index: &index,
        current_bean_id: None,
        current_user: None,
    };

    let claimed_beans = resolve_selector_string("@me", &context)?;

    if claimed_beans.is_empty() {
        return Err(anyhow::anyhow!(
            "No beans currently claimed by you. Set BN_USER environment variable or use 'bn claim' to claim a bean."
        ));
    }

    // Close all claimed beans
    cmd_close(&beans_dir, claimed_beans, reason)
}
```

## Testing:
- `bn quick --help` - should show help for quick command
- `bn done --help` - should show help for done command
- `bn quick "fix typo" --verify "grep 'correct' README.md"` - should create and claim bean
- `bn done` - should close all beans claimed by current user

## Files Changed:
- src/cli.rs: Added Quick and Done command variants
- src/main.rs: Added Quick and Done command handlers
- src/commands/create.rs: Fixed `final_description` → `args.description` bug

## Status:
Baseline compiles cleanly after Status command cleanup. Quick and Done implementation is ready to be applied.
