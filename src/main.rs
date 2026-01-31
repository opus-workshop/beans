use std::env;

use anyhow::Result;
use clap::Parser;

mod cli;

use cli::{Cli, Command, DepCommand};
use bn::commands::{
    cmd_claim, cmd_close, cmd_create, cmd_delete, cmd_dep_add, cmd_dep_cycles,
    cmd_dep_list, cmd_dep_remove, cmd_dep_tree, cmd_doctor, cmd_graph, cmd_init,
    cmd_list, cmd_ready, cmd_blocked, cmd_release, cmd_reopen, cmd_show, cmd_stats,
    cmd_sync, cmd_tree, cmd_trust, cmd_unarchive, cmd_update, cmd_verify,
};
use bn::commands::create::CreateArgs;
use bn::discovery::find_beans_dir;
use bn::util::validate_bean_id;

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
        } => {
            let title = title
                .or(set_title)
                .ok_or_else(|| anyhow::anyhow!("bn create: title is required"))?;

            cmd_create(&beans_dir, CreateArgs {
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
            })
        }

        Command::Show { id, json, short } => {
            validate_bean_id(&id)?;
            cmd_show(&id, json, short, &beans_dir)
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
            cmd_update(
                &beans_dir, &id, title, description, acceptance, notes, design,
                status, priority, assignee, add_label, remove_label,
            )
        }

        Command::Close { ids, reason } => {
            for id in &ids {
                validate_bean_id(id)?;
            }
            cmd_close(&beans_dir, ids, reason)
        }

        Command::Verify { id } => {
            validate_bean_id(&id)?;
            let passed = cmd_verify(&beans_dir, &id)?;
            if !passed {
                std::process::exit(1);
            }
            Ok(())
        }

        Command::Claim { id, release, by } => {
            validate_bean_id(&id)?;
            if release {
                cmd_release(&beans_dir, &id)
            } else {
                cmd_claim(&beans_dir, &id, by)
            }
        }

        Command::Reopen { id } => {
            validate_bean_id(&id)?;
            cmd_reopen(&beans_dir, &id)
        }

        Command::Delete { id } => {
            validate_bean_id(&id)?;
            cmd_delete(&beans_dir, &id)
        }

        Command::Dep { command } => match command {
            DepCommand::Add { id, depends_on } => {
                validate_bean_id(&id)?;
                validate_bean_id(&depends_on)?;
                cmd_dep_add(&beans_dir, &id, &depends_on)
            }
            DepCommand::Remove { id, depends_on } => {
                validate_bean_id(&id)?;
                validate_bean_id(&depends_on)?;
                cmd_dep_remove(&beans_dir, &id, &depends_on)
            }
            DepCommand::List { id } => {
                validate_bean_id(&id)?;
                cmd_dep_list(&beans_dir, &id)
            }
            DepCommand::Tree { id } => {
                if let Some(ref id_val) = id {
                    validate_bean_id(id_val)?;
                }
                cmd_dep_tree(&beans_dir, id.as_deref())
            }
            DepCommand::Cycles => cmd_dep_cycles(&beans_dir),
        },

        Command::Ready => cmd_ready(&beans_dir),
        Command::Blocked => cmd_blocked(&beans_dir),

        Command::Tree { id } => {
            if let Some(ref id_val) = id {
                validate_bean_id(id_val)?;
            }
            cmd_tree(&beans_dir, id.as_deref())
        }

        Command::Graph { format } => cmd_graph(&beans_dir, &format),
        Command::Sync => cmd_sync(&beans_dir),
        Command::Stats => cmd_stats(&beans_dir),
        Command::Doctor => cmd_doctor(&beans_dir),
        Command::Trust { revoke, check } => cmd_trust(&beans_dir, revoke, check),

        Command::Unarchive { id } => {
            validate_bean_id(&id)?;
            cmd_unarchive(&beans_dir, &id)
        }
    }
}
