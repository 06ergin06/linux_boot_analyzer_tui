//! Ready-made optimization profiles, ported from `profiles_data.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::i18n;

const PROFILES_JSON: &str = include_str!("../assets/profiles_data.json");

/// One optimization profile.
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    /// i18n key for the profile name, e.g. `prof_min_name`.
    pub name_tr_key: String,
    /// Icon name; the GTK application draws it, this TUI does not.
    #[allow(dead_code)]
    pub icon: String,
    /// i18n key for the profile description.
    pub desc_tr_key: String,
    /// Service name -> wanted unit-file state (`enable` or `disable`).
    pub services: HashMap<String, String>,
}

impl Profile {
    /// Profile name in the active language.
    pub fn name(&self) -> &str {
        i18n::t(&self.name_tr_key)
    }

    /// Profile description in the active language.
    pub fn description(&self) -> &str {
        i18n::t(&self.desc_tr_key)
    }
}

/// The whole `profiles_data.json` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfilesData {
    pub profiles: HashMap<String, Profile>,
    #[serde(default)]
    pub safe_to_disable_services: Vec<String>,
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

/// The services a profile touches, as `(wants_enable, label)` pairs.
///
/// Ported from the profile card in the original `profiles.py`: services the
/// profile enables come first (✔), then the ones it disables (✘). The label is
/// the friendly name from `profile_service_labels` when the original defines
/// one, otherwise the unit name itself.
pub fn checklist(profile_id: &str) -> Vec<(bool, &'static str)> {
    let Some(profile) = data().profiles.get(profile_id) else {
        return Vec::new();
    };

    let mut items: Vec<(bool, &'static str)> = profile
        .services
        .iter()
        .map(|(name, action)| (action == "enable", friendly_name(name)))
        .collect();

    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    items
}

/// Friendly label for a unit name, falling back to the unit name itself.
fn friendly_name(name: &'static str) -> &'static str {
    let Some(key) = data().profile_service_labels.get(name) else {
        return name;
    };
    let translated = i18n::t(key);
    if translated == key { name } else { translated }
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
            "{} {} · {} {} · {} {} · {} {}",
            i18n::t("tui_plan_enable"),
            self.enable.len(),
            i18n::t("tui_plan_start"),
            self.start.len(),
            i18n::t("tui_plan_disable"),
            self.disable.len(),
            i18n::t("tui_plan_stop"),
            self.stop.len(),
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
    fn profiles_have_translated_names_and_descriptions() {
        for id in ids() {
            let profile = &data().profiles[id];
            assert_ne!(
                profile.name(),
                profile.name_tr_key,
                "missing name translation for {id}"
            );
            assert_ne!(
                profile.description(),
                profile.desc_tr_key,
                "missing description translation for {id}"
            );
        }
    }

    #[test]
    fn empty_plan_is_reported_as_empty() {
        let plan = Plan::default();
        assert!(plan.is_empty());
        assert!(plan.summary().contains("enable 0"));
    }

    #[test]
    fn checklist_uses_friendly_names_and_lists_enables_first() {
        let items = checklist("minimum");
        assert!(!items.is_empty());

        // The original application defines a friendly name for bluetooth.
        assert!(
            items
                .iter()
                .any(|(enable, label)| !enable && *label == "Bluetooth Support"),
            "items: {items:?}"
        );

        // Everything enabled comes before the first disabled entry.
        let first_disable = items.iter().position(|(enable, _)| !enable);
        if let Some(index) = first_disable {
            assert!(items[..index].iter().all(|(enable, _)| *enable));
        }
    }

    #[test]
    fn unknown_profile_has_an_empty_checklist() {
        assert!(checklist("does-not-exist").is_empty());
    }
}
