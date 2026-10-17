use super::*;
use crate::system::{BlameEntry, BootSummary, Service, SystemInfo};
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
        custom_profiles: Ok(Vec::new()),
    });
    app.tip_filter = TipFilter::All;
    app
}

fn add_device(app: &mut App) {
    if let Some(data) = app.data.as_mut()
        && let Ok(services) = &mut data.services
    {
        services.push(Service {
            name: "dev-sda.device".to_string(),
            active: "active".to_string(),
            sub: "plugged".to_string(),
            description: "Disk".to_string(),
            enabled: "unknown".to_string(),
        });
    }
}

fn add_backup(app: &mut App) {
    if let Some(data) = app.data.as_mut() {
        data.backups = Ok(vec![Backup {
            name: "backup-1".to_string(),
            timestamp: "2026-01-01 10:00".to_string(),
            path: PathBuf::from("/tmp/backup-1.json"),
            services: BTreeMap::from([("cups.service".to_string(), "disabled".to_string())]),
        }]);
    }
}

fn add_cups(app: &mut App) {
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
}

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
fn defaults_show_disable_recommendations_sorted_by_boot_time() {
    let app = App::new();
    assert_eq!(app.tip_filter, TipFilter::Suggestion);
    assert_eq!(app.sort, SortBy::BootTime);
    assert_eq!(app.view, ServiceView::Services);
}

#[test]
fn tab_cycles_forward_and_wraps() {
    let mut app = App::new();
    for expected in [
        Tab::Services,
        Tab::Autostart,
        Tab::Profiles,
        Tab::Backups,
        Tab::Analysis,
    ] {
        app.handle_key(press(KeyCode::Tab));
        assert_eq!(app.tab, expected);
    }
}

#[test]
fn shift_tab_cycles_backward_and_wraps() {
    let mut app = App::new();
    app.handle_key(press(KeyCode::BackTab));
    assert_eq!(app.tab, Tab::Backups);
}

#[test]
fn number_keys_jump_straight_to_a_tab() {
    let mut app = App::new();
    app.handle_key(press(KeyCode::Char('3')));
    assert_eq!(app.tab, Tab::Autostart);
    app.handle_key(press(KeyCode::Char('5')));
    assert_eq!(app.tab, Tab::Backups);
}

#[test]
fn arrow_keys_never_switch_tabs() {
    let mut app = App::new();
    app.handle_key(press(KeyCode::Right));
    app.handle_key(press(KeyCode::Left));
    assert_eq!(app.tab, Tab::Analysis);
}

#[test]
fn arrows_and_v_cycle_the_unit_view_on_the_services_tab() {
    let mut app = app_with_services();
    add_device(&mut app);
    assert_eq!(app.view, ServiceView::Services);
    app.selected = 1;

    app.handle_key(press(KeyCode::Right));
    assert_eq!(app.view, ServiceView::Others);
    app.handle_key(press(KeyCode::Left));
    assert_eq!(app.view, ServiceView::Services);
    assert_eq!(app.selected, 0);

    app.handle_key(press(KeyCode::Char('v')));
    assert_eq!(app.view, ServiceView::Others);
}

#[test]
fn the_unit_view_splits_services_and_devices() {
    let mut app = app_with_services();
    add_device(&mut app);

    assert_eq!(app.view_total(), 2);
    app.view = ServiceView::Devices;
    assert_eq!(app.view_total(), 1);
    let names: Vec<&str> = app
        .visible_services()
        .iter()
        .map(|service| service.name.as_str())
        .collect();
    assert_eq!(names, ["dev-sda.device"]);
}

#[test]
fn device_units_cannot_be_managed() {
    let mut app = app_with_services();
    add_device(&mut app);
    app.view = ServiceView::Devices;

    app.handle_key(press(KeyCode::Char('a')));

    assert_eq!(app.overlay, Overlay::None);
    assert_eq!(
        app.message.as_deref(),
        Some("Device units cannot be managed from here.")
    );
}

#[test]
fn a_second_command_is_refused_while_one_runs() {
    let mut app = app_with_services();
    app.running = true;

    app.perform(Pending::RemoveBackup {
        name: "busy".to_string(),
        path: PathBuf::from("/tmp/opencode-busy-backup.json"),
    });

    assert!(app.running);
    assert_eq!(
        app.message.as_deref(),
        Some("A command is already running.")
    );
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
fn services_are_sorted_by_boot_time_by_default() {
    let app = app_with_services();
    let names: Vec<&str> = app
        .visible_services()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["b.service", "a.service"]);
}

#[test]
fn sorting_by_name_is_alphabetical() {
    let mut app = app_with_services();
    app.sort = SortBy::Name;
    let names: Vec<&str> = app
        .visible_services()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["a.service", "b.service"]);
}

#[test]
fn s_cycles_the_sort_order() {
    let mut app = app_with_services();
    app.handle_key(press(KeyCode::Char('s')));
    assert_eq!(app.sort, SortBy::Name);
}

#[test]
fn the_suggestion_filter_hides_units_outside_the_knowledge_base() {
    let mut app = app_with_services();
    app.tip_filter = TipFilter::Suggestion;
    assert!(app.visible_services().is_empty());
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
    let unit = app
        .selected_service()
        .map(|service| service.name.clone())
        .unwrap_or_default();
    app.handle_key(press(KeyCode::Char('a')));
    assert_eq!(app.overlay, Overlay::Menu);

    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.overlay, Overlay::Confirm);
    assert_eq!(app.confirmation_prompt(), format!("Enable {unit} at boot?"));

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
fn export_report_writes_a_file_and_copies_it() {
    let mut app = app_with_services();
    app.tab = Tab::Analysis;

    app.handle_key(press(KeyCode::Char('w')));

    let message = app.message.clone().unwrap_or_default();
    let path = message
        .strip_prefix("Report written to ")
        .and_then(|rest| rest.strip_suffix(" and copied to the clipboard"))
        .unwrap_or_default()
        .to_string();
    assert!(!path.is_empty(), "message was: {message}");

    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(contents.contains("Linux Boot Analysis Report"));
    let _ = std::fs::remove_file(&path);

    assert_eq!(app.take_clipboard().as_deref(), Some(contents.as_str()));
    assert!(app.take_clipboard().is_none());
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
fn boot_stages_use_the_translated_labels() {
    let mut app = app_with_services();
    if let Some(data) = app.data.as_mut() {
        data.boot = Ok(BootSummary {
            total: "18.9s".to_string(),
            detail: "Startup finished in 3.1s (firmware) + 2.2s (loader) \
                     + 1.1s (kernel) + 0.5s (initrd) + 12.0s (userspace)"
                .to_string(),
        });
    }

    let text = app.analysis_lines().join("\n");
    assert!(text.contains("Firmware: 3.1s"), "{text}");
    assert!(text.contains("Userspace: 12.0s"), "{text}");
    assert!(text.contains("(firmware)"), "{text}");
}

#[test]
fn report_includes_the_header_and_sections() {
    let app = app_with_services();
    let report = app.report();
    assert!(report.starts_with("Linux Boot Analysis Report"));
    assert!(report.contains("Slowest Services"));
    assert!(report.ends_with("Generated by linux-boot-analyzer\n"));
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
fn profiles_savings_count_enabled_slow_services() {
    let mut app = app_with_services();
    add_optimizable(&mut app);

    assert!((app.profile_savings("minimum") - 1.5).abs() < 1e-9);
    assert_eq!(app.profile_savings("does-not-exist"), 0.0);
}

#[test]
fn backups_tab_stages_a_restore_from_a_restore_point() {
    let mut app = app_with_services();
    add_cups(&mut app);
    add_backup(&mut app);
    app.tab = Tab::Backups;

    app.handle_key(press(KeyCode::Char('d')));

    assert_eq!(app.overlay, Overlay::Confirm);
    assert!(
        app.confirmation_prompt()
            .starts_with("Restore this backup?"),
        "prompt was: {}",
        app.confirmation_prompt()
    );
}

#[test]
fn backups_tab_deletes_the_selected_restore_point() {
    let mut app = app_with_services();
    add_backup(&mut app);
    app.tab = Tab::Backups;

    app.handle_key(press(KeyCode::Char('x')));

    assert_eq!(app.overlay, Overlay::Confirm);
    assert!(app.confirmation_prompt().contains("backup-1"));
}

#[test]
fn restore_points_are_not_touched_from_the_profiles_tab() {
    let mut app = app_with_services();
    add_backup(&mut app);
    app.tab = Tab::Profiles;

    for key in ['d', 'x'] {
        app.handle_key(press(KeyCode::Char(key)));
        assert_eq!(app.overlay, Overlay::None, "key {key} opened an overlay");
    }
}

#[test]
fn applying_a_profile_confirms_with_a_plan() {
    let mut app = app_with_services();
    app.tab = Tab::Profiles;

    add_cups(&mut app);

    assert_eq!(app.selected_profile(), Some("minimum"));
    let (enable, disable) = profiles::selections("minimum");
    let plan = app.build_plan(&enable, &disable);
    assert!(plan.disable.contains(&"cups.service".to_string()));

    app.handle_key(press(KeyCode::Char('a')));
    assert_eq!(app.overlay, Overlay::Confirm);
    assert!(app.confirmation_prompt().starts_with("Apply this profile?"));
}

fn add_custom_profile(app: &mut App, name: &str, services: &[(&str, &str)]) {
    if let Some(data) = app.data.as_mut() {
        data.custom_profiles = Ok(vec![CustomProfile {
            name: name.to_string(),
            services: services
                .iter()
                .map(|(unit, rule)| ((*unit).to_string(), (*rule).to_string()))
                .collect(),
            path: PathBuf::from("/tmp/custom-profile.json"),
        }]);
    }
}

#[test]
fn profile_navigation_spans_both_groups() {
    let mut app = app_with_services();
    add_custom_profile(&mut app, "Mine", &[("cups.service", "disable")]);
    app.tab = Tab::Profiles;

    for _ in 0..profiles::ids().len() {
        app.handle_key(press(KeyCode::Char('j')));
    }
    assert_eq!(app.profiles_selected, profiles::ids().len());
    assert_eq!(
        app.selected_custom_profile()
            .map(|profile| profile.name.as_str()),
        Some("Mine")
    );
    assert!(app.selected_profile().is_none());

    app.handle_key(press(KeyCode::Char('j')));
    assert_eq!(app.profiles_selected, app.profile_count() - 1);
}

#[test]
fn a_custom_profile_is_applied_through_the_same_confirmation() {
    let mut app = app_with_services();
    add_cups(&mut app);
    add_custom_profile(&mut app, "Mine", &[("cups.service", "disable")]);
    app.tab = Tab::Profiles;
    app.profiles_selected = profiles::ids().len();

    app.handle_key(press(KeyCode::Char('a')));

    assert_eq!(app.overlay, Overlay::Confirm);
    let prompt = app.confirmation_prompt();
    assert!(prompt.starts_with("Apply this profile?"), "{prompt}");
    assert!(prompt.contains("disable 1"), "{prompt}");
}

#[test]
fn deleting_a_custom_profile_asks_first() {
    let mut app = app_with_services();
    add_custom_profile(&mut app, "Mine", &[("cups.service", "disable")]);
    app.tab = Tab::Profiles;
    app.profiles_selected = profiles::ids().len();

    app.handle_key(press(KeyCode::Char('x')));

    assert_eq!(app.overlay, Overlay::Confirm);
    assert!(app.confirmation_prompt().contains("Mine"));
}

#[test]
fn built_in_profiles_are_never_deleted() {
    let mut app = app_with_services();
    add_custom_profile(&mut app, "Mine", &[("cups.service", "disable")]);
    app.tab = Tab::Profiles;

    app.handle_key(press(KeyCode::Char('x')));

    assert_eq!(app.overlay, Overlay::None);
    assert!(app.pending.is_none());
}

#[test]
fn the_new_profile_form_toggles_rules_and_demands_a_name() {
    let mut app = app_with_services();
    app.tab = Tab::Profiles;

    app.handle_key(press(KeyCode::Char('n')));
    assert_eq!(app.overlay, Overlay::ProfileForm);
    assert_eq!(app.profile_form_index, 0);
    assert!(app.profile_form_checked.iter().all(|checked| !checked));

    app.handle_key(press(KeyCode::Char(' ')));
    assert!(app.profile_form_checked.iter().all(|checked| !checked));

    app.handle_key(press(KeyCode::Down));
    app.handle_key(press(KeyCode::Char(' ')));
    assert!(app.profile_form_checked[0]);

    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.overlay, Overlay::ProfileForm);
    assert_eq!(app.message.as_deref(), Some("Please enter a profile name."));
}

#[test]
fn the_profiles_tab_has_no_save_current_state_key() {
    let mut app = app_with_services();
    app.tab = Tab::Profiles;

    app.handle_key(press(KeyCode::Char('w')));

    assert_eq!(app.overlay, Overlay::None);
    assert!(app.message.is_none());
    assert!(app.clipboard.is_none());
}

#[test]
fn help_opens_in_the_info_pane() {
    let mut app = App::new();
    app.handle_key(press(KeyCode::Char('?')));

    let info = app.info.as_ref().expect("help pane");
    assert_eq!(info.title, "Help");
    assert!(info.lines.iter().any(|line| line.contains("switch tab")));
    assert!(info.lines.iter().any(|line| line.contains("jump straight")));
}

#[test]
fn plan_ignores_services_already_in_the_wanted_state() {
    let mut app = app_with_services();

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

#[test]
fn typed_delays_are_parsed_and_capped() {
    assert_eq!(parse_delay("12"), Some(12));
    assert_eq!(parse_delay(" 0 "), Some(0));
    assert_eq!(parse_delay("9999"), Some(600));
    assert_eq!(parse_delay(""), None);
    assert_eq!(parse_delay("abc"), None);
}

#[test]
fn the_delay_prompt_opens_with_the_current_value() {
    let mut app = app_with_services();
    app.tab = Tab::Autostart;

    app.handle_key(press(KeyCode::Char('d')));

    assert_eq!(app.overlay, Overlay::Delay);
    assert_eq!(app.delay_input, "0");
}

#[test]
fn typing_replaces_the_prefilled_delay() {
    let mut app = app_with_services();
    app.tab = Tab::Autostart;
    app.handle_key(press(KeyCode::Char('d')));

    for key in typed("12") {
        app.handle_key(key);
    }
    assert_eq!(app.delay_input, "12");

    app.handle_key(press(KeyCode::Backspace));
    assert_eq!(app.delay_input, "1");

    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn arrows_step_the_delay_by_one_second() {
    let mut app = app_with_services();
    app.tab = Tab::Autostart;
    app.handle_key(press(KeyCode::Char('d')));

    app.handle_key(press(KeyCode::Up));
    assert_eq!(app.delay_input, "1");
    app.handle_key(press(KeyCode::Up));
    assert_eq!(app.delay_input, "2");
    app.handle_key(press(KeyCode::Down));
    assert_eq!(app.delay_input, "1");

    for _ in 0..5 {
        app.handle_key(press(KeyCode::Down));
    }
    assert_eq!(app.delay_input, "0");
}

#[test]
fn an_empty_delay_is_rejected() {
    let mut app = app_with_services();
    app.tab = Tab::Autostart;
    app.handle_key(press(KeyCode::Char('d')));
    app.handle_key(press(KeyCode::Backspace));
    app.handle_key(press(KeyCode::Enter));

    assert_eq!(app.overlay, Overlay::Delay);
    assert_eq!(
        app.message.as_deref(),
        Some("Enter the delay as a whole number of seconds.")
    );
}

#[test]
fn saving_a_typed_delay_writes_the_entry() {
    let mut app = app_with_services();
    app.tab = Tab::Autostart;

    let path = std::env::temp_dir().join("opencode-delay-test.desktop");
    std::fs::write(&path, "[Desktop Entry]\nName=Example\nExec=example\n")
        .expect("temporary entry");
    if let Some(data) = app.data.as_mut()
        && let Ok(entries) = &mut data.autostart
        && let Some(entry) = entries.first_mut()
    {
        entry.path = path.clone();
    }

    app.handle_key(press(KeyCode::Char('d')));
    for key in typed("12") {
        app.handle_key(key);
    }
    app.handle_key(press(KeyCode::Enter));

    assert_eq!(app.overlay, Overlay::None);
    let contents = std::fs::read_to_string(&path).expect("written entry");
    assert!(
        contents.contains("X-GNOME-Autostart-Delay=12"),
        "{contents}"
    );
    assert!(contents.contains("sleep 12"), "{contents}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_language_picker_opens_on_the_current_language() {
    let mut app = app_with_services();

    app.handle_key(press(KeyCode::Char('L')));

    assert_eq!(app.overlay, Overlay::Language);
    assert_eq!(app.language_index, 0);

    app.handle_key(press(KeyCode::Down));
    assert_eq!(app.language_index, 1);
    app.handle_key(press(KeyCode::Down));
    assert_eq!(app.language_index, 1);
    app.handle_key(press(KeyCode::Up));
    assert_eq!(app.language_index, 0);

    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.overlay, Overlay::None);
}

// There is deliberately no test that confirms a language: the language is
// process-wide state and libtest runs the tests in parallel, so switching it
// would leak into every other test. `i18n::ALL` is checked in `src/i18n.rs`.
