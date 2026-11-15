//! Frame layout: transcript (or hub), composer, statusline.

use maple_tui::text_shape::wrap_text_with_ansi;

use super::app::App;
use super::chrome::{self, short};
use super::hub::render;
use crate::session::types::SessionEntry;
use crate::tui::theme::{self, Tone};

/// Transcript rows kept in the render buffer (bounded, like the session scan).
const TRANSCRIPT_BUDGET: usize = 500;

/// Full frame as one string per terminal row.
pub fn render_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
    if app.picker.visible {
        return app.picker.rows(width, height);
    }
    let transcript_height = height.saturating_sub(4);
    let body = if app.hub.visible {
        let mut rows = render::hub_rows(app, width, transcript_height);
        if let Some(form) = &app.hub.ask {
            rows = render::ask_rows(form, width, transcript_height);
        }
        rows
    } else {
        transcript_rows(app, width, transcript_height)
    };
    let mut out = Vec::with_capacity(usize::from(height));
    out.extend(body);
    out.extend(chrome::composer_rows(app, width));
    out.push(chrome::statusline_row(app, width));
    out.truncate(usize::from(height));
    while out.len() < usize::from(height) {
        out.push(" ".repeat(usize::from(width)));
    }
    out
}

fn transcript_rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let inner = usize::from(width).saturating_sub(2);
    let start = app.entries.len().saturating_sub(TRANSCRIPT_BUDGET);
    let mut content: Vec<String> = Vec::new();
    for entry in &app.entries[start..] {
        let line = format!(
            "{}  {:<8} {}",
            theme::span(
                entry.timestamp.get(..19).unwrap_or(&entry.timestamp).to_string(),
                Tone::Dim,
            ),
            theme::span(entry.kind.clone(), kind_tone(&entry.kind)),
            summary(entry),
        );
        for wrapped in wrap_text_with_ansi(&line, inner.max(1)) {
            content.push(wrapped);
        }
    }
    let view_start = (app.scroll as usize).min(content.len());
    let visible = &content[view_start..];

    let mut rows = vec![top_border(
        width,
        &format!(" maple · {} ", short(app.session_id())),
    )];
    rows.push(format!("│{}│", " ".repeat(inner)));
    for line in visible {
        rows.push(format!("│{}│", theme::fit(line, inner)));
    }
    while rows.len() < height as usize - 1 {
        rows.push(format!("│{}│", " ".repeat(inner)));
    }
    rows.push(bottom_border(width));
    rows.truncate(height as usize);
    rows
}

fn top_border(width: u16, title: &str) -> String {
    let inner = usize::from(width).saturating_sub(2);
    let title_width = theme::visible_width(title);
    let dashes = inner.saturating_sub(title_width);
    format!("╭{}{}╮", title, "─".repeat(dashes))
}

fn bottom_border(width: u16) -> String {
    format!("╰{}╯", "─".repeat(usize::from(width).saturating_sub(2)))
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
