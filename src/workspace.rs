use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{bail, Context, Result};

use crate::config::{load_workspace, Workspace};
use crate::repo::{
    clone_repo, git_toplevel, is_git_worktree, repo_id_from_path, repo_path, RepoId,
};

const CLAUDE_LOCAL: &str = "<!-- Managed by devctl. Manual changes may be overwritten. -->\n\
\n\
# Local development environment\n\
\n\
- Source files are stored on the development VM.\n\
- Project runtime commands must run inside the Dev Container.\n\
- Use `devctl exec -- <command>`, e.g. `devctl exec -- cargo test`.\n\
- If the Dev Container is not running, start it with `devctl up`.\n\
- Do not install project dependencies directly on the host VM.\n\
- Do not run `devctl open`: it attaches an interactive Zellij session and will not return.\n";

pub fn resolve_repo(ws: &Workspace, arg: Option<RepoId>, cwd: &Path) -> Result<(RepoId, PathBuf)> {
    let (id, path) = match arg {
        Some(id) => {
            let path = repo_path(ws, &id);
            (id, path)
        }
        None => {
            let top = git_toplevel(cwd)
                .context("repository was not specified and the current directory is not in one")?;
            let id = repo_id_from_path(ws, &top).ok_or_else(|| {
                anyhow::anyhow!(
                    "current repository {} is not managed by this devctl workspace",
                    top.display()
                )
            })?;
            (id, top)
        }
    };

    if !is_git_worktree(&path) {
        bail!("managed repository {} does not exist", path.display());
    }

    Ok((id, path))
}

pub fn resolve_or_clone(
    ws: &Workspace,
    arg: Option<RepoId>,
    cwd: &Path,
) -> Result<(RepoId, PathBuf)> {
    let Some(id) = arg else {
        return resolve_repo(ws, None, cwd);
    };
    let path = repo_path(ws, &id);
    if !is_git_worktree(&path) {
        clone_repo(&id, &path)
            .with_context(|| format!("failed to clone repository {id} into {}", path.display()))?;
    }
    Ok((id, path))
}

pub fn claude_local_contents() -> &'static str {
    CLAUDE_LOCAL
}

pub fn ensure_claude_local(repo_path: &Path) -> Result<()> {
    let path = repo_path.join("CLAUDE.local.md");
    if fs::read_to_string(&path).ok().as_deref() == Some(claude_local_contents()) {
        return Ok(());
    }
    fs::write(&path, claude_local_contents())
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn exclude_with_entry(current: &str, entry: &str) -> Option<String> {
    if current.lines().any(|line| line == entry) {
        return None;
    }

    let mut updated = current.to_owned();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(entry);
    updated.push('\n');
    Some(updated)
}

pub fn ensure_git_exclude(repo_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()
        .with_context(|| format!("failed to locate git exclude for {}", repo_path.display()))?;
    if !output.status.success() {
        bail!("failed to locate git exclude for {}", repo_path.display());
    }

    let value = String::from_utf8(output.stdout).context("git exclude path was not UTF-8")?;
    let path = PathBuf::from(value.trim());
    let path = if path.is_absolute() {
        path
    } else {
        repo_path.join(path)
    };
    let current = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()))
        }
    };
    if let Some(updated) = exclude_with_entry(&current, "CLAUDE.local.md") {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

pub fn devcontainer_up(id: &RepoId, repo_path: &Path) -> Result<()> {
    let status = Command::new("devcontainer")
        .args(["up", "--workspace-folder"])
        .arg(repo_path)
        .status()
        .with_context(|| format!("failed to run devcontainer up for {id}"))?;
    if !status.success() {
        bail!("devcontainer up failed for repository {id}");
    }
    Ok(())
}

pub fn zellij_session_name(id: &RepoId) -> String {
    format!("dev-{}--{}", id.owner, id.name)
}

pub fn attach_zellij(id: &RepoId) -> Result<()> {
    let status = Command::new("zellij")
        .args(["attach", "-c"])
        .arg(zellij_session_name(id))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run zellij")?;
    if !status.success() {
        bail!("zellij exited with {status}");
    }
    Ok(())
}

pub fn cmd_open(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_or_clone(&ws, arg, &cwd)?;
    ensure_claude_local(&path)?;
    ensure_git_exclude(&path)?;
    devcontainer_up(&id, &path)?;
    attach_zellij(&id)
}

pub fn cmd_up(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    ensure_claude_local(&path)?;
    ensure_git_exclude(&path)?;
    devcontainer_up(&id, &path)
}

pub fn cmd_shell(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (_, path) = resolve_repo(&ws, arg, &cwd)?;
    let status = interactive_devcontainer_exec(&path, ["bash"])?;
    if !status.success() {
        bail!("devcontainer shell exited with {status}");
    }
    Ok(())
}

pub fn cmd_exec(arg: Option<RepoId>, argv: Vec<String>) -> Result<ExitStatus> {
    if argv.is_empty() {
        bail!("no command specified for devctl exec");
    }
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (_, path) = resolve_repo(&ws, arg, &cwd)?;
    interactive_devcontainer_exec(&path, argv)
}

fn interactive_devcontainer_exec<I, S>(repo_path: &Path, argv: I) -> Result<ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new("devcontainer")
        .args(["exec", "--workspace-folder"])
        .arg(repo_path)
        .args(argv)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| {
            format!(
                "failed to run devcontainer exec for {}",
                repo_path.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_zellij_session_name() {
        let id = RepoId {
            owner: "Gabuniku".into(),
            name: "foo".into(),
        };
        assert_eq!(zellij_session_name(&id), "dev-Gabuniku--foo");
    }

    #[test]
    fn claude_local_matches_specification() {
        assert_eq!(
            claude_local_contents(),
            "<!-- Managed by devctl. Manual changes may be overwritten. -->\n\n# Local development environment\n\n- Source files are stored on the development VM.\n- Project runtime commands must run inside the Dev Container.\n- Use `devctl exec -- <command>`, e.g. `devctl exec -- cargo test`.\n- If the Dev Container is not running, start it with `devctl up`.\n- Do not install project dependencies directly on the host VM.\n- Do not run `devctl open`: it attaches an interactive Zellij session and will not return.\n"
        );
    }

    #[test]
    fn appends_missing_exclude_entry() {
        assert_eq!(
            exclude_with_entry("target/\n", "CLAUDE.local.md"),
            Some("target/\nCLAUDE.local.md\n".into())
        );
    }

    #[test]
    fn exec_requires_a_command() {
        let error = cmd_exec(None, Vec::new()).unwrap_err();
        assert_eq!(error.to_string(), "no command specified for devctl exec");
    }

    #[test]
    fn does_not_duplicate_exclude_entry() {
        assert_eq!(
            exclude_with_entry("CLAUDE.local.md\n", "CLAUDE.local.md"),
            None
        );
    }

    #[test]
    fn partial_exclude_match_is_not_a_match() {
        assert_eq!(
            exclude_with_entry("CLAUDE.local.md.bak\n", "CLAUDE.local.md"),
            Some("CLAUDE.local.md.bak\nCLAUDE.local.md\n".into())
        );
    }

    #[test]
    fn appends_to_empty_exclude_file() {
        assert_eq!(
            exclude_with_entry("", "CLAUDE.local.md"),
            Some("CLAUDE.local.md\n".into())
        );
    }

    #[test]
    fn preserves_unterminated_exclude_line() {
        assert_eq!(
            exclude_with_entry("target/", "CLAUDE.local.md"),
            Some("target/\nCLAUDE.local.md\n".into())
        );
    }
}
