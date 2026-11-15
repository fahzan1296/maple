use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::app::{App, Mode};

/// Transcript rows kept in the render buffer (bounded, like the session scan).
const TRANSCRIPT_BUDGET: usize = 500;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_transcript(f, app, chunks[1]);
    draw_input(f, app, chunks[2]);
    draw_statusline(f, app, chunks[3]);
}

fn draw_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = format!("maple — {}", &app.session_id()[..8]);
    let lines = vec![
        Line::from(format!("cwd    {}", app.cwd())),
        Line::from(format!("model  {}", app.model)),
        Line::from(format!("pid    {}", std::process::id())),
    ];
    let block = Block::default().borders(Borders::ALL).title(title);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_transcript(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let start = app.entries.len().saturating_sub(TRANSCRIPT_BUDGET);
    let mut lines: Vec<Line> = Vec::new();
    for entry in &app.entries[start..] {
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{}]", &entry.timestamp[..entry.timestamp.len().min(19)]),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(format!(" {:<8} ", &entry.kind)),
            Span::raw(summary(entry)),
        ]));
    }
    let block = Block::default().borders(Borders::ALL).title("transcript");
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        area,
    );
}

fn summary(entry: &crate::session::types::SessionEntry) -> String {
    match entry.kind.as_str() {
        "message" => entry
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => entry.payload.to_string(),
    }
}

fn draw_input(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = match app.mode {
        Mode::Input => "input  (Enter send · Esc cancel)",
        Mode::Normal => "input  (i edit)",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(match app.mode {
            Mode::Input => Style::default().fg(Color::Green),
            Mode::Normal => Style::default(),
        });
    let text = if app.input.is_empty() {
        Line::from(Span::styled(
            "type a message…",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(app.input.as_str())
    };
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_statusline(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let hints = "q quit · i input · n new session · j/k scroll";
    let line = Line::from(vec![
        Span::styled(hints, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(
            app.status.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
