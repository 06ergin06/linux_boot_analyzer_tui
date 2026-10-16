//! A terminal UI for analyzing Linux boot time, rewritten from the Python
//! `pardus-boot-analyzer` application.

mod app;
mod autostart;
mod backups;
mod filters;
mod privilege;
mod profiles;
mod service_db;
mod system;
mod ui;

use std::io;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use app::App;
use crossterm::event::{self, Event};
use system::SystemData;

/// How long to wait for a key before checking the loader channel again.
const EVENT_POLL: Duration = Duration::from_millis(200);

/// Gather a system snapshot on a background thread so the UI stays responsive.
fn spawn_loader(sender: &Sender<SystemData>) {
    let sender = sender.clone();
    thread::spawn(move || {
        let _ = sender.send(system::load());
    });
}

fn main() -> io::Result<()> {
    // `ratatui::run` switches the terminal into raw mode and the alternate
    // screen, and restores it afterwards even if the closure panics.
    ratatui::run(|terminal| {
        let (sender, receiver) = mpsc::channel();
        spawn_loader(&sender);

        let mut app = App::new();

        while !app.should_quit {
            app.poll_data(&receiver);
            app.poll_command();
            terminal.draw(|frame| ui::render(frame, &app))?;

            if event::poll(EVENT_POLL)?
                && let Event::Key(key) = event::read()?
            {
                app.handle_key(key);
            }

            if app.reload_requested {
                app.reload_requested = false;
                spawn_loader(&sender);
            }
        }

        Ok(())
    })
}
