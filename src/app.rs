use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::autostart::{self, AutostartEntry};
use crate::backups::{self, Backup};
use crate::filters::{SortBy, StatusFilter, TipFilter};
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
            Tab::Analysis => "Analysis",
            Tab::Services => "Services",
            Tab::Autostart => "Autostart",
            Tab::Profiles => "Profiles",
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
                    "Done.".to_string()
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
                    if db.suggestion.is_empty() {
                        db.desc.clone()
                    } else {
                        db.suggestion.clone()
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
                "Loading...".to_string()
            } else {
                "No data available.".to_string()
            }];
        };

        let mut lines = vec![
            "System".to_string(),
            format!("  OS:      {}", data.info.os),
            format!("  Kernel:  {}", data.info.kernel),
            format!("  Memory:  {}", data.info.ram),
            format!("  Uptime:  {}", data.info.uptime),
            String::new(),
            "Boot time".to_string(),
        ];

        match &data.boot {
            Ok(boot) => {
                lines.push(format!("  Total: {}", boot.total));
                lines.push(format!("  {}", boot.detail));
                for stage in system::parse_breakdown(&boot.detail) {
                    lines.push(format!("    {}: {}", stage.label, stage.time));
                }
            }
            Err(err) => lines.push(format!("  {err}")),
        }

        lines.push(String::new());
        lines.push("Optimization".to_string());
        let optimizable = self.optimizable_services();
        if optimizable.is_empty() {
            lines.push("  Nothing to optimize; the system already looks lean.".to_string());
        } else {
            let total: f64 = optimizable.iter().map(|item| item.seconds).sum();
            lines.push(format!("  Potential savings: ~{total:.2} s"));
            lines.push("  Suggested services (safe to disable):".to_string());
            for item in &optimizable {
                lines.push(format!("    {:<42} {:>9}", item.name, item.time));
                if !item.suggestion.is_empty() {
                    lines.push(format!("      {}", item.suggestion));
                }
            }
            lines.push("  Press 'o' to disable all suggested services.".to_string());
        }

        lines.push(String::new());
        lines.push("Slowest units".to_string());
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
            Some(Pending::Action(action, unit)) => format!("{} {unit}?", action.label()),
            Some(Pending::Bulk(units)) => format!("Disable {} suggested service(s)?", units.len()),
            Some(Pending::RemoveAutostart { name, .. }) => {
                format!("Remove autostart entry \"{name}\"?")
            }
            Some(Pending::ApplyProfile(plan)) => {
                format!("Apply this profile? ({})", plan.summary())
            }
            Some(Pending::RestoreBackup(plan)) => {
                format!("Restore this backup? ({})", plan.summary())
            }
            Some(Pending::RemoveBackup { name, .. }) => format!("Delete backup \"{name}\"?"),
            Some(Pending::AddAutostart { .. }) | Some(Pending::CreateBackup(..)) | None => {
                "Confirm?".to_string()
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
        let mut report = String::from("Linux Boot Analysis Report\n==========================\n\n");
        for line in self.analysis_lines() {
            report.push_str(&line);
            report.push('\n');
        }
        report.push_str("\nGenerated by linux_boot_analyzer_tui\n");
        report
    }

    /// Write the report next to the working directory and report the path.
    fn export_report(&mut self) {
        if self.tab != Tab::Analysis {
            return;
        }

        let path = report_path();
        self.message = Some(match std::fs::write(&path, self.report()) {
            Ok(()) => format!("Report written to {path}"),
            Err(err) => format!("Could not write {path}: {err}"),
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
                title: format!("Log: {unit}"),
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
            let title = if reverse {
                format!("Reverse dependencies: {unit}")
            } else {
                format!("Dependencies: {unit}")
            };
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
            self.message = Some("Nothing to optimize.".to_string());
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
                    autostart::remove(&path).map(|()| "Autostart entry removed.".to_string())
                }
                Pending::AddAutostart { name, command } => autostart::add(&name, &command)
                    .map(|path| format!("Created {}", path.display())),
                Pending::ApplyProfile(plan) | Pending::RestoreBackup(plan) => {
                    run_plan(&plan, &password)
                }
                Pending::RemoveBackup { path, .. } => {
                    backups::remove(&path).map(|()| "Backup deleted.".to_string())
                }
                Pending::CreateBackup(states) => {
                    backups::create(states).map(|backup| format!("Saved {}", backup.name))
                }
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
            self.message = Some("A name is required.".to_string());
            return;
        }
        if command.trim().is_empty() {
            self.message = Some("A command is required.".to_string());
            return;
        }

        self.perform(Pending::AddAutostart { name, command });
    }

    /// Report the outcome of a local (non-privileged) file change.
    fn finish_local(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.message = Some("Autostart entry updated.".to_string());
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
            self.message = Some("The profile already matches the system.".to_string());
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
            self.message = Some("The system already matches this backup.".to_string());
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

    fn reset_selection(&mut self) {
        self.selected = 0;
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

    Ok("Done.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{BlameEntry, Service, SystemInfo};
    use crossterm::event::KeyModifiers;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(text: &str) -> Vec<KeyEvent> {
        text.chars().map(|c| press(KeyCode::Char(c))).collect()
    }

    fn app_with_services() -> App {
        let mut app = App::new();
        app.tab = Tab::Services;
        app.loading = false;
        app.data = Some(SystemData {
            info: SystemInfo {
                os: "Test".to_string(),
                kernel: "6.1.0".to_string(),
                ram: "8 GB".to_string(),
                uptime: "1m".to_string(),
            },
            boot: Err("n/a".to_string()),
            blame: Ok(vec![BlameEntry {
                time: "3.000s".to_string(),
                name: "b.service".to_string(),
            }]),
            services: Ok(vec![
                Service {
                    name: "b.service".to_string(),
                    active: "active".to_string(),
                    sub: "running".to_string(),
                    description: "Beta".to_string(),
                    enabled: "enabled".to_string(),
                },
                Service {
                    name: "a.service".to_string(),
                    active: "inactive".to_string(),
                    sub: "dead".to_string(),
                    description: "Alpha".to_string(),
                    enabled: "disabled".to_string(),
                },
            ]),
            autostart: Ok(vec![AutostartEntry {
                path: PathBuf::from("/tmp/example.desktop"),
                name: "Example".to_string(),
                exec: "example".to_string(),
                comment: String::new(),
                icon: "system-run".to_string(),
                delay: 0,
                enabled: true,
            }]),
            backups: Ok(Vec::new()),
        });
        app
    }

    /// Add one service from the "safe to disable" list to the snapshot.
    fn add_optimizable(app: &mut App) {
        if let Some(data) = app.data.as_mut() {
            if let Ok(services) = &mut data.services {
                services.push(Service {
                    name: "bluetooth.service".to_string(),
                    active: "active".to_string(),
                    sub: "running".to_string(),
                    description: "Bluetooth".to_string(),
                    enabled: "enabled".to_string(),
                });
            }
            if let Ok(blame) = &mut data.blame {
                blame.push(BlameEntry {
                    time: "1.500s".to_string(),
                    name: "bluetooth.service".to_string(),
                });
            }
        }
    }

    #[test]
    fn starts_on_the_analysis_tab() {
        let app = App::new();
        assert_eq!(app.tab, Tab::Analysis);
        assert!(!app.should_quit);
        assert!(app.loading);
    }

    #[test]
    fn right_cycles_forward_and_wraps() {
        let mut app = App::new();
        for expected in [Tab::Services, Tab::Autostart, Tab::Profiles, Tab::Analysis] {
            app.handle_key(press(KeyCode::Right));
            assert_eq!(app.tab, expected);
        }
    }

    #[test]
    fn left_cycles_backward_and_wraps() {
        let mut app = App::new();
        app.handle_key(press(KeyCode::Left));
        assert_eq!(app.tab, Tab::Profiles);
    }

    #[test]
    fn q_and_escape_quit() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let mut app = App::new();
            app.handle_key(press(code));
            assert!(app.should_quit);
        }
    }

    #[test]
    fn row_selection_stays_put_without_data() {
        let mut app = App::new();
        app.handle_key(press(KeyCode::Down));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn services_are_sorted_by_name_by_default() {
        let app = app_with_services();
        let names: Vec<&str> = app
            .visible_services()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["a.service", "b.service"]);
    }

    #[test]
    fn sorting_by_boot_time_puts_the_slowest_first() {
        let mut app = app_with_services();
        app.sort = SortBy::BootTime;
        let names: Vec<&str> = app
            .visible_services()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["b.service", "a.service"]);
    }

    #[test]
    fn typing_a_query_filters_services() {
        let mut app = app_with_services();
        app.handle_key(press(KeyCode::Char('/')));
        assert!(app.searching);
        for key in typed("alpha") {
            app.handle_key(key);
        }
        let names: Vec<&str> = app
            .visible_services()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["a.service"]);
    }

    #[test]
    fn escape_clears_the_search_query() {
        let mut app = app_with_services();
        app.handle_key(press(KeyCode::Char('/')));
        app.handle_key(press(KeyCode::Char('x')));
        app.handle_key(press(KeyCode::Esc));
        assert!(!app.searching);
        assert!(app.query.is_empty());
    }

    #[test]
    fn action_flow_confirms_then_asks_for_a_password() {
        let mut app = app_with_services();
        app.handle_key(press(KeyCode::Char('a')));
        assert_eq!(app.overlay, Overlay::Menu);

        app.handle_key(press(KeyCode::Enter));
        assert_eq!(app.overlay, Overlay::Confirm);
        assert_eq!(app.confirmation_prompt(), "Enable a.service?");

        app.handle_key(press(KeyCode::Char('y')));
        assert_eq!(app.overlay, Overlay::Password);
        assert!(app.pending.is_some());
    }

    #[test]
    fn declining_a_confirmation_closes_the_overlay() {
        let mut app = app_with_services();
        app.handle_key(press(KeyCode::Char('a')));
        app.handle_key(press(KeyCode::Enter));
        app.handle_key(press(KeyCode::Char('n')));
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.pending.is_none());
    }

    #[test]
    fn info_pane_scrolls_and_closes() {
        let mut app = app_with_services();
        app.info = Some(InfoPane {
            title: "Log".to_string(),
            lines: (0..30).map(|index| index.to_string()).collect(),
            scroll: 0,
        });

        app.handle_key(press(KeyCode::Char('j')));
        assert_eq!(app.info.as_ref().map(|info| info.scroll), Some(1));

        app.handle_key(press(KeyCode::End));
        assert_eq!(app.info.as_ref().map(|info| info.scroll), Some(29));

        app.handle_key(press(KeyCode::Esc));
        assert!(app.info.is_none());
    }

    #[test]
    fn quick_optimize_lists_safe_enabled_services() {
        let mut app = app_with_services();
        add_optimizable(&mut app);

        let optimizable = app.optimizable_services();
        assert_eq!(optimizable.len(), 1);
        assert_eq!(optimizable[0].name, "bluetooth.service");
        assert!((optimizable[0].seconds - 1.5).abs() < 1e-9);
    }

    #[test]
    fn quick_optimize_asks_for_confirmation_on_the_analysis_tab() {
        let mut app = app_with_services();
        add_optimizable(&mut app);
        app.tab = Tab::Analysis;

        app.handle_key(press(KeyCode::Char('o')));

        assert_eq!(app.overlay, Overlay::Confirm);
        assert_eq!(app.confirmation_prompt(), "Disable 1 suggested service(s)?");
    }

    #[test]
    fn analysis_lines_summarise_the_snapshot() {
        let app = app_with_services();
        let text = app.analysis_lines().join("\n");
        assert!(text.contains("System"));
        assert!(text.contains("Slowest units"));
        assert!(text.contains("Optimization"));
    }

    #[test]
    fn report_includes_the_header_and_sections() {
        let app = app_with_services();
        let report = app.report();
        assert!(report.starts_with("Linux Boot Analysis Report"));
        assert!(report.contains("Slowest units"));
        assert!(report.ends_with("Generated by linux_boot_analyzer_tui\n"));
    }

    #[test]
    fn autostart_entry_is_selectable() {
        let mut app = app_with_services();
        app.tab = Tab::Autostart;
        assert!(app.selected_autostart().is_some());

        app.handle_key(press(KeyCode::Down));
        assert_eq!(app.autostart_selected, 0);
    }

    #[test]
    fn removing_an_autostart_entry_asks_for_confirmation() {
        let mut app = app_with_services();
        app.tab = Tab::Autostart;

        app.handle_key(press(KeyCode::Char('x')));

        assert_eq!(app.overlay, Overlay::Confirm);
        assert!(app.confirmation_prompt().contains("Example"));
    }

    #[test]
    fn add_form_requires_a_name() {
        let mut app = app_with_services();
        app.tab = Tab::Autostart;

        app.handle_key(press(KeyCode::Char('n')));
        assert_eq!(app.overlay, Overlay::Form);

        app.handle_key(press(KeyCode::Enter));
        assert_eq!(app.message.as_deref(), Some("A name is required."));
    }

    #[test]
    fn form_edits_the_active_field() {
        let mut app = app_with_services();
        app.tab = Tab::Autostart;
        app.handle_key(press(KeyCode::Char('n')));

        for key in typed("My App") {
            app.handle_key(key);
        }
        app.handle_key(press(KeyCode::Tab));
        for key in typed("myapp") {
            app.handle_key(key);
        }

        assert_eq!(app.form_values, ["My App", "myapp"]);
    }

    #[test]
    fn profiles_view_toggles_between_lists() {
        let mut app = app_with_services();
        app.tab = Tab::Profiles;
        assert_eq!(app.profiles_view, ProfilesView::Profiles);

        app.handle_key(press(KeyCode::Char('v')));
        assert_eq!(app.profiles_view, ProfilesView::Backups);
    }

    #[test]
    fn applying_a_profile_confirms_with_a_plan() {
        let mut app = app_with_services();
        app.tab = Tab::Profiles;

        // cups.service belongs to the "minimum" profile and is enabled here.
        if let Some(data) = app.data.as_mut()
            && let Ok(services) = &mut data.services
        {
            services.push(Service {
                name: "cups.service".to_string(),
                active: "inactive".to_string(),
                sub: "dead".to_string(),
                description: "Printing".to_string(),
                enabled: "enabled".to_string(),
            });
        }

        assert_eq!(app.selected_profile(), Some("minimum"));
        let (enable, disable) = profiles::selections("minimum");
        let plan = app.build_plan(&enable, &disable);
        assert!(plan.disable.contains(&"cups.service".to_string()));

        app.handle_key(press(KeyCode::Char('a')));
        assert_eq!(app.overlay, Overlay::Confirm);
        assert!(app.confirmation_prompt().starts_with("Apply this profile?"));
    }

    #[test]
    fn plan_ignores_services_already_in_the_wanted_state() {
        let mut app = app_with_services();

        // Already disabled, so the "minimum" profile has nothing to do for it.
        if let Some(data) = app.data.as_mut()
            && let Ok(services) = &mut data.services
        {
            services.push(Service {
                name: "cups.service".to_string(),
                active: "inactive".to_string(),
                sub: "dead".to_string(),
                description: "Printing".to_string(),
                enabled: "disabled".to_string(),
            });
        }

        let (enable, disable) = profiles::selections("minimum");
        let plan = app.build_plan(&enable, &disable);

        assert!(!plan.disable.contains(&"cups.service".to_string()));
        assert!(!plan.stop.contains(&"cups.service".to_string()));
    }
}
