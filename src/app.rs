use std::sync::mpsc::{Receiver, TryRecvError};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::system::SystemData;

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
}

impl App {
    pub fn new() -> Self {
        Self {
            tab: Tab::Analysis,
            should_quit: false,
            data: None,
            selected: 0,
            loading: true,
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

    /// Number of rows in the services table (0 until data is loaded).
    pub fn service_count(&self) -> usize {
        self.data
            .as_ref()
            .and_then(|data| data.services.as_ref().ok())
            .map_or(0, |services| services.len())
    }

    /// Update state in response to a key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Some platforms report key release as well; only act on presses.
        if key.kind != KeyEventKind::Press {
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Right | KeyCode::Tab => self.select_next_tab(),
            KeyCode::Left | KeyCode::BackTab => self.select_previous_tab(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next_row(),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous_row(),
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

    fn select_next_row(&mut self) {
        let count = self.service_count();
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
    use crossterm::event::KeyModifiers;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
}
