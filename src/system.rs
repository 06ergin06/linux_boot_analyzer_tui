//! Reads boot timing and systemd service information from the running system.
//!
//! Ported from the Python project's `system_manager.py`. Commands run with
//! `LC_ALL=C` so that output and our parsing stay stable across locales.

use std::collections::HashMap;
use std::fs;
use std::process::Command;

use crate::autostart::{self, AutostartEntry};
use crate::backups::{self, Backup};
use crate::privilege;

/// Everything the UI needs, gathered on a background thread.
pub struct SystemData {
    pub info: SystemInfo,
    pub boot: Result<BootSummary, String>,
    pub blame: Result<Vec<BlameEntry>, String>,
    pub services: Result<Vec<Service>, String>,
    pub autostart: Result<Vec<AutostartEntry>, String>,
    pub backups: Result<Vec<Backup>, String>,
}

/// Gather all data. Each section can fail independently.
pub fn load() -> SystemData {
    SystemData {
        info: system_info(),
        boot: boot_summary(),
        blame: blame(),
        services: services(),
        autostart: autostart::entries(),
        backups: backups::list(),
    }
}

/// Run a command with a C locale and return its standard output.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| format!("could not run {program}: {err}"))?;

    if !output.status.success() {
        return Err(format!("{program} exited with an error"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// --- Boot timing ---

/// Total boot time reported by `systemd-analyze`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSummary {
    /// e.g. `"22.713s"`.
    pub total: String,
    /// The full first line, including the firmware/loader/kernel/userspace split.
    pub detail: String,
}

pub fn boot_summary() -> Result<BootSummary, String> {
    let output = run("systemd-analyze", &[])?;
    let first_line = output.lines().next().unwrap_or_default();
    parse_boot_summary(first_line)
        .ok_or_else(|| "could not parse systemd-analyze output".to_string())
}

fn parse_boot_summary(line: &str) -> Option<BootSummary> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // "Startup finished in ... = 22.713s" -> keep the part after the last '='.
    let total = line.rsplit('=').next()?.trim().to_string();
    Some(BootSummary {
        total,
        detail: line.to_string(),
    })
}

/// One stage of the boot process, e.g. `firmware` or `userspace`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootComponent {
    pub label: String,
    pub time: String,
}

/// Split the `systemd-analyze` detail line into its stages.
///
/// The line looks like
/// `Startup finished in 3.4s (firmware) + 1.2s (loader) + 5.6s (kernel) = 22.7s`.
pub fn parse_breakdown(detail: &str) -> Vec<BootComponent> {
    detail
        .split('+')
        .filter_map(|segment| {
            let (before, after) = segment.split_once('(')?;
            let label = after.split(')').next()?.trim();
            // Drop the leading "Startup finished in" from the first segment.
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

// --- systemd-analyze blame ---

/// One line of `systemd-analyze blame`.
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

/// Parse a `systemd-analyze blame` time such as `"5.678s"`, `"234ms"` or
/// `"1min 8.634s"` into seconds.
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

// --- systemctl services ---

/// One systemd service unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    /// Runtime state, e.g. `active` / `inactive`.
    pub active: String,
    /// Sub-state, e.g. `running` / `dead`.
    pub sub: String,
    pub description: String,
    /// Unit-file state, e.g. `enabled` / `disabled`.
    pub enabled: String,
}

/// Unit types the original application lists. Slices, timers, paths, scopes and
/// automounts are dropped so the list stays readable.
const LISTED_SUFFIXES: [&str; 6] = [
    ".service", ".mount", ".device", ".swap", ".socket", ".target",
];

/// Whether a unit is of a type the application shows.
pub fn is_listed_unit(name: &str) -> bool {
    LISTED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

pub fn services() -> Result<Vec<Service>, String> {
    let units = run(
        "systemctl",
        &["list-units", "--all", "--no-pager", "--no-legend"],
    )?;

    // `list-unit-files` also reports stopped and disabled units.
    let files = run(
        "systemctl",
        &["list-unit-files", "--no-pager", "--no-legend"],
    )
    .unwrap_or_default();

    Ok(merge_services(&units, parse_unit_file_states(&files)))
}

/// systemctl marks a unit in its first column with a glyph. With `LC_ALL=C`
/// (which we set for stable parsing) that glyph is a plain `*`.
const UNIT_MARKERS: &[char] = &['*', '●', '×'];

fn parse_units(output: &str) -> Vec<(String, String, String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(UNIT_MARKERS).trim();
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let load = parts.next()?;
            // Units that are not installed ("not-found") are noise, not services.
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
        // Consuming the state here keeps file-only entries in the map below.
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
        // Template units are never instantiated, so skip them.
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

// --- Journal and dependencies ---

/// Whether `journalctl` output effectively contains nothing useful.
pub fn journal_is_empty(output: &str) -> bool {
    let trimmed = output.trim();
    trimmed.is_empty() || trimmed.to_lowercase().contains("-- no entries --")
}

/// Recent journal entries for a unit.
///
/// Reading the journal may require membership in `systemd-journal`, so when the
/// unprivileged read comes back empty (or fails) and a password is available we
/// retry through `sudo -S`.
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

/// Unit dependencies as rendered by `systemctl list-dependencies`.
pub fn dependencies(unit: &str, reverse: bool) -> Result<String, String> {
    let mut args = vec!["list-dependencies", unit, "--no-pager"];
    if reverse {
        args.push("--reverse");
    }
    run("systemctl", &args)
}

// --- System information ---

/// Basic host information shown on the analysis page.
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
        kernel: run("uname", &["-r"])
            .map_or_else(|_| "Unknown".to_string(), |out| out.trim().to_string()),
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
        return "Unknown".to_string();
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
    "Unknown".to_string()
}

fn read_uptime() -> String {
    let Ok(contents) = fs::read_to_string("/proc/uptime") else {
        return "Unknown".to_string();
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
