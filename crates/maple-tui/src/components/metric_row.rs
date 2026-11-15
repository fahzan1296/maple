//! Inline metric row: labelled values laid out across the viewport width.

use crate::component::Component;
use crate::style::{Color, Style};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// One metric: a label, a value, optional unit and delta, and a style.
pub struct Metric {
    /// Text placed before the value.
    pub label: String,
    /// The metric value.
    pub value: String,
    /// Unit appended directly to the value.
    pub unit: Option<String>,
    /// Optional change indicator appended after the value.
    pub delta: Option<String>,
    /// Style for the value; the theme value style is used when absent.
    pub style: Option<Style>,
}

impl Metric {
    /// A bare value with no label.
    pub fn new(value: impl Into<String>) -> Self {
        Metric {
            label: String::new(),
            value: value.into(),
            unit: None,
            delta: None,
            style: None,
        }
    }

    /// Set the label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Set the unit.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set the delta indicator.
    pub fn with_delta(mut self, delta: impl Into<String>) -> Self {
        self.delta = Some(delta.into());
        self
    }

    /// Set the value style.
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

/// Width behavior for a metric row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricOverflow {
    /// Keep the joined text; lines are clipped to the viewport.
    Allow,
    /// Clip the joined text to the content width.
    Truncate,
    /// Word-wrap the joined text.
    Wrap,
    /// Drop metrics from the right until the row fits.
    Drop,
}

/// Styles and spacing for a [`MetricRow`].
#[derive(Clone, Debug, PartialEq)]
pub struct MetricRowTheme {
    /// Label style.
    pub label: Style,
    /// Value style.
    pub value: Style,
    /// Delta style.
    pub delta: Style,
    /// Text placed between metrics.
    pub separator: String,
    /// Horizontal padding.
    pub padding_x: u16,
    /// Vertical padding.
    pub padding_y: u16,
}

impl Default for MetricRowTheme {
    fn default() -> Self {
        MetricRowTheme {
            label: Style::fg(Color::DarkGray),
            value: Style::default(),
            delta: Style::default(),
            separator: "  ".to_string(),
            padding_x: 0,
            padding_y: 0,
        }
    }
}

/// A single-line row of metrics that wraps or truncates to fit.
pub struct MetricRow {
    metrics: Vec<Metric>,
    theme: MetricRowTheme,
    overflow: MetricOverflow,
}

impl Default for MetricRow {
    fn default() -> Self {
        MetricRow {
            metrics: Vec::new(),
            theme: MetricRowTheme::default(),
            overflow: MetricOverflow::Wrap,
        }
    }
}

impl MetricRow {
    /// A row over `metrics`.
    pub fn new(metrics: Vec<Metric>) -> Self {
        MetricRow {
            metrics,
            theme: MetricRowTheme::default(),
            overflow: MetricOverflow::Wrap,
        }
    }

    /// Replace the metrics.
    pub fn set_metrics(&mut self, metrics: Vec<Metric>) {
        self.metrics = metrics;
    }

    /// The current metrics.
    pub fn metrics(&self) -> &[Metric] {
        &self.metrics
    }

    /// The theme.
    pub fn theme(&self) -> &MetricRowTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: MetricRowTheme) {
        self.theme = theme;
    }

    /// The overflow policy.
    pub fn overflow(&self) -> MetricOverflow {
        self.overflow
    }

    /// Set the overflow policy.
    pub fn set_overflow(&mut self, overflow: MetricOverflow) {
        self.overflow = overflow;
    }

    /// Render one metric as styled text.
    fn format_metric(&self, metric: &Metric) -> String {
        let mut out = String::new();
        if !metric.label.is_empty() {
            out.push_str(&self.theme.label.paint(&single_line(&metric.label)));
            out.push(' ');
        }
        let value_text = format!(
            "{}{}",
            metric.value,
            metric.unit.as_deref().unwrap_or("")
        );
        let value_style = metric.style.unwrap_or(self.theme.value);
        out.push_str(&value_style.paint(&single_line(&value_text)));
        if let Some(delta) = &metric.delta {
            out.push(' ');
            out.push_str(&self.theme.delta.paint(&single_line(delta)));
        }
        out
    }

    /// Join the metrics, dropping from the right until the row fits.
    fn joined(&self, drop_to: Option<usize>) -> String {
        let mut segments: Vec<String> = self
            .metrics
            .iter()
            .map(|metric| self.format_metric(metric))
            .filter(|segment| !segment.is_empty())
            .collect();
        if let Some(max_width) = drop_to {
            while segments.len() > 1
                && visible_width(&segments.join(&self.theme.separator)) > max_width
            {
                segments.pop();
            }
        }
        segments.join(&self.theme.separator)
    }
}

impl Component for MetricRow {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return Vec::new();
        }
        let padding_x = (self.theme.padding_x as usize).min(width / 2);
        let content_width = width.saturating_sub(2 * padding_x).max(1);
        let joined = self.joined(match self.overflow {
            MetricOverflow::Drop => Some(content_width),
            _ => None,
        });
        let mut body: Vec<String> = match self.overflow {
            // A joining that already fits is emitted verbatim; wrapping must not
            // collapse the separator runs it contains.
            MetricOverflow::Wrap if visible_width(&joined) <= content_width => vec![joined],
            MetricOverflow::Wrap => wrap_text_with_ansi(&joined, content_width),
            MetricOverflow::Truncate => {
                vec![truncate_to_width(&joined, content_width, Ellipsis::Omit)]
            }
            MetricOverflow::Allow | MetricOverflow::Drop => vec![joined],
        };

        let mut rows = Vec::new();
        for _ in 0..self.theme.padding_y {
            rows.push(" ".repeat(width));
        }
        for line in &mut body {
            let mut padded = String::new();
            padded.push_str(&" ".repeat(padding_x));
            padded.push_str(&*line);
            padded.push_str(&" ".repeat(padding_x));
            *line = pad_to_width(&padded, width);
            if visible_width(&*line) > width {
                *line = truncate_to_width(&*line, width, Ellipsis::Omit);
            }
        }
        rows.append(&mut body);
        for _ in 0..self.theme.padding_y {
            rows.push(" ".repeat(width));
        }
        rows
    }
}

/// Replace tabs and line breaks with spaces.
fn single_line(text: &str) -> String {
    text.replace('\t', " ").replace('\r', " ").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> MetricRowTheme {
        MetricRowTheme {
            label: Style::default(),
            value: Style::default(),
            delta: Style::default(),
            separator: "  ".to_string(),
            padding_x: 0,
            padding_y: 0,
        }
    }

    fn metrics() -> Vec<Metric> {
        vec![
            Metric::new("12").with_label("CPU").with_unit("%"),
            Metric::new("4").with_label("MEM").with_unit("GiB"),
        ]
    }

    #[test]
    fn lays_metrics_across_the_width() {
        let mut row = MetricRow::new(metrics());
        row.set_theme(plain());
        assert_eq!(row.render(20), vec!["CPU 12%  MEM 4GiB   "]);
    }

    #[test]
    fn wraps_long_rows() {
        let mut row = MetricRow::new(metrics());
        row.set_theme(plain());
        assert_eq!(row.render(12), vec!["CPU 12% MEM ", "4GiB        "]);
    }

    #[test]
    fn truncates_rows() {
        let mut row = MetricRow::new(metrics());
        let mut theme = plain();
        theme.separator = "  ".to_string();
        row.set_theme(theme);
        row.set_overflow(MetricOverflow::Truncate);
        assert_eq!(row.render(8), vec!["CPU 12% "]);
    }

    #[test]
    fn drops_metrics_from_the_right() {
        let mut row = MetricRow::new(metrics());
        row.set_theme(plain());
        row.set_overflow(MetricOverflow::Drop);
        assert_eq!(row.render(8), vec!["CPU 12% "]);
    }

    #[test]
    fn renders_delta_and_allow_overflow() {
        let mut row = MetricRow::new(vec![Metric::new("12")
            .with_label("CPU")
            .with_unit("%")
            .with_delta("+3")]);
        row.set_theme(plain());
        row.set_overflow(MetricOverflow::Allow);
        assert_eq!(row.render(10), vec!["CPU 12% +3"]);
    }

    #[test]
    fn pads_and_applies_theme_styles() {
        let mut row = MetricRow::new(vec![Metric::new("7").with_label("Queue")]);
        let mut theme = plain();
        theme.label = Style::fg(Color::Red);
        theme.value = Style::fg(Color::Green);
        theme.padding_x = 1;
        theme.padding_y = 1;
        row.set_theme(theme);
        let rendered = row.render(9);
        assert_eq!(
            rendered,
            vec![
                "         ",
                " \x1b[31mQueue\x1b[0m \x1b[32m7\x1b[0m ",
                "         ",
            ]
        );
    }

    #[test]
    fn handles_empty_and_narrow_widths() {
        let mut row = MetricRow::new(Vec::new());
        row.set_theme(plain());
        assert_eq!(row.render(5), vec!["     "]);
        assert!(row.render(0).is_empty());

        let mut row = MetricRow::new(metrics());
        row.set_theme(plain());
        for width in [1, 2, 3, 5] {
            for line in row.render(width) {
                assert!(visible_width(&line) <= width as usize, "row too wide: {line:?}");
            }
        }
    }

    #[test]
    fn reflects_state_changes() {
        let mut row = MetricRow::new(metrics());
        row.set_theme(plain());
        assert_eq!(row.render(20), vec!["CPU 12%  MEM 4GiB   "]);
        row.set_metrics(vec![Metric::new("1").with_label("A")]);
        assert_eq!(row.render(20), vec!["A 1                 "]);
    }
}
