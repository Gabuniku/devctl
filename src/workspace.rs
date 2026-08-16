use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use anyhow::Result;

use crate::config::Workspace;
use crate::repo::RepoId;

pub fn resolve_repo(
    _ws: &Workspace,
    _arg: Option<RepoId>,
    _cwd: &Path,
) -> Result<(RepoId, PathBuf)> {
    unimplemented!()
}

pub fn resolve_or_clone(
    _ws: &Workspace,
    _arg: Option<RepoId>,
    _cwd: &Path,
) -> Result<(RepoId, PathBuf)> {
    unimplemented!()
}

pub fn claude_local_contents() -> &'static str {
    unimplemented!()
}

pub fn ensure_claude_local(_repo_path: &Path) -> Result<()> {
    unimplemented!()
}

pub fn exclude_with_entry(_current: &str, _entry: &str) -> Option<String> {
    unimplemented!()
}

pub fn ensure_git_exclude(_repo_path: &Path) -> Result<()> {
    unimplemented!()
}

pub fn devcontainer_up(_id: &RepoId, _repo_path: &Path) -> Result<()> {
    unimplemented!()
}

pub fn zellij_session_name(_id: &RepoId) -> String {
    unimplemented!()
}

pub fn attach_zellij(_id: &RepoId) -> Result<()> {
    unimplemented!()
}

pub fn cmd_open(_arg: Option<RepoId>) -> Result<()> {
    unimplemented!()
}

pub fn cmd_up(_arg: Option<RepoId>) -> Result<()> {
    unimplemented!()
}

pub fn cmd_shell(_arg: Option<RepoId>) -> Result<()> {
    unimplemented!()
}

pub fn cmd_exec(_arg: Option<RepoId>, _argv: Vec<String>) -> Result<ExitStatus> {
    unimplemented!()
}
