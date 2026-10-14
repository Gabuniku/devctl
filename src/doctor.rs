use std::ffi::OsString;

use anyhow::{bail, Result};

use crate::command::{capture_stdout, capture_stdout_quiet};
use crate::config::{load_workspace, ContainerEngine};

pub fn cmd_doctor() -> Result<()> {
    let mut failed = false;
    let current_dir = std::env::current_dir()?;
    let engine = load_workspace(&current_dir)
        .map(|workspace| workspace.config.container_engine)
        .unwrap_or_default();
    let engine_program = engine.program();
    let commands = ["git", "gh", engine_program, "devcontainer", "zellij"];

    for program in commands {
        let args = [OsString::from("--version")];
        match capture_stdout(program, &args) {
            Ok(output) => {
                let version = extract_version(&output)
                    .unwrap_or_else(|| output.lines().next().unwrap_or("").trim());
                println!("{program:<12} OK  {version}");
            }
            Err(_) => {
                println!("{program:<12} FAIL");
                failed = true;
            }
        }
    }

    println!();
    failed |= !print_check(
        "GitHub auth",
        capture_stdout_quiet(
            "gh",
            &[
                OsString::from("auth"),
                OsString::from("status"),
                OsString::from("--active"),
            ],
        )
        .is_ok(),
    );
    let engine_label = match engine {
        ContainerEngine::Docker => "Docker daemon",
        ContainerEngine::Podman => "Podman",
    };
    failed |= !print_check(
        engine_label,
        capture_stdout_quiet(engine_program, &[OsString::from("info")]).is_ok(),
    );

    if failed {
        bail!("one or more doctor checks failed");
    }
    Ok(())
}

fn print_check(label: &str, ok: bool) -> bool {
    println!("{label:<14} {}", if ok { "OK" } else { "FAIL" });
    ok
}

fn extract_version(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.'))
        .find(|word| {
            word.chars().next().is_some_and(|c| c.is_ascii_digit())
                && word.contains('.')
                && word.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
}

#[cfg(test)]
mod tests {
    use super::extract_version;

    #[test]
    fn extracts_version_from_common_outputs() {
        assert_eq!(extract_version("git version 2.51.0"), Some("2.51.0"));
        assert_eq!(
            extract_version("gh version 2.65.0 (2025-01-01)"),
            Some("2.65.0")
        );
        assert_eq!(
            extract_version("Docker version 27.5.1, build abc123"),
            Some("27.5.1")
        );
    }

    #[test]
    fn returns_none_when_output_has_no_version() {
        assert_eq!(extract_version("version unknown"), None);
    }
}
