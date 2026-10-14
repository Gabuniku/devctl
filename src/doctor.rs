use std::ffi::OsString;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::command::{capture_stdout, capture_stdout_quiet};
use crate::config::{load_workspace, ContainerEngine};

#[derive(Debug, Serialize)]
struct CommandCheck {
    name: String,
    ok: bool,
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    ok: bool,
}

#[derive(Debug, Serialize)]
struct DoctorOutput {
    ok: bool,
    container_engine: ContainerEngine,
    commands: Vec<CommandCheck>,
    checks: Vec<Check>,
}

fn diagnose(engine: ContainerEngine) -> DoctorOutput {
    let engine_program = engine.program();
    let commands = ["git", "gh", engine_program, "devcontainer", "zellij"];
    let commands = commands
        .into_iter()
        .map(|program| {
            let args = [OsString::from("--version")];
            match capture_stdout(program, &args) {
                Ok(output) => {
                    let version = extract_version(&output)
                        .unwrap_or_else(|| output.lines().next().unwrap_or("").trim())
                        .to_owned();
                    CommandCheck {
                        name: program.to_owned(),
                        ok: true,
                        version: Some(version),
                    }
                }
                Err(_) => CommandCheck {
                    name: program.to_owned(),
                    ok: false,
                    version: None,
                },
            }
        })
        .collect::<Vec<_>>();
    let checks = vec![
        Check {
            name: "github_auth".to_owned(),
            ok: capture_stdout_quiet(
                "gh",
                &[
                    OsString::from("auth"),
                    OsString::from("status"),
                    OsString::from("--active"),
                ],
            )
            .is_ok(),
        },
        Check {
            name: "container_engine".to_owned(),
            ok: capture_stdout_quiet(engine_program, &[OsString::from("info")]).is_ok(),
        },
    ];
    let ok = commands.iter().all(|check| check.ok) && checks.iter().all(|check| check.ok);
    DoctorOutput {
        ok,
        container_engine: engine,
        commands,
        checks,
    }
}

pub fn cmd_doctor(json: bool) -> Result<()> {
    let current_dir = std::env::current_dir()?;
    let engine = load_workspace(&current_dir)
        .map(|workspace| workspace.config.container_engine)
        .unwrap_or_default();
    let output = diagnose(engine);

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for command in &output.commands {
            if let Some(version) = &command.version {
                println!("{:<12} OK  {version}", command.name);
            } else {
                println!("{:<12} FAIL", command.name);
            }
        }
        println!();
        print_check("GitHub auth", output.checks[0].ok);
        let engine_label = match engine {
            ContainerEngine::Docker => "Docker daemon",
            ContainerEngine::Podman => "Podman",
        };
        print_check(engine_label, output.checks[1].ok);
    }

    if !output.ok {
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
    use super::*;

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

    #[test]
    fn serializes_doctor_output_with_null_version() {
        let output = DoctorOutput {
            ok: false,
            container_engine: ContainerEngine::Podman,
            commands: vec![CommandCheck {
                name: "git".into(),
                ok: false,
                version: None,
            }],
            checks: vec![Check {
                name: "github_auth".into(),
                ok: true,
            }],
        };

        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({
                "ok": false,
                "container_engine": "podman",
                "commands": [{"name": "git", "ok": false, "version": null}],
                "checks": [{"name": "github_auth", "ok": true}]
            })
        );
    }
}
