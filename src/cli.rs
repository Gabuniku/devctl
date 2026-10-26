use clap::{Parser, Subcommand};

use crate::repo::RepoId;

#[derive(Debug, Parser)]
#[command(name = "devctl")]
#[command(about = "Manage local development workspaces")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Initialize a workspace in the current directory
    Init,
    /// List managed repositories
    List {
        /// Include each Dev Container's remote user
        #[arg(long)]
        user: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Open a repository development environment
    Open { repo: Option<RepoId> },
    /// Start a repository's Dev Container
    Up {
        repo: Option<RepoId>,
        /// Remove and recreate the existing Dev Container
        #[arg(long)]
        rebuild: bool,
    },
    /// Prepare a repository without attaching (clone if needed, start its Dev Container)
    Ensure { repo: Option<RepoId> },
    /// Attach to a repository's shell session
    Shell { repo: Option<RepoId> },
    /// Execute a command in a repository's Dev Container
    Exec {
        repo: Option<RepoId>,
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Print the SSH target for a repository's Dev Container
    SshTarget {
        repo: Option<RepoId>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Check required tools and workspace configuration
    Doctor {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Proxy SSH traffic to a repository's Dev Container
    SshProxy { name: String },
    /// Print repository context for tools and automation
    Context {
        repo: Option<RepoId>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage global instructions for coding agents
    Agents {
        #[command(subcommand)]
        command: AgentsCommand,
    },
    /// Manage repository-local Dev Container configuration stored outside the repository
    LocalConfig {
        #[command(subcommand)]
        command: LocalConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    /// Install global development environment instructions
    Install {
        /// Print the managed block without writing files
        #[arg(long)]
        print: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum LocalConfigCommand {
    /// Create a local Dev Container configuration from a template
    Init {
        repo: Option<RepoId>,
        /// Dev Container template name or full reference
        #[arg(short = 't', long)]
        template: String,
    },
    /// Print the local Dev Container configuration path
    Path { repo: Option<RepoId> },
    /// Remove a local Dev Container configuration
    Rm { repo: Option<RepoId> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ensure_without_repository() {
        let cli = Cli::try_parse_from(["devctl", "ensure"]).unwrap();

        assert!(matches!(cli.command, Commands::Ensure { repo: None }));
    }

    #[test]
    fn parses_ensure_with_repository() {
        let cli = Cli::try_parse_from(["devctl", "ensure", "owner/repo"]).unwrap();

        let Commands::Ensure { repo: Some(repo) } = cli.command else {
            panic!("expected ensure command with a repository");
        };
        assert_eq!(repo.to_string(), "owner/repo");
    }

    #[test]
    fn parses_up_without_rebuild() {
        let cli = Cli::try_parse_from(["devctl", "up"]).unwrap();

        assert!(matches!(
            cli.command,
            Commands::Up {
                repo: None,
                rebuild: false
            }
        ));
    }

    #[test]
    fn parses_up_with_rebuild() {
        let cli = Cli::try_parse_from(["devctl", "up", "owner/repo", "--rebuild"]).unwrap();

        let Commands::Up {
            repo: Some(repo),
            rebuild: true,
        } = cli.command
        else {
            panic!("expected up command with a repository and rebuild enabled");
        };
        assert_eq!(repo.to_string(), "owner/repo");
    }

    #[test]
    fn parses_agents_install() {
        let cli = Cli::try_parse_from(["devctl", "agents", "install"]).unwrap();

        assert!(matches!(
            cli.command,
            Commands::Agents {
                command: AgentsCommand::Install { print: false }
            }
        ));
    }

    #[test]
    fn parses_agents_install_print() {
        let cli = Cli::try_parse_from(["devctl", "agents", "install", "--print"]).unwrap();

        assert!(matches!(
            cli.command,
            Commands::Agents {
                command: AgentsCommand::Install { print: true }
            }
        ));
    }

    #[test]
    fn parses_local_config_init() {
        let cli =
            Cli::try_parse_from(["devctl", "local-config", "init", "owner/repo", "-t", "rust"])
                .unwrap();

        let Commands::LocalConfig {
            command:
                LocalConfigCommand::Init {
                    repo: Some(repo),
                    template,
                },
        } = cli.command
        else {
            panic!("expected local-config init command");
        };
        assert_eq!(repo.to_string(), "owner/repo");
        assert_eq!(template, "rust");
    }
}
