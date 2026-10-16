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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_loaded() {
        let data = data();
        assert!(data.profiles.contains_key("minimum"));
        assert!(!data.safe_to_disable_services.is_empty());
    }
}
