//! Filters and sort order for the Services tab.

use crate::i18n;
use crate::service_db::{self, Tip};
use crate::system::Service;

/// Which unit/runtime state to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    All,
    Active,
    Inactive,
    Disabled,
    Masked,
}

impl StatusFilter {
    pub const ALL: [StatusFilter; 5] = [
        StatusFilter::All,
        StatusFilter::Active,
        StatusFilter::Inactive,
        StatusFilter::Disabled,
        StatusFilter::Masked,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StatusFilter::All => i18n::t("filter_all"),
            StatusFilter::Active => i18n::t("filter_active"),
            StatusFilter::Inactive => i18n::t("filter_inactive"),
            StatusFilter::Disabled => i18n::t("filter_disabled"),
            StatusFilter::Masked => i18n::t("filter_masked"),
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|filter| *filter == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn matches(self, service: &Service) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Active => service.active == "active",
            StatusFilter::Inactive => service.active == "inactive",
            StatusFilter::Disabled => service.enabled == "disabled" || service.sub == "disabled",
            StatusFilter::Masked => service.enabled == "masked" || service.sub == "masked",
        }
    }
}

/// Filter by the knowledge-base classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipFilter {
    All,
    Suggestion,
    Critical,
    Required,
}

impl TipFilter {
    pub const ALL: [TipFilter; 4] = [
        TipFilter::All,
        TipFilter::Suggestion,
        TipFilter::Critical,
        TipFilter::Required,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TipFilter::All => i18n::t("tip_all"),
            TipFilter::Suggestion => i18n::t("tip_suggestion"),
            TipFilter::Critical => i18n::t("tip_critical"),
            TipFilter::Required => i18n::t("tip_required"),
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|filter| *filter == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn wanted(self) -> Option<Tip> {
        match self {
            TipFilter::All => None,
            TipFilter::Suggestion => Some(Tip::Suggestion),
            TipFilter::Critical => Some(Tip::Critical),
            TipFilter::Required => Some(Tip::Required),
        }
    }

    pub fn matches(self, service: &Service) -> bool {
        match self.wanted() {
            None => true,
            Some(tip) => service_db::lookup(&service.name).map(|entry| entry.tip) == Some(tip),
        }
    }
}

/// Sort order for the services table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Name,
    BootTime,
}

impl SortBy {
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Name => i18n::t("tui_sort_name"),
            SortBy::BootTime => i18n::t("tui_sort_time"),
        }
    }

    pub fn next(self) -> Self {
        match self {
            SortBy::Name => SortBy::BootTime,
            SortBy::BootTime => SortBy::Name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, active: &str, enabled: &str) -> Service {
        Service {
            name: name.to_string(),
            active: active.to_string(),
            sub: "running".to_string(),
            description: String::new(),
            enabled: enabled.to_string(),
        }
    }

    #[test]
    fn status_filter_matches_expected_states() {
        let active = service("sshd.service", "active", "enabled");
        let disabled = service("cups.service", "inactive", "disabled");
        assert!(StatusFilter::Active.matches(&active));
        assert!(!StatusFilter::Active.matches(&disabled));
        assert!(StatusFilter::Disabled.matches(&disabled));
    }

    #[test]
    fn tip_filter_uses_the_knowledge_base() {
        // NetworkManager.service is classified as Critical in services_data.json.
        let network_manager = service("NetworkManager.service", "active", "enabled");
        assert!(TipFilter::Critical.matches(&network_manager));
        assert!(!TipFilter::Suggestion.matches(&network_manager));
    }

    #[test]
    fn filters_cycle_back_to_the_first_variant() {
        let mut status = StatusFilter::All;
        for _ in 0..StatusFilter::ALL.len() {
            status = status.next();
        }
        assert_eq!(status, StatusFilter::All);

        let mut tip = TipFilter::All;
        for _ in 0..TipFilter::ALL.len() {
            tip = tip.next();
        }
        assert_eq!(tip, TipFilter::All);
    }
}
