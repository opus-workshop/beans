use std::env;

use anyhow::Result;
use clap::Parser;

mod cli;

use cli::{Cli, Command, DepCommand};
use bn::commands::{cmd_init, cmd_create, cmd_list, cmd_show, cmd_update, cmd_close, cmd_reopen, cmd_delete, cmd_ready, cmd_blocked, cmd_dep_add, cmd_dep_remove, cmd_dep_list, cmd_dep_tree, cmd_dep_cycles, cmd_tree, cmd_graph, cmd_stats, cmd_doctor, cmd_sync, cmd_verify, cmd_claim, cmd_release};
use bn::discovery::find_beans_dir;
use bn::commands::create::CreateArgs;
use bn::util::validate_bean_id;

/// Validate a single bean ID or fail with a user-friendly error
fn validate_id(id: &str) -> Result<()> {
    validate_bean_id(id)
}

/// Validate a vector of bean IDs or fail with a user-friendly error
fn validate_ids(ids: &[String]) -> Result<()> {
    for id in ids {
        validate_bean_id(id)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { name } => {
            cmd_init(name)?;
        }
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
        } => {
            // Determine the title from either positional or --set-title flag
            let title = title.or(set_title);
            if title.is_none() {
                anyhow::bail!("bn create: title is required");
            }
            let title = title.unwrap();

            // Find the .beans directory
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;

            let args = CreateArgs {
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
            };

            cmd_create(&beans_dir, args)?;
        }
        Command::Show { id, json, short } => {
            validate_id(&id)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_show(&id, json, short, &beans_dir)?;
        }
        Command::List {
            status,
            priority,
            parent,
            label,
            assignee,
            all,
            json,
        } => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_list(
                status.as_deref(),
                priority,
                parent.as_deref(),
                label.as_deref(),
                assignee.as_deref(),
                all,
                json,
                &beans_dir,
            )?;
        }
        Command::Update {
            id,
            title,
            description,
            acceptance,
            notes,
            design,
            status,
            priority,
            assignee,
            add_label,
            remove_label,
        } => {
            validate_id(&id)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_update(
                &beans_dir,
                &id,
                title,
                description,
                acceptance,
                notes,
                design,
                status,
                priority,
                assignee,
                add_label,
                remove_label,
            )?;
        }
        Command::Close { ids, reason } => {
            validate_ids(&ids)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_close(&beans_dir, ids, reason)?;
        }
        Command::Verify { id } => {
            validate_id(&id)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            let passed = cmd_verify(&beans_dir, &id)?;
            if !passed {
                std::process::exit(1);
            }
        }
        Command::Claim { id, release, by } => {
            validate_id(&id)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            if release {
                cmd_release(&beans_dir, &id)?;
            } else {
                cmd_claim(&beans_dir, &id, by)?;
            }
        }
        Command::Reopen { id } => {
            validate_id(&id)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_reopen(&beans_dir, &id)?;
        }
        Command::Delete { id } => {
            validate_id(&id)?;
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_delete(&beans_dir, &id)?;
        }
        Command::Dep { command } => {
            // Validate IDs early depending on the subcommand
            match &command {
                DepCommand::Add { id, depends_on } => {
                    validate_id(id)?;
                    validate_id(depends_on)?;
                }
                DepCommand::Remove { id, depends_on } => {
                    validate_id(id)?;
                    validate_id(depends_on)?;
                }
                DepCommand::List { id } => {
                    validate_id(id)?;
                }
                DepCommand::Tree { id } => {
                    if let Some(id_val) = id {
                        validate_id(id_val)?;
                    }
                }
                DepCommand::Cycles => {
                    // No IDs to validate
                }
            }

            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            match command {
                DepCommand::Add { id, depends_on } => {
                    cmd_dep_add(&beans_dir, &id, &depends_on)?;
                }
                DepCommand::Remove { id, depends_on } => {
                    cmd_dep_remove(&beans_dir, &id, &depends_on)?;
                }
                DepCommand::List { id } => {
                    cmd_dep_list(&beans_dir, &id)?;
                }
                DepCommand::Tree { id } => {
                    cmd_dep_tree(&beans_dir, id.as_deref())?;
                }
                DepCommand::Cycles => {
                    cmd_dep_cycles(&beans_dir)?;
                }
            }
        },
        Command::Ready => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_ready(&beans_dir)?;
        }
        Command::Blocked => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_blocked(&beans_dir)?;
        }
        Command::Tree { id } => {
            if let Some(id_val) = &id {
                validate_id(id_val)?;
            }
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_tree(&beans_dir, id.as_deref())?;
        }
        Command::Graph { format } => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_graph(&beans_dir, &format)?;
        }
        Command::Sync => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_sync(&beans_dir)?;
        }
        Command::Stats => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_stats(&beans_dir)?;
        }
        Command::Doctor => {
            let cwd = env::current_dir()?;
            let beans_dir = find_beans_dir(&cwd)?;
            cmd_doctor(&beans_dir)?;
        }
    }

    Ok(())
}
