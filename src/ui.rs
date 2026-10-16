use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Tabs, Wrap};

use crate::app::{App, Tab};

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
        .block(Block::bordered().title(" Linux Açılış Analizi "))
        .select(app.tab.index())
        .highlight_style(Style::new().bold().underlined());

    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    let message = match app.tab {
        Tab::Analysis => "Açılış süresi analizi burada gösterilecek.",
        Tab::Services => "Systemd servisleri ve optimizasyon önerileri burada listelenecek.",
        Tab::Autostart => "Oturum başlangıcında çalışan uygulamalar burada listelenecek.",
        Tab::Profiles => "Hazır optimizasyon profilleri burada gösterilecek.",
    };

    let body = Paragraph::new(message)
        .block(Block::bordered().title(format!(" {} ", app.tab.title())))
        .wrap(Wrap { trim: true });

    frame.render_widget(body, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let footer =
        Paragraph::new("q / Esc: çıkış    ← → veya Tab: sekme değiştir").style(Style::new().dim());

    frame.render_widget(footer, area);
}
