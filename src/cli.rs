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
    List,
    /// Open a repository development environment
    Open { repo: Option<RepoId> },
    /// Start a repository's Dev Container
    Up { repo: Option<RepoId> },
    /// Attach to a repository's shell session
    Shell { repo: Option<RepoId> },
    /// Execute a command in a repository's Dev Container
    Exec {
        repo: Option<RepoId>,
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Check required tools and workspace configuration
    Doctor,
    /// Proxy SSH traffic to a repository's Dev Container
    SshProxy { name: String },
}
