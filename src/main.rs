pub mod agents;
mod cli;
pub mod command;
pub mod config;
pub mod doctor;
pub mod repo;
pub mod ssh;
pub mod workspace;

use anyhow::Result;
use clap::Parser;
use cli::{AgentsCommand, Cli, Commands, LocalConfigCommand};

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Init => config::init_workspace(&std::env::current_dir()?),
        Commands::List { user, json } => workspace::cmd_list(user, json),
        Commands::Open { repo } => workspace::cmd_open(repo),
        Commands::Up { repo, rebuild } => workspace::cmd_up(repo, rebuild),
        Commands::Ensure { repo } => workspace::cmd_ensure(repo),
        Commands::Shell { repo } => workspace::cmd_shell(repo),
        Commands::Exec { repo, args } => {
            let status = workspace::cmd_exec(repo, args)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Commands::SshTarget { repo, json } => workspace::cmd_ssh_target(repo, json),
        Commands::Doctor { json } => doctor::cmd_doctor(json),
        Commands::SshProxy { name } => ssh::cmd_ssh_proxy(&name),
        Commands::Context { repo, json } => workspace::cmd_context(repo, json),
        Commands::Agents { command } => match command {
            AgentsCommand::Install { print } => agents::cmd_install(print),
        },
        Commands::LocalConfig { command } => match command {
            LocalConfigCommand::Init { repo, template } => {
                workspace::cmd_local_config_init(repo, &template)
            }
            LocalConfigCommand::Path { repo } => workspace::cmd_local_config_path(repo),
            LocalConfigCommand::Rm { repo } => workspace::cmd_local_config_rm(repo),
        },
    }
}
