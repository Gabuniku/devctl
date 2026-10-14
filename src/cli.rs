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
        #[arg(long)]
        json: bool,
    },
    /// Open a repository development environment
    Open { repo: Option<RepoId> },
    /// Start a repository's Dev Container
    Up { repo: Option<RepoId> },
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
        #[arg(long)]
        json: bool,
    },
    /// Check required tools and workspace configuration
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Proxy SSH traffic to a repository's Dev Container
    SshProxy { name: String },
    /// Print repository context for tools and automation
    Context {
        repo: Option<RepoId>,
        #[arg(long)]
        json: bool,
    },
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
}
