//! Composer and status line rendering for the main view.

use maple_tui::text_shape::{truncate_to_width, Ellipsis};

use crate::tui::theme::{self, Tone};

use super::app::App;

pub const COMPOSER_CAPACITY: usize = 2_000;

pub fn short(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

/// Three composer rows: top border with mode, buffer with caret,
/// bottom border.
pub fn composer_rows(app: &App, width: u16) -> Vec<String> {
    let top = format!("╭─ {} ", app.mode.label());
    let buffer = theme::span(format!("{} ▍", app.input), Tone::Accent);
    let suffix = theme::span(
        format!("  {}/{}", app.input.chars().count(), COMPOSER_CAPACITY),
        Tone::Muted,
    );
    let pad = usize::from(width)
        .saturating_sub(2 + theme::visible_width(&buffer) + theme::visible_width(&suffix));
    let middle = format!("│  {buffer}{suffix}{}", " ".repeat(pad));
    let bottom = format!("╰{}", "─".repeat(usize::from(width).saturating_sub(2)));
    vec![
        truncate_to_width(&top, usize::from(width), Ellipsis::Omit),
        truncate_to_width(&middle, usize::from(width), Ellipsis::Omit),
        truncate_to_width(&bottom, usize::from(width), Ellipsis::Omit),
    ]
}

/// Bottom status strip: position, overlay markers, model, status.
pub fn statusline_row(app: &App, width: u16) -> String {
    let entries = app.entries.len();
    let index = (app.scroll as usize).min(entries.saturating_sub(1));
    let position = if entries == 0 {
        "0/0".to_string()
    } else {
        format!("{}/{}", index + 1, entries)
    };
    let mut segments = vec![format!(" {position}")];
    if !app.model.is_empty() {
        segments.push(theme::span(format!("  {}", app.model), Tone::Muted));
    }
    if app.hub.visible {
        segments.push(" [Hub]".to_string());
    }
    if app.picker.visible {
        segments.push(" [Picker]".to_string());
    }
    if !app.status.is_empty() {
        segments.push(theme::span(format!("  {}", app.status), Tone::Dim));
    }
    truncate_to_width(&segments.join(""), usize::from(width), Ellipsis::Omit)
}
