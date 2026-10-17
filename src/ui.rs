use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Clear, List, ListState, Paragraph, Row, Table, TableState, Tabs, Wrap,
};

use crate::app::{App, Level, Overlay, Tab};
use crate::autostart::AutostartEntry;
use crate::custom_profiles::{self, CustomProfile};
use crate::i18n;
use crate::i18n::Lang;
use crate::privilege::Action;
use crate::system::{self, Service};
use crate::{profiles, service_db};

// --- Theme ---------------------------------------------------------------
//
// Only the sixteen named terminal colours are used, never RGB: the interface
// then follows whatever palette the user's terminal is themed with instead of
// looking right on exactly one background. Colour is always a second signal,
// never the only one - every state is also spelled out in words.

/// Titles, headings and the active tab: where the eye should land first.
const ACCENT: Color = Color::Cyan;
/// Secondary text: hints, keys, paths and units that are simply turned off.
const MUTED: Color = Color::DarkGray;
/// Healthy: active, enabled, safe to leave alone.
const OK: Color = Color::Green;
/// Worth a look: suggestions, masked units, work in progress.
const WARN: Color = Color::Yellow;
/// Trouble: failed units, errors, services that must stay on.
const BAD: Color = Color::Red;
/// Background of the selected row. Combined with `REVERSED`, so the selection
/// stays visible even on a terminal that ignores colours.
const SELECTED: Color = Color::Blue;

/// Style for secondary text.
fn muted() -> Style {
    Style::new().fg(MUTED)
}

/// Style for the selected row in a table or list.
fn highlight() -> Style {
    Style::new()
        .fg(SELECTED)
        .add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

/// A bordered panel with a coloured title, used for every block in the UI.
fn panel(key: &str) -> Block<'static> {
    panel_titled(title(key))
}

/// A bordered panel whose title is built at runtime (unit or backup names).
fn panel_titled(title: String) -> Block<'static> {
    Block::bordered()
        .title(Line::from(title).style(Style::new().fg(ACCENT).bold()))
        .border_style(muted())
}

/// Draw the whole interface for one frame.
pub fn render(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        // Two lines: the available keys, then the status/hint line.
        Constraint::Length(2),
    ])
    .areas(frame.area());

    render_tabs(frame, app, header);
    render_body(frame, app, body);
    render_footer(frame, app, footer);
    render_overlay(frame, app);
    render_info(frame, app);
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles = Tab::ALL.iter().map(|tab| Line::from(tab.title()));
    let tabs = Tabs::new(titles)
        .block(panel_titled(" Linux Boot Analyzer ".to_string()))
        .select(app.tab.index())
        .highlight_style(Style::new().fg(ACCENT).bold().underlined())
        .divider("│")
        .style(muted());

    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    match app.tab {
        Tab::Analysis => render_analysis(frame, app, area),
        Tab::Services => render_services(frame, app, area),
        Tab::Autostart => render_autostart(frame, app, area),
        Tab::Profiles => render_profiles(frame, app, area),
        Tab::Backups => render_backups(frame, app, area),
    }
}

fn render_analysis(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app
        .analysis_lines()
        .iter()
        .map(|line| analysis_line(line))
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("tab_analysis"))
            .wrap(Wrap { trim: false })
            .scroll((app.analysis_scroll, 0)),
        area,
    );
}

/// Colour one line of the plain-text analysis snapshot.
///
/// The text itself comes from `App::analysis_lines`, which the report export
/// also prints, so this only re-styles it: section headings, `Label: value`
/// pairs and the right-aligned timings of the blame list are recognised, and
/// anything else is drawn as it is.
fn analysis_line(line: &str) -> Line<'static> {
    if line.is_empty() {
        return Line::default();
    }

    let headings = [
        i18n::t("tui_system"),
        i18n::t("boot_suresi"),
        i18n::t("startup_optimizasyonu"),
        i18n::t("tui_slowest_services"),
    ];
    if headings.contains(&line) {
        return Line::from(line.to_string()).style(Style::new().fg(ACCENT).bold());
    }

    let body = line.trim_start();
    let pad = " ".repeat(line.len() - body.len());

    if let Some((label, value)) = body.split_once(": ") {
        // The total boot time is the headline number of the page.
        let value_style = if label == i18n::t("toplam_boot") {
            Style::new().fg(WARN).bold()
        } else {
            Style::new()
        };
        return Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("{label}: "), muted()),
            Span::styled(value.to_string(), value_style),
        ]);
    }

    if let Some((time, name)) = body.split_once("  ")
        && system::parse_duration_seconds(time) > 0.0
    {
        return Line::from(vec![
            Span::raw(pad),
            Span::styled(format!("{time}  "), time_style(time)),
            Span::raw(name.to_string()),
        ]);
    }

    // The suggestion under a service name is a side note.
    if pad.len() >= 6 {
        return Line::from(line.to_string()).style(muted());
    }

    Line::from(line.to_string())
}

/// A bordered block title for a translated label.
fn title(key: &str) -> String {
    format!(" {} ", i18n::t(key))
}

fn render_services(frame: &mut Frame, app: &App, area: Rect) {
    let Some(data) = &app.data else {
        frame.render_widget(
            Paragraph::new(status_text(app)).block(panel("tab_services")),
            area,
        );
        return;
    };

    if let Err(err) = &data.services {
        frame.render_widget(
            Paragraph::new(err.as_str())
                .style(Style::new().fg(BAD))
                .block(panel("tab_services"))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let visible = app.visible_services();
    let times = app.blame_times();

    // The details grow with the knowledge-base text, so a long description is
    // never cut off; the table above keeps at least a few rows.
    let detail = service_detail_lines(visible.get(app.selected).copied());
    let [filter_area, table_area, detail_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(detail_height(&detail, area.width, area.height)),
    ])
    .areas(area);

    render_filter_bar(frame, app, filter_area);

    let header = Row::new([
        i18n::t("service_name"),
        i18n::t("state"),
        i18n::t("sub_state"),
        i18n::t("tui_boot_config"),
        i18n::t("time"),
        i18n::t("tui_tip"),
    ])
    .style(Style::new().fg(ACCENT).bold());
    let rows = visible.iter().map(|service| {
        let time = times.get(service.name.as_str()).copied().unwrap_or("-");
        Row::new(vec![
            Cell::from(service.name.as_str()),
            Cell::from(state_label(&service.active)).style(state_style(&service.active)),
            Cell::from(state_label(&service.sub)).style(state_style(&service.sub)),
            Cell::from(state_label(&service.enabled)).style(state_style(&service.enabled)),
            Cell::from(time).style(time_style(time)),
            match service_db::lookup(&service.name) {
                Some(entry) => Cell::from(entry.tip.label()).style(tip_style(entry.tip)),
                None => Cell::from("-").style(muted()),
            },
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Min(30),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(11),
        ],
    )
    .header(header)
    .block(panel_titled(services_title(app)))
    .row_highlight_style(highlight())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.selected);
    frame.render_stateful_widget(table, table_area, &mut state);

    render_service_detail(frame, &detail, detail_area);
}

/// `" Services (.service) — 12/450 services "` for the table border.
fn services_title(app: &App) -> String {
    let count = i18n::fill(
        i18n::t("tui_view_count"),
        &[
            &app.visible_services().len().to_string(),
            &app.view_total().to_string(),
            i18n::t("service_count"),
        ],
    );
    format!(" {} — {count} ", app.view.label())
}

fn render_filter_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    let filters = [
        ("f", i18n::t("tui_status"), app.status_filter.label()),
        ("t", i18n::t("tui_tip_filter"), app.tip_filter.label()),
        ("s", i18n::t("tui_sort"), app.sort.label()),
    ];
    for (key, label, value) in filters {
        spans.push(Span::styled(format!("{key}: "), muted()));
        spans.push(Span::styled(format!("{label}="), muted()));
        spans.push(Span::styled(
            format!("{value}   "),
            Style::new().fg(ACCENT).bold(),
        ));
    }

    // While the search box has focus the query is what the keys edit.
    let (search, search_style) = if app.searching || !app.query.is_empty() {
        (format!("/{}", app.query), Style::new().fg(ACCENT).bold())
    } else {
        (format!("/ {}", i18n::t("tui_search")), muted())
    };
    spans.push(Span::styled(search, search_style));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_service_detail(frame: &mut Frame, lines: &[Line], area: Rect) {
    frame.render_widget(
        Paragraph::new(lines.to_vec())
            .block(panel("tui_details"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// The knowledge-base text for the selected unit.
fn service_detail_lines(service: Option<&Service>) -> Vec<Line<'_>> {
    match service {
        Some(service) => match service_db::lookup(&service.name) {
            Some(entry) => vec![
                Line::from(vec![
                    Span::styled(service.name.as_str(), Style::new().bold()),
                    Span::raw(" "),
                    Span::styled(format!("({})", entry.tip.label()), tip_style(entry.tip)),
                ]),
                Line::from(""),
                Line::from(entry.description()),
                Line::from(entry.suggestion_text()).style(muted()),
            ],
            None => vec![
                Line::from(service.name.as_str()).bold(),
                Line::from(i18n::t("tui_no_kb_entry")).style(muted()),
            ],
        },
        None => vec![Line::from(i18n::t("tui_no_service_selected")).style(muted())],
    }
}

/// Colour for a systemd state word: healthy, transitional, off or broken.
fn state_style(state: &str) -> Style {
    match state {
        "active" | "running" | "enabled" | "enabled-runtime" => Style::new().fg(OK),
        "activating" | "deactivating" | "reloading" => Style::new().fg(WARN),
        "failed" => Style::new().fg(BAD).bold(),
        "masked" => Style::new().fg(WARN),
        _ => muted(),
    }
}

/// Boot times that matter stand out, the rest fade into the background.
fn time_style(time: &str) -> Style {
    if system::parse_duration_seconds(time) >= 1.0 {
        Style::new().fg(WARN)
    } else {
        muted()
    }
}

/// Colour for the knowledge-base tip of a service.
fn tip_style(tip: service_db::Tip) -> Style {
    match tip {
        service_db::Tip::Critical => Style::new().fg(BAD).bold(),
        service_db::Tip::Required => Style::new().fg(WARN),
        service_db::Tip::Suggestion => Style::new().fg(OK),
    }
}

/// "`Label`: value" with the label kept quiet.
fn detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), muted()),
        Span::raw(value.to_string()),
    ])
}

fn render_autostart(frame: &mut Frame, app: &App, area: Rect) {
    let entries = app.autostart_entries();
    let block = panel("tab_autostart");

    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(i18n::t("no_autostart_apps"))
                .style(muted())
                .block(block)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    // The details (a long command or file path) grow with their content.
    let detail = autostart_detail_lines(app.selected_autostart());
    let [list_area, detail_area] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(detail_height(&detail, area.width, area.height)),
    ])
    .areas(area);

    let delays: Vec<String> = entries
        .iter()
        .map(|entry| entry.delay.to_string())
        .collect();
    let rows = entries.iter().enumerate().map(|(index, entry)| {
        Row::new(vec![
            Cell::from(entry.name.as_str()),
            Cell::from(enabled_label(entry.enabled)).style(enabled_style(entry.enabled)),
            Cell::from(delays[index].as_str()).style(delay_style(entry.delay)),
            Cell::from(entry.exec.as_str()).style(muted()),
        ])
    });

    let header = Row::new([
        i18n::t("tui_name"),
        i18n::t("state"),
        i18n::t("delay"),
        i18n::t("komut"),
    ])
    .style(Style::new().fg(ACCENT).bold());
    let table = Table::new(
        rows,
        [
            Constraint::Min(20),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(block)
    .row_highlight_style(highlight())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.autostart_selected);
    frame.render_stateful_widget(table, list_area, &mut state);

    render_autostart_detail(frame, &detail, detail_area);
}

fn render_autostart_detail(frame: &mut Frame, lines: &[Line], area: Rect) {
    frame.render_widget(
        Paragraph::new(lines.to_vec())
            .block(panel("tui_details"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// The fields of the selected autostart entry.
fn autostart_detail_lines(entry: Option<&AutostartEntry>) -> Vec<Line<'_>> {
    match entry {
        Some(entry) => vec![
            Line::from(entry.name.as_str()).bold(),
            detail_line(i18n::t("komut"), &entry.exec),
            Line::from(vec![
                Span::styled(format!("{}: ", i18n::t("delay")), muted()),
                Span::styled(format!("{}s", entry.delay), delay_style(entry.delay)),
                Span::raw("    "),
                Span::styled(format!("{}: ", i18n::t("state")), muted()),
                Span::styled(enabled_label(entry.enabled), enabled_style(entry.enabled)),
            ]),
            detail_line(i18n::t("tui_file"), &entry.path.display().to_string()),
        ],
        None => vec![Line::from(i18n::t("tui_no_entry_selected")).style(muted())],
    }
}

/// "enabled" / "disabled" for an autostart entry.
fn enabled_label(enabled: bool) -> &'static str {
    if enabled {
        i18n::t("tui_enabled")
    } else {
        i18n::t("tui_disabled")
    }
}

fn enabled_style(enabled: bool) -> Style {
    if enabled {
        Style::new().fg(OK)
    } else {
        muted()
    }
}

/// A pinned delay is a configured value worth noticing; zero stays quiet.
fn delay_style(delay: u32) -> Style {
    if delay > 0 {
        Style::new().fg(ACCENT)
    } else {
        muted()
    }
}

/// Translated label for a systemd unit state; unknown states pass through.
fn state_label(state: &str) -> &str {
    match state {
        "active" => i18n::t("filter_active"),
        "inactive" => i18n::t("filter_inactive"),
        "failed" => i18n::t("tui_state_failed"),
        "masked" => i18n::t("filter_masked"),
        "enabled" | "enabled-runtime" => i18n::t("tui_state_enabled"),
        "disabled" => i18n::t("tui_state_disabled"),
        other => other,
    }
}

fn render_profiles(frame: &mut Frame, app: &App, area: Rect) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    // The ready-made profiles keep their own panel; the user's own profiles are
    // listed under it so both stay reachable with the same arrow keys.
    let builtin_rows = profiles::ids().len() as u16 + 3;
    let [builtin_area, custom_area] =
        Layout::vertical([Constraint::Length(builtin_rows), Constraint::Min(4)]).areas(list_area);

    render_profile_list(frame, app, builtin_area);
    render_custom_profile_list(frame, app, custom_area);
    render_profile_detail(frame, app, detail_area);
}

fn render_backups(frame: &mut Frame, app: &App, area: Rect) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    render_backup_list(frame, app, list_area);
    render_backup_detail(frame, app, detail_area);
}

fn render_profile_list(frame: &mut Frame, app: &App, area: Rect) {
    let ids = profiles::ids();
    let names: Vec<&str> = ids
        .iter()
        .map(|id| {
            profiles::data()
                .profiles
                .get(*id)
                .map_or(*id, |profile| profile.name())
        })
        .collect();
    let counts: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            let (enable, disable) = profiles::selections(id);
            (enable.len().to_string(), disable.len().to_string())
        })
        .collect();

    let rows = ids.iter().enumerate().map(|(index, _)| {
        Row::new(vec![
            Cell::from(names[index]),
            Cell::from(counts[index].0.as_str()).style(Style::new().fg(OK)),
            Cell::from(counts[index].1.as_str()).style(Style::new().fg(WARN)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Length(9),
            Constraint::Length(9),
        ],
    )
    .header(
        Row::new([
            i18n::t("tui_profile"),
            i18n::t("tui_enable_count"),
            i18n::t("tui_disable_count"),
        ])
        .style(Style::new().fg(ACCENT).bold()),
    )
    .block(panel("profiller_title"))
    .row_highlight_style(highlight())
    .highlight_symbol("> ");

    let selected = (app.profiles_selected < ids.len()).then_some(app.profiles_selected);
    let mut state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table, area, &mut state);
}

/// The profiles the user saved themselves.
fn render_custom_profile_list(frame: &mut Frame, app: &App, area: Rect) {
    let profiles = app.custom_profiles();
    let block = panel("user_custom_profilleri");

    if profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(i18n::t("no_custom_profiles"))
                .style(muted())
                .block(block)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let rows = profiles.iter().map(|profile| {
        Row::new(vec![
            Cell::from(profile.name.as_str()),
            Cell::from(profile.services.len().to_string()).style(muted()),
        ])
    });

    let table = Table::new(rows, [Constraint::Min(12), Constraint::Length(13)])
        .header(
            Row::new([i18n::t("tui_profile"), i18n::t("service_kurali")])
                .style(Style::new().fg(ACCENT).bold()),
        )
        .block(block)
        .row_highlight_style(highlight())
        .highlight_symbol("> ");

    // The cursor only shows here when it is on one of these rows.
    let selected = app.profiles_selected.checked_sub(profiles::ids().len());
    let mut state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_backup_list(frame: &mut Frame, app: &App, area: Rect) {
    let backups = app.backups();
    let counts: Vec<String> = backups
        .iter()
        .map(|backup| backup.services.len().to_string())
        .collect();

    let rows = backups.iter().enumerate().map(|(index, backup)| {
        Row::new(vec![
            Cell::from(backup.name.as_str()),
            Cell::from(counts[index].as_str()).style(muted()),
        ])
    });

    let table = Table::new(rows, [Constraint::Min(20), Constraint::Length(12)])
        .header(
            Row::new([i18n::t("tui_backup"), i18n::t("tui_services")])
                .style(Style::new().fg(ACCENT).bold()),
        )
        .block(panel("backup_noktalari"))
        .row_highlight_style(highlight())
        .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(Some(app.backups_selected));
    frame.render_stateful_widget(table, area, &mut state);
}

/// The form that makes a new custom profile: a name and one switch per
/// service the original dialog offers.
fn render_profile_form(frame: &mut Frame, app: &App) {
    let name_pointer = if app.profile_form_index == 0 {
        ">"
    } else {
        " "
    };
    let mut lines = vec![
        Line::from(i18n::t("yeni_profile_name_girin")),
        Line::from(vec![
            Span::raw(format!("{name_pointer} ")),
            Span::styled(format!("{}: ", i18n::t("profile_name_lbl")), muted()),
            Span::styled(app.profile_name.as_str(), Style::new().fg(ACCENT)),
        ]),
        Line::from(""),
        Line::from(format!(
            "{}  {}",
            i18n::t("service_kurallari_lbl"),
            i18n::t("service_kurallari_sub")
        ))
        .style(muted()),
    ];

    for (index, (_, label_key)) in custom_profiles::CANDIDATES.iter().enumerate() {
        let checked = app
            .profile_form_checked
            .get(index)
            .copied()
            .unwrap_or(false);
        let pointer = if app.profile_form_index == index + 1 {
            ">"
        } else {
            " "
        };
        let mark = if checked { "x" } else { " " };
        lines.push(Line::from(format!(
            "{pointer} [{mark}] {}",
            i18n::t(label_key)
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(i18n::t("tui_bar_profile_form")).style(muted()));

    let area = centered_rect(64, block_height(&lines, 64), frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("yeni_custom_profile_btn"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_profile_detail(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(profile_detail_lines(app))
            .block(panel("tui_details"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// The card of the highlighted profile, ready-made or the user's own.
fn profile_detail_lines(app: &App) -> Vec<Line<'_>> {
    if let Some(profile) = app.selected_custom_profile() {
        return custom_profile_lines(app, profile);
    }

    let Some(id) = app.selected_profile() else {
        return vec![Line::from(i18n::t("tui_no_profile_selected")).style(muted())];
    };

    let profile = profiles::data().profiles.get(id);
    let name = profile.map_or(id, |profile| profile.name());
    let description = profile.map_or("", |profile| profile.description());

    let mut lines = vec![
        Line::from(i18n::fill(i18n::t("tui_profile_label"), &[name])).bold(),
        Line::from(description).style(muted()),
        Line::from(""),
        Line::from(savings_line(app.profile_savings(id)))
            .style(savings_style(app.profile_savings(id))),
        Line::from(""),
    ];

    // The same ✔/✘ list the profile cards show in the GTK application.
    for line in checklist_lines(profiles::checklist(id)) {
        lines.push(line);
    }
    lines
}

/// The card of a user-created profile.
fn custom_profile_lines<'a>(app: &'a App, profile: &'a CustomProfile) -> Vec<Line<'a>> {
    let savings = app.custom_profile_savings(profile);
    let mut lines = vec![
        Line::from(profile.name.as_str()).bold(),
        Line::from(format!(
            "{} {}",
            profile.services.len(),
            i18n::t("service_kurali")
        ))
        .style(muted()),
        Line::from(""),
        Line::from(savings_line(savings)).style(savings_style(savings)),
        Line::from(""),
        detail_line(i18n::t("tui_file"), &profile.path.display().to_string()),
    ];

    for line in checklist_lines(profiles::checklist_of(profile.services.iter())) {
        lines.push(line);
    }
    lines
}

/// The ✔/✘ lines of a profile's service rules.
fn checklist_lines<'a>(items: Vec<(bool, &'a str)>) -> Vec<Line<'a>> {
    items
        .into_iter()
        .map(|(enable, label)| {
            let (mark, style) = if enable {
                ("✔", Style::new().fg(OK).bold())
            } else {
                ("✘", Style::new().fg(BAD).bold())
            };
            Line::from(vec![
                Span::styled(mark, style),
                Span::raw(format!("  {label}")),
            ])
        })
        .collect()
}

fn render_backup_detail(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.selected_backup() {
        Some(backup) => vec![
            Line::from(backup.name.as_str()).bold(),
            Line::from(i18n::fill(i18n::t("tui_saved"), &[&backup.timestamp])).style(muted()),
            detail_line(i18n::t("tui_services"), &backup.services.len().to_string()),
            detail_line(i18n::t("tui_file"), &backup.path.display().to_string()),
        ],
        None => vec![Line::from(i18n::t("tui_create_backup_hint")).style(muted())],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("tui_details"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// Estimated savings read green when there is something to win.
fn savings_style(seconds: f64) -> Style {
    if seconds > 0.05 {
        Style::new().fg(OK).bold()
    } else {
        muted()
    }
}

/// `"Estimated Savings: ~1.5s"`, or `< 0.1s` when it is negligible.
fn savings_line(seconds: f64) -> String {
    if seconds > 0.05 {
        format!("{}: ~{seconds:.1}s", i18n::t("tahmini_saving"))
    } else {
        format!("{}: < 0.1s", i18n::t("tahmini_saving"))
    }
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let [keys, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

    frame.render_widget(Paragraph::new(action_hints(app)).style(muted()), keys);

    // The status line shows the latest command result when there is one.
    let (status_text, style) = match (&app.message, app.running) {
        (Some(message), _) => (
            message.as_str(),
            match app.level {
                Level::Success => Style::new().fg(OK),
                Level::Error => Style::new().fg(BAD).bold(),
                Level::Info => Style::new(),
            },
        ),
        (None, true) => (i18n::t("tui_running"), Style::new().fg(WARN)),
        (None, false) => (i18n::t("tui_bar_global"), muted()),
    };

    frame.render_widget(Paragraph::new(status_text).style(style), status);
}

/// The keys that do something right now, so actions are discoverable.
fn action_hints(app: &App) -> &'static str {
    match app.overlay {
        Overlay::Menu => return i18n::t("tui_bar_menu"),
        Overlay::Confirm => return i18n::t("tui_bar_confirm"),
        Overlay::Password => return i18n::t("tui_bar_password"),
        Overlay::Form => return i18n::t("tui_bar_form"),
        Overlay::Delay => return i18n::t("tui_bar_delay"),
        Overlay::ProfileForm => return i18n::t("tui_bar_profile_form"),
        Overlay::Language => return i18n::t("tui_bar_language"),
        Overlay::None => {}
    }

    if app.info.is_some() {
        return i18n::t("tui_bar_info");
    }

    match app.tab {
        Tab::Analysis => i18n::t("tui_bar_analysis"),
        Tab::Services => i18n::t("tui_bar_services"),
        Tab::Autostart => i18n::t("tui_bar_autostart"),
        Tab::Profiles => i18n::t("tui_bar_profiles"),
        Tab::Backups => i18n::t("tui_bar_backups"),
    }
}

fn render_overlay(frame: &mut Frame, app: &App) {
    match app.overlay {
        Overlay::None => {}
        Overlay::Menu => render_action_menu(frame, app),
        Overlay::Confirm => render_confirm(frame, app),
        Overlay::Password => render_password(frame, app),
        Overlay::Form => render_form(frame, app),
        Overlay::ProfileForm => render_profile_form(frame, app),
        Overlay::Delay => render_delay(frame, app),
        Overlay::Language => render_language(frame, app),
    }
}

/// The language picker: a small menu like the action menu.
fn render_language(frame: &mut Frame, app: &App) {
    let area = centered_rect(30, Lang::ALL.len() as u16 + 2, frame.area());
    frame.render_widget(Clear, area);

    let items = Lang::ALL.iter().map(|lang| Line::from(lang.native_name()));
    let list = List::new(items)
        .block(panel("tui_language_title"))
        .highlight_style(highlight())
        .highlight_symbol("> ");

    let mut state = ListState::default().with_selected(Some(app.language_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_action_menu(frame: &mut Frame, app: &App) {
    let unit = app
        .selected_service()
        .map(|service| service.name.as_str())
        .unwrap_or(i18n::t("service_name"));

    let area = centered_rect(40, Action::ALL.len() as u16 + 2, frame.area());
    frame.render_widget(Clear, area);

    let items = Action::ALL.iter().map(|action| Line::from(action.label()));
    let list = List::new(items)
        .block(panel_titled(format!(" {unit} ")))
        .highlight_style(highlight())
        .highlight_symbol("> ");

    let mut state = ListState::default().with_selected(Some(app.menu_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_confirm(frame: &mut Frame, app: &App) {
    let prompt = app.confirmation_prompt();
    let keys = i18n::t("tui_confirm_keys");

    // The question and the key hint wrap on narrow dialogs, so size the box to
    // the rows they need instead of clipping the hint away.
    let width = frame.area().width.min(50);
    let inner_width = width.saturating_sub(2).max(1) as usize;
    let keys_rows = wrapped_rows(keys, inner_width) as u16;
    let height =
        (wrapped_rows(&prompt, inner_width) as u16 + keys_rows + 3).min(frame.area().height);

    let area = centered_rect(width, height, frame.area());
    frame.render_widget(Clear, area);

    let block = panel("tui_confirm");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [prompt_area, _, keys_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(keys_rows),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(prompt).wrap(Wrap { trim: true }),
        prompt_area,
    );
    frame.render_widget(Paragraph::new(keys).style(muted()), keys_area);
}

fn render_form(frame: &mut Frame, app: &App) {
    let labels = [i18n::t("tui_name"), i18n::t("komut")];

    let mut lines = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        let value = app.form_values.get(index).map(String::as_str).unwrap_or("");
        let marker = if index == app.form_index { ">" } else { " " };
        lines.push(Line::from(format!("{marker} {label}: {value}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(i18n::t("tui_bar_form")).style(muted()));

    let area = centered_rect(60, block_height(&lines, 60), frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("tui_new_autostart"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_password(frame: &mut Frame, app: &App) {
    let masked = "*".repeat(app.password_input.chars().count());
    let lines = vec![
        Line::from(i18n::t("tui_sudo_password")),
        Line::from(masked).bold(),
        Line::from(""),
        Line::from(i18n::t("tui_password_keys")).style(muted()),
    ];

    let area = centered_rect(40, block_height(&lines, 40), frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("tui_authentication"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_delay(frame: &mut Frame, app: &App) {
    let lines = vec![
        Line::from(i18n::t("tui_delay_prompt")),
        Line::from(format!("{}s", app.delay_input)).bold(),
        Line::from(""),
        Line::from(i18n::t("tui_delay_keys")).style(muted()),
    ];

    let area = centered_rect(40, block_height(&lines, 40), frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("delay"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_info(frame: &mut Frame, app: &App) {
    let Some(info) = &app.info else {
        return;
    };

    let full = frame.area();
    let area = centered_rect(
        full.width.saturating_sub(4),
        full.height.saturating_sub(2),
        full,
    );
    frame.render_widget(Clear, area);

    let text: Vec<Line> = info
        .lines
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect();
    let paragraph = Paragraph::new(text)
        .block(panel_titled(format!(
            " {} — {} ",
            info.title,
            i18n::t("tui_info_hint")
        )))
        .scroll((info.scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Plain text of a styled line, used to measure it.
fn line_text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// Height a bordered block needs to draw `lines` at `width` without clipping
/// any of them, the two border rows included.
fn block_height(lines: &[Line], width: u16) -> u16 {
    let inner = width.saturating_sub(2).max(1) as usize;
    let rows: usize = lines
        .iter()
        .map(|line| wrapped_rows(&line_text(line), inner))
        .sum();
    rows as u16 + 2
}

/// Height a detail pane should take: as many rows as its content needs, but
/// never so many that the list above it is squeezed away.
fn detail_height(lines: &[Line], width: u16, area_height: u16) -> u16 {
    block_height(lines, width)
        .min(area_height.saturating_sub(4))
        .max(4)
}

/// How many rows `text` needs when wrapped into `width` columns.
///
/// The greedy word wrap `Paragraph` draws with `Wrap { trim: true }`: text
/// breaks at whitespace, runs of spaces stay inside a row (they are not
/// collapsed), and a word wider than a whole row is split.
fn wrapped_rows(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }

    let mut rows = 1;
    let mut column = 0;
    let mut gap = 0;

    for part in text.split(' ') {
        if part.is_empty() {
            gap += 1;
            continue;
        }

        let length = part.chars().count();
        if column == 0 {
            column = length;
        } else if column + gap + length <= width {
            column += gap + length;
        } else {
            rows += 1;
            column = length;
        }

        // A single word can still be wider than the whole row.
        while column > width {
            rows += 1;
            column -= width;
        }

        gap = 1;
    }

    rows
}

/// A rectangle of the given size, centred inside `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn status_text(app: &App) -> &'static str {
    if app.loading {
        i18n::t("tui_loading")
    } else {
        i18n::t("tui_no_data")
    }
}

#[cfg(test)]
mod tests;
