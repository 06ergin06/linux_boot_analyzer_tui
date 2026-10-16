//! Static service knowledge base, ported from the Python project's
//! `services_data.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// Embedded at compile time so the binary works from any working directory.
const SERVICES_JSON: &str = include_str!("../assets/services_data.json");

/// How the original application classifies a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Tip {
    /// Safe to turn off and usually worth disabling.
    #[serde(rename = "suggestion")]
    Suggestion,
    /// Important: turning it off may break the system.
    #[serde(rename = "kritik")]
    Critical,
    /// Required for basic functionality.
    #[serde(rename = "gerekli")]
    Required,
}

impl Tip {
    /// Short label shown in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Tip::Suggestion => "Suggestion",
            Tip::Critical => "Critical",
            Tip::Required => "Required",
        }
    }
}

/// One entry of `services_data.json`. The `desc`/`suggestion` fields are
/// Turkish because they come from the upstream data file.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceEntry {
    pub desc: String,
    pub tip: Tip,
    pub suggestion: String,
    /// Reserved for the English locale, which is not ported yet.
    #[allow(dead_code)]
    pub desc_en: String,
    /// Reserved for the English locale, which is not ported yet.
    #[allow(dead_code)]
    pub oneri_en: String,
}

static ENTRIES: LazyLock<HashMap<String, ServiceEntry>> = LazyLock::new(|| {
    // Embedded at build time; fall back to an empty map instead of panicking.
    serde_json::from_str(SERVICES_JSON).unwrap_or_default()
});

/// Maps a template prefix (`getty`) to its template entry key (`getty@.service`).
static TEMPLATES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    ENTRIES
        .keys()
        .filter_map(|key| {
            let (prefix, _) = key.split_once('@')?;
            Some((prefix, key.as_str()))
        })
        .collect()
});

/// Look up the knowledge-base entry for a unit name.
///
/// Template instances such as `getty@tty1.service` fall back to the template
/// entry (`getty@.service`).
pub fn lookup(name: &str) -> Option<&'static ServiceEntry> {
    if let Some(entry) = ENTRIES.get(name) {
        return Some(entry);
    }

    let (prefix, _) = name.split_once('@')?;
    let template = *TEMPLATES.get(prefix)?;
    ENTRIES.get(template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_data_parses() {
        assert!(!ENTRIES.is_empty());
    }

    #[test]
    fn known_service_is_classified() {
        let entry = lookup("NetworkManager.service").expect("known service");
        assert_eq!(entry.tip, Tip::Critical);
        assert!(!entry.desc.is_empty());
    }

    #[test]
    fn template_instances_use_the_template_entry() {
        let (template_key, _) = ENTRIES
            .iter()
            .find(|(key, _)| key.contains('@'))
            .expect("a template entry exists");
        let instance = template_key.replace("@.", "@instance.");
        assert!(lookup(&instance).is_some());
    }

    #[test]
    fn unknown_services_return_none() {
        assert!(lookup("not-a-real-unit.service").is_none());
    }
}
