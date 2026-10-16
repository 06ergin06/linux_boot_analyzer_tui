use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::filters::{SortBy, StatusFilter, TipFilter};
use crate::system::{Service, SystemData, parse_duration_seconds};

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

/// All application state, kept free of rendering and I/O.
pub struct App {
    pub tab: Tab,
    pub should_quit: bool,
    /// System snapshot, filled in once the background loader finishes.
    pub data: Option<SystemData>,
    /// Selected row in the services table.
    pub selected: usize,
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
}

impl App {
    pub fn new() -> Self {
        Self {
            tab: Tab::Analysis,
            should_quit: false,
            data: None,
            selected: 0,
            loading: true,
            status_filter: StatusFilter::All,
            tip_filter: TipFilter::All,
            sort: SortBy::Name,
            query: String::new(),
            searching: false,
            reload_requested: false,
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
            KeyCode::Down | KeyCode::Char('j') => self.select_next_row(),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous_row(),
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
            KeyCode::Char('r') => self.reload_requested = true,
            _ => {}
        }
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

    fn select_next_row(&mut self) {
        let count = self.visible_services().len();
        if count > 0 {
            self.selected = (self.selected + 1).min(count - 1);
        }
    }

    fn select_previous_row(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
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
        });
        app
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
}
