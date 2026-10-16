use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

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
            Tab::Analysis => "Analiz",
            Tab::Services => "Servisler",
            Tab::Autostart => "Başlangıç",
            Tab::Profiles => "Profiller",
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
}

impl App {
    pub fn new() -> Self {
        Self {
            tab: Tab::Analysis,
            should_quit: false,
        }
    }

    /// Update state in response to a key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Some platforms report key release as well; only act on presses.
        if key.kind != KeyEventKind::Press {
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Right | KeyCode::Tab => self.select_next(),
            KeyCode::Left | KeyCode::BackTab => self.select_previous(),
            _ => {}
        }
    }

    fn select_next(&mut self) {
        let next = (self.tab.index() + 1) % Tab::ALL.len();
        self.tab = Tab::ALL[next];
    }

    fn select_previous(&mut self) {
        let count = Tab::ALL.len();
        let previous = (self.tab.index() + count - 1) % count;
        self.tab = Tab::ALL[previous];
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
}
