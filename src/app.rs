use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::autostart::{self, AutostartEntry};
use crate::backups::{self, Backup};
use crate::custom_profiles::{self, CustomProfile};
use crate::filters::{ServiceView, SortBy, StatusFilter, TipFilter};
use crate::i18n;
use crate::privilege::{self, Action};
use crate::system::{self, Service, SystemData, parse_duration_seconds};
use crate::{profiles, service_db};

const MAX_DELAY: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Analysis,
    Services,
    Autostart,
    Profiles,
    Backups,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Analysis,
        Tab::Services,
        Tab::Autostart,
        Tab::Profiles,
        Tab::Backups,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Analysis => i18n::t("tab_analysis"),
            Tab::Services => i18n::t("tab_services"),
            Tab::Autostart => i18n::t("tab_autostart"),
            Tab::Profiles => i18n::t("tab_profiles"),
            Tab::Backups => i18n::t("tab_backups"),
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Menu,
    Confirm,
    Password,
    Form,
    Delay,
    ProfileForm,
    Language,
}

pub struct OptimizableService {
    pub name: String,
    pub time: String,
    pub seconds: f64,
    pub suggestion: String,
}

pub struct InfoPane {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: u16,
}

enum Pending {
    Action(Action, String),
    Bulk(Vec<String>),
    RemoveAutostart { name: String, path: PathBuf },
    AddAutostart { name: String, command: String },
    ApplyProfile(profiles::Plan),
    RestoreBackup(profiles::Plan),
    RemoveBackup { name: String, path: PathBuf },
    RemoveCustomProfile { name: String, path: PathBuf },
    CreateBackup(BTreeMap<String, String>),
}

impl Pending {
    fn needs_password(&self) -> bool {
        matches!(
            self,
            Pending::Action(..)
                | Pending::Bulk(..)
                | Pending::ApplyProfile(..)
                | Pending::RestoreBackup(..)
        )
    }
}

enum CommandOutcome {
    Message(Result<String, String>),
    Info { title: String, lines: Vec<String> },
}

pub struct App {
    pub tab: Tab,
    pub should_quit: bool,
    pub data: Option<SystemData>,
    pub selected: usize,
    pub autostart_selected: usize,
    pub view: ServiceView,
    pub profiles_selected: usize,
    pub profile_name: String,
    pub profile_form_index: usize,
    pub profile_form_checked: Vec<bool>,
    pub backups_selected: usize,
    pub form_values: Vec<String>,
    pub form_index: usize,
    pub analysis_scroll: u16,
    pub loading: bool,
    pub status_filter: StatusFilter,
    pub tip_filter: TipFilter,
    pub sort: SortBy,
    pub query: String,
    pub searching: bool,
    pub reload_requested: bool,
    pub overlay: Overlay,
    pub menu_index: usize,
    pub language_index: usize,
    pub password_input: String,
    pub delay_input: String,
    pub delay_replace: bool,
    pub message: Option<String>,
    pub level: Level,
    pub running: bool,
    pub info: Option<InfoPane>,
    clipboard: Option<String>,
    password: Option<String>,
    pending: Option<Pending>,
    command_tx: Sender<CommandOutcome>,
    command_rx: Receiver<CommandOutcome>,
}

impl App {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        Self {
            tab: Tab::Analysis,
            should_quit: false,
            data: None,
            selected: 0,
            autostart_selected: 0,
            view: ServiceView::Services,
            profiles_selected: 0,
            profile_name: String::new(),
            profile_form_index: 0,
            profile_form_checked: vec![false; custom_profiles::CANDIDATES.len()],
            backups_selected: 0,
            form_values: Vec::new(),
            form_index: 0,
            analysis_scroll: 0,
            loading: true,
            status_filter: StatusFilter::All,
            tip_filter: TipFilter::Suggestion,
            sort: SortBy::BootTime,
            query: String::new(),
            searching: false,
            reload_requested: false,
            overlay: Overlay::None,
            menu_index: 0,
            language_index: 0,
            password_input: String::new(),
            delay_input: String::new(),
            delay_replace: false,
            message: None,
            level: Level::Info,
            running: false,
            info: None,
            clipboard: None,
            password: None,
            pending: None,
            command_tx,
            command_rx,
        }
    }

    pub fn poll_data(&mut self, receiver: &Receiver<SystemData>) {
        match receiver.try_recv() {
            Ok(data) => {
                self.data = Some(data);
                self.loading = false;
            }
            Err(TryRecvError::Disconnected) => self.loading = false,
            Err(TryRecvError::Empty) => {}
        }
    }

    fn notify(&mut self, text: String, level: Level) {
        self.message = Some(text);
        self.level = level;
    }

    fn clear_message(&mut self) {
        self.message = None;
        self.level = Level::Info;
    }

    // Two `systemctl` processes would race over the same unit states.
    fn refuse_while_running(&mut self) -> bool {
        if self.running {
            self.notify(i18n::t("tui_busy").to_string(), Level::Info);
            return true;
        }
        false
    }

    pub fn poll_command(&mut self) {
        match self.command_rx.try_recv() {
            Ok(CommandOutcome::Message(Ok(output))) => {
                self.running = false;
                self.notify(
                    if output.is_empty() {
                        i18n::t("tui_done").to_string()
                    } else {
                        output
                    },
                    Level::Success,
                );
                self.reload_requested = true;
            }
            Ok(CommandOutcome::Message(Err(error))) => {
                self.running = false;
                if privilege::is_auth_failure(&error) {
                    self.password = None;
                }
                self.notify(error, Level::Error);
            }
            Ok(CommandOutcome::Info { title, lines }) => {
                self.running = false;
                self.info = Some(InfoPane {
                    title,
                    lines,
                    scroll: 0,
                });
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
        }
    }

    pub fn visible_services(&self) -> Vec<&Service> {
        let Some(services) = self.services() else {
            return Vec::new();
        };

        let query = self.query.to_lowercase();
        let mut visible: Vec<&Service> = services
            .iter()
            .filter(|service| self.view.matches(service))
            .filter(|service| self.status_filter.matches(service))
            .filter(|service| self.tip_filter.matches(service))
            .filter(|service| {
                query.is_empty()
                    || service.name.to_lowercase().contains(&query)
                    || service.description.to_lowercase().contains(&query)
            })
            .collect();

        match self.sort {
            SortBy::Name => visible.sort_by(|a, b| a.name.cmp(&b.name)),
            SortBy::BootTime => {
                let times = self.blame_times();
                visible.sort_by(|a, b| {
                    let a_time = times
                        .get(a.name.as_str())
                        .map_or(0.0, |t| parse_duration_seconds(t));
                    let b_time = times
                        .get(b.name.as_str())
                        .map_or(0.0, |t| parse_duration_seconds(t));
                    b_time.total_cmp(&a_time)
                });
            }
        }

        visible
    }

    pub fn blame_times(&self) -> HashMap<&str, &str> {
        self.data
            .as_ref()
            .and_then(|data| data.blame.as_ref().ok())
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| (entry.name.as_str(), entry.time.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn selected_service(&self) -> Option<&Service> {
        self.visible_services().get(self.selected).copied()
    }

    pub fn view_total(&self) -> usize {
        self.services().map_or(0, |services| {
            services
                .iter()
                .filter(|service| self.view.matches(service))
                .count()
        })
    }

    pub fn optimizable_services(&self) -> Vec<OptimizableService> {
        let Some(data) = &self.data else {
            return Vec::new();
        };
        let (Ok(blame), Ok(services)) = (&data.blame, &data.services) else {
            return Vec::new();
        };
        let safe = &profiles::data().safe_to_disable_services;

        let mut optimizable: Vec<OptimizableService> = blame
            .iter()
            .filter(|entry| safe.contains(&entry.name))
            .filter_map(|entry| {
                let service = services.iter().find(|service| service.name == entry.name)?;
                if service.enabled != "enabled" && service.enabled != "enabled-runtime" {
                    return None;
                }
                let suggestion = service_db::lookup(&entry.name).map_or_else(String::new, |db| {
                    let suggestion = db.suggestion_text();
                    if suggestion.is_empty() {
                        db.description().to_string()
                    } else {
                        suggestion.to_string()
                    }
                });
                Some(OptimizableService {
                    name: entry.name.clone(),
                    time: entry.time.clone(),
                    seconds: parse_duration_seconds(&entry.time),
                    suggestion,
                })
            })
            .collect();

        optimizable.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
        optimizable
    }

    pub fn analysis_lines(&self) -> Vec<String> {
        let Some(data) = &self.data else {
            return vec![if self.loading {
                i18n::t("tui_loading").to_string()
            } else {
                i18n::t("tui_no_data").to_string()
            }];
        };

        let mut lines = vec![
            i18n::t("tui_system").to_string(),
            labelled(i18n::t("sys_os"), &data.info.os),
            labelled(i18n::t("sys_kernel"), &data.info.kernel),
            labelled(i18n::t("sys_ram"), &data.info.ram),
            labelled(i18n::t("sys_uptime"), &data.info.uptime),
            String::new(),
            i18n::t("boot_suresi").to_string(),
        ];

        match &data.boot {
            Ok(boot) => {
                lines.push(format!("  {}: {}", i18n::t("toplam_boot"), boot.total));
                lines.push(format!("  {}", boot.detail));
                for stage in system::parse_breakdown(&boot.detail) {
                    // systemd prints these names in English. The table has a
                    // translation for each of them (comp_firmware, comp_kernel,
                    // ...) and returns an unknown label unchanged.
                    let key = format!("comp_{}", stage.label.to_lowercase());
                    lines.push(format!("    {}: {}", i18n::t(&key), stage.time));
                }
            }
            Err(err) => lines.push(format!("  {err}")),
        }

        lines.push(String::new());
        lines.push(i18n::t("startup_optimizasyonu").to_string());
        let optimizable = self.optimizable_services();
        if optimizable.is_empty() {
            lines.push(format!("  {}", i18n::t("no_optimizable_services")));
        } else {
            let total: f64 = optimizable.iter().map(|item| item.seconds).sum();
            lines.push(format!(
                "  {}: ~{total:.2} {}",
                i18n::t("tui_potential_savings"),
                i18n::t("tui_second")
            ));
            lines.push(format!("  {}", i18n::t("tui_suggested_services")));
            for item in &optimizable {
                lines.push(format!("    {:<42} {:>9}", item.name, item.time));
                if !item.suggestion.is_empty() {
                    lines.push(format!("      {}", item.suggestion));
                }
            }
            lines.push(format!("  {}", i18n::t("tui_optimize_hint")));
        }

        lines.push(String::new());
        lines.push(i18n::t("tui_slowest_services").to_string());
        match &data.blame {
            Ok(entries) => {
                for entry in entries.iter().take(20) {
                    lines.push(format!("  {:>10}  {}", entry.time, entry.name));
                }
            }
            Err(err) => lines.push(format!("  {err}")),
        }

        lines
    }

    pub fn confirmation_prompt(&self) -> String {
        match &self.pending {
            Some(Pending::Action(action, unit)) => {
                i18n::fill(i18n::t(action.prompt_key()), &[unit])
            }
            Some(Pending::Bulk(units)) => {
                i18n::fill(i18n::t("tui_prompt_bulk"), &[&units.len().to_string()])
            }
            Some(Pending::RemoveAutostart { name, .. }) => {
                i18n::fill(i18n::t("tui_remove_autostart"), &[name])
            }
            Some(Pending::ApplyProfile(plan)) => {
                i18n::fill(i18n::t("tui_apply_profile_prompt"), &[&plan.summary()])
            }
            Some(Pending::RestoreBackup(plan)) => {
                i18n::fill(i18n::t("tui_restore_backup_prompt"), &[&plan.summary()])
            }
            Some(Pending::RemoveBackup { name, .. }) => {
                i18n::fill(i18n::t("tui_delete_backup_prompt"), &[name])
            }
            Some(Pending::RemoveCustomProfile { name, .. }) => {
                i18n::fill(i18n::t("tui_delete_profile_prompt"), &[name])
            }
            Some(Pending::AddAutostart { .. }) | Some(Pending::CreateBackup(..)) | None => {
                i18n::t("tui_prompt_confirm").to_string()
            }
        }
    }

    pub fn backups(&self) -> &[Backup] {
        self.data
            .as_ref()
            .and_then(|data| data.backups.as_ref().ok())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn selected_profile(&self) -> Option<&'static str> {
        profiles::ids().get(self.profiles_selected).copied()
    }

    pub fn custom_profiles(&self) -> &[CustomProfile] {
        self.data
            .as_ref()
            .and_then(|data| data.custom_profiles.as_ref().ok())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn selected_custom_profile(&self) -> Option<&CustomProfile> {
        let builtin = profiles::ids().len();
        self.custom_profiles()
            .get(self.profiles_selected.checked_sub(builtin)?)
    }

    pub fn profile_count(&self) -> usize {
        profiles::ids().len() + self.custom_profiles().len()
    }

    pub fn selected_backup(&self) -> Option<&Backup> {
        self.backups().get(self.backups_selected)
    }

    pub fn profile_savings(&self, profile_id: &str) -> f64 {
        let Some(profile) = profiles::data().profiles.get(profile_id) else {
            return 0.0;
        };
        self.savings_of(profile.services.iter())
    }

    pub fn custom_profile_savings(&self, profile: &CustomProfile) -> f64 {
        self.savings_of(profile.services.iter())
    }

    fn savings_of<'a>(&self, rules: impl Iterator<Item = (&'a String, &'a String)>) -> f64 {
        let states = self.current_states();
        let times = self.blame_times();

        rules
            .filter(|(_, action)| action.as_str() == "disable")
            .filter_map(|(name, _)| {
                let (enabled, _) = states.get(name.as_str()).copied()?;
                if !profiles::is_enabled(enabled) {
                    return None;
                }
                times
                    .get(name.as_str())
                    .map(|time| parse_duration_seconds(time))
            })
            .sum()
    }

    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    fn current_states(&self) -> HashMap<&str, (&str, &str)> {
        self.services()
            .map(|services| {
                services
                    .iter()
                    .map(|service| {
                        (
                            service.name.as_str(),
                            (service.enabled.as_str(), service.active.as_str()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn build_plan(&self, enable: &[String], disable: &[String]) -> profiles::Plan {
        let current = self.current_states();
        let mut plan = profiles::Plan::default();

        for name in enable {
            let (enabled, active) = current
                .get(name.as_str())
                .copied()
                .unwrap_or(("unknown", "unknown"));
            if !profiles::is_enabled(enabled) {
                plan.enable.push(name.clone());
            }
            if active != "active" {
                plan.start.push(name.clone());
            }
        }

        for name in disable {
            let (enabled, active) = current
                .get(name.as_str())
                .copied()
                .unwrap_or(("unknown", "unknown"));
            if profiles::is_enabled(enabled) {
                plan.disable.push(name.clone());
            }
            if active == "active" {
                plan.stop.push(name.clone());
            }
        }

        plan
    }

    pub fn autostart_entries(&self) -> &[AutostartEntry] {
        self.data
            .as_ref()
            .and_then(|data| data.autostart.as_ref().ok())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn selected_autostart(&self) -> Option<&AutostartEntry> {
        self.autostart_entries().get(self.autostart_selected)
    }

    pub fn report(&self) -> String {
        let mut report = format!(
            "{}\n{}\n\n",
            i18n::t("tui_report_title"),
            "=".repeat(i18n::t("tui_report_title").chars().count())
        );
        for line in self.analysis_lines() {
            report.push_str(&line);
            report.push('\n');
        }
        report.push_str(&format!("\n{}\n", i18n::t("tui_report_footer")));
        report
    }

    fn export_report(&mut self) {
        if self.tab != Tab::Analysis {
            return;
        }

        let path = report_path();
        let report = self.report();
        let written = std::fs::write(&path, &report);
        match written {
            Ok(()) => {
                self.clipboard = Some(report);
                let text = i18n::fill(i18n::t("tui_report_copied"), &[&path]);
                self.notify(text, Level::Success);
            }
            Err(err) => {
                let text = i18n::fill(i18n::t("tui_report_failed"), &[&path, &err.to_string()]);
                self.notify(text, Level::Error);
            }
        }
    }

    fn services(&self) -> Option<&[Service]> {
        self.data
            .as_ref()?
            .services
            .as_ref()
            .ok()
            .map(Vec::as_slice)
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Some platforms report key release as well; only act on presses.
        if key.kind != KeyEventKind::Press {
            return;
        }

        // Raw mode hands Ctrl-C over as a key instead of a signal, so it has to
        // be answered here; it quits from any screen, overlay included.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.info.is_some() {
            return self.handle_info_key(key);
        }

        match self.overlay {
            Overlay::Password => return self.handle_password_key(key),
            Overlay::Menu => return self.handle_menu_key(key),
            Overlay::Confirm => return self.handle_confirm_key(key),
            Overlay::Form => return self.handle_form_key(key),
            Overlay::Delay => return self.handle_delay_key(key),
            Overlay::ProfileForm => return self.handle_profile_form_key(key),
            Overlay::Language => return self.handle_language_key(key),
            Overlay::None => {}
        }

        if self.searching {
            match key.code {
                KeyCode::Enter => self.searching = false,
                KeyCode::Esc => {
                    self.searching = false;
                    self.query.clear();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(character) => self.query.push(character),
                _ => {}
            }
            self.reset_selection();
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => self.select_next_tab(),
            KeyCode::BackTab => self.select_previous_tab(),
            KeyCode::Char(digit @ '1'..='5') => self.select_tab(digit),
            KeyCode::Right => self.cycle_view(),
            KeyCode::Left => self.cycle_view_back(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::Enter | KeyCode::Char('a') => self.primary_action(),
            KeyCode::Char('/') => {
                self.query.clear();
                self.searching = true;
                self.reset_selection();
            }
            KeyCode::Char('f') => {
                self.status_filter = self.status_filter.next();
                self.reset_selection();
            }
            KeyCode::Char('t') => {
                self.tip_filter = self.tip_filter.next();
                self.reset_selection();
            }
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.reset_selection();
            }
            KeyCode::Char('o') => self.begin_quick_optimize(),
            KeyCode::Char('w') => self.export_report(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('L') => self.open_language_menu(),
            KeyCode::Char('b') => self.begin_create_backup(),
            KeyCode::Char('v') => self.cycle_view(),
            KeyCode::Char(' ') => self.toggle_autostart(),
            KeyCode::Char('n') => self.begin_add_form(),
            KeyCode::Char('x') => self.remove_selected(),
            KeyCode::Char('r') => self.reload_requested = true,
            KeyCode::Char('l') => self.show_journal(),
            KeyCode::Char('d') => self.secondary_action(),
            KeyCode::Char('D') => self.show_dependencies(true),
            _ => {}
        }
    }

    fn handle_info_key(&mut self, key: KeyEvent) {
        let Some(info) = self.info.as_mut() else {
            return;
        };
        let max_scroll = info.lines.len().saturating_sub(1) as u16;

        let mut close = false;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('l') => close = true,
            KeyCode::Down | KeyCode::Char('j') => info.scroll = (info.scroll + 1).min(max_scroll),
            KeyCode::Up | KeyCode::Char('k') => info.scroll = info.scroll.saturating_sub(1),
            KeyCode::PageDown => info.scroll = (info.scroll + 10).min(max_scroll),
            KeyCode::PageUp => info.scroll = info.scroll.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => info.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => info.scroll = max_scroll,
            _ => {}
        }

        if close {
            self.info = None;
        }
    }

    fn show_journal(&mut self) {
        if self.tab != Tab::Services {
            return;
        }
        let Some(unit) = self.selected_service().map(|service| service.name.clone()) else {
            return;
        };

        let password = self.password.clone();
        let sender = self.command_tx.clone();
        if self.refuse_while_running() {
            return;
        }
        self.running = true;
        self.clear_message();
        thread::spawn(move || {
            let text = system::journal(&unit, 200, password.as_deref());
            let _ = sender.send(CommandOutcome::Info {
                title: i18n::fill(i18n::t("tui_title_log"), &[&unit]),
                lines: text.lines().map(str::to_string).collect(),
            });
        });
    }

    fn show_dependencies(&mut self, reverse: bool) {
        if self.tab != Tab::Services {
            return;
        }
        let Some(unit) = self.selected_service().map(|service| service.name.clone()) else {
            return;
        };

        let sender = self.command_tx.clone();
        if self.refuse_while_running() {
            return;
        }
        self.running = true;
        self.clear_message();
        thread::spawn(move || {
            let text = system::dependencies(&unit, reverse).unwrap_or_else(|err| err);
            let key = if reverse {
                "tui_title_reverse_dependencies"
            } else {
                "tui_title_dependencies"
            };
            let title = i18n::fill(i18n::t(key), &[&unit]);
            let _ = sender.send(CommandOutcome::Info {
                title,
                lines: text.lines().map(str::to_string).collect(),
            });
        });
    }

    fn handle_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Down | KeyCode::Char('j') => {
                self.menu_index = (self.menu_index + 1).min(Action::ALL.len() - 1);
            }
            KeyCode::Up | KeyCode::Char('k') => self.menu_index = self.menu_index.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(action) = Action::ALL.get(self.menu_index).copied() {
                    self.choose_action(action);
                }
            }
            _ => {}
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => self.run_pending(),
            KeyCode::Char('n') | KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.pending = None;
            }
            _ => {}
        }
    }

    fn handle_password_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.password = Some(std::mem::take(&mut self.password_input));
                self.run_pending();
            }
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.pending = None;
            }
            KeyCode::Backspace => {
                self.password_input.pop();
            }
            KeyCode::Char(character) => self.password_input.push(character),
            _ => {}
        }
    }

    fn open_action_menu(&mut self) {
        if self.tab != Tab::Services || self.selected_service().is_none() {
            return;
        }
        // The original application disables the buttons for device units.
        if !self.view.is_manageable() {
            self.notify(
                i18n::t("tui_device_not_manageable").to_string(),
                Level::Info,
            );
            return;
        }
        self.menu_index = 0;
        self.overlay = Overlay::Menu;
    }

    fn choose_action(&mut self, action: Action) {
        let Some(unit) = self.selected_service().map(|service| service.name.clone()) else {
            return;
        };
        self.pending = Some(Pending::Action(action, unit));
        self.overlay = Overlay::Confirm;
    }

    fn begin_quick_optimize(&mut self) {
        if self.tab != Tab::Analysis {
            return;
        }
        let units: Vec<String> = self
            .optimizable_services()
            .iter()
            .map(|item| item.name.clone())
            .collect();
        if units.is_empty() {
            self.notify(i18n::t("tui_nothing_to_optimize").to_string(), Level::Info);
            return;
        }
        self.pending = Some(Pending::Bulk(units));
        self.overlay = Overlay::Confirm;
    }

    fn run_pending(&mut self) {
        self.overlay = Overlay::None;
        let Some(pending) = self.pending.as_ref() else {
            return;
        };

        if pending.needs_password() && self.password.is_none() {
            self.password_input.clear();
            self.overlay = Overlay::Password;
            return;
        }

        let Some(pending) = self.pending.take() else {
            return;
        };
        let password = self.password.clone().unwrap_or_default();
        self.spawn_pending(pending, password);
    }

    fn spawn_pending(&mut self, pending: Pending, password: String) {
        if self.refuse_while_running() {
            return;
        }
        let sender = self.command_tx.clone();
        self.running = true;
        self.clear_message();
        thread::spawn(move || {
            let result = match pending {
                Pending::Action(action, unit) => privilege::systemctl(action, &unit, &password),
                Pending::Bulk(units) => privilege::disable_all(&units, &password),
                Pending::RemoveAutostart { path, .. } => {
                    autostart::remove(&path).map(|()| i18n::t("tui_autostart_removed").to_string())
                }
                Pending::AddAutostart { name, command } => autostart::add(&name, &command)
                    .map(|path| i18n::fill(i18n::t("tui_created"), &[&path.display().to_string()])),
                Pending::ApplyProfile(plan) | Pending::RestoreBackup(plan) => {
                    run_plan(&plan, &password)
                }
                Pending::RemoveBackup { path, .. } => {
                    backups::remove(&path).map(|()| i18n::t("tui_backup_deleted").to_string())
                }
                Pending::RemoveCustomProfile { path, .. } => custom_profiles::remove(&path)
                    .map(|()| i18n::t("custom_profile_deleted").to_string()),
                Pending::CreateBackup(states) => backups::create(states)
                    .map(|backup| i18n::fill(i18n::t("tui_saved_backup"), &[&backup.name])),
            };
            let _ = sender.send(CommandOutcome::Message(result));
        });
    }

    fn perform(&mut self, pending: Pending) {
        self.pending = Some(pending);
        self.run_pending();
    }

    fn toggle_autostart(&mut self) {
        if self.tab != Tab::Autostart {
            return;
        }
        let Some(entry) = self.selected_autostart() else {
            return;
        };
        let result = autostart::set_enabled(entry, !entry.enabled);
        self.finish_local(result);
    }

    fn begin_delay_edit(&mut self) {
        if self.tab != Tab::Autostart {
            return;
        }
        let Some(delay) = self.selected_autostart().map(|entry| entry.delay) else {
            return;
        };
        self.delay_input = delay.to_string();
        self.delay_replace = true;
        self.overlay = Overlay::Delay;
    }

    fn handle_delay_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.save_autostart_delay(),
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up => self.step_delay(1),
            KeyCode::Down => self.step_delay(-1),
            KeyCode::Backspace => {
                self.delay_replace = false;
                self.delay_input.pop();
            }
            KeyCode::Char(digit) if digit.is_ascii_digit() && self.delay_input.len() < 4 => {
                if self.delay_replace {
                    self.delay_input.clear();
                    self.delay_replace = false;
                }
                self.delay_input.push(digit);
            }
            _ => {}
        }
    }

    fn step_delay(&mut self, delta: i32) {
        let current = self.delay_input.trim().parse::<u32>().unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, MAX_DELAY as i32) as u32;
        self.delay_input = next.to_string();
        self.delay_replace = false;
    }

    fn save_autostart_delay(&mut self) {
        let Some(entry) = self.selected_autostart() else {
            self.overlay = Overlay::None;
            return;
        };
        let Some(delay) = parse_delay(&self.delay_input) else {
            self.notify(i18n::t("tui_delay_invalid").to_string(), Level::Error);
            return;
        };
        let result = autostart::set_delay(entry, delay);
        self.overlay = Overlay::None;
        self.finish_local(result);
    }

    fn begin_add_form(&mut self) {
        match self.tab {
            Tab::Autostart => {
                self.form_values = vec![String::new(), String::new()];
                self.form_index = 0;
                self.overlay = Overlay::Form;
            }
            Tab::Profiles => self.begin_new_custom_profile(),
            _ => {}
        }
    }

    fn begin_remove_autostart(&mut self) {
        if self.tab != Tab::Autostart {
            return;
        }
        let Some(entry) = self.selected_autostart() else {
            return;
        };
        self.pending = Some(Pending::RemoveAutostart {
            name: entry.name.clone(),
            path: entry.path.clone(),
        });
        self.overlay = Overlay::Confirm;
    }

    fn handle_form_key(&mut self, key: KeyEvent) {
        let fields = self.form_values.len();
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => self.submit_form(),
            KeyCode::Tab | KeyCode::Down => self.form_index = (self.form_index + 1) % fields,
            KeyCode::BackTab | KeyCode::Up => {
                self.form_index = (self.form_index + fields - 1) % fields
            }
            KeyCode::Backspace => {
                if let Some(field) = self.form_values.get_mut(self.form_index) {
                    field.pop();
                }
            }
            KeyCode::Char(character) => {
                if let Some(field) = self.form_values.get_mut(self.form_index) {
                    field.push(character);
                }
            }
            _ => {}
        }
    }

    fn submit_form(&mut self) {
        let name = self.form_values.first().cloned().unwrap_or_default();
        let command = self.form_values.get(1).cloned().unwrap_or_default();
        self.overlay = Overlay::None;

        if name.trim().is_empty() {
            self.notify(i18n::t("tui_name_required").to_string(), Level::Error);
            return;
        }
        if command.trim().is_empty() {
            self.notify(i18n::t("tui_command_required").to_string(), Level::Error);
            return;
        }

        self.perform(Pending::AddAutostart { name, command });
    }

    fn finish_local(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.notify(i18n::t("tui_autostart_updated").to_string(), Level::Success);
                self.reload_requested = true;
            }
            Err(error) => self.notify(error, Level::Error),
        }
    }

    fn primary_action(&mut self) {
        match self.tab {
            Tab::Services => self.open_action_menu(),
            Tab::Profiles => self.begin_apply_profile(),
            Tab::Backups => self.begin_restore_backup(),
            _ => {}
        }
    }

    fn secondary_action(&mut self) {
        match self.tab {
            Tab::Services => self.show_dependencies(false),
            Tab::Backups => self.begin_restore_backup(),
            Tab::Autostart => self.begin_delay_edit(),
            _ => {}
        }
    }

    fn remove_selected(&mut self) {
        match self.tab {
            Tab::Autostart => self.begin_remove_autostart(),
            Tab::Backups => self.begin_remove_backup(),
            Tab::Profiles => self.begin_remove_custom_profile(),
            _ => {}
        }
    }

    fn cycle_view(&mut self) {
        if self.tab == Tab::Services {
            self.view = self.view.next();
            self.reset_selection();
        }
    }

    fn cycle_view_back(&mut self) {
        if self.tab == Tab::Services {
            self.view = self.view.previous();
            self.reset_selection();
        }
    }

    fn begin_create_backup(&mut self) {
        if self.tab != Tab::Profiles && self.tab != Tab::Backups {
            return;
        }
        let states = self
            .services()
            .map(|services| {
                services
                    .iter()
                    .map(|service| (service.name.clone(), service.enabled.clone()))
                    .collect()
            })
            .unwrap_or_default();

        self.perform(Pending::CreateBackup(states));
    }

    fn begin_apply_profile(&mut self) {
        if self.tab != Tab::Profiles {
            return;
        }
        let (enable, disable) = match self.selected_custom_profile() {
            Some(profile) => profile.selections(),
            None => match self.selected_profile() {
                Some(profile_id) => profiles::selections(profile_id),
                None => return,
            },
        };

        let plan = self.build_plan(&enable, &disable);
        if plan.is_empty() {
            self.notify(i18n::t("tui_profile_matches").to_string(), Level::Info);
            return;
        }

        self.pending = Some(Pending::ApplyProfile(plan));
        self.overlay = Overlay::Confirm;
    }

    fn begin_new_custom_profile(&mut self) {
        self.profile_name.clear();
        self.profile_form_index = 0;
        self.profile_form_checked
            .resize(custom_profiles::CANDIDATES.len(), false);
        self.overlay = Overlay::ProfileForm;
    }

    fn handle_profile_form_key(&mut self, key: KeyEvent) {
        let rows = custom_profiles::CANDIDATES.len() + 1;
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => self.submit_profile_form(),
            KeyCode::Tab | KeyCode::Down => {
                self.profile_form_index = (self.profile_form_index + 1) % rows
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.profile_form_index = (self.profile_form_index + rows - 1) % rows
            }
            KeyCode::Char(' ') => {
                if self.profile_form_index > 0
                    && let Some(checked) = self
                        .profile_form_checked
                        .get_mut(self.profile_form_index - 1)
                {
                    *checked = !*checked;
                }
            }
            KeyCode::Backspace if self.profile_form_index == 0 => {
                self.profile_name.pop();
            }
            KeyCode::Char(character) if self.profile_form_index == 0 => {
                self.profile_name.push(character);
            }
            _ => {}
        }
    }

    fn submit_profile_form(&mut self) {
        let name = self.profile_name.trim().to_string();
        if name.is_empty() {
            self.notify(
                i18n::t("profile_name_girin_error").to_string(),
                Level::Error,
            );
            return;
        }

        // Off means "disable": the original form stores a rule for every
        // service it offers, in both directions.
        let services = custom_profiles::form_rules(&self.profile_form_checked);

        self.overlay = Overlay::None;
        match custom_profiles::save(&name, &services) {
            Ok(_) => {
                self.notify(
                    i18n::fill(i18n::t("custom_profile_olusturuldu"), &[&name]),
                    Level::Success,
                );
                self.reload_requested = true;
            }
            Err(error) => self.notify(error, Level::Error),
        }
    }

    fn begin_remove_custom_profile(&mut self) {
        let Some(profile) = self.selected_custom_profile() else {
            return;
        };

        self.pending = Some(Pending::RemoveCustomProfile {
            name: profile.name.clone(),
            path: profile.path.clone(),
        });
        self.overlay = Overlay::Confirm;
    }

    fn begin_restore_backup(&mut self) {
        if self.tab != Tab::Backups {
            return;
        }
        let Some(backup) = self.selected_backup() else {
            return;
        };

        let current = self.current_states();
        let mut enable = Vec::new();
        let mut disable = Vec::new();
        for (name, state) in &backup.services {
            let Some((enabled, _)) = current.get(name.as_str()).copied() else {
                continue;
            };
            if profiles::is_enabled(state) && !profiles::is_enabled(enabled) {
                enable.push(name.clone());
            } else if !profiles::is_enabled(state) && profiles::is_enabled(enabled) {
                disable.push(name.clone());
            }
        }

        let plan = self.build_plan(&enable, &disable);
        if plan.is_empty() {
            self.notify(i18n::t("tui_backup_matches").to_string(), Level::Info);
            return;
        }

        self.pending = Some(Pending::RestoreBackup(plan));
        self.overlay = Overlay::Confirm;
    }

    fn begin_remove_backup(&mut self) {
        if self.tab != Tab::Backups {
            return;
        }
        let Some(backup) = self.selected_backup() else {
            return;
        };

        self.pending = Some(Pending::RemoveBackup {
            name: backup.name.clone(),
            path: backup.path.clone(),
        });
        self.overlay = Overlay::Confirm;
    }

    fn open_help(&mut self) {
        let keys = [
            "tui_help_navigation",
            "tui_help_quit",
            "tui_help_tab",
            "tui_help_tab_keys",
            "tui_help_move",
            "tui_help_page",
            "tui_help_language",
            "",
            "tui_help_services",
            "tui_help_view",
            "tui_help_search",
            "tui_help_filters",
            "tui_help_actions",
            "tui_help_log",
            "tui_help_deps",
            "",
            "tui_help_analysis",
            "tui_help_optimize",
            "tui_help_report",
            "",
            "tui_help_autostart",
            "tui_help_toggle",
            "tui_help_delay",
            "tui_help_add_remove",
            "",
            "tui_help_profiles",
            "tui_help_apply",
            "tui_help_custom_new",
            "tui_help_custom_delete",
            "tui_help_backups",
            "tui_help_backup",
            "tui_help_restore",
            "",
            "tui_help_reload",
        ];
        let mut lines: Vec<String> = keys.iter().map(|key| i18n::t(key).to_string()).collect();
        lines.push(String::new());
        lines.push(i18n::fill(
            i18n::t("tui_language_set"),
            &[i18n::lang().native_name()],
        ));

        self.info = Some(InfoPane {
            title: i18n::t("tui_help_title").to_string(),
            lines,
            scroll: 0,
        });
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
    }

    fn open_language_menu(&mut self) {
        self.language_index = i18n::Lang::ALL
            .iter()
            .position(|lang| *lang == i18n::lang())
            .unwrap_or(0);
        self.overlay = Overlay::Language;
    }

    fn handle_language_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Down | KeyCode::Char('j') => {
                self.language_index = (self.language_index + 1).min(i18n::Lang::ALL.len() - 1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.language_index = self.language_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(lang) = i18n::Lang::ALL.get(self.language_index).copied() {
                    i18n::choose(lang);
                }
                self.overlay = Overlay::None;
            }
            _ => {}
        }
    }

    fn select_next_tab(&mut self) {
        let next = (self.tab.index() + 1) % Tab::ALL.len();
        self.tab = Tab::ALL[next];
    }

    fn select_previous_tab(&mut self) {
        let count = Tab::ALL.len();
        let previous = (self.tab.index() + count - 1) % count;
        self.tab = Tab::ALL[previous];
    }

    fn select_tab(&mut self, digit: char) {
        let index = digit as usize - '1' as usize;
        if let Some(tab) = Tab::ALL.get(index) {
            self.tab = *tab;
        }
    }

    fn move_down(&mut self) {
        match self.tab {
            Tab::Services => {
                let count = self.visible_services().len();
                if count > 0 {
                    self.selected = (self.selected + 1).min(count - 1);
                }
            }
            Tab::Autostart => {
                let count = self.autostart_entries().len();
                if count > 0 {
                    self.autostart_selected = (self.autostart_selected + 1).min(count - 1);
                }
            }
            Tab::Profiles => {
                let count = self.profile_count();
                if count > 0 {
                    self.profiles_selected = (self.profiles_selected + 1).min(count - 1);
                }
            }
            Tab::Backups => {
                let count = self.backups().len();
                if count > 0 {
                    self.backups_selected = (self.backups_selected + 1).min(count - 1);
                }
            }
            Tab::Analysis => self.scroll_analysis(1),
        }
    }

    fn move_up(&mut self) {
        match self.tab {
            Tab::Services => self.selected = self.selected.saturating_sub(1),
            Tab::Autostart => self.autostart_selected = self.autostart_selected.saturating_sub(1),
            Tab::Profiles => self.profiles_selected = self.profiles_selected.saturating_sub(1),
            Tab::Backups => self.backups_selected = self.backups_selected.saturating_sub(1),
            Tab::Analysis => self.scroll_analysis(-1),
        }
    }

    fn page_down(&mut self) {
        if self.tab == Tab::Analysis {
            self.scroll_analysis(10);
        }
    }

    fn page_up(&mut self) {
        if self.tab == Tab::Analysis {
            self.scroll_analysis(-10);
        }
    }

    fn scroll_analysis(&mut self, delta: i32) {
        let max = self.analysis_lines().len().saturating_sub(1) as i32;
        let next = (i32::from(self.analysis_scroll) + delta).clamp(0, max);
        self.analysis_scroll = next as u16;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn labelled(label: &str, value: &str) -> String {
    format!("  {label:<10}{value}")
}

fn report_path() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("boot-report-{seconds}.txt")
}

fn parse_delay(input: &str) -> Option<u32> {
    let delay = input.trim().parse::<u32>().ok()?;
    Some(delay.min(MAX_DELAY))
}

fn run_plan(plan: &profiles::Plan, password: &str) -> Result<String, String> {
    let steps: [(&str, &[String]); 4] = [
        ("enable", &plan.enable),
        ("start", &plan.start),
        ("disable", &plan.disable),
        ("stop", &plan.stop),
    ];

    for (verb, units) in steps {
        if units.is_empty() {
            continue;
        }
        let names: Vec<&str> = units.iter().map(String::as_str).collect();
        privilege::systemctl_many(verb, &names, password)?;
    }

    Ok(i18n::t("tui_done").to_string())
}

#[cfg(test)]
mod tests;
