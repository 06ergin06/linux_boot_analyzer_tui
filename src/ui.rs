use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Tabs, Wrap};

use crate::app::{App, Tab};
use crate::system::Service;
use crate::{profiles, service_db};

/// Draw the whole interface for one frame.
pub fn render(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_tabs(frame, app, header);
    render_body(frame, app, body);
    render_footer(frame, footer);
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
        Tab::Autostart => render_placeholder(
            frame,
            app.tab,
            area,
            "Session autostart entries will be listed here.",
        ),
        Tab::Profiles => render_profiles(frame, area),
    }
}

fn render_analysis(frame: &mut Frame, app: &App, area: Rect) {
    let [info, boot, blame] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Min(0),
    ])
    .areas(area);

    render_system_info(frame, app, info);
    render_boot(frame, app, boot);
    render_blame(frame, app, blame);
}

fn render_system_info(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match &app.data {
        Some(data) => vec![
            Line::from(format!("OS:      {}", data.info.os)),
            Line::from(format!("Kernel:  {}", data.info.kernel)),
            Line::from(format!("Memory:  {}", data.info.ram)),
            Line::from(format!("Uptime:  {}", data.info.uptime)),
        ],
        None => vec![Line::from(status_text(app))],
    };

    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" System ")),
        area,
    );
}

fn render_boot(frame: &mut Frame, app: &App, area: Rect) {
    let lines = match &app.data {
        Some(data) => match &data.boot {
            Ok(boot) => vec![
                Line::from(format!("Total: {}", boot.total)),
                Line::from(boot.detail.as_str()).dim(),
            ],
            Err(err) => vec![Line::from(err.as_str())],
        },
        None => vec![Line::from(status_text(app))],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Boot Time "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_blame(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = match &app.data {
        Some(data) => match &data.blame {
            Ok(entries) => entries
                .iter()
                .take(20)
                .map(|entry| Line::from(format!("{:>10}  {}", entry.time, entry.name)))
                .collect(),
            Err(err) => vec![Line::from(err.as_str())],
        },
        None => vec![Line::from(status_text(app))],
    };

    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Slowest Units ")),
        area,
    );
}

fn render_services(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" Services ");
    let Some(data) = &app.data else {
        frame.render_widget(Paragraph::new(status_text(app)).block(block), area);
        return;
    };

    let services = match &data.services {
        Ok(services) => services,
        Err(err) => {
            frame.render_widget(
                Paragraph::new(err.as_str())
                    .block(block)
                    .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
    };

    let header = Row::new(["Service", "Active", "Enabled", "Tip"]).style(Style::new().bold());
    let rows = services.iter().map(|service| {
        let tip = service_db::lookup(&service.name)
            .map(|entry| entry.tip.label())
            .unwrap_or("-");
        Row::new([
            service.name.as_str(),
            service.active.as_str(),
            service.enabled.as_str(),
            tip,
        ])
    });

    let [table_area, detail_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(6)]).areas(area);

    let table = Table::new(
        rows,
        [
            Constraint::Min(30),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(block)
    .row_highlight_style(Style::new().reversed())
    .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(app.selected);
    frame.render_stateful_widget(table, table_area, &mut state);

    render_service_detail(frame, services.get(app.selected), detail_area);
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

fn render_profiles(frame: &mut Frame, area: Rect) {
    let profiles = profiles::data();
    let mut ids: Vec<&String> = profiles.profiles.keys().collect();
    ids.sort();

    let mut lines = vec![Line::from("Available optimization profiles:")];
    for id in ids {
        let count = profiles.profiles[id].services.len();
        lines.push(Line::from(format!("  - {id} ({count} services)")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Applying profiles is not implemented yet.").dim());

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Profiles "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_placeholder(frame: &mut Frame, tab: Tab, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(format!(" {} ", tab.title())))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let footer =
        Paragraph::new("q/Esc: quit    Left/Right or Tab: switch tab    Up/Down: move in list")
            .style(Style::new().dim());

    frame.render_widget(footer, area);
}

fn status_text(app: &App) -> &'static str {
    if app.loading {
        "Loading..."
    } else {
        "No data available."
    }
}

#[cfg(test)]
mod tests {
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
}
