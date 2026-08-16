use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Result;

use crate::config::Workspace;

#[derive(Clone, Debug)]
pub struct RepoId {
    pub owner: String,
    pub name: String,
}

impl FromStr for RepoId {
    type Err = anyhow::Error;

    fn from_str(_value: &str) -> Result<Self> {
        unimplemented!()
    }
}

impl Display for RepoId {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!()
    }
}

pub fn repo_path(_ws: &Workspace, _id: &RepoId) -> PathBuf {
    unimplemented!()
}

pub fn repo_id_from_path(_ws: &Workspace, _path: &Path) -> Option<RepoId> {
    unimplemented!()
}

pub fn is_git_worktree(_path: &Path) -> bool {
    unimplemented!()
}

pub fn git_toplevel(_cwd: &Path) -> Result<PathBuf> {
    unimplemented!()
}

pub fn clone_repo(_id: &RepoId, _dest: &Path) -> Result<()> {
    unimplemented!()
}
