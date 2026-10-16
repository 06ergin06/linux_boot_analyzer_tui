use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Clear, List, ListState, Paragraph, Row, Table, TableState, Tabs, Wrap,
};

use crate::app::{App, Overlay, ProfilesView, Tab};
use crate::autostart::AutostartEntry;
use crate::i18n;
use crate::privilege::Action;
use crate::system::Service;
use crate::{profiles, service_db};

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
        .block(Block::bordered().title(" Linux Boot Analyzer "))
        .select(app.tab.index())
        .highlight_style(Style::new().bold().underlined());

    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    match app.tab {
        Tab::Analysis => render_analysis(frame, app, area),
        Tab::Services => render_services(frame, app, area),
        Tab::Autostart => render_autostart(frame, app, area),
        Tab::Profiles => render_profiles(frame, app, area),
    }
}

fn render_analysis(frame: &mut Frame, app: &App, area: Rect) {
    let content = app.analysis_lines();
    let lines: Vec<Line> = content
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title("tab_analysis")))
            .wrap(Wrap { trim: false })
            .scroll((app.analysis_scroll, 0)),
        area,
    );
}

/// A bordered block title for a translated label.
fn title(key: &str) -> String {
    format!(" {} ", i18n::t(key))
}

fn render_services(frame: &mut Frame, app: &App, area: Rect) {
    let Some(data) = &app.data else {
        frame.render_widget(
            Paragraph::new(status_text(app)).block(Block::bordered().title(title("tab_services"))),
            area,
        );
        return;
    };

    if let Err(err) = &data.services {
        frame.render_widget(
            Paragraph::new(err.as_str())
                .block(Block::bordered().title(title("tab_services")))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let visible = app.visible_services();
    let times = app.blame_times();

    let [filter_area, table_area, detail_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(6),
    ])
    .areas(area);

    render_filter_bar(frame, app, filter_area);

    let header = Row::new([
        i18n::t("service_name"),
        i18n::t("filter_active"),
        i18n::t("tui_boot_config"),
        i18n::t("time"),
        i18n::t("tui_tip"),
    ])
    .style(Style::new().bold());
    let rows = visible.iter().map(|service| {
        let tip = service_db::lookup(&service.name)
            .map(|entry| entry.tip.label())
            .unwrap_or("-");
        let time = times.get(service.name.as_str()).copied().unwrap_or("-");
        Row::new([
            service.name.as_str(),
            state_label(&service.active),
            state_label(&service.enabled),
            time,
            tip,
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Min(30),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(Block::bordered().title(title("tab_services")))
    .row_highlight_style(Style::new().reversed())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.selected);
    frame.render_stateful_widget(table, table_area, &mut state);

    render_service_detail(frame, visible.get(app.selected).copied(), detail_area);
}

fn render_filter_bar(frame: &mut Frame, app: &App, area: Rect) {
    let search = if app.searching || !app.query.is_empty() {
        format!("/{}", app.query)
    } else {
        format!("/ {}", i18n::t("tui_search"))
    };
    let line = format!(
        "f: {}={}   t: {}={}   s: {}={}   {}",
        i18n::t("tui_status"),
        app.status_filter.label(),
        i18n::t("tui_tip_filter"),
        app.tip_filter.label(),
        i18n::t("tui_sort"),
        app.sort.label(),
        search,
    );

    frame.render_widget(Paragraph::new(line).style(Style::new().dim()), area);
}

fn render_service_detail(frame: &mut Frame, service: Option<&Service>, area: Rect) {
    let lines = match service {
        Some(service) => match service_db::lookup(&service.name) {
            Some(entry) => vec![
                Line::from(format!("{} ({})", service.name, entry.tip.label())),
                Line::from(""),
                Line::from(entry.description()),
                Line::from(entry.suggestion_text()).dim(),
            ],
            None => vec![
                Line::from(service.name.as_str()),
                Line::from(i18n::t("tui_no_kb_entry")),
            ],
        },
        None => vec![Line::from(i18n::t("tui_no_service_selected"))],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title("tui_details")))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_autostart(frame: &mut Frame, app: &App, area: Rect) {
    let entries = app.autostart_entries();
    let block = Block::bordered().title(title("tab_autostart"));

    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(i18n::t("no_autostart_apps"))
                .block(block)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let [list_area, detail_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(6)]).areas(area);

    let delays: Vec<String> = entries
        .iter()
        .map(|entry| entry.delay.to_string())
        .collect();
    let rows = entries.iter().enumerate().map(|(index, entry)| {
        Row::new([
            entry.name.as_str(),
            enabled_label(entry.enabled),
            delays[index].as_str(),
            entry.exec.as_str(),
        ])
    });

    let header = Row::new([
        i18n::t("tui_name"),
        i18n::t("state"),
        i18n::t("delay"),
        i18n::t("komut"),
    ])
    .style(Style::new().bold());
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
    .row_highlight_style(Style::new().reversed())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.autostart_selected);
    frame.render_stateful_widget(table, list_area, &mut state);

    render_autostart_detail(frame, app.selected_autostart(), detail_area);
}

fn render_autostart_detail(frame: &mut Frame, entry: Option<&AutostartEntry>, area: Rect) {
    let lines = match entry {
        Some(entry) => vec![
            Line::from(entry.name.as_str()),
            Line::from(format!("{} {}", i18n::t("komut"), entry.exec)),
            Line::from(format!(
                "{}: {}s    {}: {}",
                i18n::t("delay"),
                entry.delay,
                i18n::t("state"),
                enabled_label(entry.enabled)
            )),
            Line::from(format!("{} {}", i18n::t("tui_file"), entry.path.display())),
        ],
        None => vec![Line::from(i18n::t("tui_no_entry_selected"))],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title("tui_details")))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// "enabled" / "disabled" for an autostart entry.
fn enabled_label(enabled: bool) -> &'static str {
    if enabled {
        i18n::t("tui_enabled")
    } else {
        i18n::t("tui_disabled")
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
    let [lists, detail] = Layout::vertical([Constraint::Min(0), Constraint::Length(9)]).areas(area);
    let [profiles_area, backups_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(lists);

    render_profile_list(frame, app, profiles_area);
    render_backup_list(frame, app, backups_area);
    render_profile_detail(frame, app, detail);
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
        Row::new([
            names[index],
            counts[index].0.as_str(),
            counts[index].1.as_str(),
        ])
    });

    let block_title = if app.profiles_view == ProfilesView::Profiles {
        format!(" {} {} ", i18n::t("profiller_title"), i18n::t("tui_active"))
    } else {
        title("profiller_title")
    };

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Length(12),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new([
            i18n::t("tui_profile"),
            i18n::t("tui_enable_count"),
            i18n::t("tui_disable_count"),
        ])
        .style(Style::new().bold()),
    )
    .block(Block::bordered().title(block_title))
    .row_highlight_style(Style::new().reversed())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(Some(app.profiles_selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_backup_list(frame: &mut Frame, app: &App, area: Rect) {
    let backups = app.backups();
    let counts: Vec<String> = backups
        .iter()
        .map(|backup| backup.services.len().to_string())
        .collect();

    let rows = backups
        .iter()
        .enumerate()
        .map(|(index, backup)| Row::new([backup.name.as_str(), counts[index].as_str()]));

    let block_title = if app.profiles_view == ProfilesView::Backups {
        format!(
            " {} {} ",
            i18n::t("backup_noktalari"),
            i18n::t("tui_active")
        )
    } else {
        title("backup_noktalari")
    };

    let table = Table::new(rows, [Constraint::Min(20), Constraint::Length(12)])
        .header(
            Row::new([i18n::t("tui_backup"), i18n::t("tui_services")]).style(Style::new().bold()),
        )
        .block(Block::bordered().title(block_title))
        .row_highlight_style(Style::new().reversed())
        .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(Some(app.backups_selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_profile_detail(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.profiles_view {
        ProfilesView::Profiles => match app.selected_profile() {
            Some(id) => {
                let profile = profiles::data().profiles.get(id);
                let name = profile.map_or(id, |profile| profile.name());
                let description = profile.map_or("", |profile| profile.description());
                let (enable, disable) = profiles::selections(id);
                vec![
                    Line::from(i18n::fill(i18n::t("tui_profile_label"), &[name])),
                    Line::from(description),
                    Line::from(""),
                    Line::from(format!(
                        "{}: {}",
                        i18n::t("tui_enable_count"),
                        join_or_dash(&enable)
                    )),
                    Line::from(format!(
                        "{}: {}",
                        i18n::t("tui_disable_count"),
                        join_or_dash(&disable)
                    )),
                ]
            }
            None => vec![Line::from(i18n::t("tui_no_profile_selected"))],
        },
        ProfilesView::Backups => match app.selected_backup() {
            Some(backup) => vec![
                Line::from(backup.name.as_str()),
                Line::from(format!("{} {}", i18n::t("tui_saved"), backup.timestamp)),
                Line::from(format!(
                    "{}: {}",
                    i18n::t("tui_services"),
                    backup.services.len()
                )),
                Line::from(format!("{} {}", i18n::t("tui_file"), backup.path.display())),
            ],
            None => vec![Line::from(i18n::t("tui_create_backup_hint"))],
        },
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title("tui_details")))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn join_or_dash(names: &[String]) -> String {
    if names.is_empty() {
        "-".to_string()
    } else {
        names.join(", ")
    }
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let [keys, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

    frame.render_widget(
        Paragraph::new(action_hints(app)).style(Style::new().dim()),
        keys,
    );

    // The status line shows the latest command result when there is one.
    let status_text: &str = match (&app.message, app.running) {
        (Some(message), _) => message,
        (None, true) => i18n::t("tui_running"),
        (None, false) => i18n::t("tui_bar_global"),
    };

    frame.render_widget(Paragraph::new(status_text), status);
}

/// The keys that do something right now, so actions are discoverable.
fn action_hints(app: &App) -> &'static str {
    match app.overlay {
        Overlay::Menu => return i18n::t("tui_bar_menu"),
        Overlay::Confirm => return i18n::t("tui_bar_confirm"),
        Overlay::Password => return i18n::t("tui_bar_password"),
        Overlay::Form => return i18n::t("tui_bar_form"),
        Overlay::None => {}
    }

    if app.info.is_some() {
        return i18n::t("tui_bar_info");
    }

    match app.tab {
        Tab::Analysis => i18n::t("tui_bar_analysis"),
        Tab::Services => i18n::t("tui_bar_services"),
        Tab::Autostart => i18n::t("tui_bar_autostart"),
        Tab::Profiles => match app.profiles_view {
            ProfilesView::Profiles => i18n::t("tui_bar_profiles"),
            ProfilesView::Backups => i18n::t("tui_bar_backups"),
        },
    }
}

fn render_overlay(frame: &mut Frame, app: &App) {
    match app.overlay {
        Overlay::None => {}
        Overlay::Menu => render_action_menu(frame, app),
        Overlay::Confirm => render_confirm(frame, app),
        Overlay::Password => render_password(frame, app),
        Overlay::Form => render_form(frame, app),
    }
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
        .block(Block::bordered().title(format!(" {unit} ")))
        .highlight_style(Style::new().reversed())
        .highlight_symbol("> ");

    let mut state = ListState::default().with_selected(Some(app.menu_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_confirm(frame: &mut Frame, app: &App) {
    let area = centered_rect(50, 5, frame.area());
    frame.render_widget(Clear, area);

    let text = vec![
        Line::from(app.confirmation_prompt()),
        Line::from(""),
        Line::from(i18n::t("tui_confirm_keys")).dim(),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(title("tui_confirm")))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_form(frame: &mut Frame, app: &App) {
    let labels = [i18n::t("tui_name"), i18n::t("komut")];

    let area = centered_rect(60, labels.len() as u16 + 4, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        let value = app.form_values.get(index).map(String::as_str).unwrap_or("");
        let marker = if index == app.form_index { ">" } else { " " };
        lines.push(Line::from(format!("{marker} {label}: {value}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(i18n::t("tui_bar_form")).dim());

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title("tui_new_autostart")))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_password(frame: &mut Frame, app: &App) {
    let area = centered_rect(40, 5, frame.area());
    frame.render_widget(Clear, area);

    let masked = "*".repeat(app.password_input.chars().count());
    let text = vec![
        Line::from(i18n::t("tui_sudo_password")),
        Line::from(masked),
        Line::from(""),
        Line::from(i18n::t("tui_password_keys")).dim(),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(title("tui_authentication")))
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
        .block(Block::bordered().title(format!(" {} — {} ", info.title, i18n::t("tui_info_hint"))))
        .scroll((info.scroll, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
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
