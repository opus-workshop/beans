use std::env;

use anyhow::Result;
use clap::Parser;

mod cli;

use cli::{Cli, Command, ConfigCommand, DepCommand};
use bn::commands::{
    cmd_adopt, cmd_claim, cmd_close, cmd_config_get, cmd_config_set, cmd_context, cmd_create,
    cmd_delete, cmd_dep_add, cmd_dep_cycles, cmd_dep_list, cmd_dep_remove, cmd_dep_tree, cmd_doctor,
    cmd_edit, cmd_graph, cmd_init, cmd_list, cmd_quick, cmd_ready, cmd_blocked, cmd_release,
    cmd_reopen, cmd_resolve, cmd_show, cmd_stats, cmd_status, cmd_sync, cmd_tidy, cmd_tree,
    cmd_trust, cmd_unarchive, cmd_update, cmd_verify,
};
use bn::commands::create::CreateArgs;
use bn::commands::quick::QuickArgs;
use bn::discovery::find_beans_dir;
use bn::index::Index;
use bn::selector::{SelectionContext, resolve_selector_string};
use bn::util::validate_bean_id;

// Helper to resolve a single bean ID (handles selectors)
fn resolve_bean_id(id: &str, beans_dir: &std::path::Path) -> Result<String> {
    let index = Index::load(beans_dir)?;
    let context = SelectionContext {
        index: &index,
        current_bean_id: None,
        current_user: None,
    };
    let resolved = resolve_selector_string(id, &context)?;
    resolved.into_iter().next()
        .ok_or_else(|| anyhow::anyhow!("Selector '{}' resolved to no beans", id))
}

// Helper to resolve multiple bean IDs (handles selectors and expands @blocked)
fn resolve_bean_ids(ids: Vec<String>, beans_dir: &std::path::Path) -> Result<Vec<String>> {
    let index = Index::load(beans_dir)?;
    let context = SelectionContext {
        index: &index,
        current_bean_id: None,
        current_user: None,
    };
    
    let mut resolved_ids = Vec::new();
    for id in ids {
        let resolved = resolve_selector_string(&id, &context)?;
        resolved_ids.extend(resolved);
    }
    Ok(resolved_ids)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Init is special - doesn't need beans_dir
    if let Command::Init { name } = cli.command {
        return cmd_init(None, name);
    }

    // All other commands need beans_dir
    let beans_dir = find_beans_dir(&env::current_dir()?)?;

    match cli.command {
        Command::Init { .. } => unreachable!(),

        Command::Create {
            title,
            set_title,
            description,
            acceptance,
            notes,
            design,
            verify,
            parent,
            priority,
            labels,
            assignee,
            deps,
            produces,
            requires,
            pass_ok,
            claim,
            by,
            run,
        } => {
            let title = title
                .or(set_title)
                .ok_or_else(|| anyhow::anyhow!("bn create: title is required"))?;

            // --run requires --verify
            if run && verify.is_none() {
                anyhow::bail!(
                    "--run requires --verify\n\n\
                     Cannot spawn an agent without a test. If you can't write a verify command,\n\
                     this is a GOAL that needs decomposition, not a SPEC ready for implementation."
                );
            }

            let bean_id = cmd_create(&beans_dir, CreateArgs {
                title,
                description,
                acceptance,
                notes,
                design,
                verify,
                priority,
                labels,
                assignee,
                deps,
                parent,
                produces,
                requires,
                pass_ok,
                claim,
                by,
            })?;

            // --run: spawn a deli agent for the new bean
            if run {
                println!("Spawning agent via deli...");
                let status = std::process::Command::new("deli")
                    .args(["spawn", &bean_id])
                    .status();
                match status {
                    Ok(s) if s.success() => {}
                    Ok(s) => eprintln!("deli spawn exited with code {}", s.code().unwrap_or(-1)),
                    Err(e) => eprintln!("Failed to run deli spawn: {}", e),
                }
            }

            Ok(())
        }

        Command::Show { id, json, short } => {
            // Skip validation for selectors (start with @)
            if !id.starts_with('@') {
                validate_bean_id(&id)?;
            }
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_show(&resolved_id, json, short, &beans_dir)
        }

        Command::Edit { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_edit(&beans_dir, &resolved_id)
        }

        Command::List { status, priority, parent, label, assignee, all, json } => {
            cmd_list(
                status.as_deref(),
                priority,
                parent.as_deref(),
                label.as_deref(),
                assignee.as_deref(),
                all,
                json,
                &beans_dir,
            )
        }

        Command::Update {
            id, title, description, acceptance, notes, design,
            status, priority, assignee, add_label, remove_label,
        } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_update(
                &beans_dir, &resolved_id, title, description, acceptance, notes, design,
                status, priority, assignee, add_label, remove_label,
            )
        }

        Command::Close { ids, reason, force } => {
            for id in &ids {
                validate_bean_id(id)?;
            }
            let resolved_ids = resolve_bean_ids(ids, &beans_dir)?;
            cmd_close(&beans_dir, resolved_ids, reason, force)
        }

        Command::Verify { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            let passed = cmd_verify(&beans_dir, &resolved_id)?;
            if !passed {
                std::process::exit(1);
            }
            Ok(())
        }

        Command::Claim { id, release, by } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            if release {
                cmd_release(&beans_dir, &resolved_id)
            } else {
                cmd_claim(&beans_dir, &resolved_id, by)
            }
        }

        Command::Reopen { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_reopen(&beans_dir, &resolved_id)
        }

        Command::Delete { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_delete(&beans_dir, &resolved_id)
        }

        Command::Dep { command } => match command {
            DepCommand::Add { id, depends_on } => {
                validate_bean_id(&id)?;
                validate_bean_id(&depends_on)?;
                let resolved_id = resolve_bean_id(&id, &beans_dir)?;
                let resolved_depends_on = resolve_bean_id(&depends_on, &beans_dir)?;
                cmd_dep_add(&beans_dir, &resolved_id, &resolved_depends_on)
            }
            DepCommand::Remove { id, depends_on } => {
                validate_bean_id(&id)?;
                validate_bean_id(&depends_on)?;
                let resolved_id = resolve_bean_id(&id, &beans_dir)?;
                let resolved_depends_on = resolve_bean_id(&depends_on, &beans_dir)?;
                cmd_dep_remove(&beans_dir, &resolved_id, &resolved_depends_on)
            }
            DepCommand::List { id } => {
                validate_bean_id(&id)?;
                let resolved_id = resolve_bean_id(&id, &beans_dir)?;
                cmd_dep_list(&beans_dir, &resolved_id)
            }
            DepCommand::Tree { id } => {
                if let Some(ref id_val) = id {
                    validate_bean_id(id_val)?;
                }
                cmd_dep_tree(&beans_dir, id.as_deref())
            }
            DepCommand::Cycles => cmd_dep_cycles(&beans_dir),
        },

        Command::Ready { json } => cmd_ready(json, &beans_dir),
        Command::Blocked { json } => cmd_blocked(json, &beans_dir),
        Command::Status { json } => cmd_status(json, &beans_dir),

        Command::Context { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_context(&beans_dir, &resolved_id)
        }

        Command::Tree { id } => {
            if let Some(ref id_val) = id {
                validate_bean_id(id_val)?;
            }
            cmd_tree(&beans_dir, id.as_deref())
        }
        Command::Graph { format } => cmd_graph(&beans_dir, &format),
        Command::Sync => cmd_sync(&beans_dir),
        Command::Tidy { dry_run } => cmd_tidy(&beans_dir, dry_run),
        Command::Stats => cmd_stats(&beans_dir),
        Command::Doctor { fix } => cmd_doctor(&beans_dir, fix),
        Command::Trust { revoke, check } => cmd_trust(&beans_dir, revoke, check),

        Command::Unarchive { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_unarchive(&beans_dir, &resolved_id)
        }

        Command::Quick { title, description, acceptance, notes, verify, priority, by, produces, requires, pass_ok } => {
            cmd_quick(&beans_dir, QuickArgs {
                title,
                description,
                acceptance,
                notes,
                verify,
                priority,
                by,
                produces,
                requires,
                pass_ok,
            })
        }

        Command::Adopt { parent, children } => {
            validate_bean_id(&parent)?;
            for child in &children {
                validate_bean_id(child)?;
            }
            let resolved_parent = resolve_bean_id(&parent, &beans_dir)?;
            let resolved_children = resolve_bean_ids(children, &beans_dir)?;
            cmd_adopt(&beans_dir, &resolved_parent, &resolved_children).map(|_| ())
        }

        Command::Resolve { id, field, choice } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_resolve(&beans_dir, &resolved_id, &field, choice)
        }

        Command::Config { command } => match command {
            ConfigCommand::Get { key } => cmd_config_get(&beans_dir, &key),
            ConfigCommand::Set { key, value } => cmd_config_set(&beans_dir, &key, &value),
        },
    }
}
