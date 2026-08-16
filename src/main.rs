mod cli;
pub mod command;
pub mod config;
pub mod doctor;
pub mod repo;
pub mod workspace;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Init => config::init_workspace(&std::env::current_dir()?),
        Commands::List => workspace::cmd_list(),
        Commands::Open { repo } => workspace::cmd_open(repo),
        Commands::Up { repo } => workspace::cmd_up(repo),
        Commands::Shell { repo } => workspace::cmd_shell(repo),
        Commands::Exec { repo, args } => {
            let status = workspace::cmd_exec(repo, args)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Commands::Doctor => doctor::cmd_doctor(),
    }
}
