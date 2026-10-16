//! A terminal UI for analyzing Linux boot time, rewritten from the Python
//! `pardus-boot-analyzer` application.

mod app;
mod ui;

use std::io;

use app::App;
use crossterm::event::{self, Event};

fn main() -> io::Result<()> {
    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, and restores it afterwards even if the closure panics.
    ratatui::run(|terminal| {
        let mut app = App::new();

        while !app.should_quit {
            terminal.draw(|frame| ui::render(frame, &app))?;

            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        Ok(())
    })
}
