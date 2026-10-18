use super::*;
use crate::custom_profiles::CustomProfile;
use crate::system::{BlameEntry, BootSummary, Service, SystemData, SystemInfo};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        custom_profiles: Ok(Vec::new()),
    }
}

fn loaded_app(tab: Tab) -> App {
    let mut app = App::new();
    app.tab = tab;
    app.loading = false;
    app.data = Some(sample_data());
    app.tip_filter = crate::filters::TipFilter::All;
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
fn profiles_tab_lists_profiles_and_their_plan() {
    let app = loaded_app(Tab::Profiles);
    let text = render_to_string(&app, 120, 24);
    assert!(text.contains("Minimum Services"));
    assert!(text.contains("Estimated Savings"));
    assert!(text.contains("✘"));
}

#[test]
fn backups_tab_lists_restore_points() {
    let mut app = loaded_app(Tab::Backups);
    if let Some(data) = app.data.as_mut() {
        data.backups = Ok(vec![crate::backups::Backup {
            name: "backup-1".to_string(),
            timestamp: "2026-01-01 10:00".to_string(),
            path: std::path::PathBuf::from("/tmp/backup-1.json"),
            services: std::collections::BTreeMap::from([(
                "cups.service".to_string(),
                "disabled".to_string(),
            )]),
        }]);
    }

    let text = render_to_string(&app, 120, 24);
    assert!(text.contains("Restore Points"));
    assert!(text.contains("backup-1"));
    assert!(text.contains("2026-01-01 10:00"));
}

#[test]
fn services_title_shows_the_unit_type_and_count() {
    let app = loaded_app(Tab::Services);
    let text = render_to_string(&app, 100, 24);
    assert!(text.contains("Services (.service)"));
    assert!(text.contains("1/1 services"));
}

#[test]
fn action_bar_shows_the_keys_for_the_active_tab() {
    let analysis = render_to_string(&loaded_app(Tab::Analysis), 120, 24);
    assert!(analysis.contains("o: optimize"));
    assert!(analysis.contains("w: report"));

    let services = render_to_string(&loaded_app(Tab::Services), 120, 24);
    assert!(services.contains("Enter: actions"));
    assert!(services.contains("v: unit type"));

    let autostart = render_to_string(&loaded_app(Tab::Autostart), 120, 24);
    assert!(autostart.contains("space: enable/disable"));

    let profiles = render_to_string(&loaded_app(Tab::Profiles), 120, 24);
    assert!(profiles.contains("n: new"));
    assert!(profiles.contains("b: restore point"));

    let backups = render_to_string(&loaded_app(Tab::Backups), 120, 24);
    assert!(backups.contains("d: restore"));
}

#[test]
fn the_action_bar_fits_a_normal_terminal() {
    for tab in [
        Tab::Analysis,
        Tab::Services,
        Tab::Autostart,
        Tab::Profiles,
        Tab::Backups,
    ] {
        let hints = action_hints(&loaded_app(tab));
        assert!(hints.chars().count() <= 80, "{tab:?}: {hints}");
    }
}

#[test]
fn the_status_line_advertises_the_language_key() {
    let text = render_to_string(&loaded_app(Tab::Analysis), 120, 24);
    assert!(text.contains("L: language"));
}

fn render_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| render(frame, app)).expect("draw");
    terminal.backend().buffer().clone()
}

#[test]
fn the_theme_paints_titles_states_and_the_selection() {
    let mut app = loaded_app(Tab::Services);
    if let Some(data) = app.data.as_mut()
        && let Ok(services) = &mut data.services
    {
        services.push(Service {
            name: "cups.service".to_string(),
            active: "failed".to_string(),
            sub: "failed".to_string(),
            description: "Printing".to_string(),
            enabled: "disabled".to_string(),
        });
    }

    let buffer = render_buffer(&app, 100, 30);
    let colours: Vec<Color> = buffer.content.iter().map(|cell| cell.fg).collect();

    assert!(colours.contains(&ACCENT), "no accent colour");
    assert!(colours.contains(&OK), "no healthy colour");
    assert!(colours.contains(&BAD), "no failure colour");
    assert!(colours.contains(&MUTED), "no muted colour");
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.modifier.contains(Modifier::REVERSED)),
        "the selected row is not highlighted"
    );
}

#[test]
fn the_status_line_reports_success_and_failure_in_colour() {
    let mut app = loaded_app(Tab::Services);
    app.message = Some("Disable failed.".to_string());
    app.level = Level::Error;
    let buffer = render_buffer(&app, 100, 30);
    assert!(buffer.content.iter().any(|cell| cell.fg == BAD));

    app.level = Level::Success;
    let buffer = render_buffer(&app, 100, 30);
    assert!(buffer.content.iter().any(|cell| cell.fg == OK));
}

#[test]
fn tiny_terminals_do_not_panic() {
    let mut confirming = loaded_app(Tab::Services);
    confirming.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    confirming.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    for (width, height) in [(20, 6), (10, 4), (1, 1)] {
        let _ = render_to_string(&loaded_app(Tab::Services), width, height);
        let _ = render_to_string(&confirming, width, height);
    }
}

#[test]
fn fits_asks_for_both_dimensions() {
    assert!(fits(80, 24));
    assert!(fits(200, 60));
    assert!(!fits(79, 24));
    assert!(!fits(80, 23));
}

#[test]
fn a_terminal_below_the_minimum_shows_only_the_notice() {
    let app = loaded_app(Tab::Services);

    let text = render_to_string(&app, 79, 23);

    assert!(text.contains("Terminal too small"), "{text}");
    assert!(text.contains("Size: 79x23"), "{text}");
    assert!(text.contains("80x24 needed"), "{text}");
    assert!(text.contains("press q to quit"), "{text}");
    assert!(!text.contains("NetworkManager.service"), "{text}");
    assert!(!text.contains("Analysis"), "{text}");
}

#[test]
fn the_smallest_supported_terminal_draws_the_interface() {
    let app = loaded_app(Tab::Services);

    let text = render_to_string(&app, MIN_WIDTH, MIN_HEIGHT);

    assert!(!text.contains("Terminal too small"), "{text}");
    assert!(text.contains("NetworkManager.service"), "{text}");
}

#[test]
fn the_confirmation_dialog_counts_wrapped_rows() {
    assert_eq!(wrapped_rows("short question?", 40), 1);
    assert_eq!(wrapped_rows("a question that does not fit", 10), 3);
    assert_eq!(wrapped_rows("unbreakablesupercalifragilistic", 5), 7);
    assert_eq!(wrapped_rows("anything", 0), 1);
    assert_eq!(wrapped_rows("ab    cd", 8), 1);
    assert_eq!(wrapped_rows("ab    cd", 7), 2);
}

#[test]
fn password_dialog_keeps_the_key_hint_visible() {
    let mut app = loaded_app(Tab::Services);
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

    let text = render_to_string(&app, 80, 24);
    assert!(text.contains("Enter to run"), "{text}");
    assert!(text.contains("Esc to cancel"), "{text}");
}

#[test]
fn the_language_menu_lists_every_language() {
    let mut app = loaded_app(Tab::Analysis);
    app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    let text = render_to_string(&app, 80, 24);
    assert!(text.contains("Language"), "{text}");
    assert!(text.contains("English"), "{text}");
    assert!(text.contains("Türkçe"), "{text}");
    assert!(text.contains("Up/Down: choose"), "{text}");
}

#[test]
fn the_delay_dialog_shows_the_value_and_the_key_hint() {
    let mut app = loaded_app(Tab::Autostart);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    let text = render_to_string(&app, 80, 24);
    assert!(text.contains("Delay in seconds:"), "{text}");
    assert!(text.contains("5s"), "{text}");
    assert!(
        text.contains("↑/↓: 1s    Enter: save    Esc: cancel"),
        "{text}"
    );
}

#[test]
fn confirmation_dialog_keeps_the_key_hint_visible() {
    let mut app = loaded_app(Tab::Services);
    if let Some(data) = app.data.as_mut() {
        data.services = Ok(vec![Service {
            name: "systemd-networkd-wait-online.service".to_string(),
            active: "active".to_string(),
            sub: "running".to_string(),
            description: "Wait for the network to come online".to_string(),
            enabled: "enabled".to_string(),
        }]);
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let text = render_to_string(&app, 80, 24);
    assert!(
        text.contains("Enable systemd-networkd-wait-online"),
        "{text}"
    );
    assert!(text.contains("Enter to confirm"), "{text}");
    assert!(text.contains("Esc to cancel"), "{text}");
}

#[test]
fn a_long_service_description_is_not_clipped() {
    let app = loaded_app(Tab::Services);

    let text = render_to_string(&app, MIN_WIDTH, 26);

    assert!(
        text.contains("Manages Ethernet, Wi-Fi, and mobile network connections."),
        "{text}"
    );
    assert!(text.contains("Do not disable."), "{text}");
    assert!(text.contains("connectivity."), "{text}");
}

#[test]
fn a_wrapping_autostart_path_is_not_clipped() {
    let mut app = loaded_app(Tab::Autostart);
    let long_path = format!("/home/user/.config/autostart/{}.desktop", "a".repeat(60));
    if let Some(data) = app.data.as_mut()
        && let Ok(entries) = &mut data.autostart
    {
        entries[0].path = std::path::PathBuf::from(&long_path);
    }

    let text = render_to_string(&app, MIN_WIDTH, MIN_HEIGHT);

    assert!(text.contains("Command: example --start"), "{text}");
    assert!(text.contains("File:"), "{text}");
    assert!(text.contains("/home/user/.config/autostart/"), "{text}");
    assert!(text.contains(".desktop"), "{text}");
}

fn with_custom_profile(tab: Tab) -> App {
    let mut app = loaded_app(tab);
    if let Some(data) = app.data.as_mut() {
        data.custom_profiles = Ok(vec![CustomProfile {
            name: "My Profile".to_string(),
            services: std::collections::BTreeMap::from([
                ("cups.service".to_string(), "disable".to_string()),
                ("bluetooth.service".to_string(), "disable".to_string()),
            ]),
            path: std::path::PathBuf::from(
                "/home/user/.config/linux-boot-analyzer-tui/profiles/myprofile.json",
            ),
        }]);
    }
    app
}

#[test]
fn the_profiles_tab_lists_the_users_own_profiles() {
    let mut app = with_custom_profile(Tab::Profiles);
    app.profiles_selected = crate::profiles::ids().len();

    let text = render_to_string(&app, 100, 24);

    assert!(text.contains("User Custom Profiles"), "{text}");
    assert!(text.contains("> My Profile"), "{text}");
    assert!(text.contains("Minimum Services"), "{text}");
    assert!(text.contains("2 service rules"), "{text}");
    assert!(text.contains("Printer Service (CUPS)"), "{text}");
}

#[test]
fn an_empty_custom_list_says_so() {
    let text = render_to_string(&loaded_app(Tab::Profiles), 100, 24);
    assert!(text.contains("No custom profiles found."), "{text}");
}

#[test]
fn the_new_profile_form_shows_every_candidate_and_its_hint() {
    let mut app = loaded_app(Tab::Profiles);
    app.overlay = Overlay::ProfileForm;
    app.profile_name = "Work".to_string();
    app.profile_form_index = 1;
    app.profile_form_checked[0] = true;

    let text = render_to_string(&app, 90, 30);

    assert!(text.contains("Create New Custom Profile"), "{text}");
    assert!(text.contains("Profile Name: Work"), "{text}");
    assert!(text.contains("> [x] Printer Service (CUPS)"), "{text}");
    assert!(text.contains("[ ] Cellular Modem Controller"), "{text}");
    assert!(text.contains("Enter: create"), "{text}");
}
