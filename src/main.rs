use anyhow::Result;
use clap::Parser;

mod cli;

use cli::{Cli, Command, DepCommand};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { .. } => {
            eprintln!("bn init: not yet implemented");
        }
        Command::Create { .. } => {
            eprintln!("bn create: not yet implemented");
        }
        Command::Show { .. } => {
            eprintln!("bn show: not yet implemented");
        }
        Command::List { .. } => {
            eprintln!("bn list: not yet implemented");
        }
        Command::Update { .. } => {
            eprintln!("bn update: not yet implemented");
        }
        Command::Close { .. } => {
            eprintln!("bn close: not yet implemented");
        }
        Command::Verify { .. } => {
            eprintln!("bn verify: not yet implemented");
        }
        Command::Reopen { .. } => {
            eprintln!("bn reopen: not yet implemented");
        }
        Command::Delete { .. } => {
            eprintln!("bn delete: not yet implemented");
        }
        Command::Dep { command } => match command {
            DepCommand::Add { .. } => {
                eprintln!("bn dep add: not yet implemented");
            }
            DepCommand::Remove { .. } => {
                eprintln!("bn dep remove: not yet implemented");
            }
            DepCommand::List { .. } => {
                eprintln!("bn dep list: not yet implemented");
            }
            DepCommand::Tree { .. } => {
                eprintln!("bn dep tree: not yet implemented");
            }
            DepCommand::Cycles => {
                eprintln!("bn dep cycles: not yet implemented");
            }
        },
        Command::Ready => {
            eprintln!("bn ready: not yet implemented");
        }
        Command::Blocked => {
            eprintln!("bn blocked: not yet implemented");
        }
        Command::Tree { .. } => {
            eprintln!("bn tree: not yet implemented");
        }
        Command::Graph { .. } => {
            eprintln!("bn graph: not yet implemented");
        }
        Command::Sync => {
            eprintln!("bn sync: not yet implemented");
        }
        Command::Stats => {
            eprintln!("bn stats: not yet implemented");
        }
        Command::Doctor => {
            eprintln!("bn doctor: not yet implemented");
        }
    }

    Ok(())
}
