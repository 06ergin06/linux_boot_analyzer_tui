//! A terminal UI for analyzing boot time, rewritten from the Python
//! `pardus-boot-analyzer` application.

mod app;
mod autostart;
mod backups;
mod clipboard;
mod custom_profiles;
mod filters;
mod i18n;
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

const EVENT_POLL: Duration = Duration::from_millis(200);

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cli {
    Run,
    Help,
    Version,
}

fn parse_cli(args: impl Iterator<Item = String>) -> Cli {
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Cli::Help,
            "-V" | "--version" => return Cli::Version,
            _ => {}
        }
    }
    Cli::Run
}

fn print_help() {
    println!("{NAME} {VERSION}");
    println!();
    println!("A terminal UI that analyses Linux boot time.");
    println!();
    println!("Usage: {NAME} [OPTION]");
    println!();
    println!("Options:");
    println!("  -h, --help     print this help and exit");
    println!("  -V, --version  print the version and exit");
    println!();
    println!("Without an option the interface starts. It is driven by the keyboard,");
    println!("so there is nothing else to pass in; press ? inside it for the key list.");
}

fn spawn_loader(sender: &Sender<SystemData>) {
    let sender = sender.clone();
    thread::spawn(move || {
        let _ = sender.send(system::load());
    });
}

fn main() -> io::Result<()> {
    match parse_cli(std::env::args().skip(1)) {
        Cli::Help => {
            print_help();
            return Ok(());
        }
        Cli::Version => {
            println!("{NAME} {VERSION}");
            return Ok(());
        }
        Cli::Run => {}
    }

    i18n::init();

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

            // The keyboard has to know what the frame just showed: below the
            // minimum size only the notice is drawn, and then only the quit
            // keys are answered.
            let size = terminal.size()?;
            app.too_small = !ui::fits(size.width, size.height);

            // Anything the app queued for the clipboard goes out as an OSC 52
            // escape sequence, after the frame has been painted.
            if let Some(text) = app.take_clipboard() {
                clipboard::copy(&text);
            }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|arg| (*arg).to_string())
    }

    #[test]
    fn no_argument_starts_the_interface() {
        assert_eq!(parse_cli(args(&[])), Cli::Run);
        assert_eq!(parse_cli(args(&["--nonsense"])), Cli::Run);
    }

    #[test]
    fn help_and_version_are_recognised() {
        assert_eq!(parse_cli(args(&["-h"])), Cli::Help);
        assert_eq!(parse_cli(args(&["--help"])), Cli::Help);
        assert_eq!(parse_cli(args(&["-V"])), Cli::Version);
        assert_eq!(parse_cli(args(&["--version"])), Cli::Version);
    }
}
