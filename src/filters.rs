//! Filters and sort order for the Services tab.

use crate::i18n;
use crate::service_db::{self, Tip};
use crate::system::Service;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceView {
    Services,
    Others,
    Devices,
}

impl ServiceView {
    pub const ALL: [ServiceView; 3] = [
        ServiceView::Services,
        ServiceView::Others,
        ServiceView::Devices,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ServiceView::Services => i18n::t("view_services"),
            ServiceView::Others => i18n::t("view_others"),
            ServiceView::Devices => i18n::t("view_devices"),
        }
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.position() + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        Self::ALL[(self.position() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    fn position(self) -> usize {
        Self::ALL.iter().position(|view| *view == self).unwrap_or(0)
    }

    pub fn matches(self, service: &Service) -> bool {
        match self {
            ServiceView::Services => service.name.ends_with(".service"),
            ServiceView::Devices => service.name.ends_with(".device"),
            ServiceView::Others => {
                !service.name.ends_with(".service") && !service.name.ends_with(".device")
            }
        }
    }

    pub fn is_manageable(self) -> bool {
        self != ServiceView::Devices
    }
}

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
        let network_manager = service("NetworkManager.service", "active", "enabled");
        assert!(TipFilter::Critical.matches(&network_manager));
        assert!(!TipFilter::Suggestion.matches(&network_manager));
    }

    #[test]
    fn service_view_splits_units_by_type() {
        let service_unit = service("sshd.service", "active", "enabled");
        let device_unit = service("dev-sda1.device", "active", "plugged");
        let socket_unit = service("cups.socket", "active", "enabled");

        assert!(ServiceView::Services.matches(&service_unit));
        assert!(!ServiceView::Services.matches(&device_unit));
        assert!(ServiceView::Devices.matches(&device_unit));
        assert!(!ServiceView::Devices.matches(&socket_unit));
        assert!(ServiceView::Others.matches(&socket_unit));
        assert!(!ServiceView::Others.matches(&service_unit));
        assert!(!ServiceView::Others.matches(&device_unit));

        assert!(ServiceView::Services.is_manageable());
        assert!(!ServiceView::Devices.is_manageable());
    }

    #[test]
    fn service_view_cycles_back_to_the_first_variant() {
        let mut view = ServiceView::Services;
        for _ in 0..ServiceView::ALL.len() {
            view = view.next();
        }
        assert_eq!(view, ServiceView::Services);
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
