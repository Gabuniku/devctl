use std::ffi::OsString;
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{bail, Context, Result};

pub fn run_inherited(program: &str, args: &[OsString]) -> Result<ExitStatus> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn {program}"))?
        .wait()
        .with_context(|| format!("failed to wait for {program}"))
}

pub fn run_checked(program: &str, args: &[OsString], ctx: &str) -> Result<()> {
    let status = run_inherited(program, args).with_context(|| ctx.to_owned())?;
    if !status.success() {
        bail!("{ctx}: {status}");
    }
    Ok(())
}

pub fn capture_stdout(program: &str, args: &[OsString]) -> Result<String> {
    capture_stdout_with_stderr(program, args, Stdio::inherit())
}

pub fn capture_stdout_quiet(program: &str, args: &[OsString]) -> Result<String> {
    capture_stdout_with_stderr(program, args, Stdio::null())
}

fn capture_stdout_with_stderr(program: &str, args: &[OsString], stderr: Stdio) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(stderr)
        .output()
        .with_context(|| format!("failed to run {program}"))?;
    if !output.status.success() {
        bail!("{program} failed: {}", output.status);
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("{program} produced non-UTF-8 output"))
        .map(|stdout| stdout.trim().to_owned())
}

pub fn command_exists(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}
