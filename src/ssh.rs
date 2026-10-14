use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::config::load_workspace;
use crate::repo::{repo_path, RepoId};

fn parse_proxy_name(name: &str) -> Result<RepoId> {
    if let Some(repo) = name.strip_prefix("devctl-") {
        let (owner, name) = repo
            .split_once("--")
            .ok_or_else(|| anyhow::anyhow!("invalid SSH proxy name: {name}"))?;
        return format!("{owner}/{name}").parse();
    }

    name.parse()
}

pub fn cmd_ssh_proxy(name: &str) -> Result<()> {
    let id = parse_proxy_name(name)?;
    let workspace = load_workspace(&std::env::current_dir()?)?;
    let program = workspace.config.container_engine.program();
    let path = repo_path(&workspace, &id);
    let filter = format!("label=devcontainer.local_folder={}", path.display());
    let output = Command::new(program)
        .args(["ps", "-q", "--filter"])
        .arg(filter)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .context("failed to find Dev Container")?;

    if !output.status.success() {
        bail!("{program} ps failed: {}", output.status);
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("{program} produced non-UTF-8 output"))?;
    let Some(container_id) = stdout.lines().find(|line| !line.is_empty()) else {
        bail!("Dev Container for {id} is not running; run `devctl up {id}` first");
    };

    let error = Command::new(program)
        .args(["exec", "-i", container_id, "/usr/sbin/sshd", "-i"])
        .exec();
    Err(error).context("failed to exec sshd in Dev Container")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proxy_host_name() {
        assert_eq!(
            parse_proxy_name("devctl-owner--repo").unwrap().to_string(),
            "owner/repo"
        );
    }

    #[test]
    fn parses_repo_id() {
        assert_eq!(
            parse_proxy_name("owner/repo").unwrap().to_string(),
            "owner/repo"
        );
    }

    #[test]
    fn rejects_bogus_name() {
        assert!(parse_proxy_name("bogus").is_err());
    }
}
