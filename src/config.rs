use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const CONFIG_FILE: &str = "devctl.toml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub projects_dir: String,
}

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub config: Config,
}

impl Workspace {
    pub fn projects_root(&self) -> PathBuf {
        self.root.join(&self.config.projects_dir)
    }
}

pub fn find_workspace_root(start: &Path) -> Result<PathBuf> {
    find_workspace_root_with_home(
        start,
        std::env::var_os("HOME").map(PathBuf::from).as_deref(),
    )
}

fn find_workspace_root_with_home(start: &Path, home: Option<&Path>) -> Result<PathBuf> {
    for directory in start.ancestors() {
        if directory.join(CONFIG_FILE).is_file() {
            return Ok(directory.to_path_buf());
        }
    }

    if let Some(default_root) = home.map(|home| home.join("workspaces")) {
        if default_root.join(CONFIG_FILE).is_file() {
            return Ok(default_root);
        }
    }

    bail!("devctl workspace not found; run `devctl init` first")
}

pub fn load_workspace(start: &Path) -> Result<Workspace> {
    let root = find_workspace_root(start)?;
    let config_path = root.join(CONFIG_FILE);
    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    Ok(Workspace { root, config })
}

pub fn init_workspace(dir: &Path) -> Result<()> {
    let config_path = dir.join(CONFIG_FILE);
    let mut config_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config_path)
        .with_context(|| format!("refusing to overwrite {}", config_path.display()))?;
    config_file
        .write_all(b"projects_dir = \"projects\"\n")
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    fs::create_dir_all(dir.join("projects")).context("failed to create projects directory")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("devctl-test-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_workspace_in_parent() {
        let temp = TestDir::new();
        fs::write(temp.0.join(CONFIG_FILE), "projects_dir = \"projects\"\n").unwrap();
        let nested = temp.0.join("projects/owner/repo");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            find_workspace_root_with_home(&nested, None).unwrap(),
            temp.0
        );
    }

    #[test]
    fn reports_when_workspace_is_not_found() {
        let temp = TestDir::new();
        let fake_home = temp.0.join("home");
        fs::create_dir(&fake_home).unwrap();

        let error = find_workspace_root_with_home(&temp.0, Some(&fake_home)).unwrap_err();
        assert!(error.to_string().contains("workspace not found"));
    }

    #[test]
    fn refuses_existing_config_without_creating_projects() {
        let temp = TestDir::new();
        let config_path = temp.0.join(CONFIG_FILE);
        fs::write(&config_path, "existing = true\n").unwrap();

        assert!(init_workspace(&temp.0).is_err());
        assert_eq!(
            fs::read_to_string(config_path).unwrap(),
            "existing = true\n"
        );
        assert!(!temp.0.join("projects").exists());
    }
}
