//! Color tokens and text shaping helpers backed by `maple-tui`.

use maple_tui::style::{Color, Style};
use maple_tui::text_shape::{pad_to_width, truncate_to_width, Ellipsis};

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

    /// Style that paints text in this tone.
    pub fn style(self) -> Style {
        if matches!(self, Tone::Reset) {
            Style::default()
        } else {
            Style::fg(self.color())
        }
    }
}

/// Paint `text` in `tone`, closing the style afterwards.
pub fn span(text: impl Into<String>, tone: Tone) -> String {
    tone.style().paint(&text.into())
}

/// Display width of a string, ignoring ANSI escapes.
pub fn visible_width(text: &str) -> usize {
    maple_tui::text_shape::visible_width(text)
}

/// Truncate to `width` columns, marking the cut with an ellipsis.
pub fn truncate(text: &str, width: usize) -> String {
    truncate_to_width(text, width, Ellipsis::Unicode)
}

/// Pad or truncate to exactly `width` columns.
pub fn fit(text: &str, width: usize) -> String {
    pad_to_width(&truncate(text, width), width)
}
