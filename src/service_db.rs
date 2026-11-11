//! Static service knowledge base, read from the canonical database.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::i18n::{self, Lang};

const SERVICES_JSON: &str = include_str!("../assets/services_data.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Suggestion,
    Critical,
    Required,
}

impl ServiceType {
    pub fn label(self) -> &'static str {
        match self {
            ServiceType::Suggestion => i18n::t("tui_type_suggestion"),
            ServiceType::Critical => i18n::t("tui_type_critical"),
            ServiceType::Required => i18n::t("tui_type_required"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceEntry {
    pub description_tr: String,
    pub description_en: String,
    pub suggestion_tr: String,
    pub suggestion_en: String,
    pub critical: bool,
    pub required: bool,
}

impl ServiceEntry {
    pub fn service_type(&self) -> ServiceType {
        if self.critical {
            ServiceType::Critical
        } else if self.required {
            ServiceType::Required
        } else {
            ServiceType::Suggestion
        }
    }

    pub fn description(&self) -> &str {
        localized(&self.description_tr, &self.description_en)
    }

    pub fn suggestion_text(&self) -> &str {
        localized(&self.suggestion_tr, &self.suggestion_en)
    }
}

fn localized<'a>(tr: &'a str, en: &'a str) -> &'a str {
    match i18n::lang() {
        Lang::En => {
            if en.is_empty() {
                tr
            } else {
                en
            }
        }
        Lang::Tr => {
            if tr.is_empty() {
                en
            } else {
                tr
            }
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
        assert_eq!(entry.service_type(), ServiceType::Critical);
        assert!(!entry.description_tr.is_empty());
    }

    #[test]
    fn required_service_is_classified() {
        let entry = lookup("archlinux-keyring-wkd-sync.service").expect("known service");
        assert_eq!(entry.service_type(), ServiceType::Required);
        assert!(!entry.description_tr.is_empty());
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
