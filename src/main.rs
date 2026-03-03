use std::env;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{CommandFactory, Parser};

mod cli;

use bn::commands::create::CreateArgs;
use bn::commands::plan::PlanArgs;
use bn::commands::quick::QuickArgs;
use bn::commands::{
    cmd_adopt, cmd_agents, cmd_claim, cmd_close, cmd_config_get, cmd_config_set, cmd_context,
    cmd_create, cmd_delete, cmd_dep_add, cmd_dep_list, cmd_dep_remove, cmd_doctor, cmd_edit,
    cmd_fact, cmd_graph, cmd_init, cmd_list, cmd_locks, cmd_locks_clear, cmd_logs, cmd_mcp_serve,
    cmd_memory_context, cmd_plan, cmd_quick, cmd_recall, cmd_release, cmd_reopen, cmd_run,
    cmd_show, cmd_stats, cmd_status, cmd_sync, cmd_tidy, cmd_trace, cmd_tree, cmd_trust,
    cmd_unarchive, cmd_update, cmd_verify, cmd_verify_facts,
    review::{cmd_review, ReviewArgs},
};
use bn::discovery::find_beans_dir;
use bn::index::Index;
use bn::util::validate_bean_id;
use cli::{Cli, Command, ConfigCommand, CreateOpts, CreateSubcommand, DepCommand, McpCommand};

// Helper to resolve a single bean ID (handles @latest selector or plain IDs)
fn resolve_bean_id(id: &str, beans_dir: &std::path::Path) -> Result<String> {
    if id == "@latest" {
        let index = Index::load(beans_dir)?;
        index
            .beans
            .iter()
            .max_by_key(|e| e.updated_at)
            .map(|e| e.id.clone())
            .ok_or_else(|| anyhow::anyhow!("@latest: no beans in index"))
    } else if id.starts_with('@') {
        anyhow::bail!("Unknown selector: {}", id)
    } else {
        Ok(id.to_string())
    }
}

// Helper to resolve multiple bean IDs
fn resolve_bean_ids(ids: Vec<String>, beans_dir: &std::path::Path) -> Result<Vec<String>> {
    ids.into_iter()
        .map(|id| resolve_bean_id(&id, beans_dir))
        .collect()
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

    // Completions don't need beans_dir either
    if let Command::Completions { shell } = cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(shell, &mut cmd, "bn", &mut std::io::stdout());
        return Ok(());
    }

    // All other commands need beans_dir
    let beans_dir = find_beans_dir(&env::current_dir()?)?;

    match cli.command {
        Command::Init { .. } => unreachable!(),
        Command::Completions { .. } => unreachable!(),

        Command::Create { args } => {
            let CreateOpts {
                subcommand,
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
                paths,
                on_fail,
                pass_ok,
                verify_timeout,
                claim,
                by,
                run,
                interactive,
                json,
            } = *args;
            // Handle 'bn create next' subcommand
            if let Some(CreateSubcommand::Next {
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
                paths: next_paths,
                on_fail,
                pass_ok,
                verify_timeout,
                claim,
                by,
                run,
                json,
            }) = subcommand
            {
                // Resolve @latest to get the most recently created/updated bean
                let latest_id = resolve_bean_id("@latest", &beans_dir).map_err(|_| {
                    anyhow::anyhow!(
                        "No previous bean found. 'bn create next' requires at least one existing bean.\n\
                         Use 'bn create' for the first bean in a chain."
                    )
                })?;

                // Merge @latest dep with any explicit --deps
                let merged_deps = match deps {
                    Some(d) => Some(format!("{},{}", latest_id, d)),
                    None => Some(latest_id.clone()),
                };

                use bn::commands::stdin::resolve_stdin_opt;
                let description = resolve_stdin_opt(description)?;
                let acceptance = resolve_stdin_opt(acceptance)?;
                let notes = resolve_stdin_opt(notes)?;

                let resolved_title = title.or(set_title);
                let title = resolved_title
                    .ok_or_else(|| anyhow::anyhow!("bn create next: title is required"))?;

                if run && verify.is_none() {
                    anyhow::bail!(
                        "--run requires --verify\n\n\
                         Cannot spawn an agent without a test."
                    );
                }

                let on_fail = on_fail
                    .map(|s| bn::commands::create::parse_on_fail(&s))
                    .transpose()?;

                let bean_id = cmd_create(
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
                        deps: merged_deps,
                        parent,
                        produces,
                        requires,
                        paths: next_paths,
                        on_fail,
                        pass_ok,
                        claim,
                        by,
                        verify_timeout,
                    },
                )?;

                eprintln!("⛓ Chained after bean {} (@latest)", latest_id);

                if json {
                    let bean_path = bn::discovery::find_bean_file(&beans_dir, &bean_id)?;
                    let bean = bn::bean::Bean::from_file(&bean_path)?;
                    println!("{}", serde_json::to_string(&bean)?);
                }

                if run {
                    use bn::config::Config;
                    let config = Config::load_with_extends(&beans_dir)?;
                    match &config.run {
                        Some(template) => {
                            let cmd = template.replace("{id}", &bean_id);
                            eprintln!("Spawning: {}", cmd);
                            let status =
                                std::process::Command::new("sh").args(["-c", &cmd]).status();
                            match status {
                                Ok(s) if s.success() => {}
                                Ok(s) => eprintln!(
                                    "Run command exited with code {}",
                                    s.code().unwrap_or(-1)
                                ),
                                Err(e) => eprintln!("Failed to run command: {}", e),
                            }
                        }
                        None => {
                            anyhow::bail!(
                                "--run requires a configured agent.\n\
                                 Run: bn init --setup"
                            );
                        }
                    }
                }

                return Ok(());
            }

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
                        paths,
                        on_fail,
                        pass_ok,
                        verify_timeout,
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
                            "--run requires a configured agent.\n\n\
                             Run: bn init --setup\n\n\
                             Or set manually: bn config set run \"<command>\"\n\n\
                             The command template uses {{id}} as a placeholder for the bean ID.\n\n\
                             Examples:\n  \
                               bn config set run \"pi @.beans/{{id}}-*.md 'implement and bn close {{id}}'\"\n  \
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
            ..
        } => cmd_list(
            status.as_deref(),
            priority,
            parent.as_deref(),
            label.as_deref(),
            assignee.as_deref(),
            false, // mine (not yet exposed in CLI)
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
            failed,
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
            if failed {
                bn::commands::close::cmd_close_failed(&beans_dir, resolved_ids, reason)
            } else {
                cmd_close(&beans_dir, resolved_ids, reason, force)
            }
        }

        Command::Verify { id, json, .. } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            let out = bn::output::Output::new();
            let passed = cmd_verify(&beans_dir, &resolved_id, &out)?;
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

        Command::Claim {
            id,
            release,
            by,
            force,
        } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            if release {
                cmd_release(&beans_dir, &resolved_id)
            } else {
                cmd_claim(&beans_dir, &resolved_id, by, force)
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
        },

        Command::Status { json } => cmd_status(json, &beans_dir),

        Command::Context {
            id,
            json,
            structure_only,
            agent_prompt,
        } => {
            match id {
                Some(ref id_str) => {
                    validate_bean_id(id_str)?;
                    let resolved_id = resolve_bean_id(id_str, &beans_dir)?;
                    cmd_context(&beans_dir, &resolved_id, json, structure_only, agent_prompt)
                }
                None => {
                    // No ID: output memory context
                    cmd_memory_context(&beans_dir, json)
                }
            }
        }

        Command::Tree { id } => {
            if let Some(ref id_val) = id {
                validate_bean_id(id_val)?;
            }
            cmd_tree(&beans_dir, id.as_deref())
        }
        Command::Graph { format } => cmd_graph(&beans_dir, &format),
        Command::Sync => cmd_sync(&beans_dir),
        Command::Tidy { dry_run, .. } => {
            let out = bn::output::Output::new();
            cmd_tidy(&beans_dir, dry_run, &out)
        }
        Command::Stats { json } => cmd_stats(&beans_dir, json),
        Command::Doctor { fix } => cmd_doctor(&beans_dir, fix),
        Command::Trust { revoke, check } => cmd_trust(&beans_dir, revoke, check),

        Command::Unarchive { id } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_unarchive(&beans_dir, &resolved_id)
        }

        Command::Locks { clear } => {
            if clear {
                cmd_locks_clear(&beans_dir)
            } else {
                cmd_locks(&beans_dir)
            }
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
            verify_timeout,
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
                    verify_timeout,
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

        Command::Run {
            id,
            jobs,
            dry_run,
            loop_mode,
            auto_plan,
            keep_going,
            timeout,
            idle_timeout,
            json_stream,
            review,
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
                json_stream,
                review,
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

        Command::Fact {
            title,
            verify,
            description,
            paths,
            ttl,
            pass_ok,
        } => {
            cmd_fact(&beans_dir, title, verify, description, paths, ttl, pass_ok)?;
            Ok(())
        }

        Command::Recall { query, all, json } => cmd_recall(&beans_dir, &query, all, json),

        Command::VerifyFacts => cmd_verify_facts(&beans_dir),

        Command::Config { command } => match command {
            ConfigCommand::Get { key } => cmd_config_get(&beans_dir, &key),
            ConfigCommand::Set { key, value } => cmd_config_set(&beans_dir, &key, &value),
        },

        Command::Mcp { command } => match command {
            McpCommand::Serve => cmd_mcp_serve(&beans_dir),
        },

        Command::Trace { id, json } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_trace(&resolved_id, json, &beans_dir)
        }

        Command::Review { id, diff, model } => {
            validate_bean_id(&id)?;
            let resolved_id = resolve_bean_id(&id, &beans_dir)?;
            cmd_review(
                &beans_dir,
                ReviewArgs {
                    id: resolved_id,
                    model,
                    diff_only: diff,
                },
            )
        }
    }
}
