//! Context-usage chrome: level thresholds, the status-line gauge, and the
//! token-occupancy map.
//!
//! Ported from `l/src/chrome/context-thresholds.ts`, `l/src/chrome/context-map.ts`,
//! `l/src/chrome/context-presentation.ts`, and the gauge fill in
//! `l/src/status-line/component.ts`. Glyphs are the Unicode preset values from
//! `l/src/theme/symbols.ts`; they are carried here because `symbols.rs` does not
//! yet expose the `context.*` group.

use crate::component::Component;
use crate::status_line::segments::format_number;
use crate::style::{Color, Style};
use crate::text_shape::{truncate_to_width, Ellipsis};

/// Glyph marking where speculative compaction starts.
pub const CONTEXT_SPECULATION_GLYPH: &str = "╎";
/// Glyph marking where auto-compaction fires.
pub const CONTEXT_COMPACTION_GLYPH: &str = "┃";
/// Glyph for prompt/tool content segments.
pub const CONTEXT_CONTENT_GLYPH: &str = "⛁";
/// Glyph for message segments.
pub const CONTEXT_MESSAGES_GLYPH: &str = "⛃";
/// Glyph for the compaction buffer.
pub const CONTEXT_BUFFER_GLYPH: &str = "⛝";
/// Glyph for the free remainder.
pub const CONTEXT_FREE_GLYPH: &str = "⛶";

/// Horizontal rule glyph used by the gauge.
const GAUGE_RULE: char = '─';

/// The `context.*` glyph group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextSymbols {
    /// Speculative-compaction marker.
    pub speculation: String,
    /// Auto-compaction marker.
    pub compaction: String,
    /// Content segment.
    pub content: String,
    /// Message segment.
    pub messages: String,
    /// Buffer segment.
    pub buffer: String,
    /// Free segment.
    pub free: String,
}

impl Default for ContextSymbols {
    fn default() -> Self {
        ContextSymbols {
            speculation: CONTEXT_SPECULATION_GLYPH.to_string(),
            compaction: CONTEXT_COMPACTION_GLYPH.to_string(),
            content: CONTEXT_CONTENT_GLYPH.to_string(),
            messages: CONTEXT_MESSAGES_GLYPH.to_string(),
            buffer: CONTEXT_BUFFER_GLYPH.to_string(),
            free: CONTEXT_FREE_GLYPH.to_string(),
        }
    }
}

/// Occupancy level of a context window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextUsageLevel {
    /// Well below every threshold.
    Normal,
    /// At or past the warning threshold.
    Warning,
    /// At or past the purple threshold.
    Purple,
    /// At or past the error threshold.
    Error,
}

/// Warning/error shares (0–1) of a context window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextMeterThresholds {
    /// Share where the level becomes warning.
    pub warn: f64,
    /// Share where the level becomes error.
    pub bad: f64,
}

const WARNING_PERCENT: f64 = 50.0;
const WARNING_TOKENS: u64 = 150_000;
const PURPLE_PERCENT: f64 = 70.0;
const PURPLE_TOKENS: u64 = 270_000;
const ERROR_PERCENT: f64 = 90.0;
const ERROR_TOKENS: u64 = 500_000;

fn reaches_threshold(percent: f64, window: u64, percent_threshold: f64, token_threshold: u64) -> bool {
    if !percent.is_finite() || percent <= 0.0 {
        return false;
    }
    if window == 0 {
        return percent >= percent_threshold;
    }
    let token_percent = (token_threshold as f64 / window as f64) * 100.0;
    percent >= percent_threshold.min(token_percent)
}

/// Level for `percent` against `window`.
pub fn context_usage_level(percent: f64, window: u64) -> ContextUsageLevel {
    if reaches_threshold(percent, window, ERROR_PERCENT, ERROR_TOKENS) {
        ContextUsageLevel::Error
    } else if reaches_threshold(percent, window, PURPLE_PERCENT, PURPLE_TOKENS) {
        ContextUsageLevel::Purple
    } else if reaches_threshold(percent, window, WARNING_PERCENT, WARNING_TOKENS) {
        ContextUsageLevel::Warning
    } else {
        ContextUsageLevel::Normal
    }
}

fn threshold_share(window: u64, percent_threshold: f64, token_threshold: u64) -> f64 {
    let percent = if window > 0 {
        percent_threshold.min((token_threshold as f64 / window as f64) * 100.0)
    } else {
        percent_threshold
    };
    percent / 100.0
}

/// Warning/error shares of `window` for the context meter tone.
pub fn context_meter_thresholds(window: u64) -> ContextMeterThresholds {
    ContextMeterThresholds {
        warn: threshold_share(window, WARNING_PERCENT, WARNING_TOKENS),
        bad: threshold_share(window, ERROR_PERCENT, ERROR_TOKENS),
    }
}

/// `<percent>%/<window>` when the window is known, else `<tokens>/?`.
pub fn format_context_usage(percent: Option<f64>, window: u64, used_tokens: Option<u64>) -> String {
    if window == 0 {
        return format!("{}/?", format_number(used_tokens.unwrap_or(0) as f64));
    }
    let label = match percent {
        Some(percent) => format!("{percent:.1}%"),
        None => "?".to_string(),
    };
    format!("{label}/{}", format_number(window as f64))
}

/// Gauge display mode, mirroring `contextLine` settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextLineMode {
    /// Solid accent rule, no context feedback.
    Off,
    /// Used share against the window, no labels.
    Percentage,
    /// Percentage plus compaction markers.
    Annotated,
    /// Markers plus absorbed percentage/window labels.
    Embedded,
}

impl Default for ContextLineMode {
    fn default() -> Self {
        ContextLineMode::Embedded
    }
}

/// Paints for one gauge.
#[derive(Clone, Debug)]
pub struct ContextGaugeStyles {
    /// Used portion of the bar.
    pub used: Style,
    /// Unused portion of the bar and the `off` rule.
    pub unused: Style,
    /// Speculation marker.
    pub speculation: Style,
    /// Compaction marker and window label.
    pub threshold: Style,
    /// Percentage label; error colour when usage overflows the window.
    pub label: Style,
    /// Overflowing percentage label.
    pub overflow: Style,
}

impl Default for ContextGaugeStyles {
    fn default() -> Self {
        ContextGaugeStyles {
            used: Style::fg(Color::Cyan),
            unused: Style::fg(Color::DarkGray),
            speculation: Style::fg(Color::DarkGray),
            threshold: Style::fg(Color::Cyan),
            label: Style::fg(Color::Cyan),
            overflow: Style::fg(Color::Red),
        }
    }
}

fn embedded_context_gauge_min_width(percent: Option<f64>, window: u64) -> usize {
    let percent_width = match percent {
        Some(percent) => {
            let text = if percent > 0.0 && percent < 1.0 {
                format!("{percent:.1}%")
            } else {
                format!("{}%", percent.round() as i64)
            };
            text.chars().count() + 2
        }
        None => 0,
    };
    percent_width + format_number(window as f64).chars().count() + 2
}

/// The status-line context gauge: a rule whose lit share tracks usage, with
/// optional compaction markers and absorbed labels.
pub struct ContextGauge {
    window: u64,
    used_tokens: u64,
    percent: Option<f64>,
    mode: ContextLineMode,
    auto_compact_enabled: bool,
    speculation_percent: Option<f64>,
    compaction_percent: Option<f64>,
    symbols: ContextSymbols,
    styles: ContextGaugeStyles,
    memo: Option<(u16, Vec<String>)>,
}

impl Default for ContextGauge {
    fn default() -> Self {
        ContextGauge::new(0, 0)
    }
}

impl ContextGauge {
    /// A gauge for a `window` (tokens) with `used_tokens` consumed.
    pub fn new(window: u64, used_tokens: u64) -> Self {
        ContextGauge {
            window,
            used_tokens,
            percent: None,
            mode: ContextLineMode::default(),
            auto_compact_enabled: false,
            speculation_percent: None,
            compaction_percent: None,
            symbols: ContextSymbols::default(),
            styles: ContextGaugeStyles::default(),
            memo: None,
        }
    }

    /// Replace the window size.
    pub fn set_window(&mut self, window: u64) {
        self.window = window;
        self.memo = None;
    }

    /// Replace the used-token count.
    pub fn set_used_tokens(&mut self, used_tokens: u64) {
        self.used_tokens = used_tokens;
        self.memo = None;
    }

    /// Set the explicit percentage; `None` derives it from used/window.
    pub fn set_percent(&mut self, percent: Option<f64>) {
        self.percent = percent;
        self.memo = None;
    }

    /// Replace the display mode.
    pub fn set_mode(&mut self, mode: ContextLineMode) {
        self.mode = mode;
        self.memo = None;
    }

    /// Enable or disable auto-compaction markers.
    pub fn set_auto_compact_enabled(&mut self, enabled: bool) {
        self.auto_compact_enabled = enabled;
        self.memo = None;
    }

    /// Set the speculative-compaction share (percent) when known.
    pub fn set_speculation_percent(&mut self, percent: Option<f64>) {
        self.speculation_percent = percent;
        self.memo = None;
    }

    /// Set the auto-compaction threshold share (percent) when known.
    pub fn set_compaction_percent(&mut self, percent: Option<f64>) {
        self.compaction_percent = percent;
        self.memo = None;
    }

    /// Replace the glyph group.
    pub fn set_symbols(&mut self, symbols: ContextSymbols) {
        self.symbols = symbols;
        self.memo = None;
    }

    /// Replace the paints.
    pub fn set_styles(&mut self, styles: ContextGaugeStyles) {
        self.styles = styles;
        self.memo = None;
    }

    /// The resolved percentage, deriving it when not set explicitly.
    pub fn resolved_percent(&self) -> Option<f64> {
        self.percent.or_else(|| {
            if self.window == 0 {
                None
            } else {
                Some(self.used_tokens as f64 / self.window as f64 * 100.0)
            }
        })
    }

    /// The occupancy level for the resolved usage.
    pub fn level(&self) -> ContextUsageLevel {
        match self.resolved_percent() {
            Some(percent) => context_usage_level(percent, self.window),
            None => ContextUsageLevel::Normal,
        }
    }

    fn gauge_row(&self, width: u16) -> String {
        let gap = width as usize;
        let percent = self.resolved_percent();
        if self.mode == ContextLineMode::Off || (percent.is_none() && self.window == 0) {
            return self.styles.unused.paint(&GAUGE_RULE.to_string().repeat(gap));
        }
        let clamped = percent.unwrap_or(0.0).clamp(0.0, 100.0);
        let overflow = percent.is_some_and(|value| value > 100.0);
        let used_count = ((clamped / 100.0) * gap as f64).round() as usize;
        let used_count = used_count.clamp(1, gap);

        let mut cells: Vec<(char, Style)> = (0..gap)
            .map(|index| {
                (
                    GAUGE_RULE,
                    if index < used_count {
                        self.styles.used
                    } else {
                        self.styles.unused
                    },
                )
            })
            .collect();

        let mut threshold_idx: Option<usize> = None;
        let mut speculation_idx: Option<usize> = None;
        if matches!(
            self.mode,
            ContextLineMode::Annotated | ContextLineMode::Embedded
        ) && self.auto_compact_enabled
            && gap >= 8
        {
            if let Some(compaction) = self.compaction_percent {
                let cell = ((compaction / 100.0) * gap as f64).round() as usize;
                let cell = cell.min(gap - 1);
                threshold_idx = Some(cell);
                if let Some(speculation) = self.speculation_percent {
                    let spec = ((speculation / 100.0) * gap as f64).round() as usize;
                    let spec = spec.min(gap - 1);
                    if spec != cell {
                        speculation_idx = Some(spec);
                    }
                }
            }
        }

        let mut percent_label: Option<(String, usize)> = None;
        let mut window_label: Option<(String, usize)> = None;
        if self.mode == ContextLineMode::Embedded && gap >= embedded_context_gauge_min_width(percent, self.window)
        {
            let window_text = format_number(self.window as f64);
            let window_start = gap - window_text.chars().count() - 1;
            window_label = Some((window_text, window_start));
            if let Some(percent) = percent {
                let text = if percent > 0.0 && percent < 1.0 {
                    format!("{percent:.1}%")
                } else {
                    format!("{}%", percent.round() as i64)
                };
                let start = window_start.saturating_sub(text.chars().count() + 1);
                percent_label = Some((text, start));
            }
        }

        // Window label sits on the threshold colour.
        if let Some((text, start)) = &window_label {
            paint_chars(&mut cells, text, *start, self.styles.threshold);
        }
        if let Some(index) = speculation_idx {
            paint_glyph(&mut cells, &self.symbols.speculation, index, self.styles.speculation);
        }
        if let Some(index) = threshold_idx {
            paint_glyph(&mut cells, &self.symbols.compaction, index, self.styles.threshold);
        }
        // Percentage label wins every cell it covers.
        if let Some((text, start)) = &percent_label {
            let style = if overflow {
                self.styles.overflow
            } else {
                self.styles.label
            };
            paint_chars(&mut cells, text, *start, style);
        }

        let mut out = String::new();
        let mut index = 0usize;
        while index < cells.len() {
            let style = cells[index].1;
            let start = index;
            while index < cells.len() && cells[index].1 == style {
                index += 1;
            }
            let text: String = cells[start..index].iter().map(|(glyph, _)| *glyph).collect();
            out.push_str(&style.paint(&text));
        }
        out
    }
}

fn paint_chars(cells: &mut [(char, Style)], text: &str, start: usize, style: Style) {
    for (offset, ch) in text.chars().enumerate() {
        if let Some(cell) = cells.get_mut(start + offset) {
            *cell = (ch, style);
        }
    }
}

fn paint_glyph(cells: &mut [(char, Style)], glyph: &str, index: usize, style: Style) {
    if let Some(ch) = glyph.chars().next() {
        if let Some(cell) = cells.get_mut(index) {
            *cell = (ch, style);
        }
    }
}

impl Component for ContextGauge {
    fn render(&mut self, width: u16) -> Vec<String> {
        if let Some((memo_width, rows)) = &self.memo {
            if *memo_width == width {
                return rows.clone();
            }
        }
        if width == 0 {
            return Vec::new();
        }
        let rows = vec![truncate_to_width(
            &self.gauge_row(width),
            width as usize,
            Ellipsis::Omit,
        )];
        self.memo = Some((width, rows.clone()));
        rows
    }

    fn invalidate(&mut self) {
        self.memo = None;
    }
}

/// Occupancy category of a context segment, matching
/// `CONTEXT_PRESENTATION` plus `buffer`/`free`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextSegmentId {
    /// System prompt.
    SystemPrompt,
    /// Instruction files.
    InstructionFiles,
    /// Skills.
    Skills,
    /// Built-in tools.
    BuiltInTools,
    /// Custom tools.
    CustomTools,
    /// User / developer messages.
    UserMessages,
    /// Assistant messages.
    AssistantMessages,
    /// Assistant thinking.
    AssistantThinking,
    /// Tool calls.
    ToolCalls,
    /// Tool output.
    ToolOutput,
    /// Compaction buffer.
    Buffer,
    /// Free remainder.
    Free,
}

impl ContextSegmentId {
    /// The glyph for this segment.
    pub fn glyph(self, symbols: &ContextSymbols) -> &str {
        match self {
            ContextSegmentId::SystemPrompt
            | ContextSegmentId::InstructionFiles
            | ContextSegmentId::Skills
            | ContextSegmentId::BuiltInTools
            | ContextSegmentId::CustomTools
            | ContextSegmentId::AssistantThinking
            | ContextSegmentId::ToolCalls => &symbols.content,
            ContextSegmentId::UserMessages
            | ContextSegmentId::AssistantMessages
            | ContextSegmentId::ToolOutput => &symbols.messages,
            ContextSegmentId::Buffer => &symbols.buffer,
            ContextSegmentId::Free => &symbols.free,
        }
    }

    /// The theme colour for this segment.
    pub fn color(self) -> Color {
        match self {
            ContextSegmentId::SystemPrompt | ContextSegmentId::UserMessages => Color::Cyan,
            ContextSegmentId::InstructionFiles | ContextSegmentId::ToolOutput => Color::DarkGray,
            ContextSegmentId::Skills => Color::Green,
            ContextSegmentId::BuiltInTools | ContextSegmentId::ToolCalls | ContextSegmentId::Buffer => {
                Color::Yellow
            }
            ContextSegmentId::CustomTools | ContextSegmentId::AssistantThinking => Color::Magenta,
            ContextSegmentId::AssistantMessages => Color::White,
            ContextSegmentId::Free => Color::DarkGray,
        }
    }
}

/// One occupancy cell of a [`ContextMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMapCell {
    /// Segment covering most of the cell.
    pub id: ContextSegmentId,
    /// Whether the segment covers at least 70% of the cell.
    pub full: bool,
}

/// Lay `segments` over a `columns` × `rows` grid representing `window` tokens.
///
/// Returns an empty vector when the window or grid is unusable.
pub fn plan_context_map(
    segments: &[(ContextSegmentId, u64)],
    window: u64,
    columns: usize,
    rows: usize,
) -> Vec<ContextMapCell> {
    if window == 0 || columns == 0 || rows == 0 {
        return Vec::new();
    }
    let cell_count = columns * rows;
    let tokens_per_cell = window as f64 / cell_count as f64;
    let mut ranges: Vec<(ContextSegmentId, f64, f64)> = Vec::new();
    let mut cursor = 0.0;
    for (id, tokens) in segments {
        if *tokens == 0 {
            continue;
        }
        let end = (cursor + *tokens as f64).min(window as f64);
        if end > cursor {
            ranges.push((*id, cursor, end));
        }
        cursor = end;
        if cursor >= window as f64 {
            break;
        }
    }

    let mut cells = Vec::new();
    let mut first = 0usize;
    for index in 0..cell_count {
        let start = index as f64 * tokens_per_cell;
        let end = (index + 1) as f64 * tokens_per_cell;
        while first < ranges.len() && ranges[first].2 <= start {
            first += 1;
        }
        let mut winner: Option<ContextSegmentId> = None;
        let mut overlap = 0.0f64;
        for range in ranges.iter().skip(first) {
            if range.1 >= end {
                break;
            }
            let covered = end.min(range.2) - start.max(range.1);
            if covered > overlap {
                overlap = covered;
                winner = Some(range.0);
            }
        }
        if let Some(id) = winner {
            cells.push(ContextMapCell {
                id,
                full: overlap / tokens_per_cell >= 0.7,
            });
        }
    }
    cells
}

/// A grid of occupancy glyphs for the `/context` map.
pub struct ContextMap {
    segments: Vec<(ContextSegmentId, u64)>,
    window: u64,
    columns: usize,
    rows: usize,
    symbols: ContextSymbols,
    memo: Option<(u16, Vec<String>)>,
}

impl Default for ContextMap {
    fn default() -> Self {
        ContextMap::new(0, 4, 2)
    }
}

impl ContextMap {
    /// A map of `window` tokens over `columns` × `rows` cells.
    pub fn new(window: u64, columns: usize, rows: usize) -> Self {
        ContextMap {
            segments: Vec::new(),
            window,
            columns,
            rows,
            symbols: ContextSymbols::default(),
            memo: None,
        }
    }

    /// Replace the window size.
    pub fn set_window(&mut self, window: u64) {
        self.window = window;
        self.memo = None;
    }

    /// Replace the segments as `(id, tokens)` pairs.
    pub fn set_segments(&mut self, segments: &[(ContextSegmentId, u64)]) {
        self.segments = segments.to_vec();
        self.memo = None;
    }

    /// Replace the glyph group.
    pub fn set_symbols(&mut self, symbols: ContextSymbols) {
        self.symbols = symbols;
        self.memo = None;
    }

    /// The planned cells for the current configuration.
    pub fn cells(&self) -> Vec<ContextMapCell> {
        plan_context_map(&self.segments, self.window, self.columns, self.rows)
    }
}

impl Component for ContextMap {
    fn render(&mut self, width: u16) -> Vec<String> {
        if let Some((memo_width, rows)) = &self.memo {
            if *memo_width == width {
                return rows.clone();
            }
        }
        if width == 0 || self.columns == 0 {
            return Vec::new();
        }
        let cells = self.cells();
        let mut rows = Vec::with_capacity(self.rows);
        for row in 0..self.rows {
            let mut line = String::new();
            for column in 0..self.columns {
                let cell = cells.get(row * self.columns + column);
                match cell {
                    Some(cell) => {
                        let style = if cell.full {
                            Style::fg(cell.id.color())
                        } else {
                            Style::fg(cell.id.color()).dim()
                        };
                        line.push_str(&style.paint(cell.id.glyph(&self.symbols)));
                    }
                    None => line.push(' '),
                }
            }
            rows.push(truncate_to_width(&line, width as usize, Ellipsis::Omit));
        }
        self.memo = Some((width, rows.clone()));
        rows
    }

    fn invalidate(&mut self) {
        self.memo = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_percent_and_token_thresholds() {
        assert_eq!(context_usage_level(10.0, 200_000), ContextUsageLevel::Normal);
        assert_eq!(context_usage_level(50.0, 200_000), ContextUsageLevel::Warning);
        // 150K of a 200K window is 75% but the token threshold trips first.
        assert_eq!(context_usage_level(75.0, 200_000), ContextUsageLevel::Purple);
        assert_eq!(context_usage_level(95.0, 200_000), ContextUsageLevel::Error);
        assert_eq!(context_usage_level(0.0, 200_000), ContextUsageLevel::Normal);
    }

    #[test]
    fn unknown_window_uses_percent_thresholds() {
        assert_eq!(context_usage_level(60.0, 0), ContextUsageLevel::Warning);
        assert_eq!(context_usage_level(92.0, 0), ContextUsageLevel::Error);
    }

    #[test]
    fn meter_thresholds_are_shares() {
        let thresholds = context_meter_thresholds(200_000);
        assert!((thresholds.warn - 0.5).abs() < 1e-9);
        assert!((thresholds.bad - 0.9).abs() < 1e-9);
        // A large window trips on the token threshold first.
        let small = context_meter_thresholds(1_000_000);
        assert!((small.warn - 0.15).abs() < 1e-9);
        assert!((small.bad - 0.5).abs() < 1e-9);
    }

    #[test]
    fn usage_label_formats_known_and_unknown_windows() {
        assert_eq!(format_context_usage(Some(12.34), 200_000, None), "12.3%/200K");
        assert_eq!(format_context_usage(None, 200_000, Some(5_000)), "?/200K");
        assert_eq!(format_context_usage(Some(12.34), 0, Some(5_000)), "5K/?");
    }

    #[test]
    fn gauge_off_mode_is_a_solid_rule() {
        let mut gauge = ContextGauge::new(100, 40);
        gauge.set_mode(ContextLineMode::Off);
        assert_eq!(gauge.render(10), vec!["\x1b[90m──────────\x1b[0m"]);
    }

    #[test]
    fn gauge_lights_the_used_share() {
        let mut gauge = ContextGauge::new(100, 25);
        gauge.set_mode(ContextLineMode::Percentage);
        let row = gauge.render(10).remove(0);
        assert_eq!(visible_rule_cells(&row), 10);
        assert!(row.contains("\x1b[36m───"), "{row}");
        assert!(row.contains("\x1b[90m───────"), "{row}");
    }

    #[test]
    fn gauge_marks_compaction_boundaries() {
        let mut gauge = ContextGauge::new(200_000, 100_000);
        gauge.set_mode(ContextLineMode::Annotated);
        gauge.set_auto_compact_enabled(true);
        gauge.set_compaction_percent(Some(80.0));
        gauge.set_speculation_percent(Some(70.0));
        let row = gauge.render(20).remove(0);
        assert!(row.contains(CONTEXT_COMPACTION_GLYPH), "{row}");
        assert!(row.contains(CONTEXT_SPECULATION_GLYPH), "{row}");
    }

    #[test]
    fn gauge_threshold_wins_a_shared_cell() {
        let mut gauge = ContextGauge::new(200_000, 100_000);
        gauge.set_mode(ContextLineMode::Annotated);
        gauge.set_auto_compact_enabled(true);
        gauge.set_compaction_percent(Some(80.0));
        gauge.set_speculation_percent(Some(80.0));
        let row = gauge.render(20).remove(0);
        assert!(row.contains(CONTEXT_COMPACTION_GLYPH), "{row}");
        assert!(!row.contains(CONTEXT_SPECULATION_GLYPH), "{row}");
    }

    #[test]
    fn gauge_embeds_labels() {
        let mut gauge = ContextGauge::new(200_000, 100_000);
        gauge.set_mode(ContextLineMode::Embedded);
        let row = gauge.render(30).remove(0);
        assert!(row.contains("50%"), "{row}");
        assert!(row.contains("200K"), "{row}");
    }

    #[test]
    fn gauge_rows_stay_within_width() {
        let mut gauge = ContextGauge::new(200_000, 100_000);
        gauge.set_auto_compact_enabled(true);
        gauge.set_compaction_percent(Some(80.0));
        for width in [1u16, 4, 8, 12, 20] {
            for row in gauge.render(width) {
                assert!(
                    crate::text_shape::visible_width(&row) <= width as usize,
                    "width {width}: {row:?}"
                );
            }
        }
        assert!(gauge.render(0).is_empty());
    }

    #[test]
    fn map_plans_segments_over_the_grid() {
        let cells = plan_context_map(
            &[
                (ContextSegmentId::SystemPrompt, 50),
                (ContextSegmentId::UserMessages, 30),
                (ContextSegmentId::Buffer, 10),
                (ContextSegmentId::Free, 10),
            ],
            100,
            5,
            2,
        );
        assert_eq!(cells.len(), 10);
        assert_eq!(cells[0].id, ContextSegmentId::SystemPrompt);
        assert!(cells[0].full);
        assert_eq!(cells[9].id, ContextSegmentId::Free);
    }

    #[test]
    fn map_ignores_unusable_grids() {
        assert!(plan_context_map(&[], 0, 4, 2).is_empty());
        assert!(plan_context_map(&[], 100, 0, 2).is_empty());
        assert!(plan_context_map(&[], 100, 4, 0).is_empty());
    }

    #[test]
    fn map_renders_rows_of_glyphs() {
        let mut map = ContextMap::new(100, 4, 2);
        map.set_segments(&[
            (ContextSegmentId::SystemPrompt, 50),
            (ContextSegmentId::Free, 50),
        ]);
        let rows = map.render(8);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains(CONTEXT_CONTENT_GLYPH), "{}", rows[0]);
        assert!(rows[1].contains(CONTEXT_FREE_GLYPH), "{}", rows[1]);
        assert!(map.render(0).is_empty());
    }

    fn visible_rule_cells(row: &str) -> usize {
        row.chars().filter(|ch| *ch == '─').count()
    }
}
