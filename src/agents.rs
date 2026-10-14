use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const BEGIN_MARKER: &str =
    "<!-- BEGIN devctl managed block. Manual changes inside may be overwritten. -->";
const END_MARKER: &str = "<!-- END devctl managed block -->";

const INSTRUCTIONS: &str = "# Development VM\n\
\n\
- Source files live on the host VM; edit them there.\n\
- Project runtimes live in Dev Containers. Run project commands with `devctl exec -- <command>` from inside the repository.\n\
- Use `devctl up` to start the environment of an existing repository; use `devctl ensure owner/repo` when it may not be cloned yet (non-interactive, idempotent).\n\
- `devctl exec` refuses to run from a git worktree. Run it from the main repository root and `cd` inside the container, e.g. `devctl exec -- bash -lc 'cd .claude/worktrees/<name> && <command>'`. Create worktrees inside the repository (e.g. `.claude/worktrees/`) so the container can see them.\n\
- Do not install project dependencies directly on the host VM.\n\
- Do not run `devctl open` or `devctl shell`: they are interactive and will not return.\n\
- devctl requires a Dev Container. For a repository without one, follow that repository's own instructions instead.";

pub fn managed_block() -> String {
    format!("{BEGIN_MARKER}\n{INSTRUCTIONS}\n{END_MARKER}\n")
}

pub fn upsert_managed_block(current: Option<&str>, block: &str) -> Result<Option<String>> {
    let Some(current) = current else {
        return Ok(Some(block.to_owned()));
    };

    let begins = current.match_indices(BEGIN_MARKER).collect::<Vec<_>>();
    let ends = current.match_indices(END_MARKER).collect::<Vec<_>>();
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => {
            let mut updated = current.to_owned();
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            if !updated.is_empty() {
                updated.push('\n');
            }
            updated.push_str(block);
            Ok(Some(updated))
        }
        ([(begin, _)], [(end, _)]) if begin < end => {
            let end = end + END_MARKER.len();
            let replacement = block.strip_suffix('\n').unwrap_or(block);
            let updated = format!("{}{replacement}{}", &current[..*begin], &current[end..]);
            if updated == current {
                Ok(None)
            } else {
                Ok(Some(updated))
            }
        }
        _ => bail!("invalid devctl managed block markers"),
    }
}

fn install_file(path: &Path, block: &str) -> Result<bool> {
    let current = match fs::read_to_string(path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()))
        }
    };
    let updated = upsert_managed_block(current.as_deref(), block)
        .with_context(|| format!("invalid managed block in {}", path.display()))?;
    let Some(updated) = updated else {
        return Ok(false);
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn instruction_paths(home: &Path, codex_home: Option<&Path>) -> [PathBuf; 2] {
    [
        home.join(".claude/CLAUDE.md"),
        codex_home.map_or_else(
            || home.join(".codex/AGENTS.md"),
            |path| path.join("AGENTS.md"),
        ),
    ]
}

fn install(home: &Path, codex_home: Option<&Path>) -> Result<()> {
    let block = managed_block();
    for path in instruction_paths(home, codex_home) {
        let status = if install_file(&path, &block)? {
            "updated"
        } else {
            "unchanged"
        };
        println!("{status} {}", path.display());
    }
    Ok(())
}

pub fn cmd_install(print: bool) -> Result<()> {
    let block = managed_block();
    if print {
        print!("{block}");
        return Ok(());
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let codex_home = std::env::var_os("CODEX_HOME");
    install(Path::new(&home), codex_home.as_deref().map(Path::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_block_for_missing_file() {
        assert_eq!(
            upsert_managed_block(None, "BLOCK\n").unwrap(),
            Some("BLOCK\n".to_owned())
        );
    }

    #[test]
    fn appends_after_blank_line() {
        let current = "user text";
        assert_eq!(
            upsert_managed_block(Some(current), "BLOCK\n").unwrap(),
            Some("user text\n\nBLOCK\n".to_owned())
        );
    }

    #[test]
    fn appends_to_newline_terminated_text() {
        assert_eq!(
            upsert_managed_block(Some("user text\n"), "BLOCK\n").unwrap(),
            Some("user text\n\nBLOCK\n".to_owned())
        );
    }

    #[test]
    fn replaces_only_managed_block() {
        let current = format!("prefix\n{BEGIN_MARKER}\nold\n{END_MARKER}\nsuffix");
        let block = format!("{BEGIN_MARKER}\nnew\n{END_MARKER}\n");
        assert_eq!(
            upsert_managed_block(Some(&current), &block).unwrap(),
            Some(format!("prefix\n{BEGIN_MARKER}\nnew\n{END_MARKER}\nsuffix"))
        );
    }

    #[test]
    fn returns_none_for_identical_contents() {
        let block = managed_block();
        assert_eq!(upsert_managed_block(Some(&block), &block).unwrap(), None);
    }

    #[test]
    fn rejects_begin_marker_only() {
        assert!(upsert_managed_block(Some(BEGIN_MARKER), "BLOCK\n").is_err());
    }

    #[test]
    fn rejects_end_marker_only() {
        assert!(upsert_managed_block(Some(END_MARKER), "BLOCK\n").is_err());
    }

    #[test]
    fn rejects_reversed_markers() {
        let current = format!("{END_MARKER}\n{BEGIN_MARKER}");
        assert!(upsert_managed_block(Some(&current), "BLOCK\n").is_err());
    }

    #[test]
    fn rejects_duplicate_markers() {
        let current = format!("{BEGIN_MARKER}\n{END_MARKER}\n{BEGIN_MARKER}\n{END_MARKER}");
        assert!(upsert_managed_block(Some(&current), "BLOCK\n").is_err());
    }

    #[test]
    fn uses_codex_home_when_set() {
        assert_eq!(
            instruction_paths(Path::new("/home/user"), Some(Path::new("/opt/codex"))),
            [
                PathBuf::from("/home/user/.claude/CLAUDE.md"),
                PathBuf::from("/opt/codex/AGENTS.md")
            ]
        );
    }

    #[test]
    fn defaults_codex_home_below_home() {
        assert_eq!(
            instruction_paths(Path::new("/home/user"), None),
            [
                PathBuf::from("/home/user/.claude/CLAUDE.md"),
                PathBuf::from("/home/user/.codex/AGENTS.md")
            ]
        );
    }
}
