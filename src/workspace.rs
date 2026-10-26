use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{load_workspace, Config, ContainerEngine, Mount, Workspace};
use crate::repo::{
    clone_repo, git_common_root, git_toplevel, is_git_worktree, repo_id_from_path, repo_path,
    RepoId,
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
            let top = git_common_root(cwd).context(
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

fn display_repo(repo: String, local_config: bool) -> String {
    if local_config {
        format!("{repo} (local config)")
    } else {
        repo
    }
}

fn sorted_repo_lines(repos: Vec<(String, bool)>) -> Vec<String> {
    let mut lines = repos
        .into_iter()
        .map(|(repo, local_config)| display_repo(repo, local_config))
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

#[derive(Debug, Serialize)]
struct ListEntry {
    repo: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_user: Option<String>,
    local_config: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DevcontainerConfig {
    Repository,
    Local(PathBuf),
    None,
}

impl DevcontainerConfig {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::Local(path) => Some(path),
            Self::Repository | Self::None => None,
        }
    }

    fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

fn config_from_presence(
    repository_exists: bool,
    local_exists: bool,
    local_path: PathBuf,
) -> DevcontainerConfig {
    if repository_exists {
        DevcontainerConfig::Repository
    } else if local_exists {
        DevcontainerConfig::Local(local_path)
    } else {
        DevcontainerConfig::None
    }
}

fn local_config_path(ws: &Workspace, id: &RepoId) -> PathBuf {
    ws.local_config_root(&id.owner, &id.name)
        .join(".devcontainer/devcontainer.json")
}

fn resolve_devcontainer_config(
    ws: &Workspace,
    id: &RepoId,
    repo_path: &Path,
) -> DevcontainerConfig {
    let repository_exists = repo_path.join(".devcontainer/devcontainer.json").is_file()
        || repo_path.join(".devcontainer.json").is_file();
    let local_path = local_config_path(ws, id);
    let local_exists = local_path.is_file();
    let resolved = config_from_presence(repository_exists, local_exists, local_path.clone());
    if repository_exists && local_exists {
        eprintln!(
            "warning: {id} has its own devcontainer.json; ignoring local config {}",
            local_path.display()
        );
    }
    resolved
}

fn managed_repos(ws: &Workspace) -> Result<Vec<(RepoId, PathBuf)>> {
    let projects_root = ws.projects_root();
    let owners = match fs::read_dir(&projects_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
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

    repos.sort_by_key(|(id, _)| id.to_string());
    Ok(repos)
}

pub fn cmd_list(include_user: bool, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let repos = managed_repos(&ws)?;

    if json {
        let entries = repos
            .into_iter()
            .map(|(id, path)| {
                let config = resolve_devcontainer_config(&ws, &id, &path);
                let remote_user = include_user
                    .then(|| {
                        devcontainer_remote_user(
                            &id,
                            &path,
                            ws.config.container_engine,
                            config.path(),
                        )
                    })
                    .transpose()?;
                Ok(ListEntry {
                    repo: id.to_string(),
                    path: path.display().to_string(),
                    remote_user,
                    local_config: config.is_local(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    let lines = if include_user {
        let repos = repos
            .into_iter()
            .map(|(id, path)| {
                let config = resolve_devcontainer_config(&ws, &id, &path);
                let user = devcontainer_remote_user(
                    &id,
                    &path,
                    ws.config.container_engine,
                    config.path(),
                )?;
                Ok((display_repo(id.to_string(), config.is_local()), user))
            })
            .collect::<Result<Vec<_>>>()?;
        sorted_repo_user_lines(repos)
    } else {
        sorted_repo_lines(
            repos
                .into_iter()
                .map(|(id, path)| {
                    let config = resolve_devcontainer_config(&ws, &id, &path);
                    (id.to_string(), config.is_local())
                })
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceConfiguration {
    workspace_folder: PathBuf,
}

#[derive(Deserialize)]
struct WorkspaceReadConfiguration {
    workspace: WorkspaceConfiguration,
}

fn remote_user_from_json_lines(output: &str) -> Option<String> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<ReadConfiguration>(line).ok())
        .find_map(|configuration| configuration.merged_configuration.remote_user)
}

fn workspace_folder_from_json_lines(output: &str) -> Option<PathBuf> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<WorkspaceReadConfiguration>(line).ok())
        .map(|configuration| configuration.workspace.workspace_folder)
        .next()
}

fn devcontainer_workspace_folder(
    id: &RepoId,
    repo_path: &Path,
    engine: ContainerEngine,
    config: Option<&Path>,
) -> Result<PathBuf> {
    let args = devcontainer_read_configuration_args(repo_path, engine, config);
    let output = Command::new("devcontainer")
        .args(args)
        .output()
        .with_context(|| format!("failed to read Dev Container configuration for {id}"))?;
    if !output.status.success() {
        bail!("failed to resolve workspace folder for repository {id}");
    }
    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("Dev Container configuration for {id} was not UTF-8"))?;
    workspace_folder_from_json_lines(&stdout)
        .ok_or_else(|| anyhow::anyhow!("workspace folder is not configured for repository {id}"))
}

fn devcontainer_remote_user(
    id: &RepoId,
    repo_path: &Path,
    engine: ContainerEngine,
    config: Option<&Path>,
) -> Result<String> {
    let args = devcontainer_read_configuration_args(repo_path, engine, config);
    let output = Command::new("devcontainer")
        .args(args)
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

fn devcontainer_read_configuration_args(
    repo_path: &Path,
    engine: ContainerEngine,
    config: Option<&Path>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("read-configuration"),
        OsString::from("--workspace-folder"),
        repo_path.as_os_str().to_owned(),
        OsString::from("--include-merged-configuration"),
    ];
    args.extend(devcontainer_config_args(config));
    args.extend(docker_path_args(engine));
    args
}

pub fn ssh_target(id: &RepoId, user: &str) -> String {
    format!("{user}@devctl-{}--{}", id.owner, id.name)
}

#[derive(Debug, Serialize)]
struct SshTargetOutput {
    repo: String,
    user: String,
    target: String,
}

pub fn cmd_ssh_target(arg: Option<RepoId>, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    let config = resolve_devcontainer_config(&ws, &id, &path);
    let user = devcontainer_remote_user(&id, &path, ws.config.container_engine, config.path())?;
    let target = ssh_target(&id, &user);
    if json {
        let output = SshTargetOutput {
            repo: id.to_string(),
            user,
            target,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{target}");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct ContextOutput {
    repo: String,
    workspace_root: String,
    repo_path: String,
    container_engine: ContainerEngine,
    remote_user: String,
    session: String,
    ssh_target: String,
}

fn format_context(output: &ContextOutput) -> String {
    format!(
        "repo: {}\nworkspace_root: {}\nrepo_path: {}\ncontainer_engine: {}\nremote_user: {}\nsession: {}\nssh_target: {}",
        output.repo,
        output.workspace_root,
        output.repo_path,
        output.container_engine.program(),
        output.remote_user,
        output.session,
        output.ssh_target
    )
}

pub fn cmd_context(arg: Option<RepoId>, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    let config = resolve_devcontainer_config(&ws, &id, &path);
    let remote_user =
        devcontainer_remote_user(&id, &path, ws.config.container_engine, config.path())?;
    let output = ContextOutput {
        repo: id.to_string(),
        workspace_root: ws.root.display().to_string(),
        repo_path: path.display().to_string(),
        container_engine: ws.config.container_engine,
        session: zellij_session_name(&id),
        ssh_target: ssh_target(&id, &remote_user),
        remote_user,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", format_context(&output));
    }
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

const REMOTE_USER_PLACEHOLDER: &str = "{remoteUser}";

fn mount_spec(mount: &Mount, home: Option<&Path>, remote_user: Option<&str>) -> Result<String> {
    let source = if mount.source == "~" {
        home.context("HOME is not set; cannot expand mount source ~")?
            .to_path_buf()
    } else if let Some(relative) = mount.source.strip_prefix("~/") {
        home.context("HOME is not set; cannot expand mount source starting with ~/")?
            .join(relative)
    } else {
        PathBuf::from(&mount.source)
    };
    let source = if source.is_absolute() {
        source
    } else {
        std::env::current_dir()
            .context("failed to resolve mount source")?
            .join(source)
    };
    let target = remote_user.map_or_else(
        || mount.target.clone(),
        |user| mount.target.replace(REMOTE_USER_PLACEHOLDER, user),
    );
    Ok(format!(
        "type=bind,source={},target={}",
        source.display(),
        target
    ))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UpOptions {
    pub remove_existing_container: bool,
}

fn devcontainer_up_args(
    config: &Config,
    repo_path: &Path,
    home: Option<&Path>,
    options: UpOptions,
    remote_user: Option<&str>,
    devcontainer_config: Option<&Path>,
) -> Result<Vec<OsString>> {
    let mut args = vec![
        OsString::from("up"),
        OsString::from("--workspace-folder"),
        repo_path.as_os_str().to_owned(),
    ];
    args.extend(devcontainer_config_args(devcontainer_config));
    if options.remove_existing_container {
        args.push(OsString::from("--remove-existing-container"));
    }
    if !config.personal_features.is_empty() {
        args.push(OsString::from("--additional-features"));
        args.push(serde_json::to_string(&config.personal_features)?.into());
    }
    for mount in &config.mounts {
        if mount.target.contains(REMOTE_USER_PLACEHOLDER) && remote_user.is_none() {
            continue;
        }
        args.push(OsString::from("--mount"));
        args.push(mount_spec(mount, home, remote_user)?.into());
    }
    args.extend(docker_path_args(config.container_engine));
    Ok(args)
}

fn docker_path_args(engine: ContainerEngine) -> Vec<OsString> {
    match engine {
        ContainerEngine::Docker => Vec::new(),
        ContainerEngine::Podman => vec!["--docker-path".into(), engine.program().into()],
    }
}

fn devcontainer_config_args(config: Option<&Path>) -> Vec<OsString> {
    config.map_or_else(Vec::new, |path| {
        vec![OsString::from("--config"), path.as_os_str().to_owned()]
    })
}

fn devcontainer_up(
    id: &RepoId,
    repo_path: &Path,
    config: &Config,
    options: UpOptions,
    devcontainer_config: &DevcontainerConfig,
) -> Result<()> {
    let needs_remote_user = config
        .mounts
        .iter()
        .any(|mount| mount.target.contains(REMOTE_USER_PLACEHOLDER));
    let remote_user = if needs_remote_user {
        match devcontainer_remote_user(
            id,
            repo_path,
            config.container_engine,
            devcontainer_config.path(),
        ) {
            Ok(user) => Some(user),
            Err(error) => {
                for mount in config
                    .mounts
                    .iter()
                    .filter(|mount| mount.target.contains(REMOTE_USER_PLACEHOLDER))
                {
                    eprintln!(
                        concat!(
                            "warning: skipping mount for {} (target {}): {:#}\n",
                            "         set \"remoteUser\" in devcontainer.json to enable it"
                        ),
                        id, mount.target, error
                    );
                }
                None
            }
        }
    } else {
        None
    };
    let args = devcontainer_up_args(
        config,
        repo_path,
        std::env::var_os("HOME").map(PathBuf::from).as_deref(),
        options,
        remote_user.as_deref(),
        devcontainer_config.path(),
    )?;
    let status = Command::new("devcontainer")
        .args(args)
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

fn prepare_repository(arg: Option<RepoId>) -> Result<RepoId> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_or_clone(&ws, arg, &cwd)?;
    let devcontainer_config = resolve_devcontainer_config(&ws, &id, &path);
    ensure_claude_local(&path)?;
    ensure_git_exclude(&path)?;
    devcontainer_up(
        &id,
        &path,
        &ws.config,
        UpOptions::default(),
        &devcontainer_config,
    )?;
    Ok(id)
}

pub fn cmd_open(arg: Option<RepoId>) -> Result<()> {
    let id = prepare_repository(arg)?;
    attach_zellij(&id)
}

pub fn cmd_ensure(arg: Option<RepoId>) -> Result<()> {
    prepare_repository(arg).map(|_| ())
}

pub fn cmd_up(arg: Option<RepoId>, rebuild: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    let devcontainer_config = resolve_devcontainer_config(&ws, &id, &path);
    ensure_claude_local(&path)?;
    ensure_git_exclude(&path)?;
    devcontainer_up(
        &id,
        &path,
        &ws.config,
        UpOptions {
            remove_existing_container: rebuild,
        },
        &devcontainer_config,
    )
}

pub fn cmd_shell(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let explicit = arg.is_some();
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    let config = resolve_devcontainer_config(&ws, &id, &path);
    let argv = execution_argv(
        &id,
        &path,
        &cwd,
        explicit,
        ws.config.container_engine,
        config.path(),
        [OsString::from("bash")],
    )?;
    let status =
        interactive_devcontainer_exec(&path, ws.config.container_engine, config.path(), argv)?;
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
    let explicit = arg.is_some();
    let (id, path) = resolve_repo(&ws, arg, &cwd)?;
    let config = resolve_devcontainer_config(&ws, &id, &path);
    let argv = execution_argv(
        &id,
        &path,
        &cwd,
        explicit,
        ws.config.container_engine,
        config.path(),
        argv.into_iter().map(OsString::from),
    )?;
    interactive_devcontainer_exec(&path, ws.config.container_engine, config.path(), argv)
}

fn relative_execution_path(repo_root: &Path, cwd: &Path) -> Result<Option<PathBuf>> {
    let relative = cwd.strip_prefix(repo_root).map_err(|_| {
        anyhow::anyhow!(
            "worktree {} is outside the repository {}; the Dev Container cannot see it\ncreate the worktree inside the repository (for example, under .claude/worktrees/)",
            cwd.display(),
            repo_root.display()
        )
    })?;
    Ok((!relative.as_os_str().is_empty()).then(|| relative.to_path_buf()))
}

fn wrap_exec_argv<I>(container_dir: &Path, argv: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut wrapped = vec![
        OsString::from("sh"),
        OsString::from("-c"),
        OsString::from(r#"cd "$1" && shift && exec "$@""#),
        OsString::from("sh"),
        container_dir.as_os_str().to_owned(),
    ];
    wrapped.extend(argv);
    wrapped
}

fn execution_argv<I>(
    id: &RepoId,
    repo_root: &Path,
    cwd: &Path,
    explicit: bool,
    engine: ContainerEngine,
    config: Option<&Path>,
    argv: I,
) -> Result<Vec<OsString>>
where
    I: IntoIterator<Item = OsString>,
{
    let argv = argv.into_iter().collect::<Vec<_>>();
    if explicit {
        return Ok(argv);
    }
    let canonical_root = repo_root
        .canonicalize()
        .with_context(|| format!("failed to resolve repository path {}", repo_root.display()))?;
    let canonical_cwd = cwd
        .canonicalize()
        .with_context(|| format!("failed to resolve current directory {}", cwd.display()))?;
    let Some(relative) = relative_execution_path(&canonical_root, &canonical_cwd)? else {
        return Ok(argv);
    };
    let workspace_folder = devcontainer_workspace_folder(id, repo_root, engine, config)?;
    Ok(wrap_exec_argv(&workspace_folder.join(relative), argv))
}

fn devcontainer_exec_args(
    repo_path: &Path,
    engine: ContainerEngine,
    config: Option<&Path>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("exec"),
        OsString::from("--workspace-folder"),
        repo_path.as_os_str().to_owned(),
    ];
    args.extend(devcontainer_config_args(config));
    args.extend(docker_path_args(engine));
    args
}

fn interactive_devcontainer_exec<I, S>(
    repo_path: &Path,
    engine: ContainerEngine,
    config: Option<&Path>,
    argv: I,
) -> Result<ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let args = devcontainer_exec_args(repo_path, engine, config);
    Command::new("devcontainer")
        .args(args)
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

fn template_reference(template: &str) -> String {
    if template.contains('/') {
        template.to_owned()
    } else {
        format!("ghcr.io/devcontainers/templates/{template}:latest")
    }
}

pub fn cmd_local_config_init(arg: Option<RepoId>, template: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, repo_path) = resolve_repo(&ws, arg, &cwd)?;
    let config_path = local_config_path(&ws, &id);
    match resolve_devcontainer_config(&ws, &id, &repo_path) {
        DevcontainerConfig::Repository => {
            bail!("repository {id} has its own devcontainer.json")
        }
        DevcontainerConfig::Local(_) => {
            bail!("local config already exists at {}", config_path.display())
        }
        DevcontainerConfig::None => {}
    }

    let config_root = ws.local_config_root(&id.owner, &id.name);
    fs::create_dir_all(&config_root)
        .with_context(|| format!("failed to create {}", config_root.display()))?;
    let status = Command::new("devcontainer")
        .args(["templates", "apply", "--workspace-folder"])
        .arg(&config_root)
        .args([
            "-t",
            &template_reference(template),
            "--omit-paths",
            r#"[".github/*"]"#,
        ])
        .status()
        .with_context(|| format!("failed to apply Dev Container template for {id}"));
    match status {
        Ok(status) if status.success() && config_path.is_file() => {
            println!("{}", config_path.display());
            Ok(())
        }
        Ok(status) => {
            let _ = fs::remove_dir_all(&config_root);
            if status.success() {
                bail!(
                    "Dev Container template did not create {}",
                    config_path.display()
                );
            }
            bail!("devcontainer templates apply failed for repository {id}")
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&config_root);
            Err(error)
        }
    }
}

pub fn cmd_local_config_path(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, _) = resolve_repo(&ws, arg, &cwd)?;
    let path = local_config_path(&ws, &id);
    println!("{}", path.display());
    if !path.is_file() {
        eprintln!("note: not created yet");
    }
    Ok(())
}

pub fn cmd_local_config_rm(arg: Option<RepoId>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ws = load_workspace(&cwd)?;
    let (id, _) = resolve_repo(&ws, arg, &cwd)?;
    let config_root = ws.local_config_root(&id.owner, &id.name);
    if !config_root.exists() {
        bail!("local config does not exist at {}", config_root.display());
    }
    fs::remove_dir_all(&config_root)
        .with_context(|| format!("failed to remove {}", config_root.display()))?;
    let owner_root = ws.root.join("local-configs").join(&id.owner);
    match fs::remove_dir(&owner_root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to remove {}", owner_root.display()))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(contents: &str) -> Config {
        toml::from_str(&format!("projects_dir = \"projects\"\n{contents}")).unwrap()
    }

    #[test]
    fn resolves_devcontainer_config_from_presence() {
        let local =
            PathBuf::from("/workspace/local-configs/owner/repo/.devcontainer/devcontainer.json");
        assert_eq!(
            config_from_presence(true, true, local.clone()),
            DevcontainerConfig::Repository
        );
        assert_eq!(
            config_from_presence(false, true, local.clone()),
            DevcontainerConfig::Local(local.clone())
        );
        assert_eq!(
            config_from_presence(false, false, local),
            DevcontainerConfig::None
        );
    }

    #[test]
    fn expands_short_template_name_and_preserves_full_reference() {
        assert_eq!(
            template_reference("rust"),
            "ghcr.io/devcontainers/templates/rust:latest"
        );
        assert_eq!(
            template_reference("example.com/team/template:1"),
            "example.com/team/template:1"
        );
    }

    #[test]
    fn adds_config_to_read_configuration_args() {
        assert_eq!(
            devcontainer_read_configuration_args(
                Path::new("/repo"),
                ContainerEngine::Docker,
                Some(Path::new("/local/devcontainer.json")),
            ),
            vec![
                "read-configuration",
                "--workspace-folder",
                "/repo",
                "--include-merged-configuration",
                "--config",
                "/local/devcontainer.json",
            ]
        );
        assert_eq!(
            devcontainer_read_configuration_args(Path::new("/repo"), ContainerEngine::Docker, None,),
            vec![
                "read-configuration",
                "--workspace-folder",
                "/repo",
                "--include-merged-configuration",
            ]
        );
    }

    #[test]
    fn exec_args_include_config_only_when_provided() {
        assert_eq!(
            devcontainer_exec_args(
                Path::new("/repo"),
                ContainerEngine::Docker,
                Some(Path::new("/local/devcontainer.json")),
            ),
            vec![
                "exec",
                "--workspace-folder",
                "/repo",
                "--config",
                "/local/devcontainer.json",
            ]
        );
        assert_eq!(
            devcontainer_exec_args(Path::new("/repo"), ContainerEngine::Docker, None),
            vec!["exec", "--workspace-folder", "/repo"]
        );
    }

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
                ("yattulab/qi-bot-rs".into(), false),
                ("Gabuniku/foo".into(), false),
            ]),
            vec!["Gabuniku/foo", "yattulab/qi-bot-rs"]
        );
    }

    #[test]
    fn marks_local_config_in_repo_lines() {
        assert_eq!(
            sorted_repo_lines(vec![("owner/repo".into(), true)]),
            vec!["owner/repo (local config)"]
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
    fn aligns_repo_user_lines_with_local_config_marker() {
        assert_eq!(
            sorted_repo_user_lines(vec![
                ("owner/one (local config)".into(), "vscode".into()),
                ("owner/two".into(), "node".into()),
            ]),
            vec![
                "owner/one (local config)   vscode",
                "owner/two                  node",
            ]
        );
    }

    #[test]
    fn serializes_list_entries_with_optional_remote_user() {
        let entries = vec![
            ListEntry {
                repo: "owner/one".into(),
                path: "/work/owner/one".into(),
                remote_user: None,
                local_config: false,
            },
            ListEntry {
                repo: "owner/two".into(),
                path: "/work/owner/two".into(),
                remote_user: Some("vscode".into()),
                local_config: true,
            },
        ];

        assert_eq!(
            serde_json::to_value(entries).unwrap(),
            serde_json::json!([
                {"repo": "owner/one", "path": "/work/owner/one", "local_config": false},
                {"repo": "owner/two", "path": "/work/owner/two", "remote_user": "vscode", "local_config": true}
            ])
        );
    }

    #[test]
    fn serializes_ssh_target_output() {
        let output = SshTargetOutput {
            repo: "owner/repo".into(),
            user: "vscode".into(),
            target: "vscode@devctl-owner--repo".into(),
        };

        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({
                "repo": "owner/repo",
                "user": "vscode",
                "target": "vscode@devctl-owner--repo"
            })
        );
    }

    fn context_output() -> ContextOutput {
        ContextOutput {
            repo: "owner/repo".into(),
            workspace_root: "/work".into(),
            repo_path: "/work/projects/owner/repo".into(),
            container_engine: ContainerEngine::Docker,
            remote_user: "vscode".into(),
            session: "dev-owner--repo".into(),
            ssh_target: "vscode@devctl-owner--repo".into(),
        }
    }

    #[test]
    fn serializes_context_output_in_the_documented_shape() {
        assert_eq!(
            serde_json::to_value(context_output()).unwrap(),
            serde_json::json!({
                "repo": "owner/repo",
                "workspace_root": "/work",
                "repo_path": "/work/projects/owner/repo",
                "container_engine": "docker",
                "remote_user": "vscode",
                "session": "dev-owner--repo",
                "ssh_target": "vscode@devctl-owner--repo"
            })
        );
    }

    #[test]
    fn formats_context_as_key_value_lines() {
        assert_eq!(
            format_context(&context_output()),
            "repo: owner/repo\n\
             workspace_root: /work\n\
             repo_path: /work/projects/owner/repo\n\
             container_engine: docker\n\
             remote_user: vscode\n\
             session: dev-owner--repo\n\
             ssh_target: vscode@devctl-owner--repo"
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
    fn reads_workspace_folder_from_json_lines() {
        let output = r#"log line
{"workspace":{"workspaceFolder":"/workspaces/repo"},"mergedConfiguration":{}}
"#;
        assert_eq!(
            workspace_folder_from_json_lines(output).as_deref(),
            Some(Path::new("/workspaces/repo"))
        );
    }

    #[test]
    fn ignores_lines_without_workspace_folder() {
        let output = "log line\n{\"workspace\":{}}\n";
        assert_eq!(workspace_folder_from_json_lines(output), None);
    }

    #[test]
    fn calculates_relative_execution_path() {
        assert_eq!(
            relative_execution_path(
                Path::new("/repos/project"),
                Path::new("/repos/project/src/bin")
            )
            .unwrap(),
            Some(PathBuf::from("src/bin"))
        );
        assert_eq!(
            relative_execution_path(Path::new("/repos/project"), Path::new("/repos/project"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn rejects_execution_path_outside_repository() {
        let error = relative_execution_path(
            Path::new("/repos/project"),
            Path::new("/tmp/project-worktree"),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "worktree /tmp/project-worktree is outside the repository /repos/project; the Dev Container cannot see it\ncreate the worktree inside the repository (for example, under .claude/worktrees/)"
        );
    }

    #[test]
    fn wraps_exec_argv_with_container_directory() {
        assert_eq!(
            wrap_exec_argv(
                Path::new("/workspaces/repo/src"),
                [OsString::from("cargo"), OsString::from("test")]
            ),
            vec![
                "sh",
                "-c",
                r#"cd "$1" && shift && exec "$@""#,
                "sh",
                "/workspaces/repo/src",
                "cargo",
                "test"
            ]
        );
    }

    #[test]
    fn adds_personal_features_as_json() {
        let config = config(
            r#"
[personal_features]
"ghcr.io/devcontainers/features/sshd:1" = { version = "latest" }
"#,
        );
        let args = devcontainer_up_args(
            &config,
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--additional-features",
                r#"{"ghcr.io/devcontainers/features/sshd:1":{"version":"latest"}}"#,
            ]
        );
    }

    #[test]
    fn up_args_include_config_only_when_provided() {
        let with_config = devcontainer_up_args(
            &config(""),
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            Some(Path::new("/local/devcontainer.json")),
        )
        .unwrap();
        let without_config = devcontainer_up_args(
            &config(""),
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            with_config,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--config",
                "/local/devcontainer.json"
            ]
        );
        assert_eq!(without_config, vec!["up", "--workspace-folder", "/repo"]);
    }

    #[test]
    fn adds_mounts_with_expanded_absolute_sources() {
        let config = config(
            r#"
[[mounts]]
source = "~/.ssh/devcontainer_authorized_keys"
target = "/home/vscode/.ssh/authorized_keys"
"#,
        );
        let args = devcontainer_up_args(
            &config,
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--mount",
                "type=bind,source=/home/me/.ssh/devcontainer_authorized_keys,target=/home/vscode/.ssh/authorized_keys",
            ]
        );
    }

    #[test]
    fn replaces_remote_user_in_mount_target() {
        let config = config(
            r#"
[[mounts]]
source = "~/.ssh/devcontainer_authorized_keys"
target = "/home/{remoteUser}/.ssh/authorized_keys"
"#,
        );
        let args = devcontainer_up_args(
            &config,
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            Some("vscode"),
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--mount",
                "type=bind,source=/home/me/.ssh/devcontainer_authorized_keys,target=/home/vscode/.ssh/authorized_keys",
            ]
        );
    }

    #[test]
    fn omits_unresolved_remote_user_mount_and_keeps_other_mounts() {
        let config = config(
            r#"
[[mounts]]
source = "/host/keys"
target = "/home/{remoteUser}/.ssh/authorized_keys"

[[mounts]]
source = "/host/cache"
target = "/cache"
"#,
        );
        let args = devcontainer_up_args(
            &config,
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--mount",
                "type=bind,source=/host/cache,target=/cache",
            ]
        );
    }

    #[test]
    fn replaces_every_remote_user_placeholder_in_mount_target() {
        let config = config(
            r#"
[[mounts]]
source = "/host/{remoteUser}"
target = "/home/{remoteUser}/{remoteUser}"
"#,
        );
        let args = devcontainer_up_args(
            &config,
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            Some("node"),
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--mount",
                "type=bind,source=/host/{remoteUser},target=/home/node/node",
            ]
        );
    }

    #[test]
    fn omits_personal_feature_and_mount_flags_when_empty() {
        let args = devcontainer_up_args(
            &config(""),
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(args, vec!["up", "--workspace-folder", "/repo"]);
    }

    #[test]
    fn omits_docker_path_for_docker() {
        assert!(docker_path_args(ContainerEngine::Docker).is_empty());
    }

    #[test]
    fn selects_podman_as_docker_path() {
        assert_eq!(
            docker_path_args(ContainerEngine::Podman),
            vec!["--docker-path", "podman"]
        );
    }

    #[test]
    fn adds_podman_docker_path_to_up() {
        let args = devcontainer_up_args(
            &config("container_engine = \"podman\"\n"),
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--docker-path",
                "podman"
            ]
        );
    }

    #[test]
    fn adds_remove_existing_container_when_requested() {
        let args = devcontainer_up_args(
            &config(""),
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions {
                remove_existing_container: true,
            },
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--remove-existing-container"
            ]
        );
    }

    #[test]
    fn combines_rebuild_with_podman_docker_path() {
        let args = devcontainer_up_args(
            &config("container_engine = \"podman\"\n"),
            Path::new("/repo"),
            Some(Path::new("/home/me")),
            UpOptions {
                remove_existing_container: true,
            },
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            args,
            vec![
                "up",
                "--workspace-folder",
                "/repo",
                "--remove-existing-container",
                "--docker-path",
                "podman"
            ]
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
