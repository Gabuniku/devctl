use std::ffi::OsString;
use std::fmt::{self, Display};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use anyhow::{bail, Context, Result};

use crate::command::{capture_stdout, run_checked};
use crate::config::Workspace;

#[derive(Clone, Debug)]
pub struct RepoId {
    pub owner: String,
    pub name: String,
}

impl FromStr for RepoId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let mut parts = value.split('/');
        let owner = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();

        if parts.next().is_some()
            || owner.is_empty()
            || name.is_empty()
            || matches!(owner, "." | "..")
            || matches!(name, "." | "..")
        {
            bail!("repository must be in non-empty owner/name format");
        }

        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

impl Display for RepoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.name)
    }
}

pub fn repo_path(ws: &Workspace, id: &RepoId) -> PathBuf {
    ws.projects_root().join(&id.owner).join(&id.name)
}

pub fn repo_id_from_path(ws: &Workspace, path: &Path) -> Option<RepoId> {
    let relative = path.strip_prefix(ws.projects_root()).ok()?;
    let mut components = relative.components();
    let owner = match components.next()? {
        Component::Normal(owner) => owner.to_str()?,
        _ => return None,
    };
    let name = match components.next()? {
        Component::Normal(name) => name.to_str()?,
        _ => return None,
    };
    if components.next().is_some() {
        return None;
    }

    format!("{owner}/{name}").parse().ok()
}

pub fn is_git_worktree(path: &Path) -> bool {
    let args = [
        OsString::from("-C"),
        path.as_os_str().to_owned(),
        OsString::from("rev-parse"),
        OsString::from("--is-inside-work-tree"),
    ];
    capture_stdout("git", &args).is_ok_and(|output| output == "true")
}

pub fn git_toplevel(cwd: &Path) -> Result<PathBuf> {
    let args = [
        OsString::from("-C"),
        cwd.as_os_str().to_owned(),
        OsString::from("rev-parse"),
        OsString::from("--show-toplevel"),
    ];
    capture_stdout("git", &args)
        .map(PathBuf::from)
        .with_context(|| format!("failed to find git toplevel from {}", cwd.display()))
}

pub fn clone_repo(id: &RepoId, dest: &Path) -> Result<()> {
    let args = [
        OsString::from("repo"),
        OsString::from("clone"),
        OsString::from(id.to_string()),
        dest.as_os_str().to_owned(),
    ];
    run_checked("gh", &args, &format!("failed to clone repository {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn workspace() -> Workspace {
        Workspace {
            root: PathBuf::from("/workspace"),
            config: Config {
                projects_dir: "projects".to_owned(),
            },
        }
    }

    #[test]
    fn parses_repo_id() {
        let id: RepoId = "Gabuniku/foo".parse().unwrap();
        assert_eq!(id.owner, "Gabuniku");
        assert_eq!(id.name, "foo");
        assert_eq!(id.to_string(), "Gabuniku/foo");
    }

    #[test]
    fn rejects_invalid_repo_ids() {
        for value in ["foo", "a/b/c", "/foo", "foo/", "./foo", "../foo", "foo/.."] {
            assert!(value.parse::<RepoId>().is_err(), "accepted {value}");
        }
    }

    #[test]
    fn builds_repo_path() {
        let id: RepoId = "Gabuniku/foo".parse().unwrap();
        assert_eq!(
            repo_path(&workspace(), &id),
            PathBuf::from("/workspace/projects/Gabuniku/foo")
        );
    }

    #[test]
    fn resolves_repo_id_from_managed_path() {
        let id =
            repo_id_from_path(&workspace(), Path::new("/workspace/projects/Gabuniku/foo")).unwrap();
        assert_eq!(id.to_string(), "Gabuniku/foo");
    }

    #[test]
    fn rejects_paths_outside_or_at_wrong_depth() {
        let ws = workspace();
        for path in [
            "/elsewhere/Gabuniku/foo",
            "/workspace/projects/Gabuniku",
            "/workspace/projects/Gabuniku/foo/extra",
        ] {
            assert!(
                repo_id_from_path(&ws, Path::new(path)).is_none(),
                "accepted {path}"
            );
        }
    }
}
