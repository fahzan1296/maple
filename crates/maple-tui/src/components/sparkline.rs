//! Deterministic block and braille sparklines.
//! Reference: `l/src/components/sparkline.ts`.

use crate::component::Component;
use crate::style::Style;
use crate::text_shape::{truncate_to_width, Ellipsis};

/// Block glyphs from lowest to highest.
pub const SPARK_LEVELS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Rendering mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SparklineMode {
    /// Block glyphs, optionally stacked over several rows.
    Block,
    /// Braille dots, four dot rows per text row.
    Braille,
}

/// The block glyph for a 0–1 fraction of the value domain.
pub fn spark_glyph(fraction: f64) -> &'static str {
    let clamped = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let index = (clamped * SPARK_LEVELS.len() as f64).floor() as usize;
    SPARK_LEVELS[index.min(SPARK_LEVELS.len() - 1)]
}

/// Squeeze `values` into at most `cells` bucket means, newest last.
pub fn fit_spark_values(values: &[f64], cells: usize) -> Vec<f64> {
    if values.len() <= cells || cells == 0 {
        return values.to_vec();
    }
    let mut out = Vec::with_capacity(cells);
    for bucket in 0..cells {
        let from = bucket * values.len() / cells;
        let to = ((bucket + 1) * values.len() / cells).max(from + 1);
        let sum: f64 = values[from..to].iter().sum();
        out.push(sum / (to - from) as f64);
    }
    out
}

/// A sparkline over numeric samples.
pub struct Sparkline {
    values: Vec<f64>,
    height: usize,
    min: f64,
    max: Option<f64>,
    color: Option<Style>,
    gradient: Vec<Style>,
    mode: SparklineMode,
    gap: String,
}

impl Default for Sparkline {
    fn default() -> Self {
        Sparkline {
            values: Vec::new(),
            height: 1,
            min: 0.0,
            max: None,
            color: None,
            gradient: Vec::new(),
            mode: SparklineMode::Block,
            gap: " ".to_string(),
        }
    }
}

impl Sparkline {
    /// An empty one-row block sparkline.
    pub fn new() -> Self {
        Sparkline::default()
    }

    /// Replace the samples.
    pub fn set_values(&mut self, values: Vec<f64>) {
        self.values = values;
    }

    /// The current samples.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Set the number of rendered rows (dot rows in braille mode).
    pub fn set_height(&mut self, height: usize) {
        self.height = height;
    }

    /// The rendered row count.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Set the domain low end.
    pub fn set_minimum(&mut self, min: f64) {
        self.min = min;
    }

    /// Set the domain high end; `None` uses the largest sample.
    pub fn set_maximum(&mut self, max: Option<f64>) {
        self.max = max;
    }

    /// The domain high end override.
    pub fn maximum(&self) -> Option<f64> {
        self.max
    }

    /// Style every glyph.
    pub fn set_color(&mut self, style: Style) {
        self.color = Some(style);
    }

    /// Remove the single-color style.
    pub fn clear_color(&mut self) {
        self.color = None;
    }

    /// Style glyphs by fraction; the last style wins at the top of the domain.
    pub fn set_gradient(&mut self, gradient: Vec<Style>) {
        self.gradient = gradient;
    }

    /// The gradient styles.
    pub fn gradient(&self) -> &[Style] {
        &self.gradient
    }

    /// Select block or braille rendering.
    pub fn set_mode(&mut self, mode: SparklineMode) {
        self.mode = mode;
    }

    /// The rendering mode.
    pub fn mode(&self) -> SparklineMode {
        self.mode
    }

    /// Set the text used for missing samples.
    pub fn set_gap(&mut self, gap: impl Into<String>) {
        self.gap = gap.into();
    }

    /// The style for a fraction, preferring the gradient.
    fn style_for(&self, fraction: f64) -> Option<Style> {
        if self.gradient.is_empty() {
            return self.color;
        }
        let index = ((fraction * self.gradient.len() as f64).floor() as usize)
            .min(self.gradient.len() - 1);
        Some(self.gradient[index])
    }

    /// The domain high end, defaulting to the largest finite sample.
    fn effective_max(&self, values: &[f64]) -> f64 {
        match self.max {
            Some(max) => max,
            None => {
                let mut max = self.min;
                for value in values {
                    if value.is_finite() && *value > max {
                        max = *value;
                    }
                }
                max
            }
        }
    }

    fn render_block(&self, fractions: &[Option<f64>], width: usize) -> Vec<String> {
        let height = self.height;
        if height <= 1 {
            let mut line = String::new();
            for fraction in fractions {
                match fraction {
                    None => line.push_str(&self.gap),
                    Some(fraction) => {
                        let glyph = spark_glyph(*fraction);
                        match self.style_for(*fraction) {
                            Some(style) => line.push_str(&style.paint(glyph)),
                            None => line.push_str(glyph),
                        }
                    }
                }
            }
            return vec![truncate_to_width(&line, width, Ellipsis::Omit)];
        }

        let mut columns: Vec<(String, Option<f64>)> = Vec::with_capacity(fractions.len());
        for fraction in fractions {
            let mut column = String::with_capacity(height);
            match fraction {
                None => column.push_str(&" ".repeat(height)),
                Some(fraction) => {
                    // A real sample always draws at least the bottom block.
                    let level = ((*fraction * height as f64).round() as usize).clamp(1, height);
                    for row in 0..height {
                        if height - row <= level {
                            column.push('█');
                        } else {
                            column.push(' ');
                        }
                    }
                }
            }
            columns.push((column, *fraction));
        }

        let mut rows = Vec::with_capacity(height);
        for row in 0..height {
            let mut line = String::new();
            for (column, fraction) in &columns {
                let glyph = column.chars().nth(row).unwrap_or(' ');
                match fraction {
                    Some(fraction) => match self.style_for(*fraction) {
                        Some(style) => line.push_str(&style.paint(&glyph.to_string())),
                        None => line.push(glyph),
                    },
                    None => line.push(glyph),
                }
            }
            rows.push(truncate_to_width(&line, width, Ellipsis::Omit));
        }
        rows
    }

    fn render_braille(&self, fractions: &[Option<f64>], width: usize) -> Vec<String> {
        let dot_rows = self.height;
        if dot_rows == 0 {
            return Vec::new();
        }
        let text_rows = (dot_rows + 3) / 4;
        let cell_count = (fractions.len() + 1) / 2;
        let mut bits = vec![0u32; text_rows * cell_count];
        let mut cell_fraction: Vec<Option<f64>> = vec![None; cell_count];

        for (index, fraction) in fractions.iter().enumerate() {
            let cell_column = index / 2;
            if cell_fraction[cell_column].is_none() {
                cell_fraction[cell_column] = *fraction;
            }
            let Some(fraction) = fraction else {
                continue;
            };
            let level = ((*fraction * dot_rows as f64).round() as usize).min(dot_rows);
            for dot_row in 0..dot_rows {
                if dot_rows - dot_row > level {
                    continue;
                }
                let text_row = dot_row / 4;
                let bit = match (dot_row % 4, index % 2) {
                    (0, 0) => 0x01,
                    (1, 0) => 0x02,
                    (2, 0) => 0x04,
                    (3, 0) => 0x40,
                    (0, 1) => 0x08,
                    (1, 1) => 0x10,
                    (2, 1) => 0x20,
                    (_, _) => 0x80,
                };
                bits[text_row * cell_count + cell_column] |= bit;
            }
        }

        let mut rows = Vec::with_capacity(text_rows);
        for text_row in 0..text_rows {
            let mut line = String::new();
            for cell_column in 0..cell_count {
                let cell_bits = bits[text_row * cell_count + cell_column];
                let glyph = char::from_u32(0x2800 + cell_bits).unwrap_or(' ');
                let styled = match cell_fraction[cell_column] {
                    Some(fraction) => match self.style_for(fraction) {
                        Some(style) => style.paint(&glyph.to_string()),
                        None => glyph.to_string(),
                    },
                    None => glyph.to_string(),
                };
                line.push_str(&styled);
            }
            rows.push(truncate_to_width(&line, width, Ellipsis::Omit));
        }
        rows
    }
}

/// The 0–1 position of `value` in `min..=max`; non-finite values are gaps.
fn fraction(value: f64, min: f64, max: f64) -> Option<f64> {
    if !value.is_finite() || !min.is_finite() || !max.is_finite() {
        return None;
    }
    let span = max - min;
    if span > 0.0 {
        Some(((value - min) / span).clamp(0.0, 1.0))
    } else if value > min {
        Some(1.0)
    } else {
        Some(0.0)
    }
}

impl Component for Sparkline {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 || self.values.is_empty() {
            return Vec::new();
        }
        let values = fit_spark_values(&self.values, width);
        let max = self.effective_max(&values);
        let fractions: Vec<Option<f64>> = values.iter().map(|value| fraction(*value, self.min, max)).collect();
        match self.mode {
            SparklineMode::Block => self.render_block(&fractions, width),
            SparklineMode::Braille => self.render_braille(&fractions, width),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    #[test]
    fn renders_block_glyphs() {
        let mut spark = Sparkline::new();
        spark.set_values(vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        spark.set_maximum(Some(4.0));
        assert_eq!(spark.render(5), vec!["▁▃▅▇█"]);
    }

    #[test]
    fn stacks_blocks_over_height() {
        let mut spark = Sparkline::new();
        spark.set_values(vec![0.0, 2.0, 4.0]);
        spark.set_maximum(Some(4.0));
        spark.set_height(3);
        assert_eq!(spark.render(3), vec!["  █", " ██", "███"]);
    }

    #[test]
    fn renders_braille_dots() {
        let mut spark = Sparkline::new();
        spark.set_values(vec![4.0]);
        spark.set_maximum(Some(4.0));
        spark.set_mode(SparklineMode::Braille);
        spark.set_height(1);
        assert_eq!(spark.render(1), vec!["⠁"]);
        spark.set_height(4);
        assert_eq!(spark.render(1), vec!["⡇"]);
        spark.set_values(vec![2.0]);
        assert_eq!(spark.render(1), vec!["⡄"]);
    }

    #[test]
    fn applies_color_and_gradient() {
        let mut spark = Sparkline::new();
        spark.set_values(vec![4.0]);
        spark.set_maximum(Some(4.0));
        spark.set_color(Style::fg(Color::Red));
        assert_eq!(spark.render(1), vec!["\x1b[31m█\x1b[0m"]);

        let mut gradient = Sparkline::new();
        gradient.set_values(vec![0.0, 4.0]);
        gradient.set_maximum(Some(4.0));
        gradient.set_gradient(vec![Style::fg(Color::Red), Style::fg(Color::Green)]);
        assert_eq!(
            gradient.render(2),
            vec!["\x1b[31m▁\x1b[0m\x1b[32m█\x1b[0m"]
        );
    }

    #[test]
    fn fits_long_series_to_width() {
        let mut spark = Sparkline::new();
        spark.set_values((0..40).map(|value| value as f64).collect());
        let rendered = spark.render(8);
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].chars().count() <= 8);
        assert_eq!(rendered[0].chars().count(), 8);
    }

    #[test]
    fn handles_empty_and_narrow_widths() {
        let mut spark = Sparkline::new();
        assert!(spark.render(5).is_empty());
        spark.set_values(vec![1.0, 2.0, 3.0]);
        assert!(spark.render(0).is_empty());
        for width in [1, 2, 3] {
            for line in spark.render(width) {
                assert!(line.chars().count() <= width as usize, "row too wide: {line:?}");
            }
        }
    }

    #[test]
    fn reflects_state_changes_and_stays_deterministic() {
        let mut spark = Sparkline::new();
        spark.set_values(vec![0.0, 2.0]);
        spark.set_maximum(Some(2.0));
        let first = spark.render(2);
        assert_eq!(first, spark.render(2));
        assert_eq!(first, vec!["▁█"]);
        spark.set_values(vec![2.0, 2.0]);
        assert_eq!(spark.render(2), vec!["██"]);
        spark.set_values(vec![0.0, 2.0]);
        spark.set_height(2);
        assert_eq!(spark.render(2), vec![" █", "██"]);
    }

    #[test]
    fn draws_gaps_for_non_finite_values() {
        let mut spark = Sparkline::new();
        spark.set_values(vec![f64::NAN, 4.0]);
        spark.set_maximum(Some(4.0));
        assert_eq!(spark.render(2), vec![" █"]);
        spark.set_gap("-");
        assert_eq!(spark.render(2), vec!["-█"]);
    }
}
