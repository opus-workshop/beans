use std::env;

use std::io::IsTerminal;

use anyhow::Result;
use clap::Parser;

mod cli;

use bn::commands::create::CreateArgs;
use bn::commands::plan::PlanArgs;
use bn::commands::quick::QuickArgs;
use bn::commands::{
    cmd_adopt, cmd_agents, cmd_blocked, cmd_claim, cmd_close, cmd_config_get, cmd_config_set,
    cmd_context, cmd_create, cmd_delete, cmd_dep_add, cmd_dep_cycles, cmd_dep_list, cmd_dep_remove,
    cmd_dep_tree, cmd_doctor, cmd_edit, cmd_graph, cmd_init, cmd_list, cmd_logs, cmd_plan,
    cmd_quick, cmd_ready, cmd_release, cmd_reopen, cmd_resolve, cmd_run, cmd_show, cmd_stats,
    cmd_status, cmd_sync, cmd_tidy, cmd_tree, cmd_trust, cmd_unarchive, cmd_update, cmd_verify,
};
use bn::discovery::find_beans_dir;
use bn::index::Index;
use bn::selector::{resolve_selector_string, SelectionContext};
use bn::util::validate_bean_id;
use cli::{Cli, Command, ConfigCommand, DepCommand};

// Helper to resolve a single bean ID (handles selectors)
fn resolve_bean_id(id: &str, beans_dir: &std::path::Path) -> Result<String> {
    let index = Index::load(beans_dir)?;
    let context = SelectionContext {
        index: &index,
        current_bean_id: None,
        current_user: None,
    };
    let resolved = resolve_selector_string(id, &context)?;
    resolved
        .into_iter()
        .next()
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
    if let Command::Init {
        name,
        agent,
        run,
        plan,
        setup,
        no_agent,
    } = cli.command
    {
        return cmd_init(
            None,
            bn::commands::init::InitArgs {
                project_name: name,
                agent,
                run,
                plan,
                setup,
                no_agent,
            },
        );
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
            on_fail,
            pass_ok,
            claim,
            by,
            run,
            interactive,
            json,
        } => {
            // Resolve "-" values from stdin
            use bn::commands::stdin::resolve_stdin_opt;
            let description = resolve_stdin_opt(description)?;
            let acceptance = resolve_stdin_opt(acceptance)?;
            let notes = resolve_stdin_opt(notes)?;

            let resolved_title = title.or(set_title);

            // Determine if we should enter interactive mode:
            // 1. Explicit -i / --interactive flag, OR
            // 2. No title provided + stderr is a TTY + not --run
            let use_interactive = interactive
                || (resolved_title.is_none() && !run && std::io::stderr().is_terminal());

            let (bean_id, run_after) = if use_interactive {
                use bn::commands::interactive::{interactive_create, Prefill};

                // Pass any CLI flags as prefill — they skip prompts
                let prefill = Prefill {
                    title: resolved_title,
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
                    pass_ok: if pass_ok { Some(true) } else { None },
                };

                let args = interactive_create(&beans_dir, prefill)?;
                let id = cmd_create(&beans_dir, args)?;
                (id, false)
            } else {
                let title = resolved_title
                    .ok_or_else(|| anyhow::anyhow!("bn create: title is required"))?;

                // --run requires --verify
                if run && verify.is_none() {
                    anyhow::bail!(
                        "--run requires --verify\n\n\
                         Cannot spawn an agent without a test. If you can't write a verify command,\n\
                         this is a GOAL that needs decomposition, not a SPEC ready for implementation."
                    );
                }

                // Parse --on-fail flag
                let on_fail = on_fail
                    .map(|s| bn::commands::create::parse_on_fail(&s))
                    .transpose()?;

                let id = cmd_create(
                    &beans_dir,
                    CreateArgs {
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
                        on_fail,
                        pass_ok,
                        claim,
                        by,
                    },
                )?;
                (id, run)
            };
            let run = run_after;

            // JSON output for piping (human messages go to stderr)
            if json {
                let bean_path = bn::discovery::find_bean_file(&beans_dir, &bean_id)?;
                let bean = bn::bean::Bean::from_file(&bean_path)?;
                println!("{}", serde_json::to_string(&bean)?);
            }

            // --run: spawn an agent for the new bean using configured command
            if run {
                use bn::config::Config;
                let config = Config::load_with_extends(&beans_dir)?;
                match &config.run {
                    Some(template) => {
                        let cmd = template.replace("{id}", &bean_id);
                        eprintln!("Spawning: {}", cmd);
                        let status = std::process::Command::new("sh").args(["-c", &cmd]).status();
                        match status {
                            Ok(s) if s.success() => {}
                            Ok(s) => {
                                eprintln!("Run command exited with code {}", s.code().unwrap_or(-1))
                            }
                            Err(e) => eprintln!("Failed to run command: {}", e),
                        }
                    }
                    None => {
                        anyhow::bail!(
                            "--run requires a configured run command.\n\n\
                             Set it with: bn config set run \"<command>\"\n\n\
                             The command template uses {{id}} as a placeholder for the bean ID.\n\n\
                             Examples:\n  \
                               bn config set run \"deli spawn {{id}}\"\n  \
                               bn config set run \"claude -p 'implement bean {{id}} and run bn close {{id}}'\""
                        );
                    }
                }
            }

            Ok(())
        }

        Command::Show {
            id,
            json,
            short,
            history,
        } => {
            // Skip validation for selectors (start with @)
            if !id.starts_with('@') {
                validate_bean_id(&id)?;
            }
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_show(&resolved_id, json, short, history, &beans_dir)
        }

        Command::Edit { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_edit(&beans_dir, &resolved_id)
        }

        Command::List {
            status,
            priority,
            parent,
            label,
            assignee,
            all,
            json,
            ids,
            format,
        } => cmd_list(
            status.as_deref(),
            priority,
            parent.as_deref(),
            label.as_deref(),
            assignee.as_deref(),
            all,
            json,
            ids,
            format.as_deref(),
            &beans_dir,
        ),

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
            use bn::commands::stdin::resolve_stdin_opt;
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;

            // Resolve "-" values from stdin
            let description = resolve_stdin_opt(description)?;
            let notes = resolve_stdin_opt(notes)?;
            let acceptance = resolve_stdin_opt(acceptance)?;

            cmd_update(
                &beans_dir,
                &resolved_id,
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
            )
        }

        Command::Close {
            ids,
            reason,
            force,
            stdin,
        } => {
            let ids = if stdin {
                bn::commands::stdin::read_ids_from_stdin()?
            } else {
                ids
            };
            for id in &ids {
                validate_bean_id(id)?;
            }
            let resolved_ids = resolve_bean_ids(ids, &beans_dir)?;
            cmd_close(&beans_dir, resolved_ids, reason, force)
        }

        Command::Verify { id, json } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            let passed = cmd_verify(&beans_dir, &resolved_id)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({"id": resolved_id, "passed": passed})
                );
            }
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

        Command::Context { id, json } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_context(&beans_dir, &resolved_id, json)
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

        Command::Quick {
            title,
            description,
            acceptance,
            notes,
            verify,
            priority,
            by,
            produces,
            requires,
            parent,
            on_fail,
            pass_ok,
        } => {
            if let Some(ref p) = parent {
                validate_bean_id(p)?;
            }

            // Parse --on-fail flag
            let on_fail = on_fail
                .map(|s| bn::commands::create::parse_on_fail(&s))
                .transpose()?;

            cmd_quick(
                &beans_dir,
                QuickArgs {
                    title,
                    description,
                    acceptance,
                    notes,
                    verify,
                    priority,
                    by,
                    produces,
                    requires,
                    parent,
                    on_fail,
                    pass_ok,
                },
            )
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

        Command::Run {
            id,
            jobs,
            dry_run,
            loop_mode,
            auto_plan,
            keep_going,
            timeout,
            idle_timeout,
            watch,
            foreground,
            stop,
        } => cmd_run(
            &beans_dir,
            bn::commands::run::RunArgs {
                id,
                jobs,
                dry_run,
                loop_mode,
                auto_plan,
                keep_going,
                timeout,
                idle_timeout,
                watch,
                foreground,
                stop,
            },
        ),

        Command::Plan {
            id,
            strategy,
            auto,
            force,
            dry_run,
        } => {
            if let Some(ref id_val) = id {
                validate_bean_id(id_val)?;
            }
            let resolved_id = match id {
                Some(ref id_val) => Some(resolve_bean_id(id_val, &beans_dir)?),
                None => None,
            };
            cmd_plan(
                &beans_dir,
                PlanArgs {
                    id: resolved_id,
                    strategy,
                    auto,
                    force,
                    dry_run,
                },
            )
        }

        Command::Agents { json } => cmd_agents(&beans_dir, json),

        Command::Logs { id, follow, all } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_logs(&beans_dir, &resolved_id, follow, all)
        }

        Command::Config { command } => match command {
            ConfigCommand::Get { key } => cmd_config_get(&beans_dir, &key),
            ConfigCommand::Set { key, value } => cmd_config_set(&beans_dir, &key, &value),
        },
    }
}
