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
    assert_eq!(app.confirmation_prompt(), "Enable a.service at boot?");

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
    assert_eq!(
        app.confirmation_prompt(),
        "Disable 1 recommended service(s)?"
    );
}

#[test]
fn quick_optimize_says_so_when_nothing_is_optimizable() {
    let mut app = app_with_services();
    app.tab = Tab::Analysis;

    app.handle_key(press(KeyCode::Char('o')));

    assert_eq!(app.overlay, Overlay::None);
    let message = app.message.clone().unwrap_or_default();
    assert!(
        message.contains("Nothing to optimize"),
        "message was: {message}"
    );
}

#[test]
fn export_report_writes_a_file_and_reports_its_path() {
    let mut app = app_with_services();
    app.tab = Tab::Analysis;

    app.handle_key(press(KeyCode::Char('w')));

    let message = app.message.clone().unwrap_or_default();
    let path = message
        .strip_prefix("Report written to ")
        .unwrap_or_default()
        .to_string();
    assert!(!path.is_empty(), "message was: {message}");

    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(contents.contains("Linux Boot Analysis Report"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn analysis_lines_summarise_the_snapshot() {
    let app = app_with_services();
    let text = app.analysis_lines().join("\n");
    assert!(text.contains("System"));
    assert!(text.contains("Slowest Services"));
    assert!(text.contains("Optimization"));
}

#[test]
fn report_includes_the_header_and_sections() {
    let app = app_with_services();
    let report = app.report();
    assert!(report.starts_with("Linux Boot Analysis Report"));
    assert!(report.contains("Slowest Services"));
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
fn help_opens_in_the_info_pane() {
    let mut app = App::new();
    app.handle_key(press(KeyCode::Char('?')));

    let info = app.info.as_ref().expect("help pane");
    assert_eq!(info.title, "Help");
    assert!(info.lines.iter().any(|line| line.contains("switch tab")));
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
