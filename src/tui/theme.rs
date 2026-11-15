//! Color tokens, text measurement, and span helpers.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

/// Semantic colors shared by every surface (transcript, hub, picker, statusline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Success,
    Warning,
    Error,
    Dim,
    Muted,
    Border,
    Reset,
}

impl Tone {
    pub fn color(self) -> Color {
        match self {
            Tone::Accent => Color::Rgb(0xE2, 0xB0, 0x5A),
            Tone::Success => Color::Rgb(0x8F, 0xBF, 0x6A),
            Tone::Warning => Color::Rgb(0xD1, 0x9A, 0x66),
            Tone::Error => Color::Rgb(0xE0, 0x6C, 0x75),
            Tone::Dim => Color::Indexed(245),
            Tone::Muted => Color::Indexed(239),
            Tone::Border => Color::Indexed(238),
            Tone::Reset => Color::Reset,
        }
    }
}

pub fn fg(tone: Tone) -> Style {
    Style::default().fg(tone.color())
}

pub fn span(text: impl Into<String>, tone: Tone) -> Span<'static> {
    Span::styled(text.into(), fg(tone))
}

/// Display width of a string; the box/nav glyphs used here are single width.
pub fn visible_width(text: &str) -> usize {
    text.chars().count()
}

/// Truncate to `width` columns, marking the cut with an ellipsis.
pub fn truncate(text: &str, width: usize) -> String {
    if visible_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Pad or truncate to exactly `width` columns.
pub fn fit(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let len = visible_width(&text);
    if len < width {
        format!("{text}{}", " ".repeat(width - len))
    } else {
        text
    }
}
