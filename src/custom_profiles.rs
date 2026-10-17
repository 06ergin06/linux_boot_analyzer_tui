//! User-created profiles: one JSON file per profile under the app's config
//! directory, in the Python application's own format.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CANDIDATES: [(&str, &str); 9] = [
    ("cups.service", "cups_service_desc"),
    ("cups-browsed.service", "cups_browsed_desc"),
    ("bluetooth.service", "bluetooth_desc"),
    ("docker.service", "docker_desc"),
    ("postgresql.service", "postgresql_desc"),
    ("mysql.service", "mysql_desc"),
    ("ssh.service", "ssh_desc"),
    ("avahi-daemon.service", "avahi_desc"),
    ("ModemManager.service", "modem_desc"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProfile {
    pub name: String,
    pub services: BTreeMap<String, String>,
    pub path: PathBuf,
}

impl CustomProfile {
    pub fn selections(&self) -> (Vec<String>, Vec<String>) {
        selections_of(&self.services)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ProfileFile {
    name: String,
    #[serde(default)]
    services: BTreeMap<String, String>,
}

pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/linux-boot-analyzer-tui/profiles")
}

pub fn list() -> Result<Vec<CustomProfile>, String> {
    list_in(&dir())
}

pub fn save(name: &str, services: &BTreeMap<String, String>) -> Result<PathBuf, String> {
    save_in(&dir(), name, services)
}

pub fn remove(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|err| format!("could not remove {}: {err}", path.display()))
}

pub fn selections_of(services: &BTreeMap<String, String>) -> (Vec<String>, Vec<String>) {
    let mut enable = Vec::new();
    let mut disable = Vec::new();
    for (name, action) in services {
        match action.as_str() {
            "enable" => enable.push(name.clone()),
            "disable" => disable.push(name.clone()),
            _ => {}
        }
    }
    (enable, disable)
}

pub fn form_rules(checked: &[bool]) -> BTreeMap<String, String> {
    CANDIDATES
        .iter()
        .zip(checked)
        .map(|((unit, _), checked)| {
            let rule = if *checked { "enable" } else { "disable" };
            ((*unit).to_string(), rule.to_string())
        })
        .collect()
}

fn list_in(dir: &Path) -> Result<Vec<CustomProfile>, String> {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("could not read {}: {err}", dir.display())),
    };

    let mut profiles = Vec::new();
    for item in listing.flatten() {
        let path = item.path();
        let is_json = path
            .extension()
            .is_some_and(|extension| extension == "json");
        if !is_json {
            continue;
        }

        if let Ok(contents) = fs::read_to_string(&path)
            && let Ok(file) = serde_json::from_str::<ProfileFile>(&contents)
        {
            profiles.push(CustomProfile {
                name: file.name,
                services: file.services,
                path,
            });
        }
    }

    profiles.sort_by_key(|profile| profile.name.to_lowercase());
    Ok(profiles)
}

fn save_in(dir: &Path, name: &str, services: &BTreeMap<String, String>) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;

    let path = unique_path(dir, name);
    let file = ProfileFile {
        name: name.to_string(),
        services: services.clone(),
    };
    let mut contents = serde_json::to_string_pretty(&file)
        .map_err(|err| format!("could not write the profile: {err}"))?;
    contents.push('\n');

    fs::write(&path, contents)
        .map_err(|err| format!("could not write {}: {err}", path.display()))?;
    Ok(path)
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let slug: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    let slug = slug.to_lowercase();
    let slug = if slug.is_empty() {
        "custom".to_string()
    } else {
        slug
    };

    let mut path = dir.join(format!("{slug}.json"));
    let mut counter = 1;
    while path.exists() {
        path = dir.join(format!("{slug}-{counter}.json"));
        counter += 1;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opencode-custom-profiles-{case}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn rules(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(unit, action)| ((*unit).to_string(), (*action).to_string()))
            .collect()
    }

    #[test]
    fn a_saved_profile_is_listed_again() {
        let dir = temp_dir("roundtrip");
        let services = rules(&[("cups.service", "disable"), ("ssh.service", "enable")]);

        let path = save_in(&dir, "My Profile", &services).expect("save must succeed");
        let listed = list_in(&dir).expect("list must succeed");
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "My Profile");
        assert_eq!(listed[0].services, services);
        assert_eq!(listed[0].path, path);
    }

    #[test]
    fn a_missing_directory_has_no_profiles() {
        let dir = temp_dir("missing");
        assert_eq!(list_in(&dir).expect("list must succeed").len(), 0);
    }

    #[test]
    fn a_broken_file_is_skipped() {
        let dir = temp_dir("broken");
        fs::create_dir_all(&dir).expect("create");
        fs::write(dir.join("good.json"), r#"{"name": "Good", "services": {}}"#).expect("write");
        fs::write(dir.join("bad.json"), "{ not json").expect("write");
        fs::write(dir.join("notes.txt"), "ignore me").expect("write");

        let listed = list_in(&dir).expect("list must succeed");
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Good");
    }

    #[test]
    fn profiles_are_ordered_by_name() {
        let dir = temp_dir("order");
        save_in(&dir, "b profile", &BTreeMap::new()).expect("save");
        save_in(&dir, "A profile", &BTreeMap::new()).expect("save");

        let listed = list_in(&dir).expect("list must succeed");
        let _ = fs::remove_dir_all(&dir);

        let names: Vec<&str> = listed.iter().map(|profile| profile.name.as_str()).collect();
        assert_eq!(names, ["A profile", "b profile"]);
    }

    #[test]
    fn a_name_clash_gets_a_counter() {
        let dir = temp_dir("clash");
        save_in(&dir, "Same", &BTreeMap::new()).expect("save");
        let second = save_in(&dir, "Same", &BTreeMap::new()).expect("save");

        let listed = list_in(&dir).expect("list must succeed");
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(listed.len(), 2);
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("same-1.json")
        );
    }

    #[test]
    fn unique_path_slugifies_the_name() {
        let dir = Path::new("/nonexistent-profiles-dir");
        assert_eq!(unique_path(dir, "My Profile!"), dir.join("myprofile.json"));
        assert_eq!(unique_path(dir, "a b/c"), dir.join("abc.json"));
        assert_eq!(unique_path(dir, "!!!"), dir.join("custom.json"));
    }

    #[test]
    fn selections_split_both_directions() {
        let services = rules(&[("a.service", "disable"), ("b.service", "enable")]);
        let (enable, disable) = selections_of(&services);
        assert_eq!(enable, ["b.service"]);
        assert_eq!(disable, ["a.service"]);
    }

    #[test]
    fn removing_a_profile_deletes_its_file() {
        let dir = temp_dir("remove");
        let path = save_in(&dir, "Mine", &BTreeMap::new()).expect("save");

        remove(&path).expect("remove must succeed");

        assert!(!path.exists());
        assert_eq!(list_in(&dir).expect("list must succeed").len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn form_rules_follow_the_switches() {
        let mut checked = vec![false; CANDIDATES.len()];
        checked[0] = true;

        let rules = form_rules(&checked);

        assert_eq!(rules.len(), CANDIDATES.len());
        assert_eq!(
            rules.get("cups.service").map(String::as_str),
            Some("enable")
        );
        assert_eq!(
            rules.get("ModemManager.service").map(String::as_str),
            Some("disable")
        );
    }
}
