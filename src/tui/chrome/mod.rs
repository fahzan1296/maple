//! Frame furniture: composer box and omp-style statusline.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::theme::{self, Tone};

use super::app::{App, Mode};

/// Session id short form (`short(id)` in omp rows).
pub fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

pub fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.mode {
        Mode::Input => " input  (Enter send · Esc cancel)",
        Mode::Normal => " input  (i edit)",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(match app.mode {
            Mode::Input => Style::default().fg(Tone::Success.color()),
            Mode::Normal => Style::default().fg(Tone::Border.color()),
        });
    let text = if app.input.is_empty() {
        Line::from(Span::styled("type a message…", theme::fg(Tone::Dim)))
    } else if app.hub.filter_editing {
        Line::from(theme::span(format!("/{}", app.hub.filter), Tone::Accent))
    } else {
        Line::from(app.input.clone())
    };
    f.render_widget(Paragraph::new(text).block(block), area);
}

pub fn draw_statusline(f: &mut Frame, app: &App, area: Rect) {
    let right = format!(
        "({} peers) · {} · {}",
        app.hub.roster.len(),
        app.model,
        app.cwd()
    );
    let hints = match app.mode {
        Mode::Input => super::nav::input_hints(),
        Mode::Normal if app.hub.visible => super::nav::hub_hints(),
        Mode::Normal => super::nav::normal_hints(),
    };
    let right_width = (theme::visible_width(&right) as u16 + 2).min(area.width);
    let chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)])
        .split(area);
    let hint = theme::truncate(hints, usize::from(chunks[0].width));
    let left = if app.status.is_empty() {
        theme::span(hint, Tone::Dim)
    } else {
        theme::span(theme::truncate(&app.status, usize::from(chunks[0].width)), Tone::Accent)
    };
    f.render_widget(Paragraph::new(Line::from(left)), chunks[0]);
    let peers = format!("({} peers)", app.hub.roster.len());
    let rest = format!(" · {} · {}", app.model, app.cwd());
    let right_line = Line::from(vec![
        theme::span(peers, Tone::Accent),
        theme::span(rest, Tone::Muted),
    ]);
    f.render_widget(Paragraph::new(right_line), chunks[1]);
}
