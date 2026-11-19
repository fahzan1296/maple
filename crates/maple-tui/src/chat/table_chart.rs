//! Charts drawn under assistant tables.
//!
//! Reference: `l/src/chat/table-chart.ts`.
//!
//! The host picks how tables become charts (`tui.autoGraph`).
//! [`split_table_charts`] cuts a Markdown block after each top-level table that
//! gets a chart; [`lookup_table_chart`] plans, builds and draws that chart —
//! locally under [`TableChartMode::Always`], through the host's model-backed
//! [`TableChartPlanner`] under [`TableChartMode::Smart`] for the tables
//! [`should_judge`] picks — and caches it per table source.
//! [`TableChartFigure`] shows it as a terminal image; [`chart_fence`] hands a
//! terminal the SVG as a ```svg fence, which it draws in the palette it shows.
//!
//! Deviations: this crate has no SVG renderer, so drawing is injected
//! ([`ChartSvgRenderer`]) — without one a table gets no chart. The planner is
//! synchronous (`None` = no judge to ask right now), because the render path
//! here has no promise to wait on, so a lookup never stays pending. Tables are
//! found by scanning lines for a GFM delimiter row instead of lexing the whole
//! document.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::charts::chart_plan::{
    build_chart, plan_chart, should_judge, worth_charting, ChartPlan, ChartSpec,
};
use crate::charts::table_data::{analyze_table, TableAnalysis};
use crate::chat::svg_figure::{SvgFigure, SvgFigureOptions};
use crate::component::Component;
use crate::components::markdown::{Markdown, MarkdownTheme};

/// Tables cached per source; a long session keeps its recent charts without
/// growing without bound.
const CACHE_LIMIT: usize = 256;

/// How assistant tables become charts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableChartMode {
    /// Ask the host's planner about tables the local guess is unsure of.
    Smart,
    /// Draw the local guess for every table worth charting.
    Always,
    /// Leave tables as tables.
    #[default]
    Off,
}

impl TableChartMode {
    /// Parse the host's `tui.autoGraph` value; unknown values mean off.
    pub fn parse(value: &str) -> Self {
        match value {
            "smart" => TableChartMode::Smart,
            "always" => TableChartMode::Always,
            _ => TableChartMode::Off,
        }
    }

    /// The host's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            TableChartMode::Smart => "smart",
            TableChartMode::Always => "always",
            TableChartMode::Off => "off",
        }
    }
}

/// One table handed to the planner.
#[derive(Clone, Debug)]
pub struct TableChartRequest {
    /// The table's Markdown source.
    pub markdown: String,
    /// The parsed table.
    pub table: TableAnalysis,
    /// The local best guess; the planner may keep it.
    pub guess: Option<ChartPlan>,
}

/// Picks the chart for a table under [`TableChartMode::Smart`].
///
/// `None` means there is no judge to ask right now, which keeps the local guess
/// at once; `Some(None)` rejects the chart; `Some(Some(plan))` is the pick.
pub type TableChartPlanner = Rc<dyn Fn(&TableChartRequest) -> Option<Option<ChartPlan>>>;

/// Builds a table's chart SVG; absent means tables get no chart.
pub type ChartSvgRenderer = Rc<dyn Fn(&ChartSpec) -> Option<String>>;

/// One run of a Markdown block: prose (ending at a charted table), or the chart
/// drawn after that table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChartSegment {
    /// Prose, the charted table included.
    Markdown(String),
    /// The table's own Markdown source, whose chart follows it.
    Chart(String),
}

/// A table read once per source: its local guess and, once known, its chart.
#[derive(Clone)]
struct ChartEntry {
    analysis: TableAnalysis,
    guess: Option<ChartPlan>,
    /// The host's planner picks this table's chart.
    smart: bool,
    /// The chart's SVG; `None` until drawn or judged chartless.
    chart: Option<String>,
    /// Whether `chart` is final.
    decided: bool,
}

impl ChartEntry {
    /// Whether the table still may get a chart drawn under it.
    fn chartable(&self) -> bool {
        self.guess.is_some() && (self.chart.is_some() || !self.decided)
    }
}

#[derive(Default)]
struct ChartState {
    mode: TableChartMode,
    planner: Option<TableChartPlanner>,
    renderer: Option<ChartSvgRenderer>,
    charts: HashMap<String, ChartEntry>,
}

thread_local! {
    static STATE: RefCell<ChartState> = RefCell::new(ChartState::default());
}

/// Set how tables become charts (the host's `tui.autoGraph`), its `smart`
/// planner and its SVG renderer; the host rebuilds the transcript after.
pub fn set_table_charts(
    mode: TableChartMode,
    planner: Option<TableChartPlanner>,
    renderer: Option<ChartSvgRenderer>,
) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.mode = mode;
        state.planner = planner;
        state.renderer = renderer;
        state.charts.clear();
    });
}

/// How assistant tables currently become charts.
pub fn table_chart_mode() -> TableChartMode {
    STATE.with(|state| state.borrow().mode)
}

/// Whether `markdown` may hold a table [`split_table_charts`] would chart.
pub fn has_chart_table(markdown: &str) -> bool {
    table_chart_mode() != TableChartMode::Off
        && markdown.contains('|')
        && !find_tables(markdown).is_empty()
}

/// Split `markdown` after each top-level table that has (or may get) a chart.
///
/// A table still streaming at the block's tail is left whole: rows may follow.
pub fn split_table_charts(markdown: &str, streaming: bool) -> Vec<ChartSegment> {
    if table_chart_mode() == TableChartMode::Off {
        return vec![ChartSegment::Markdown(markdown.to_string())];
    }
    let tables = find_tables(markdown);
    let mut segments = Vec::new();
    let mut prose_start = 0usize;
    for table in &tables {
        let closed = !streaming
            || markdown[table.end..]
                .lines()
                .any(|line| !line.trim().is_empty());
        let entry = closed.then(|| chart_entry(&table.raw)).flatten();
        let Some(entry) = entry else {
            continue;
        };
        if !entry.chartable() {
            continue;
        }
        segments.push(ChartSegment::Markdown(markdown[prose_start..table.end].to_string()));
        segments.push(ChartSegment::Chart(table.raw.clone()));
        prose_start = table.end;
    }
    let tail = &markdown[prose_start..];
    if !tail.trim().is_empty() || segments.is_empty() {
        segments.push(ChartSegment::Markdown(tail.to_string()));
    }
    segments
}

/// The chart under `table`: its SVG, or `None` when it gets no chart.
pub fn lookup_table_chart(table: &str) -> Option<String> {
    if table_chart_mode() == TableChartMode::Off {
        return None;
    }
    let mut entry = chart_entry(table)?;
    if entry.decided {
        return entry.chart;
    }
    let (planner, renderer) = STATE.with(|state| {
        let state = state.borrow();
        (state.planner.clone(), state.renderer.clone())
    });
    let request = TableChartRequest {
        markdown: table.to_string(),
        table: entry.analysis.clone(),
        guess: entry.guess.clone(),
    };
    let (plan, guessed) = match planner.map(|planner| planner(&request)) {
        None => (entry.guess.clone(), true),
        Some(None) => (entry.guess.clone(), true),
        Some(Some(pick)) => (pick, false),
    };
    entry.chart = plan.and_then(|plan| {
        let spec = build_chart(&entry.analysis, &plan)?;
        if guessed && !worth_charting(&spec) {
            return None;
        }
        renderer.as_ref()?(&spec)
    });
    entry.decided = true;
    let chart = entry.chart.clone();
    STATE.with(|state| {
        if let Some(cached) = state.borrow_mut().charts.get_mut(table) {
            cached.chart = entry.chart;
            cached.decided = true;
        }
    });
    chart
}

/// `table` read under the current mode, cached per source. A table the local
/// guess decides is drawn at once; a smart pick waits for its first lookup.
fn chart_entry(table: &str) -> Option<ChartEntry> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(cached) = state.charts.remove(table) {
            // Re-insert so eviction drops the least recently read table.
            let entry = cached;
            state.charts.insert(table.to_string(), entry.clone());
            return Some(entry);
        }
        if state.charts.len() >= CACHE_LIMIT {
            if let Some(oldest) = state.charts.keys().next().cloned() {
                state.charts.remove(&oldest);
            }
        }
        let (header, rows) = parse_table(table)?;
        let analysis = analyze_table(&header, &rows);
        let guess = plan_chart(&analysis);
        let smart = state.mode == TableChartMode::Smart
            && state.planner.is_some()
            && should_judge(&analysis, guess.as_ref());
        let mut entry = ChartEntry {
            analysis,
            guess,
            smart,
            chart: None,
            decided: false,
        };
        if !smart {
            let renderer = state.renderer.clone();
            entry.chart = entry.guess.as_ref().and_then(|guess| {
                let spec = build_chart(&entry.analysis, guess)?;
                if !worth_charting(&spec) {
                    return None;
                }
                renderer.as_ref()?(&spec)
            });
            entry.decided = true;
        }
        state.charts.insert(table.to_string(), entry.clone());
        Some(entry)
    })
}

/// Chart `svg` as a Markdown ```svg fence, for a terminal's `md` node. Its
/// figure tokens stay unresolved: the terminal draws the fence as a figure in
/// the palette it shows the prose in.
pub fn chart_fence(svg: &str) -> String {
    format!("```svg\n{svg}\n```")
}

/// Options of a [`TableChartFigure`].
#[derive(Clone, Default)]
pub struct TableChartFigureOptions {
    /// Markdown for the fallback the chart figure shows without a rasterizer.
    pub markdown: Option<Rc<dyn Fn(&str) -> Markdown>>,
    /// Rows the chart may fill, its separator included, read on every render;
    /// under two hides it.
    pub max_rows: Option<Rc<dyn Fn() -> Option<usize>>>,
    /// The chart's rows changed outside a text update: its pick or raster
    /// landed.
    pub on_change: Option<Rc<dyn Fn()>>,
}

/// The chart under one table, as a terminal image and a blank row below it.
///
/// Looks the chart up on first render — a block rebuilt mid-stream never asks
/// for a pick it drops — and renders nothing until it is drawn, or when the
/// table gets no chart.
pub struct TableChartFigure {
    options: TableChartFigureOptions,
    figure: SvgFigure,
    table: Option<String>,
    /// The chart's SVG; `None` while not looked up.
    chart_state: Option<Option<String>>,
}

impl TableChartFigure {
    /// An empty figure.
    pub fn new(options: TableChartFigureOptions) -> Self {
        let max_rows = options.max_rows.clone();
        let figure_options = SvgFigureOptions {
            markdown: options.markdown.clone(),
            max_rows: max_rows.map(|max_rows| {
                Rc::new(move || max_rows().map(|rows| rows.saturating_sub(1)))
                    as Rc<dyn Fn() -> Option<usize>>
            }),
            on_change: options.on_change.clone(),
            rasterizer: None,
            cell: Default::default(),
        };
        TableChartFigure {
            options,
            table: None,
            figure: SvgFigure::new(figure_options),
            chart_state: None,
        }
    }

    /// The image's share of the caller's ceiling: all but the separator row.
    fn figure_rows(&self) -> Option<usize> {
        self.options
            .max_rows
            .as_ref()
            .and_then(|max_rows| max_rows())
            .map(|rows| rows.saturating_sub(1))
    }

    /// Point the figure at `table`'s Markdown source.
    pub fn update(&mut self, table: &str) {
        if self.table.as_deref() == Some(table) {
            return;
        }
        self.table = Some(table.to_string());
        self.chart_state = None;
    }

    /// Whether the chart is still being looked up, picked or drawn.
    pub fn pending(&self) -> bool {
        self.figure_rows() != Some(0)
            && self.table.is_some()
            && match &self.chart_state {
                None => true,
                Some(None) => false,
                Some(Some(_)) => self.figure.pending(),
            }
    }

    /// Drop caches a theme or layout change invalidates.
    pub fn invalidate(&mut self) {
        self.figure.invalidate();
    }

    /// Stop future rasterization.
    pub fn dispose(&mut self) {
        self.figure.dispose();
    }

    fn resolve(&mut self) {
        let Some(table) = self.table.clone() else {
            return;
        };
        let chart = lookup_table_chart(&table);
        if self.table.as_deref() != Some(table.as_str()) {
            return;
        }
        if let Some(svg) = &chart {
            self.figure.update(svg, true);
        }
        self.chart_state = Some(chart);
    }
}

impl Component for TableChartFigure {
    fn render(&mut self, width: u16) -> Vec<String> {
        if self.figure_rows() == Some(0) {
            return Vec::new();
        }
        if self.chart_state.is_none() {
            self.resolve();
        }
        if self.chart_state.as_ref().map_or(true, |chart| chart.is_none()) {
            return Vec::new();
        }
        let rows = self.figure.render(width);
        if rows.is_empty() {
            return rows;
        }
        let mut out = Vec::with_capacity(rows.len() + 1);
        out.push(String::new());
        out.extend(rows);
        out
    }
}

/// One top-level table inside a Markdown block.
struct TableSpan {
    /// The table's Markdown source, header row first.
    raw: String,
    /// Byte offset just past the table's last row.
    end: usize,
}

/// Whether `line` is a GFM delimiter row (`|---|:--:|`).
fn is_delimiter_row(line: &str) -> bool {
    if !line.contains('|') {
        return false;
    }
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    if indent > 3 {
        return false;
    }
    cells_of(line).iter().all(|cell| {
        let cell = cell.trim_start_matches(':').trim_end_matches(':');
        !cell.is_empty() && cell.chars().all(|ch| ch == '-')
    })
}

/// Split one table row into trimmed cells.
fn cells_of(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|cell| cell.trim().to_string()).collect()
}

/// The top-level tables of `markdown`, fenced code excluded.
fn find_tables(markdown: &str) -> Vec<TableSpan> {
    let lines: Vec<&str> = markdown.split_inclusive('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0usize;
    for line in &lines {
        starts.push(offset);
        offset += line.len();
    }
    let line_end = |index: usize| starts.get(index).copied().unwrap_or(markdown.len());
    let mut tables = Vec::new();
    let mut index = 0usize;
    let mut fence: Option<&str> = None;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        match fence {
            Some(open) => {
                if trimmed.starts_with(open) && trimmed.len() >= open.len() {
                    fence = None;
                }
                index += 1;
                continue;
            }
            None if trimmed.starts_with("```") || trimmed.starts_with("~~~") => {
                fence = Some(&trimmed[..3]);
                index += 1;
                continue;
            }
            None => {}
        }
        let next_is_delimiter = lines
            .get(index + 1)
            .map(|next| is_delimiter_row(next))
            .unwrap_or(false);
        if !lines[index].contains('|') || !next_is_delimiter {
            index += 1;
            continue;
        }
        let start = index;
        index += 2;
        while index < lines.len()
            && lines[index].contains('|')
            && !lines[index].trim().is_empty()
        {
            index += 1;
        }
        let end = if index == start + 2 {
            line_end(index)
        } else {
            line_end(index - 1) + lines[index - 1].len()
        };
        tables.push(TableSpan {
            raw: markdown[starts[start]..end].to_string(),
            end,
        });
    }
    tables
}

/// Parse a table's Markdown source into its header and body cells.
fn parse_table(source: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut lines = source.lines().filter(|line| !line.trim().is_empty());
    let header = cells_of(lines.next()?);
    lines.next()?; // delimiter row
    if header.is_empty() {
        return None;
    }
    let rows: Vec<Vec<String>> = lines.map(cells_of).collect();
    Some((header, rows))
}

/// Test-only helper: a Markdown factory using the default theme.
pub fn default_markdown_factory() -> Rc<dyn Fn(&str) -> Markdown> {
    Rc::new(|text: &str| {
        let mut markdown = Markdown::new();
        markdown.set_text(text);
        markdown.set_theme(MarkdownTheme::default());
        markdown
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "| Quarter | Revenue |\n|---|---|\n| Q1 | 10 |\n| Q2 | 14 |\n";

    fn document() -> String {
        format!("Here you go:\n\n{TABLE}\nThat trend is clear.\n")
    }

    #[test]
    fn mode_parsing_round_trips() {
        for mode in [
            TableChartMode::Smart,
            TableChartMode::Always,
            TableChartMode::Off,
        ] {
            assert_eq!(TableChartMode::parse(mode.as_str()), mode);
        }
        assert_eq!(TableChartMode::parse("nonsense"), TableChartMode::Off);
    }

    #[test]
    fn detects_tables_cheaply() {
        assert!(find_tables(TABLE).len() == 1);
        assert!(find_tables("| not | a table |").is_empty());
        assert!(!has_chart_table(TABLE), "off mode reports none");
    }

    #[test]
    fn fenced_tables_are_ignored() {
        let fenced = format!("```md\n{TABLE}```\n");
        assert!(find_tables(&fenced).is_empty());
    }

    #[test]
    fn parses_header_and_rows() {
        let (header, rows) = parse_table(TABLE).unwrap();
        assert_eq!(header, vec!["Quarter", "Revenue"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], vec!["Q2", "14"]);
    }

    #[test]
    fn splitting_off_mode_is_one_prose_run() {
        let segments = split_table_charts(&document(), false);
        assert_eq!(segments, vec![ChartSegment::Markdown(document())]);
    }

    #[test]
    fn trailing_tables_stay_whole_while_streaming() {
        set_table_charts(
            TableChartMode::Always,
            None,
            Some(Rc::new(|_spec| Some("<svg/>".to_string()))),
        );
        let streaming = split_table_charts(TABLE, true);
        assert_eq!(streaming.len(), 1, "rows may still follow");
        let settled = split_table_charts(&document(), false);
        assert!(settled
            .iter()
            .any(|segment| matches!(segment, ChartSegment::Chart(_))));
        set_table_charts(TableChartMode::Off, None, None);
    }

    #[test]
    fn chart_fence_wraps_svg() {
        assert_eq!(chart_fence("<svg/>"), "```svg\n<svg/>\n```");
    }

    #[test]
    fn figure_stays_empty_without_a_chart() {
        set_table_charts(TableChartMode::Always, None, None);
        let mut figure = TableChartFigure::new(TableChartFigureOptions {
            markdown: Some(default_markdown_factory()),
            ..TableChartFigureOptions::default()
        });
        figure.update(TABLE);
        assert!(figure.render(80).is_empty());
        assert!(!figure.pending());
        set_table_charts(TableChartMode::Off, None, None);
    }

    #[test]
    fn figure_renders_the_chart_under_a_blank_row() {
        set_table_charts(
            TableChartMode::Always,
            None,
            Some(Rc::new(|_spec| Some("<svg></svg>".to_string()))),
        );
        let mut figure = TableChartFigure::new(TableChartFigureOptions {
            markdown: Some(default_markdown_factory()),
            max_rows: Some(Rc::new(|| Some(12))),
            on_change: None,
        });
        figure.update(TABLE);
        let rows = figure.render(80);
        assert!(!rows.is_empty());
        assert_eq!(rows[0], "");
        set_table_charts(TableChartMode::Off, None, None);
    }

    #[test]
    fn lookups_are_cached_per_source() {
        let calls = Rc::new(RefCell::new(0usize));
        let counted = calls.clone();
        set_table_charts(
            TableChartMode::Always,
            None,
            Some(Rc::new(move |_spec| {
                *counted.borrow_mut() += 1;
                Some("<svg/>".to_string())
            })),
        );
        let _ = lookup_table_chart(TABLE);
        let _ = lookup_table_chart(TABLE);
        assert_eq!(*calls.borrow(), 1, "the second lookup hits the cache");
        set_table_charts(TableChartMode::Off, None, None);
    }

    #[test]
    fn smart_rejection_never_yields_a_chart() {
        set_table_charts(
            TableChartMode::Smart,
            Some(Rc::new(|_request| Some(None))),
            Some(Rc::new(|_spec| Some("<svg/>".to_string()))),
        );
        assert!(lookup_table_chart(TABLE).is_none());
        set_table_charts(TableChartMode::Off, None, None);
    }
}
