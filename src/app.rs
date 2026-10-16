use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::autostart::{self, AutostartEntry};
use crate::backups::{self, Backup};
use crate::filters::{SortBy, StatusFilter, TipFilter};
use crate::i18n;
use crate::privilege::{self, Action};
use crate::system::{self, Service, SystemData, parse_duration_seconds};
use crate::{profiles, service_db};

/// The top-level screens, mirroring the pages of the original GTK application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Analysis,
    Services,
    Autostart,
    Profiles,
}

impl Tab {
    /// All tabs in display order.
    pub const ALL: [Tab; 4] = [Tab::Analysis, Tab::Services, Tab::Autostart, Tab::Profiles];

    /// Label shown in the tab bar.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Analysis => i18n::t("tab_analysis"),
            Tab::Services => i18n::t("tab_services"),
            Tab::Autostart => i18n::t("tab_autostart"),
            Tab::Profiles => i18n::t("tab_profiles"),
        }
    }

    /// Position in [`Tab::ALL`], used by the tab bar and for cycling.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

/// A modal presented on top of the current tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// Choose an action for the selected service.
    Menu,
    /// Confirm the pending action.
    Confirm,
    /// Ask for the sudo password.
    Password,
    /// Fill in the "new autostart entry" form.
    Form,
}

/// A unit that is enabled, safe to disable, and slow to boot.
pub struct OptimizableService {
    pub name: String,
    /// Boot time as reported by `systemd-analyze blame`.
    pub time: String,
    pub seconds: f64,
    pub suggestion: String,
}

/// A scrollable text pane (journal log or dependency tree).
pub struct InfoPane {
    pub title: String,
    pub lines: Vec<String>,
    /// Index of the first visible line.
    pub scroll: u16,
}

/// Which list the Profiles tab is focused on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilesView {
    Profiles,
    Backups,
}

/// Work waiting for confirmation and, if necessary, a password.
enum Pending {
    /// A single `systemctl` action on one unit.
    Action(Action, String),
    /// Disable every suggested service.
    Bulk(Vec<String>),
    /// Delete an autostart entry.
    RemoveAutostart { name: String, path: PathBuf },
    /// Create a new autostart entry.
    AddAutostart { name: String, command: String },
    /// Apply an optimization profile.
    ApplyProfile(profiles::Plan),
    /// Restore unit-file states from a backup.
    RestoreBackup(profiles::Plan),
    /// Delete a restore point.
    RemoveBackup { name: String, path: PathBuf },
    /// Save the current unit-file states as a restore point.
    CreateBackup(BTreeMap<String, String>),
}

impl Pending {
    /// Whether the work needs root privileges.
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

/// Result of a background task started by the UI.
enum CommandOutcome {
    /// Output of a privileged command.
    Message(Result<String, String>),
    /// Text to show in the [`InfoPane`].
    Info { title: String, lines: Vec<String> },
}

/// All application state, kept free of rendering and I/O.
pub struct App {
    pub tab: Tab,
    pub should_quit: bool,
    /// System snapshot, filled in once the background loader finishes.
    pub data: Option<SystemData>,
    /// Selected row in the services table.
    pub selected: usize,
    /// Selected row in the autostart list.
    pub autostart_selected: usize,
    /// Which list the Profiles tab is focused on.
    pub profiles_view: ProfilesView,
    /// Selected profile in the profiles list.
    pub profiles_selected: usize,
    /// Selected backup in the backups list.
    pub backups_selected: usize,
    /// Values of the open form, one per field.
    pub form_values: Vec<String>,
    /// Which form field is being edited.
    pub form_index: usize,
    /// First visible line of the analysis page.
    pub analysis_scroll: u16,
    /// Whether the initial snapshot is still loading.
    pub loading: bool,
    /// Services-tab filters, sort order and search query.
    pub status_filter: StatusFilter,
    pub tip_filter: TipFilter,
    pub sort: SortBy,
    pub query: String,
    /// Whether the search box is capturing keystrokes.
    pub searching: bool,
    /// Set when the user asks for a data refresh; the main loop reloads.
    pub reload_requested: bool,
    /// Modal currently shown, if any.
    pub overlay: Overlay,
    /// Selected entry in the action menu.
    pub menu_index: usize,
    /// Whatever the user has typed into the password prompt.
    pub password_input: String,
    /// Latest command result, shown in the status line.
    pub message: Option<String>,
    /// Whether a privileged command is currently running.
    pub running: bool,
    /// Journal/dependency text pane, when open.
    pub info: Option<InfoPane>,
    /// Password cached after the first privileged command.
    password: Option<String>,
    /// Work waiting for confirmation and a password.
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
            profiles_view: ProfilesView::Profiles,
            profiles_selected: 0,
            backups_selected: 0,
            form_values: Vec::new(),
            form_index: 0,
            analysis_scroll: 0,
            loading: true,
            status_filter: StatusFilter::All,
            tip_filter: TipFilter::All,
            sort: SortBy::Name,
            query: String::new(),
            searching: false,
            reload_requested: false,
            overlay: Overlay::None,
            menu_index: 0,
            password_input: String::new(),
            message: None,
            running: false,
            info: None,
            password: None,
            pending: None,
            command_tx,
            command_rx,
        }
    }

    /// Take the loaded snapshot from the background loader, if it is ready.
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

    /// Take the result of a background task, if one finished.
    pub fn poll_command(&mut self) {
        match self.command_rx.try_recv() {
            Ok(CommandOutcome::Message(Ok(output))) => {
                self.running = false;
                self.message = Some(if output.is_empty() {
                    i18n::t("tui_done").to_string()
                } else {
                    output
                });
                // The unit states changed, so refresh the snapshot.
                self.reload_requested = true;
            }
            Ok(CommandOutcome::Message(Err(error))) => {
                self.running = false;
                if privilege::is_auth_failure(&error) {
                    self.password = None;
                }
                self.message = Some(error);
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

    // --- Services tab data ---

    /// Services matching the current filters, in the current sort order.
    pub fn visible_services(&self) -> Vec<&Service> {
        let Some(services) = self.services() else {
            return Vec::new();
        };

        let query = self.query.to_lowercase();
        let mut visible: Vec<&Service> = services
            .iter()
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

    /// Boot time reported by `systemd-analyze blame`, keyed by unit name.
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

    /// The service currently highlighted in the table.
    pub fn selected_service(&self) -> Option<&Service> {
        self.visible_services().get(self.selected).copied()
    }

    // --- Analysis tab data ---

    /// Enabled, safe-to-disable services that slow down the boot.
    ///
    /// Ported from the original `_get_optimizable_services`.
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

    /// Plain-text content of the analysis page, one entry per line.
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
                    lines.push(format!("    {}: {}", stage.label, stage.time));
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

    /// The question shown in the confirmation dialog.
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
            Some(Pending::AddAutostart { .. }) | Some(Pending::CreateBackup(..)) | None => {
                i18n::t("tui_prompt_confirm").to_string()
            }
        }
    }

    // --- Profiles and backups ---

    /// Restore points, newest first.
    pub fn backups(&self) -> &[Backup] {
        self.data
            .as_ref()
            .and_then(|data| data.backups.as_ref().ok())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The profile currently highlighted.
    pub fn selected_profile(&self) -> Option<&'static str> {
        profiles::ids().get(self.profiles_selected).copied()
    }

    /// The backup currently highlighted.
    pub fn selected_backup(&self) -> Option<&Backup> {
        self.backups().get(self.backups_selected)
    }

    /// Current enabled/active state of every known unit.
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

    /// Work out the exact units to act on to reach the wanted state.
    ///
    /// Ported from `ProfileManager.apply_profile_batch`.
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

    // --- Autostart tab data ---

    /// Autostart entries from `~/.config/autostart`.
    pub fn autostart_entries(&self) -> &[AutostartEntry] {
        self.data
            .as_ref()
            .and_then(|data| data.autostart.as_ref().ok())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The autostart entry currently highlighted.
    pub fn selected_autostart(&self) -> Option<&AutostartEntry> {
        self.autostart_entries().get(self.autostart_selected)
    }

    /// Plain-text report of the current analysis, for exporting to a file.
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

    /// Write the report next to the working directory and report the path.
    fn export_report(&mut self) {
        if self.tab != Tab::Analysis {
            return;
        }

        let path = report_path();
        self.message = Some(match std::fs::write(&path, self.report()) {
            Ok(()) => i18n::fill(i18n::t("tui_report_written"), &[&path]),
            Err(err) => i18n::fill(i18n::t("tui_report_failed"), &[&path, &err.to_string()]),
        });
    }

    /// The loaded services, if any.
    fn services(&self) -> Option<&[Service]> {
        self.data
            .as_ref()?
            .services
            .as_ref()
            .ok()
            .map(Vec::as_slice)
    }

    /// Update state in response to a key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Some platforms report key release as well; only act on presses.
        if key.kind != KeyEventKind::Press {
            return;
        }

        // The info pane is modal: it captures scrolling keys.
        if self.info.is_some() {
            return self.handle_info_key(key);
        }

        match self.overlay {
            Overlay::Password => return self.handle_password_key(key),
            Overlay::Menu => return self.handle_menu_key(key),
            Overlay::Confirm => return self.handle_confirm_key(key),
            Overlay::Form => return self.handle_form_key(key),
            Overlay::None => {}
        }

        // While searching, keys edit the query instead of controlling the UI.
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
            KeyCode::Right | KeyCode::Tab => self.select_next_tab(),
            KeyCode::Left | KeyCode::BackTab => self.select_previous_tab(),
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
            KeyCode::Char('L') => self.toggle_language(),
            KeyCode::Char('b') => self.begin_create_backup(),
            KeyCode::Char('v') => self.toggle_profiles_view(),
            KeyCode::Char(' ') => self.toggle_autostart(),
            KeyCode::Char('n') => self.begin_add_form(),
            KeyCode::Char('x') => self.remove_selected(),
            KeyCode::Char('[') => self.adjust_autostart_delay(-1),
            KeyCode::Char(']') => self.adjust_autostart_delay(1),
            KeyCode::Char('r') => self.reload_requested = true,
            KeyCode::Char('l') => self.show_journal(),
            KeyCode::Char('d') => self.dependencies_or_restore(),
            KeyCode::Char('D') => self.show_dependencies(true),
            _ => {}
        }
    }

    /// Scroll or close the info pane.
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

    /// Load the selected service's journal into the info pane.
    fn show_journal(&mut self) {
        if self.tab != Tab::Services {
            return;
        }
        let Some(unit) = self.selected_service().map(|service| service.name.clone()) else {
            return;
        };

        let password = self.password.clone();
        let sender = self.command_tx.clone();
        self.running = true;
        self.message = None;
        thread::spawn(move || {
            let text = system::journal(&unit, 200, password.as_deref());
            let _ = sender.send(CommandOutcome::Info {
                title: i18n::fill(i18n::t("tui_title_log"), &[&unit]),
                lines: text.lines().map(str::to_string).collect(),
            });
        });
    }

    /// Load the selected service's dependencies into the info pane.
    fn show_dependencies(&mut self, reverse: bool) {
        if self.tab != Tab::Services {
            return;
        }
        let Some(unit) = self.selected_service().map(|service| service.name.clone()) else {
            return;
        };

        let sender = self.command_tx.clone();
        self.running = true;
        self.message = None;
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
        if self.tab == Tab::Services && self.selected_service().is_some() {
            self.menu_index = 0;
            self.overlay = Overlay::Menu;
        }
    }

    /// Stage a single-unit action and ask for confirmation.
    fn choose_action(&mut self, action: Action) {
        let Some(unit) = self.selected_service().map(|service| service.name.clone()) else {
            return;
        };
        self.pending = Some(Pending::Action(action, unit));
        self.overlay = Overlay::Confirm;
    }

    /// Stage the bulk disable of every suggested service.
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
            self.message = Some(i18n::t("tui_nothing_to_optimize").to_string());
            return;
        }
        self.pending = Some(Pending::Bulk(units));
        self.overlay = Overlay::Confirm;
    }

    /// Run the pending work, asking for a password first if needed.
    fn run_pending(&mut self) {
        self.overlay = Overlay::None;
        let Some(pending) = self.pending.as_ref() else {
            return;
        };

        // Only privileged work needs a password.
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
        let sender = self.command_tx.clone();
        self.running = true;
        self.message = None;
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
                Pending::CreateBackup(states) => backups::create(states)
                    .map(|backup| i18n::fill(i18n::t("tui_saved_backup"), &[&backup.name])),
            };
            let _ = sender.send(CommandOutcome::Message(result));
        });
    }

    /// Start a pending job without a confirmation step.
    fn perform(&mut self, pending: Pending) {
        self.pending = Some(pending);
        self.run_pending();
    }

    // --- Autostart actions ---

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

    fn adjust_autostart_delay(&mut self, delta: i32) {
        if self.tab != Tab::Autostart {
            return;
        }
        let Some(entry) = self.selected_autostart() else {
            return;
        };
        let delay = (entry.delay as i32 + delta).clamp(0, 600) as u32;
        if delay == entry.delay {
            return;
        }
        let result = autostart::set_delay(entry, delay);
        self.finish_local(result);
    }

    fn begin_add_form(&mut self) {
        if self.tab != Tab::Autostart {
            return;
        }
        self.form_values = vec![String::new(), String::new()];
        self.form_index = 0;
        self.overlay = Overlay::Form;
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
            self.message = Some(i18n::t("tui_name_required").to_string());
            return;
        }
        if command.trim().is_empty() {
            self.message = Some(i18n::t("tui_command_required").to_string());
            return;
        }

        self.perform(Pending::AddAutostart { name, command });
    }

    /// Report the outcome of a local (non-privileged) file change.
    fn finish_local(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.message = Some(i18n::t("tui_autostart_updated").to_string());
                self.reload_requested = true;
            }
            Err(error) => self.message = Some(error),
        }
    }

    // --- Profiles and backups ---

    /// Enter/a: the contextual primary action for the current tab.
    fn primary_action(&mut self) {
        match self.tab {
            Tab::Services => self.open_action_menu(),
            Tab::Profiles => self.begin_apply_profile(),
            _ => {}
        }
    }

    /// d: dependencies on the Services tab, restore on the Profiles tab.
    fn dependencies_or_restore(&mut self) {
        match self.tab {
            Tab::Services => self.show_dependencies(false),
            Tab::Profiles => self.begin_restore_backup(),
            _ => {}
        }
    }

    /// x: delete the selected autostart entry or restore point.
    fn remove_selected(&mut self) {
        match self.tab {
            Tab::Autostart => self.begin_remove_autostart(),
            Tab::Profiles => self.begin_remove_backup(),
            _ => {}
        }
    }

    fn toggle_profiles_view(&mut self) {
        if self.tab != Tab::Profiles {
            return;
        }
        self.profiles_view = match self.profiles_view {
            ProfilesView::Profiles => ProfilesView::Backups,
            ProfilesView::Backups => ProfilesView::Profiles,
        };
    }

    /// Save the current unit-file states as a restore point.
    fn begin_create_backup(&mut self) {
        if self.tab != Tab::Profiles {
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

    /// Stage the application of the selected profile.
    fn begin_apply_profile(&mut self) {
        if self.tab != Tab::Profiles || self.profiles_view != ProfilesView::Profiles {
            return;
        }
        let Some(profile_id) = self.selected_profile() else {
            return;
        };

        let (enable, disable) = profiles::selections(profile_id);
        let plan = self.build_plan(&enable, &disable);
        if plan.is_empty() {
            self.message = Some(i18n::t("tui_profile_matches").to_string());
            return;
        }

        self.pending = Some(Pending::ApplyProfile(plan));
        self.overlay = Overlay::Confirm;
    }

    /// Stage the restoration of the selected backup.
    fn begin_restore_backup(&mut self) {
        if self.tab != Tab::Profiles || self.profiles_view != ProfilesView::Backups {
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
            self.message = Some(i18n::t("tui_backup_matches").to_string());
            return;
        }

        self.pending = Some(Pending::RestoreBackup(plan));
        self.overlay = Overlay::Confirm;
    }

    fn begin_remove_backup(&mut self) {
        if self.tab != Tab::Profiles || self.profiles_view != ProfilesView::Backups {
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

    /// Show the key bindings in the scrollable info pane.
    fn open_help(&mut self) {
        // Empty entries are the blank separator lines.
        let keys = [
            "tui_help_navigation",
            "tui_help_quit",
            "tui_help_tab",
            "tui_help_move",
            "tui_help_page",
            "tui_help_language",
            "",
            "tui_help_services",
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
            "tui_help_view",
            "tui_help_apply",
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

    /// L: switch between English and Turkish, and remember the choice.
    fn toggle_language(&mut self) {
        let lang = i18n::toggle();
        self.message = Some(i18n::fill(
            i18n::t("tui_language_set"),
            &[lang.native_name()],
        ));
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
                let count = self.focused_list_len();
                let selected = match self.profiles_view {
                    ProfilesView::Profiles => &mut self.profiles_selected,
                    ProfilesView::Backups => &mut self.backups_selected,
                };
                if count > 0 {
                    *selected = (*selected + 1).min(count - 1);
                }
            }
            Tab::Analysis => self.scroll_analysis(1),
        }
    }

    fn move_up(&mut self) {
        match self.tab {
            Tab::Services => self.selected = self.selected.saturating_sub(1),
            Tab::Autostart => self.autostart_selected = self.autostart_selected.saturating_sub(1),
            Tab::Profiles => {
                let selected = match self.profiles_view {
                    ProfilesView::Profiles => &mut self.profiles_selected,
                    ProfilesView::Backups => &mut self.backups_selected,
                };
                *selected = selected.saturating_sub(1);
            }
            Tab::Analysis => self.scroll_analysis(-1),
        }
    }

    /// Number of rows in the list the Profiles tab is focused on.
    fn focused_list_len(&self) -> usize {
        match self.profiles_view {
            ProfilesView::Profiles => profiles::ids().len(),
            ProfilesView::Backups => self.backups().len(),
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

/// A `label value` row of the analysis page, with the label padded so the
/// values line up in both languages.
fn labelled(label: &str, value: &str) -> String {
    format!("  {label:<10}{value}")
}

/// A unique report filename based on the current Unix time.
fn report_path() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("boot-report-{seconds}.txt")
}

/// Carry out a profile/backup plan as root, one verb at a time.
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
