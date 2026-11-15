//! Frame layout: transcript (or hub), composer, statusline.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::app::App;
use super::chrome::{draw_input, draw_statusline, short};
use super::hub::render::draw_hub;
use crate::session::types::SessionEntry;
use crate::tui::theme::{self, Tone};

/// Transcript rows kept in the render buffer (bounded, like the session scan).
const TRANSCRIPT_BUDGET: usize = 500;

pub fn draw(f: &mut Frame, app: &mut App) {
    if app.picker.visible {
        app.picker.render(f, f.area());
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(f.area());

    if app.hub.visible {
        draw_hub(f, app, chunks[0]);
        if let Some(form) = &app.hub.ask {
            super::hub::render::draw_ask(f, form, chunks[0]);
        }
    } else {
        draw_transcript(f, app, chunks[0]);
    }
    draw_input(f, app, chunks[1]);
    draw_statusline(f, app, chunks[2]);
}

fn draw_transcript(f: &mut Frame, app: &App, area: Rect) {
    let start = app.entries.len().saturating_sub(TRANSCRIPT_BUDGET);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for entry in &app.entries[start..] {
        lines.push(Line::from(vec![
            theme::span(
                entry.timestamp.get(..19).unwrap_or(&entry.timestamp).to_string(),
                Tone::Dim,
            ),
            Span::raw("  "),
            theme::span(format!("{:<8}", entry.kind), kind_tone(&entry.kind)),
            Span::raw(" "),
            Span::raw(summary(entry)),
        ]));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" maple · {} ", short(app.session_id())))
        .border_style(Style::default().fg(Tone::Border.color()));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        area,
    );
}

fn kind_tone(kind: &str) -> Tone {
    match kind {
        "message" => Tone::Accent,
        "usage" => Tone::Success,
        "error" | "abort" => Tone::Error,
        _ => Tone::Muted,
    }
}

/// Human-readable row text; usage rows become a compact fact line.
fn summary(entry: &SessionEntry) -> String {
    match entry.kind.as_str() {
        "message" => entry
            .payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "usage" => {
            let mut facts: Vec<String> = Vec::new();
            if let Some(cost) = entry.payload.get("cost").and_then(|v| v.as_f64()) {
                facts.push(super::hub::row::format_cost(cost));
            }
            if let Some(tokens) = entry.payload.get("tokens").and_then(|v| v.as_u64()) {
                facts.push(format!("{tokens} tok"));
            }
            if let Some(tool) = entry.payload.get("tool").and_then(|v| v.as_str()) {
                facts.push(tool.to_string());
            }
            if facts.is_empty() {
                entry.payload.to_string()
            } else {
                facts.join(super::nav::SEP_DOT)
            }
        }
        _ => entry.payload.to_string(),
    }
}
