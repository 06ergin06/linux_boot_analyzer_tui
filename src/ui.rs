use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Clear, List, ListState, Paragraph, Row, Table, TableState, Tabs, Wrap,
};

use crate::app::{App, Overlay, ProfilesView, Tab};
use crate::autostart::AutostartEntry;
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
            .block(Block::bordered().title(" Analysis "))
            .wrap(Wrap { trim: false })
            .scroll((app.analysis_scroll, 0)),
        area,
    );
}

fn render_services(frame: &mut Frame, app: &App, area: Rect) {
    let Some(data) = &app.data else {
        frame.render_widget(
            Paragraph::new(status_text(app)).block(Block::bordered().title(" Services ")),
            area,
        );
        return;
    };

    if let Err(err) = &data.services {
        frame.render_widget(
            Paragraph::new(err.as_str())
                .block(Block::bordered().title(" Services "))
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

    let header =
        Row::new(["Service", "Active", "Enabled", "Time", "Tip"]).style(Style::new().bold());
    let rows = visible.iter().map(|service| {
        let tip = service_db::lookup(&service.name)
            .map(|entry| entry.tip.label())
            .unwrap_or("-");
        let time = times.get(service.name.as_str()).copied().unwrap_or("-");
        Row::new([
            service.name.as_str(),
            service.active.as_str(),
            service.enabled.as_str(),
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
    .block(Block::bordered().title(" Services "))
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
        "/ search".to_string()
    };
    let line = format!(
        "f: status={}   t: tip={}   s: sort={}   {}",
        app.status_filter.label(),
        app.tip_filter.label(),
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
                Line::from(entry.desc.as_str()),
                Line::from(entry.suggestion.as_str()).dim(),
            ],
            None => vec![
                Line::from(service.name.as_str()),
                Line::from("No knowledge-base entry for this service."),
            ],
        },
        None => vec![Line::from("No service selected.")],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Details "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_autostart(frame: &mut Frame, app: &App, area: Rect) {
    let entries = app.autostart_entries();
    let block = Block::bordered().title(" Autostart ");

    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No autostart entries. Press 'n' to add one.")
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
            if entry.enabled { "enabled" } else { "disabled" },
            delays[index].as_str(),
            entry.exec.as_str(),
        ])
    });

    let header = Row::new(["Name", "State", "Delay", "Command"]).style(Style::new().bold());
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
            Line::from(format!("Command: {}", entry.exec)),
            Line::from(format!(
                "Delay: {}s    Enabled: {}",
                entry.delay, entry.enabled
            )),
            Line::from(format!("File: {}", entry.path.display())),
            Line::from("space: toggle   [ ]: delay   x: remove").dim(),
        ],
        None => vec![Line::from("No entry selected.")],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Details "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_profiles(frame: &mut Frame, app: &App, area: Rect) {
    let [lists, detail] = Layout::vertical([Constraint::Min(0), Constraint::Length(7)]).areas(area);
    let [profiles_area, backups_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(lists);

    render_profile_list(frame, app, profiles_area);
    render_backup_list(frame, app, backups_area);
    render_profile_detail(frame, app, detail);
}

fn render_profile_list(frame: &mut Frame, app: &App, area: Rect) {
    let ids = profiles::ids();
    let counts: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            let (enable, disable) = profiles::selections(id);
            (enable.len().to_string(), disable.len().to_string())
        })
        .collect();

    let rows = ids
        .iter()
        .enumerate()
        .map(|(index, id)| Row::new([*id, counts[index].0.as_str(), counts[index].1.as_str()]));

    let title = if app.profiles_view == ProfilesView::Profiles {
        " Profiles (active) "
    } else {
        " Profiles "
    };

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Length(8),
            Constraint::Length(8),
        ],
    )
    .header(Row::new(["Profile", "Enable", "Disable"]).style(Style::new().bold()))
    .block(Block::bordered().title(title))
    .row_highlight_style(Style::new().reversed())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.profiles_selected);
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

    let title = if app.profiles_view == ProfilesView::Backups {
        " Restore Points (active) "
    } else {
        " Restore Points "
    };

    let table = Table::new(rows, [Constraint::Min(20), Constraint::Length(8)])
        .header(Row::new(["Backup", "Services"]).style(Style::new().bold()))
        .block(Block::bordered().title(title))
        .row_highlight_style(Style::new().reversed())
        .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.backups_selected);
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_profile_detail(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match app.profiles_view {
        ProfilesView::Profiles => match app.selected_profile() {
            Some(id) => {
                let (enable, disable) = profiles::selections(id);
                vec![
                    Line::from(format!("Profile: {id}")),
                    Line::from(format!("Enables:  {}", join_or_dash(&enable))),
                    Line::from(format!("Disables: {}", join_or_dash(&disable))),
                    Line::from("Enter/a: apply profile   v: show restore points").dim(),
                ]
            }
            None => vec![Line::from("No profile selected.")],
        },
        ProfilesView::Backups => match app.selected_backup() {
            Some(backup) => vec![
                Line::from(backup.name.as_str()),
                Line::from(format!("Saved: {}", backup.timestamp)),
                Line::from(format!("Services: {}", backup.services.len())),
                Line::from(format!("File: {}", backup.path.display())),
                Line::from("d: restore   x: delete   b: create new   v: show profiles").dim(),
            ],
            None => vec![Line::from(
                "No restore point yet. Press 'b' to create one.".to_string(),
            )],
        },
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Details "))
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
        (None, true) => "Running...",
        (None, false) => "q: quit    Up/Down: move    Left/Right: tab    ?: help",
    };

    frame.render_widget(Paragraph::new(status_text), status);
}

/// The keys that do something right now, so actions are discoverable.
fn action_hints(app: &App) -> &'static str {
    match app.overlay {
        Overlay::Menu => return "Enter: choose    Up/Down: move    Esc: cancel",
        Overlay::Confirm => return "y: confirm    n or Esc: cancel",
        Overlay::Password => return "Enter: run    Esc: cancel",
        Overlay::Form => return "Tab: next field    Enter: create    Esc: cancel",
        Overlay::None => {}
    }

    if app.info.is_some() {
        return "Up/Down or j/k: scroll    PageUp/PageDown: page    Esc: close";
    }

    match app.tab {
        Tab::Analysis => "o: disable suggested services    w: write report    r: reload",
        Tab::Services => {
            "Enter/a: actions (enable, disable, start, stop, mask)    l: journal    d/D: dependencies    /: search    f/t/s: filters    r: reload"
        }
        Tab::Autostart => {
            "space: enable/disable    [ ]: delay -/+1s    n: add    x: remove    r: reload"
        }
        Tab::Profiles => match app.profiles_view {
            ProfilesView::Profiles => {
                "Enter/a: apply profile    b: create restore point    v: show restore points    r: reload"
            }
            ProfilesView::Backups => {
                "d: restore    x: delete    b: create restore point    v: show profiles    r: reload"
            }
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
        .unwrap_or("service");

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
        Line::from("y / Enter to confirm, n / Esc to cancel").dim(),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(" Confirm "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_form(frame: &mut Frame, app: &App) {
    const LABELS: [&str; 2] = ["Name", "Command"];

    let area = centered_rect(60, LABELS.len() as u16 + 4, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    for (index, label) in LABELS.iter().enumerate() {
        let value = app.form_values.get(index).map(String::as_str).unwrap_or("");
        let marker = if index == app.form_index { ">" } else { " " };
        lines.push(Line::from(format!("{marker} {label}: {value}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Tab: next field   Enter: create   Esc: cancel").dim());

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" New Autostart Entry "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_password(frame: &mut Frame, app: &App) {
    let area = centered_rect(40, 5, frame.area());
    frame.render_widget(Clear, area);

    let masked = "*".repeat(app.password_input.chars().count());
    let text = vec![
        Line::from("sudo password:"),
        Line::from(masked),
        Line::from(""),
        Line::from("Enter to run, Esc to cancel").dim(),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(" Authentication "))
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
        .block(Block::bordered().title(format!(" {} — Esc: close, j/k: scroll ", info.title)))
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
        "Loading..."
    } else {
        "No data available."
    }
}

#[cfg(test)]
mod tests;
