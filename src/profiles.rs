//! Ready-made optimization profiles, ported from `profiles_data.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::i18n;

const PROFILES_JSON: &str = include_str!("../assets/profiles_data.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub name_tr_key: String,
    #[allow(dead_code)]
    pub icon: String,
    pub desc_tr_key: String,
    pub services: HashMap<String, String>,
}

impl Profile {
    pub fn name(&self) -> &str {
        i18n::t(&self.name_tr_key)
    }

    pub fn description(&self) -> &str {
        i18n::t(&self.desc_tr_key)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfilesData {
    pub profiles: HashMap<String, Profile>,
    #[serde(default)]
    pub safe_to_disable_services: Vec<String>,
    #[serde(default)]
    pub profile_service_labels: HashMap<String, String>,
}

static DATA: LazyLock<ProfilesData> =
    LazyLock::new(|| serde_json::from_str(PROFILES_JSON).unwrap_or_default());

pub fn data() -> &'static ProfilesData {
    &DATA
}

pub fn ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = data().profiles.keys().map(String::as_str).collect();
    ids.sort_unstable();
    ids
}

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

pub fn is_enabled(state: &str) -> bool {
    state == "enabled" || state == "enabled-runtime"
}

pub fn checklist(profile_id: &str) -> Vec<(bool, &'static str)> {
    let Some(profile) = data().profiles.get(profile_id) else {
        return Vec::new();
    };
    checklist_of(profile.services.iter())
}

pub fn checklist_of<'a>(
    services: impl Iterator<Item = (&'a String, &'a String)>,
) -> Vec<(bool, &'a str)> {
    let mut items: Vec<(bool, &'a str)> = services
        .map(|(name, action)| (action == "enable", friendly_name(name)))
        .collect();

    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    items
}

fn friendly_name(name: &str) -> &str {
    let Some(key) = data().profile_service_labels.get(name) else {
        return name;
    };
    let translated = i18n::t(key);
    if translated == key { name } else { translated }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub enable: Vec<String>,
    pub start: Vec<String>,
    pub disable: Vec<String>,
    pub stop: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.enable.is_empty()
            && self.start.is_empty()
            && self.disable.is_empty()
            && self.stop.is_empty()
    }

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

        assert!(
            items
                .iter()
                .any(|(enable, label)| !enable && *label == "Bluetooth Support"),
            "items: {items:?}"
        );

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
