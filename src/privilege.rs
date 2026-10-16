//! Runs privileged `systemctl` commands through `sudo -S`.
//!
//! The password is written to sudo's standard input; commands are passed as
//! arguments (never through a shell) so unit names cannot be interpreted.

use std::io::Write;
use std::process::{Command, Stdio};

/// A unit action the UI can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Enable,
    Disable,
    Start,
    Stop,
    Mask,
    Unmask,
}

impl Action {
    /// Menu order.
    pub const ALL: [Action; 6] = [
        Action::Enable,
        Action::Disable,
        Action::Start,
        Action::Stop,
        Action::Mask,
        Action::Unmask,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Enable => "Enable",
            Action::Disable => "Disable",
            Action::Start => "Start",
            Action::Stop => "Stop",
            Action::Mask => "Mask",
            Action::Unmask => "Unmask",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Action::Enable => "enable",
            Action::Disable => "disable",
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Mask => "mask",
            Action::Unmask => "unmask",
        }
    }
}

/// Run `systemctl <verb> <unit>` as root.
pub fn systemctl(action: Action, unit: &str, password: &str) -> Result<String, String> {
    run(
        &[
            "systemctl",
            "--no-pager",
            "--no-ask-password",
            action.verb(),
            unit,
        ],
        password,
    )
}

/// Disable several units in sequence, reporting how many succeeded.
pub fn disable_all(units: &[String], password: &str) -> Result<String, String> {
    let mut disabled = 0;
    let mut failures = Vec::new();

    for unit in units {
        match systemctl(Action::Disable, unit, password) {
            Ok(_) => disabled += 1,
            Err(error) => failures.push(format!("{unit}: {error}")),
        }
    }

    if failures.is_empty() {
        Ok(format!("Disabled {disabled} service(s)."))
    } else {
        Err(failures.join("\n"))
    }
}

/// Run a command as root, feeding the password to `sudo -S`.
///
/// `LC_ALL=C` keeps sudo's error messages in English so callers can detect
/// authentication failures reliably.
pub fn run(args: &[&str], password: &str) -> Result<String, String> {
    let mut child = Command::new("sudo")
        .arg("-S")
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("could not run sudo: {err}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        writeln!(stdin, "{password}").map_err(|err| format!("could not send password: {err}"))?;
    }
    // Close stdin so sudo stops waiting for input.
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .map_err(|err| format!("sudo failed: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(stdout.trim().to_string())
    } else {
        let message = stderr.trim();
        if message.is_empty() {
            Err(stdout.trim().to_string())
        } else {
            Err(message.to_string())
        }
    }
}

/// Whether an error message looks like a rejected password.
pub fn is_auth_failure(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("incorrect password")
        || lower.contains("sorry, try again")
        || lower.contains("no password was provided")
}
