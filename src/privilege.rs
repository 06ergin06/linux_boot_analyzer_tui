//! Privileged `systemctl` through `sudo -S`: the password goes to stdin and
//! unit names are passed as arguments, never through a shell.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::i18n;

const PAM_FILES: [&str; 3] = [
    "/etc/pam.d/sudo",
    "/etc/pam.d/system-auth",
    "/etc/pam.d/common-auth",
];

// `pam_fprintd` is `sufficient` on most distributions, so it runs before
// `pam_unix` asks for the password and paints a fingerprint prompt over the
// TUI. An address with no bus behind it makes the module give up at once and
// authentication falls through to the password already on stdin; `env -u`
// drops the address again so the real command still sees the system bus.
const DISABLED_BUS: &str = "unix:path=/nonexistent-disabled-fprintd-bus";

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
            Action::Enable => i18n::t("tui_action_enable"),
            Action::Disable => i18n::t("tui_action_disable"),
            Action::Start => i18n::t("tui_action_start"),
            Action::Stop => i18n::t("tui_action_stop"),
            Action::Mask => i18n::t("tui_action_mask"),
            Action::Unmask => i18n::t("tui_action_unmask"),
        }
    }

    pub fn prompt_key(self) -> &'static str {
        match self {
            Action::Enable => "tui_prompt_enable",
            Action::Disable => "tui_prompt_disable",
            Action::Start => "tui_prompt_start",
            Action::Stop => "tui_prompt_stop",
            Action::Mask => "tui_prompt_mask",
            Action::Unmask => "tui_prompt_unmask",
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

pub fn systemctl(action: Action, unit: &str, password: &str) -> Result<String, String> {
    systemctl_many(action.verb(), &[unit], password)
}

pub fn systemctl_many(verb: &str, units: &[&str], password: &str) -> Result<String, String> {
    if units.is_empty() {
        return Ok(String::new());
    }

    let mut args: Vec<&str> = vec!["systemctl", "--no-pager", "--no-ask-password", verb];
    args.extend_from_slice(units);
    run(&args, password)
}

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
        Ok(i18n::fill(
            i18n::t("tui_disabled_count"),
            &[&disabled.to_string()],
        ))
    } else {
        Err(failures.join("\n"))
    }
}

fn pam_fprintd_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();

    *CACHED.get_or_init(|| {
        PAM_FILES
            .iter()
            .any(|path| fs::read_to_string(path).is_ok_and(|config| loads_fprintd(&config)))
    })
}

fn loads_fprintd(config: &str) -> bool {
    config
        .lines()
        .map(str::trim_start)
        .any(|line| !line.starts_with('#') && line.contains("pam_fprintd"))
}

pub fn run(args: &[&str], password: &str) -> Result<String, String> {
    let mut command = Command::new("sudo");
    command.arg("-S");

    if pam_fprintd_enabled() {
        command
            .args(["env", "-u", "DBUS_SYSTEM_BUS_ADDRESS"])
            .env("DBUS_SYSTEM_BUS_ADDRESS", DISABLED_BUS);
    }

    let mut child = command
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

pub fn is_auth_failure(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("incorrect password")
        || lower.contains("sorry, try again")
        || lower.contains("no password was provided")
}

#[cfg(test)]
mod tests {
    use super::loads_fprintd;

    #[test]
    fn fprintd_enabled_pam_stacks_are_detected() {
        assert!(loads_fprintd("auth sufficient pam_fprintd.so\n"));
        assert!(loads_fprintd(
            "#%PAM-1.0\nauth\tsufficient\tpam_fprintd.so\n"
        ));
        assert!(!loads_fprintd("auth include system-auth\n"));
        assert!(!loads_fprintd("#auth sufficient pam_fprintd.so\n"));
        assert!(!loads_fprintd(""));
    }
}
