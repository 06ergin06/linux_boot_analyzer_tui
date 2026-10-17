//! Restore points: snapshots of unit-file states under the user's config dir.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Backup {
    pub timestamp: String,
    pub name: String,
    pub services: BTreeMap<String, String>,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct BackupFile {
    timestamp: String,
    name: String,
    services: BTreeMap<String, String>,
}

pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/linux-boot-analyzer-tui/backups")
}

pub fn list() -> Result<Vec<Backup>, String> {
    let dir = dir();
    let listing = match fs::read_dir(&dir) {
        Ok(listing) => listing,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("could not read {}: {err}", dir.display())),
    };

    let mut backups = Vec::new();
    for item in listing.flatten() {
        let path = item.path();
        let is_backup = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("backup_") && name.ends_with(".json"));
        if !is_backup {
            continue;
        }

        if let Ok(contents) = fs::read_to_string(&path)
            && let Ok(file) = serde_json::from_str::<BackupFile>(&contents)
        {
            backups.push(Backup {
                timestamp: file.timestamp,
                name: file.name,
                services: file.services,
                path,
            });
        }
    }

    backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(backups)
}

pub fn create(services: BTreeMap<String, String>) -> Result<Backup, String> {
    let dir = dir();
    fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;

    let (timestamp, pretty) = now();
    let name = format!("Restore point {pretty}");
    let path = dir.join(format!("backup_{timestamp}.json"));

    let file = BackupFile {
        timestamp: timestamp.clone(),
        name: name.clone(),
        services: services.clone(),
    };
    let json = serde_json::to_string_pretty(&file)
        .map_err(|err| format!("could not encode backup: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("could not write {}: {err}", path.display()))?;

    Ok(Backup {
        timestamp,
        name,
        services,
        path,
    })
}

pub fn remove(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|err| format!("could not remove {}: {err}", path.display()))
}

fn now() -> (String, String) {
    const FORMAT: &str = "+%Y-%m-%d_%H-%M-%S|%d.%m.%Y %H:%M:%S";

    if let Ok(output) = Command::new("date").arg(FORMAT).output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some((stamp, pretty)) = text.split_once('|') {
            return (stamp.to_string(), pretty.to_string());
        }
    }

    let fallback = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs())
    );
    (fallback.clone(), fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_file_round_trips_through_json() {
        let mut services = BTreeMap::new();
        services.insert("sshd.service".to_string(), "enabled".to_string());
        services.insert("cups.service".to_string(), "disabled".to_string());

        let file = BackupFile {
            timestamp: "2026-01-01_00-00-00".to_string(),
            name: "Restore point 01.01.2026 00:00:00".to_string(),
            services,
        };

        let json = serde_json::to_string_pretty(&file).expect("encode");
        let parsed: BackupFile = serde_json::from_str(&json).expect("decode");

        assert_eq!(parsed.timestamp, "2026-01-01_00-00-00");
        assert_eq!(
            parsed.services.get("sshd.service").map(String::as_str),
            Some("enabled")
        );
    }

    #[test]
    fn now_returns_two_non_empty_parts() {
        let (stamp, pretty) = now();
        assert!(!stamp.is_empty());
        assert!(!pretty.is_empty());
    }
}
