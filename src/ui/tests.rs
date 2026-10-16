use super::*;
use crate::system::{BlameEntry, BootSummary, Service, SystemData, SystemInfo};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn sample_data() -> SystemData {
    SystemData {
        info: SystemInfo {
            os: "Pardus".to_string(),
            kernel: "6.1.0".to_string(),
            ram: "8.0 GB".to_string(),
            uptime: "1h 2m".to_string(),
        },
        boot: Ok(BootSummary {
            total: "22.713s".to_string(),
            detail: "Startup finished in 22.713s".to_string(),
        }),
        blame: Ok(vec![BlameEntry {
            time: "2.345s".to_string(),
            name: "NetworkManager-wait-online.service".to_string(),
        }]),
        services: Ok(vec![Service {
            name: "NetworkManager.service".to_string(),
            active: "active".to_string(),
            sub: "running".to_string(),
            description: "Network Manager".to_string(),
            enabled: "enabled".to_string(),
        }]),
        autostart: Ok(vec![AutostartEntry {
            path: std::path::PathBuf::from("/home/user/.config/autostart/example.desktop"),
            name: "Example App".to_string(),
            exec: "example --start".to_string(),
            comment: String::new(),
            icon: "system-run".to_string(),
            delay: 5,
            enabled: false,
        }]),
        backups: Ok(Vec::new()),
    }
}

fn loaded_app(tab: Tab) -> App {
    let mut app = App::new();
    app.tab = tab;
    app.loading = false;
    app.data = Some(sample_data());
    app
}

fn render_to_string(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| render(frame, app)).expect("draw");
    terminal.backend().to_string()
}

#[test]
fn analysis_tab_shows_system_and_boot_data() {
    let app = loaded_app(Tab::Analysis);
    let text = render_to_string(&app, 90, 24);
    assert!(text.contains("Analysis"));
    assert!(text.contains("Pardus"));
    assert!(text.contains("22.713s"));
    assert!(text.contains("NetworkManager-wait-online.service"));
}

#[test]
fn services_tab_shows_the_table_and_details() {
    let app = loaded_app(Tab::Services);
    let text = render_to_string(&app, 100, 24);
    assert!(text.contains("Service"));
    assert!(text.contains("NetworkManager.service"));
    assert!(text.contains("Critical"));
}

#[test]
fn loading_state_is_shown_before_data_arrives() {
    let app = App::new();
    let text = render_to_string(&app, 90, 24);
    assert!(text.contains("Loading..."));
}

#[test]
fn action_menu_overlay_lists_the_available_actions() {
    let mut app = loaded_app(Tab::Services);
    app.overlay = Overlay::Menu;
    let text = render_to_string(&app, 100, 24);
    assert!(text.contains("Enable"));
    assert!(text.contains("Unmask"));
}

#[test]
fn info_pane_shows_its_title_and_content() {
    let mut app = loaded_app(Tab::Services);
    app.info = Some(crate::app::InfoPane {
        title: "Log: sshd.service".to_string(),
        lines: vec!["Oct 16 host systemd[1]: Started sshd.".to_string()],
        scroll: 0,
    });

    let text = render_to_string(&app, 100, 24);
    assert!(text.contains("Log: sshd.service"));
    assert!(text.contains("Started sshd."));
}

#[test]
fn autostart_tab_lists_entries() {
    let app = loaded_app(Tab::Autostart);
    let text = render_to_string(&app, 100, 24);
    assert!(text.contains("Example App"));
    assert!(text.contains("disabled"));
    assert!(text.contains("example --start"));
}

#[test]
fn add_form_overlay_shows_both_fields() {
    let mut app = loaded_app(Tab::Autostart);
    app.overlay = Overlay::Form;
    app.form_values = vec!["My App".to_string(), "myapp".to_string()];

    let text = render_to_string(&app, 100, 24);
    assert!(text.contains("New Autostart Entry"));
    assert!(text.contains("My App"));
}

#[test]
fn profiles_tab_lists_profiles_and_restore_points() {
    let app = loaded_app(Tab::Profiles);
    let text = render_to_string(&app, 110, 24);
    assert!(text.contains("Minimum Services"));
    assert!(text.contains("Restore Points"));
}

#[test]
fn action_bar_shows_the_keys_for_the_active_tab() {
    let analysis = render_to_string(&loaded_app(Tab::Analysis), 120, 24);
    assert!(analysis.contains("o: disable recommended services"));
    assert!(analysis.contains("w: write report"));

    let services = render_to_string(&loaded_app(Tab::Services), 120, 24);
    assert!(services.contains("Enter/a: actions"));
    assert!(services.contains("enable, disable, start, stop"));

    let autostart = render_to_string(&loaded_app(Tab::Autostart), 120, 24);
    assert!(autostart.contains("space: enable/disable"));

    let profiles = render_to_string(&loaded_app(Tab::Profiles), 120, 24);
    assert!(profiles.contains("b: create restore point"));
}

#[test]
fn the_status_line_advertises_the_language_key() {
    let text = render_to_string(&loaded_app(Tab::Analysis), 120, 24);
    assert!(text.contains("L: language"));
}

#[test]
fn tiny_terminals_do_not_panic() {
    for (width, height) in [(20, 6), (10, 4), (1, 1)] {
        let app = loaded_app(Tab::Services);
        let _ = render_to_string(&app, width, height);
    }
}
