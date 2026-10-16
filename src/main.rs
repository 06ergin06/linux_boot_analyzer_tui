//! A terminal UI for analyzing Linux boot time, rewritten from the Python
//! `pardus-boot-analyzer` application.

mod app;
mod profiles;
mod service_db;
mod system;
mod ui;

use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use app::App;
use crossterm::event::{self, Event};

/// How long to wait for a key before checking the loader channel again.
const EVENT_POLL: Duration = Duration::from_millis(200);

fn main() -> io::Result<()> {
    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, and restores it afterwards even if the closure panics.
    ratatui::run(|terminal| {
        // Gather system data in the background so the UI stays responsive.
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(system::load());
        });

        let mut app = App::new();

        while !app.should_quit {
            app.poll_data(&receiver);
            terminal.draw(|frame| ui::render(frame, &app))?;

            if event::poll(EVENT_POLL)? {
                if let Event::Key(key) = event::read()? {
                    app.handle_key(key);
                }
            }
        }

        Ok(())
    })
}
