//! Determinate and indeterminate progress bar.

use crate::component::Component;
use crate::style::Style;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// Glyphs and styles for a [`ProgressBar`].
#[derive(Clone, Debug, PartialEq)]
pub struct ProgressBarTheme {
    /// Glyph repeated for completed cells.
    pub filled: String,
    /// Glyph repeated for remaining cells.
    pub empty: String,
    /// Glyph used when the value is unknown; defaults to `empty`.
    pub indeterminate: Option<String>,
    /// Style for completed cells.
    pub completed: Style,
    /// Style for remaining cells and indeterminate bars.
    pub incomplete: Style,
    /// Style layered over the whole bar.
    pub overall: Style,
    /// Style for the percentage label.
    pub percentage: Style,
}

impl Default for ProgressBarTheme {
    fn default() -> Self {
        ProgressBarTheme {
            filled: "█".to_string(),
            empty: " ".to_string(),
            indeterminate: None,
            completed: Style::default(),
            incomplete: Style::default(),
            overall: Style::default(),
            percentage: Style::default(),
        }
    }
}

/// A single-line progress bar bounded to the viewport width.
pub struct ProgressBar {
    progress: Option<f64>,
    minimum: f64,
    total: f64,
    prefix: String,
    suffix: String,
    percentage_separator: String,
    show_percentage: bool,
    theme: ProgressBarTheme,
}

impl Default for ProgressBar {
    fn default() -> Self {
        ProgressBar::new()
    }
}

impl ProgressBar {
    /// An indeterminate bar over the range `0.0..=1.0`.
    pub fn new() -> Self {
        ProgressBar {
            progress: None,
            minimum: 0.0,
            total: 1.0,
            prefix: String::new(),
            suffix: String::new(),
            percentage_separator: " ".to_string(),
            show_percentage: false,
            theme: ProgressBarTheme::default(),
        }
    }

    /// Set the value; `None` (or a non-finite value) renders indeterminate.
    pub fn set_progress(&mut self, value: Option<f64>) {
        self.progress = value.filter(|value| value.is_finite());
    }

    /// The current value, or `None` when indeterminate.
    pub fn progress(&self) -> Option<f64> {
        self.progress
    }

    /// Set the range high end.
    pub fn set_total(&mut self, total: f64) {
        self.total = total;
    }

    /// The range high end.
    pub fn total(&self) -> f64 {
        self.total
    }

    /// Set the range low end.
    pub fn set_minimum(&mut self, minimum: f64) {
        self.minimum = minimum;
    }

    /// The range low end.
    pub fn minimum(&self) -> f64 {
        self.minimum
    }

    /// The clamped completion fraction, or `None` when indeterminate.
    pub fn fraction(&self) -> Option<f64> {
        progress_fraction(self.progress, self.minimum, self.total)
    }

    /// Text placed before the bar.
    pub fn set_prefix(&mut self, prefix: impl Into<String>) {
        self.prefix = prefix.into();
    }

    /// Text placed after the bar.
    pub fn set_suffix(&mut self, suffix: impl Into<String>) {
        self.suffix = suffix.into();
    }

    /// Text placed between the bar and the percentage label.
    pub fn set_percentage_separator(&mut self, separator: impl Into<String>) {
        self.percentage_separator = separator.into();
    }

    /// Whether the percentage label is shown for determinate values.
    pub fn set_show_percentage(&mut self, show: bool) {
        self.show_percentage = show;
    }

    /// Whether the percentage label is shown.
    pub fn show_percentage(&self) -> bool {
        self.show_percentage
    }

    /// The theme.
    pub fn theme(&self) -> &ProgressBarTheme {
        &self.theme
    }

    /// A mutable theme; glyphs live here.
    pub fn theme_mut(&mut self) -> &mut ProgressBarTheme {
        &mut self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: ProgressBarTheme) {
        self.theme = theme;
    }

    /// Set the completed-cell glyph.
    pub fn set_filled_glyph(&mut self, glyph: impl Into<String>) {
        self.theme.filled = glyph.into();
    }

    /// Set the remaining-cell glyph.
    pub fn set_empty_glyph(&mut self, glyph: impl Into<String>) {
        self.theme.empty = glyph.into();
    }

    /// Set the indeterminate glyph; `None` falls back to the empty glyph.
    pub fn set_indeterminate_glyph(&mut self, glyph: Option<String>) {
        self.theme.indeterminate = glyph;
    }

    /// Layer a style over the whole bar.
    pub fn set_style(&mut self, style: Style) {
        self.theme.overall = style;
    }

    /// Layer a style over the whole bar.
    pub fn set_overall_style(&mut self, style: Style) {
        self.theme.overall = style;
    }

    /// Style completed cells.
    pub fn set_completed_style(&mut self, style: Style) {
        self.theme.completed = style;
    }

    /// Style remaining cells and indeterminate bars.
    pub fn set_incomplete_style(&mut self, style: Style) {
        self.theme.incomplete = style;
    }

    /// Style the percentage label.
    pub fn set_percentage_style(&mut self, style: Style) {
        self.theme.percentage = style;
    }
}

impl Component for ProgressBar {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return Vec::new();
        }
        let fraction = self.fraction();
        let prefix = single_line(&self.prefix);
        let suffix = single_line(&self.suffix);
        let percentage = match fraction {
            Some(fraction) if self.show_percentage => format_percentage(fraction),
            _ => String::new(),
        };
        let separator = if percentage.is_empty() {
            String::new()
        } else {
            single_line(&self.percentage_separator)
        };
        let chrome = visible_width(&prefix)
            + visible_width(&suffix)
            + visible_width(&separator)
            + visible_width(&percentage);
        let bar_width = width.saturating_sub(chrome);

        let bar = match fraction {
            None => {
                let glyph = self
                    .theme
                    .indeterminate
                    .clone()
                    .unwrap_or_else(|| self.theme.empty.clone());
                let raw = fill_cells(&glyph, bar_width);
                self.theme.incomplete.paint(&raw)
            }
            Some(fraction) => {
                let filled_width = ((fraction * bar_width as f64).round() as usize).min(bar_width);
                let filled = self
                    .theme
                    .completed
                    .paint(&fill_cells(&self.theme.filled, filled_width));
                let empty = self
                    .theme
                    .incomplete
                    .paint(&fill_cells(&self.theme.empty, bar_width - filled_width));
                format!("{filled}{empty}")
            }
        };
        let bar = self.theme.overall.paint(&bar);
        let percentage = self.theme.percentage.paint(&percentage);
        let line = format!("{prefix}{bar}{suffix}{separator}{percentage}");
        vec![truncate_to_width(
            &single_line(&line),
            width,
            Ellipsis::Omit,
        )]
    }
}

/// Normalize a value to a clamped fraction, preserving `None` as indeterminate.
pub fn progress_fraction(value: Option<f64>, minimum: f64, maximum: f64) -> Option<f64> {
    let value = value?;
    if !minimum.is_finite() || !maximum.is_finite() {
        return None;
    }
    if maximum <= minimum {
        return Some(if value >= maximum { 1.0 } else { 0.0 });
    }
    Some(((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0))
}

/// Default percentage label: whole percents with a `%` suffix.
pub fn format_percentage(fraction: f64) -> String {
    format!("{}%", (fraction * 100.0).round() as i64)
}

/// Repeat `glyph` to exactly `width` display cells, space-padding the rest.
fn fill_cells(glyph: &str, width: usize) -> String {
    let glyph = single_line(glyph);
    let glyph_width = visible_width(&glyph);
    if glyph_width == 0 {
        return " ".repeat(width);
    }
    let count = width / glyph_width;
    let mut out = glyph.repeat(count);
    out.push_str(&" ".repeat(width - count * glyph_width));
    out
}

/// Replace tabs and line breaks with spaces.
fn single_line(text: &str) -> String {
    text.replace('\t', " ").replace('\r', " ").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    #[test]
    fn renders_bar_and_percentage() {
        let mut bar = ProgressBar::new();
        bar.set_total(1.0);
        bar.set_progress(Some(0.5));
        bar.set_show_percentage(true);
        bar.set_filled_glyph("=");
        bar.set_empty_glyph("-");
        assert_eq!(bar.fraction(), Some(0.5));
        assert_eq!(bar.render(12), vec!["====---- 50%"]);
    }

    #[test]
    fn renders_indeterminate_bar() {
        let mut bar = ProgressBar::new();
        bar.set_progress(None);
        bar.set_indeterminate_glyph(Some("~".to_string()));
        assert_eq!(bar.render(5), vec!["~~~~~"]);
    }

    #[test]
    fn clamps_fraction_to_the_range() {
        let mut bar = ProgressBar::new();
        bar.set_total(2.0);
        bar.set_progress(Some(5.0));
        assert_eq!(bar.fraction(), Some(1.0));
        bar.set_progress(Some(-1.0));
        assert_eq!(bar.fraction(), Some(0.0));
        bar.set_progress(Some(f64::NAN));
        assert_eq!(bar.fraction(), None);
        bar.set_progress(Some(1.0));
        assert_eq!(bar.render(4), vec!["██  "]);
    }

    #[test]
    fn applies_completed_and_percentage_styles() {
        let mut bar = ProgressBar::new();
        bar.set_progress(Some(0.5));
        bar.set_show_percentage(true);
        bar.set_filled_glyph("=");
        bar.set_empty_glyph("-");
        bar.set_completed_style(Style::fg(Color::Red));
        bar.set_percentage_style(Style::fg(Color::Green));
        let rendered = bar.render(12);
        assert_eq!(
            rendered[0],
            "\x1b[31m====\x1b[0m---- \x1b[32m50%\x1b[0m"
        );
        assert_eq!(visible_width(&rendered[0]), 12);
    }

    #[test]
    fn truncates_when_chrome_exceeds_width() {
        let mut bar = ProgressBar::new();
        bar.set_progress(Some(0.5));
        bar.set_show_percentage(true);
        bar.set_filled_glyph("=");
        bar.set_empty_glyph("-");
        assert_eq!(bar.render(2), vec![" 5"]);
        assert!(bar.render(0).is_empty());
    }

    #[test]
    fn tracks_state_changes() {
        let mut bar = ProgressBar::new();
        bar.set_total(10.0);
        bar.set_progress(Some(0.0));
        assert_eq!(bar.render(5), vec!["     "]);
        bar.set_progress(Some(5.0));
        assert_eq!(bar.render(5), vec!["███  "]);
        bar.set_total(5.0);
        assert_eq!(bar.fraction(), Some(1.0));
        assert_eq!(bar.render(5), vec!["█████"]);
    }

    #[test]
    fn supports_prefix_suffix_and_separator() {
        let mut bar = ProgressBar::new();
        bar.set_progress(Some(0.5));
        bar.set_prefix("[");
        bar.set_suffix("]");
        bar.set_filled_glyph("=");
        bar.set_empty_glyph("-");
        bar.set_percentage_separator(" / ");
        bar.set_show_percentage(true);
        assert_eq!(bar.render(14), vec!["[===---] / 50%"]);
    }
}
