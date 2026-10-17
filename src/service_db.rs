//! Static service knowledge base, ported from `services_data.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::i18n::{self, Lang};

const SERVICES_JSON: &str = include_str!("../assets/services_data.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Tip {
    #[serde(rename = "suggestion")]
    Suggestion,
    #[serde(rename = "kritik")]
    Critical,
    #[serde(rename = "gerekli")]
    Required,
}

impl Tip {
    pub fn label(self) -> &'static str {
        match self {
            Tip::Suggestion => i18n::t("tui_tip_suggestion"),
            Tip::Critical => i18n::t("tui_tip_critical"),
            Tip::Required => i18n::t("tui_tip_required"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceEntry {
    pub desc: String,
    pub tip: Tip,
    pub suggestion: String,
    pub desc_en: String,
    pub oneri_en: String,
}

impl ServiceEntry {
    pub fn description(&self) -> &str {
        let (primary, fallback) = match i18n::lang() {
            Lang::En => (&self.desc_en, &self.desc),
            Lang::Tr => (&self.desc, &self.desc_en),
        };
        if primary.is_empty() {
            fallback
        } else {
            primary
        }
    }

    pub fn suggestion_text(&self) -> &str {
        let (primary, fallback) = match i18n::lang() {
            Lang::En => (&self.oneri_en, &self.suggestion),
            Lang::Tr => (&self.suggestion, &self.oneri_en),
        };
        if primary.is_empty() {
            fallback
        } else {
            primary
        }
    }
}

static ENTRIES: LazyLock<HashMap<String, ServiceEntry>> =
    LazyLock::new(|| serde_json::from_str(SERVICES_JSON).unwrap_or_default());

static TEMPLATES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    ENTRIES
        .keys()
        .filter_map(|key| {
            let (prefix, _) = key.split_once('@')?;
            Some((prefix, key.as_str()))
        })
        .collect()
});

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
