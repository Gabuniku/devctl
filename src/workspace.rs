use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::config::{load_workspace, Workspace};
use crate::repo::{
    clone_repo, git_toplevel, is_git_worktree, repo_id_from_path, repo_path, RepoId,
};

const CLAUDE_LOCAL: &str = "<!-- Managed by devctl. Manual changes may be overwritten. -->\n\
\n\
# Local development environment\n\
\n\
- Source files are stored on the development VM. Edit them here.\n\
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
            let top = git_toplevel(cwd).context(
                "repository was not specified and the current directory is not in one\nrun `devctl list` to see managed repositories",
            )?;
            let id = repo_id_from_path(ws, &top).ok_or_else(|| {
                anyhow::anyhow!(
                    "current repository {} is not managed by this devctl workspace\nrun `devctl list` to see managed repositories",
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

fn sorted_repo_lines(repos: Vec<(String, String)>) -> Vec<String> {
    let mut lines = repos
        .into_iter()
        .map(|(owner, name)| format!("{owner}/{name}"))
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn sorted_repo_user_lines(mut repos: Vec<(String, String)>) -> Vec<String> {
    repos.sort_by(|left, right| left.0.cmp(&right.0));
    let width = repos.iter().map(|(repo, _)| repo.len()).max().unwrap_or(0);
    repos
        .into_iter()
        .map(|(repo, user)| format!("{repo:<width$}   {user}"))
        .collect()
}

fn is_repo_root(path: &Path) -> bool {
    if !is_git_worktree(path) {
        return false;
    }
    let Ok(top) = git_toplevel(path) else {
        return false;
    };
    let (Ok(path), Ok(top)) = (path.canonicalize(), top.canonicalize()) else {
        return false;
    };
    path == top
}

pub fn cmd_list(include_user: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let projects_root = ws.projects_root();
    let owners = match fs::read_dir(&projects_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", projects_root.display()))
        }
    };

    let mut repos = Vec::new();
    for owner in owners {
        let owner = owner.with_context(|| format!("failed to read {}", projects_root.display()))?;
        let owner_path = owner.path();
        if !owner_path.is_dir() {
            continue;
        }
        let owner_name = owner.file_name().to_string_lossy().into_owned();
        let entries = fs::read_dir(&owner_path)
            .with_context(|| format!("failed to read {}", owner_path.display()))?;
        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read {}", owner_path.display()))?;
            let path = entry.path();
            if is_repo_root(&path) {
                repos.push((
                    RepoId {
                        owner: owner_name.clone(),
                        name: entry.file_name().to_string_lossy().into_owned(),
                    },
                    path,
                ));
            }
        }
    }

    let lines = if include_user {
        let repos = repos
            .into_iter()
            .map(|(id, path)| {
                let user = devcontainer_remote_user(&id, &path)?;
                Ok((id.to_string(), user))
            })
            .collect::<Result<Vec<_>>>()?;
        sorted_repo_user_lines(repos)
    } else {
        sorted_repo_lines(
            repos
                .into_iter()
                .map(|(id, _)| (id.owner, id.name))
                .collect(),
        )
    };
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadConfiguration {
    merged_configuration: MergedConfiguration,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergedConfiguration {
    remote_user: Option<String>,
}

fn remote_user_from_json_lines(output: &str) -> Option<String> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<ReadConfiguration>(line).ok())
        .find_map(|configuration| configuration.merged_configuration.remote_user)
}

fn devcontainer_remote_user(id: &RepoId, repo_path: &Path) -> Result<String> {
    let output = Command::new("devcontainer")
        .args(["read-configuration", "--workspace-folder"])
        .arg(repo_path)
        .arg("--include-merged-configuration")
        .output()
        .with_context(|| format!("failed to read Dev Container configuration for {id}"))?;
    if !output.status.success() {
        bail!("failed to resolve remote user for repository {id}");
    }
    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("Dev Container configuration for {id} was not UTF-8"))?;
    remote_user_from_json_lines(&stdout)
        .ok_or_else(|| anyhow::anyhow!("remote user is not configured for repository {id}"))
}

pub fn ssh_target(id: &RepoId, user: &str) -> String {
    format!("{user}@devctl-{}--{}", id.owner, id.name)
}

pub fn cmd_ssh_target(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    let user = devcontainer_remote_user(&id, &path)?;
    println!("{}", ssh_target(&id, &user));
    Ok(())
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

fn should_attach(current_session: Option<&str>, target: &str) -> bool {
    current_session != Some(target)
}

pub fn attach_zellij(id: &RepoId) -> Result<()> {
    let target = zellij_session_name(id);
    if !should_attach(
        std::env::var("ZELLIJ_SESSION_NAME").ok().as_deref(),
        &target,
    ) {
        println!("already attached to zellij session {target}; skipping attach");
        return Ok(());
    }

    let status = Command::new("zellij")
        .args(["attach", "-c"])
        .arg(target)
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
    fn does_not_attach_to_current_zellij_session() {
        assert!(!should_attach(
            Some("dev-Gabuniku--foo"),
            "dev-Gabuniku--foo"
        ));
    }

    #[test]
    fn attaches_to_a_different_zellij_session() {
        assert!(should_attach(
            Some("dev-Gabuniku--bar"),
            "dev-Gabuniku--foo"
        ));
    }

    #[test]
    fn attaches_from_outside_zellij() {
        assert!(should_attach(None, "dev-Gabuniku--foo"));
    }

    #[test]
    fn sorts_repo_lines() {
        assert_eq!(
            sorted_repo_lines(vec![
                ("yattulab".into(), "qi-bot-rs".into()),
                ("Gabuniku".into(), "foo".into()),
            ]),
            vec!["Gabuniku/foo", "yattulab/qi-bot-rs"]
        );
    }

    #[test]
    fn empty_repo_lines_stay_empty() {
        assert!(sorted_repo_lines(Vec::new()).is_empty());
    }

    #[test]
    fn builds_ssh_target() {
        let id = RepoId {
            owner: "mizlinx".into(),
            name: "gyotaku-rockchip".into(),
        };
        assert_eq!(
            ssh_target(&id, "vscode"),
            "vscode@devctl-mizlinx--gyotaku-rockchip"
        );
    }

    #[test]
    fn sorts_and_aligns_repo_user_lines() {
        assert_eq!(
            sorted_repo_user_lines(vec![
                ("yattulab/qi-bot-rs".into(), "vscode".into()),
                ("mizlinx/gyotaku-rockchip".into(), "vscode".into()),
            ]),
            vec![
                "mizlinx/gyotaku-rockchip   vscode",
                "yattulab/qi-bot-rs         vscode",
            ]
        );
    }

    #[test]
    fn reads_remote_user_from_merged_configuration() {
        let output = r#"log line
{"configuration":{"remoteUser":"wrong"},"mergedConfiguration":{"name":"test","remoteUser":"vscode"}}
"#;
        assert_eq!(
            remote_user_from_json_lines(output).as_deref(),
            Some("vscode")
        );
    }

    #[test]
    fn claude_local_matches_specification() {
        assert_eq!(
            claude_local_contents(),
            "<!-- Managed by devctl. Manual changes may be overwritten. -->\n\n# Local development environment\n\n- Source files are stored on the development VM. Edit them here.\n- Project runtime commands must run inside the Dev Container.\n- Use `devctl exec -- <command>`, e.g. `devctl exec -- cargo test`.\n- If the Dev Container is not running, start it with `devctl up`.\n- Do not install project dependencies directly on the host VM.\n- Do not run `devctl open`: it attaches an interactive Zellij session and will not return.\n"
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
