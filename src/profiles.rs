//! Ready-made optimization profiles, ported from `profiles_data.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

const PROFILES_JSON: &str = include_str!("../assets/profiles_data.json");

/// One optimization profile.
///
/// The locale keys and icon are kept for a future port of the i18n layer and
/// are not read yet.
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    #[allow(dead_code)]
    pub name_tr_key: String,
    #[allow(dead_code)]
    pub icon: String,
    #[allow(dead_code)]
    pub desc_tr_key: String,
    /// Service name -> wanted unit-file state (`enable` or `disable`).
    pub services: HashMap<String, String>,
}

/// The whole `profiles_data.json` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfilesData {
    pub profiles: HashMap<String, Profile>,
    #[allow(dead_code)]
    #[serde(default)]
    pub safe_to_disable_services: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    pub profile_service_labels: HashMap<String, String>,
}

static DATA: LazyLock<ProfilesData> = LazyLock::new(|| {
    // Embedded at build time; fall back to an empty default instead of panicking.
    serde_json::from_str(PROFILES_JSON).unwrap_or_default()
});

pub fn data() -> &'static ProfilesData {
    &DATA
}

/// Profile ids in a stable order.
pub fn ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = data().profiles.keys().map(String::as_str).collect();
    ids.sort_unstable();
    ids
}

/// The services a profile wants enabled and disabled.
pub fn selections(profile_id: &str) -> (Vec<String>, Vec<String>) {
    let Some(profile) = data().profiles.get(profile_id) else {
        return (Vec::new(), Vec::new());
    };

    let mut enable = Vec::new();
    let mut disable = Vec::new();
    for (name, action) in &profile.services {
        if action == "enable" {
            enable.push(name.clone());
        } else if action == "disable" {
            disable.push(name.clone());
        }
    }
    enable.sort();
    disable.sort();

    (enable, disable)
}

/// Whether a unit-file state means "enabled".
pub fn is_enabled(state: &str) -> bool {
    state == "enabled" || state == "enabled-runtime"
}

/// The units to act on when applying a profile or restoring a backup.
///
/// Ported from `ProfileManager.apply_profile_batch`, which derives these from
/// the current unit-file and runtime states.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub enable: Vec<String>,
    pub start: Vec<String>,
    pub disable: Vec<String>,
    pub stop: Vec<String>,
}

impl Plan {
    /// Whether the plan would change anything at all.
    pub fn is_empty(&self) -> bool {
        self.enable.is_empty()
            && self.start.is_empty()
            && self.disable.is_empty()
            && self.stop.is_empty()
    }

    /// One-line summary for the confirmation dialog.
    pub fn summary(&self) -> String {
        format!(
            "enable {}, start {}, disable {}, stop {}",
            self.enable.len(),
            self.start.len(),
            self.disable.len(),
            self.stop.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_loaded() {
        let data = data();
        assert!(data.profiles.contains_key("minimum"));
        assert!(!data.safe_to_disable_services.is_empty());
    }

    #[test]
    fn profile_ids_are_sorted() {
        let ids = ids();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        assert!(!ids.is_empty());
    }

    #[test]
    fn selections_split_enable_and_disable() {
        let (enable, disable) = selections("minimum");
        assert!(!enable.is_empty() || !disable.is_empty());
        // Every returned name must exist in the profile itself.
        let profile = &data().profiles["minimum"];
        for name in enable.iter().chain(disable.iter()) {
            assert!(profile.services.contains_key(name));
        }
    }

    #[test]
    fn empty_plan_is_reported_as_empty() {
        let plan = Plan::default();
        assert!(plan.is_empty());
        assert!(plan.summary().contains("enable 0"));
    }
}
