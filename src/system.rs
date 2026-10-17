//! Reads boot timing and systemd service information; commands run with `LC_ALL=C`.

use std::collections::HashMap;
use std::fs;
use std::process::Command;

use crate::autostart::{self, AutostartEntry};
use crate::backups::{self, Backup};
use crate::custom_profiles::{self, CustomProfile};
use crate::i18n;
use crate::privilege;

pub struct SystemData {
    pub info: SystemInfo,
    pub boot: Result<BootSummary, String>,
    pub blame: Result<Vec<BlameEntry>, String>,
    pub services: Result<Vec<Service>, String>,
    pub autostart: Result<Vec<AutostartEntry>, String>,
    pub backups: Result<Vec<Backup>, String>,
    pub custom_profiles: Result<Vec<CustomProfile>, String>,
}

pub fn load() -> SystemData {
    SystemData {
        info: system_info(),
        boot: boot_summary(),
        blame: blame(),
        services: services(),
        autostart: autostart::entries(),
        backups: backups::list(),
        custom_profiles: custom_profiles::list(),
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| i18n::fill(i18n::t("err_run"), &[program, &err.to_string()]))?;

    if !output.status.success() {
        return Err(i18n::fill(i18n::t("err_exit"), &[program]));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSummary {
    pub total: String,
    pub detail: String,
}

pub fn boot_summary() -> Result<BootSummary, String> {
    let output = run("systemd-analyze", &[])?;
    let first_line = output.lines().next().unwrap_or_default();
    parse_boot_summary(first_line)
        .ok_or_else(|| i18n::fill(i18n::t("err_parse"), &["systemd-analyze"]))
}

fn parse_boot_summary(line: &str) -> Option<BootSummary> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let total = line.rsplit('=').next()?.trim().to_string();
    Some(BootSummary {
        total,
        detail: line.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootComponent {
    pub label: String,
    pub time: String,
}

pub fn parse_breakdown(detail: &str) -> Vec<BootComponent> {
    detail
        .split('+')
        .filter_map(|segment| {
            let (before, after) = segment.split_once('(')?;
            let label = after.split(')').next()?.trim();
            let time = before.split_whitespace().last()?;
            if label.is_empty() {
                return None;
            }
            Some(BootComponent {
                label: label.to_string(),
                time: time.to_string(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameEntry {
    pub time: String,
    pub name: String,
}

pub fn blame() -> Result<Vec<BlameEntry>, String> {
    let output = run("systemd-analyze", &["blame"])?;
    Ok(parse_blame(&output))
}

fn parse_blame(output: &str) -> Vec<BlameEntry> {
    output
        .lines()
        .filter_map(|line| {
            let (time, name) = line.trim().split_once(char::is_whitespace)?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(BlameEntry {
                time: time.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

pub fn parse_duration_seconds(value: &str) -> f64 {
    let mut seconds = 0.0;
    for token in value.split_whitespace() {
        if let Some(ms) = token.strip_suffix("ms") {
            seconds += ms.parse::<f64>().unwrap_or_default() / 1000.0;
        } else if let Some(minutes) = token.strip_suffix("min") {
            seconds += minutes.parse::<f64>().unwrap_or_default() * 60.0;
        } else if let Some(secs) = token.strip_suffix('s') {
            seconds += secs.parse::<f64>().unwrap_or_default();
        } else if let Some(minutes) = token.strip_suffix('m') {
            seconds += minutes.parse::<f64>().unwrap_or_default() * 60.0;
        } else if let Some(hours) = token.strip_suffix('h') {
            seconds += hours.parse::<f64>().unwrap_or_default() * 3600.0;
        }
    }
    seconds
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub active: String,
    pub sub: String,
    pub description: String,
    pub enabled: String,
}

const LISTED_SUFFIXES: [&str; 6] = [
    ".service", ".mount", ".device", ".swap", ".socket", ".target",
];

pub fn is_listed_unit(name: &str) -> bool {
    LISTED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

pub fn services() -> Result<Vec<Service>, String> {
    let units = run(
        "systemctl",
        &["list-units", "--all", "--no-pager", "--no-legend"],
    )?;

    let files = run(
        "systemctl",
        &["list-unit-files", "--no-pager", "--no-legend"],
    )
    .unwrap_or_default();

    Ok(merge_services(&units, parse_unit_file_states(&files)))
}

const UNIT_MARKERS: &[char] = &['*', '●', '×'];

fn parse_units(output: &str) -> Vec<(String, String, String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(UNIT_MARKERS).trim();
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let load = parts.next()?;
            if load == "not-found" {
                return None;
            }
            let active = parts.next()?.to_string();
            let sub = parts.next()?.to_string();
            let description = parts.collect::<Vec<_>>().join(" ");
            Some((name, active, sub, description))
        })
        .collect()
}

fn parse_unit_file_states(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            Some((name, state))
        })
        .collect()
}

fn merge_services(units: &str, mut file_states: HashMap<String, String>) -> Vec<Service> {
    let mut services = Vec::new();

    for (name, active, sub, description) in parse_units(units) {
        if !is_listed_unit(&name) {
            continue;
        }
        let enabled = file_states
            .remove(&name)
            .unwrap_or_else(|| "unknown".to_string());
        services.push(Service {
            name,
            active,
            sub,
            description,
            enabled,
        });
    }

    for (name, state) in file_states {
        if name.contains('@') || !is_listed_unit(&name) {
            continue;
        }
        services.push(Service {
            name,
            active: "inactive".to_string(),
            sub: "dead".to_string(),
            description: String::new(),
            enabled: state,
        });
    }

    services.sort_by(|a, b| a.name.cmp(&b.name));
    services
}

pub fn journal_is_empty(output: &str) -> bool {
    let trimmed = output.trim();
    trimmed.is_empty() || trimmed.to_lowercase().contains("-- no entries --")
}

pub fn journal(unit: &str, lines: usize, password: Option<&str>) -> String {
    let count = lines.to_string();
    let args = ["-u", unit, "--no-pager", "-n", count.as_str()];

    let unprivileged = run("journalctl", &args);
    match unprivileged {
        Ok(output) if !journal_is_empty(&output) => output,
        other => match password {
            Some(password) => privilege::run(
                &["journalctl", "-u", unit, "--no-pager", "-n", count.as_str()],
                password,
            )
            .unwrap_or_else(|err| err),
            None => other.unwrap_or_else(|err| err),
        },
    }
}

pub fn dependencies(unit: &str, reverse: bool) -> Result<String, String> {
    let mut args = vec!["list-dependencies", unit, "--no-pager"];
    if reverse {
        args.push("--reverse");
    }
    run("systemctl", &args)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub os: String,
    pub kernel: String,
    pub ram: String,
    pub uptime: String,
}

pub fn system_info() -> SystemInfo {
    SystemInfo {
        os: read_os_name(),
        kernel: run("uname", &["-r"]).map_or_else(
            |_| i18n::t("unknown").to_string(),
            |out| out.trim().to_string(),
        ),
        ram: read_ram(),
        uptime: read_uptime(),
    }
}

fn read_os_name() -> String {
    let Ok(contents) = fs::read_to_string("/etc/os-release") else {
        return "Linux".to_string();
    };
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return value.trim().trim_matches('"').to_string();
        }
    }
    "Linux".to_string()
}

fn read_ram() -> String {
    let Ok(contents) = fs::read_to_string("/proc/meminfo") else {
        return i18n::t("unknown").to_string();
    };
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:")
            && let Some(kb) = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
        {
            return format!("{:.1} GB", kb as f64 / 1024.0 / 1024.0);
        }
    }
    i18n::t("unknown").to_string()
}

fn read_uptime() -> String {
    let Ok(contents) = fs::read_to_string("/proc/uptime") else {
        return i18n::t("unknown").to_string();
    };
    let seconds = contents
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let hours = (seconds / 3600.0) as u64;
    let minutes = ((seconds % 3600.0) / 60.0) as u64;

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_total_boot_time() {
        let summary = parse_boot_summary(
            "Startup finished in 3.456s (firmware) + 1.234s (loader) + 5.678s (kernel) + 12.345s (userspace) = 22.713s",
        )
        .expect("summary");
        assert_eq!(summary.total, "22.713s");
    }

    #[test]
    fn empty_boot_line_is_rejected() {
        assert!(parse_boot_summary("   ").is_none());
    }

    #[test]
    fn a_missing_command_is_reported_in_the_ui_language() {
        let err = run("no-such-program-here", &[]).unwrap_err();
        assert!(
            err.starts_with("could not run no-such-program-here:"),
            "{err}"
        );
    }

    #[test]
    fn a_failing_command_is_reported_in_the_ui_language() {
        assert_eq!(run("false", &[]).unwrap_err(), "false exited with an error");
    }

    #[test]
    fn parses_boot_stages() {
        let components = parse_breakdown(
            "Startup finished in 3.456s (firmware) + 1.234s (loader) + 5.678s (kernel) + 12.345s (userspace) = 22.713s",
        );
        assert_eq!(components.len(), 4);
        assert_eq!(components[0].label, "firmware");
        assert_eq!(components[0].time, "3.456s");
        assert_eq!(components[3].label, "userspace");
        assert_eq!(components[3].time, "12.345s");
    }

    #[test]
    fn breakdown_without_stages_is_empty() {
        assert!(parse_breakdown("Startup finished in 22.713s").is_empty());
    }

    #[test]
    fn parses_blame_lines() {
        let entries =
            parse_blame("  2.345s NetworkManager-wait-online.service\n1.234s foo.service\n\n");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].time, "2.345s");
        assert_eq!(entries[0].name, "NetworkManager-wait-online.service");
    }

    #[test]
    fn parses_blame_durations() {
        assert!((parse_duration_seconds("5.678s") - 5.678).abs() < 1e-9);
        assert!((parse_duration_seconds("234ms") - 0.234).abs() < 1e-9);
        assert!((parse_duration_seconds("1min 8.634s") - 68.634).abs() < 1e-6);
        assert_eq!(parse_duration_seconds(""), 0.0);
    }

    #[test]
    fn parses_unit_file_states() {
        let states =
            parse_unit_file_states("NetworkManager.service enabled\nsshd.service  disabled\n");
        assert_eq!(
            states.get("NetworkManager.service").map(String::as_str),
            Some("enabled")
        );
        assert_eq!(
            states.get("sshd.service").map(String::as_str),
            Some("disabled")
        );
    }

    #[test]
    fn merge_includes_stopped_service_files() {
        let units = "● sshd.service loaded active running OpenSSH server\n";
        let mut files = HashMap::new();
        files.insert("sshd.service".to_string(), "enabled".to_string());
        files.insert("cups.service".to_string(), "disabled".to_string());

        let services = merge_services(units, files);

        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "cups.service");
        assert_eq!(services[0].active, "inactive");
        assert_eq!(services[0].enabled, "disabled");
        assert_eq!(services[1].name, "sshd.service");
    }

    #[test]
    fn merge_skips_unit_types_the_app_does_not_list() {
        let units = "● dev-sda.device loaded active plugged Disk\n\
                     ● systemd-tmpfiles-clean.timer loaded active waiting Cleanup\n\
                     ● sshd.socket loaded active listening Socket\n";
        let mut files = HashMap::new();
        files.insert("foo.slice".to_string(), "static".to_string());
        files.insert("sshd.socket".to_string(), "enabled".to_string());

        let services = merge_services(units, files);
        let names: Vec<&str> = services
            .iter()
            .map(|service| service.name.as_str())
            .collect();

        assert_eq!(names, ["dev-sda.device", "sshd.socket"]);
    }

    #[test]
    fn detects_empty_journal_output() {
        assert!(journal_is_empty(""));
        assert!(journal_is_empty("  -- No entries --  "));
        assert!(!journal_is_empty(
            "Oct 16 host systemd[1]: Started foo.service"
        ));
    }

    #[test]
    fn strips_markers_and_skips_not_found_units() {
        // With LC_ALL=C systemctl marks active units with `*`; not-found units
        // must not leak into the list.
        let units = "* sshd.service loaded active running OpenSSH server\n\
                     ● alsa.service loaded active exited Sound Card\n\
                     * auditd.service not-found inactive dead auditd.service\n";

        let services = merge_services(units, HashMap::new());

        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "alsa.service");
        assert_eq!(services[1].name, "sshd.service");
        assert_eq!(services[1].active, "active");
    }
}
