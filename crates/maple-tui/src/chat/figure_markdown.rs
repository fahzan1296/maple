//! Assistant Markdown whose top-level ```svg fences are lifted out of the prose
//! and drawn as images ([`SvgFigure`]), and whose numeric tables get a chart
//! drawn under them ([`TableChartFigure`]), with ordinary [`Markdown`] for the
//! prose between them.
//!
//! Reference: `l/src/chat/figure-markdown.ts`.
//!
//! Stands in for Markdown on an assistant text block that holds an svg fence or
//! a chartable table: [`FigureMarkdown::set_text`] streams in place, reusing
//! every part whose kind is unchanged, and
//! [`FigureMarkdown::set_transient_render_cache`] marks the block's tail as
//! still streaming — the last prose run renders transient, the last figure
//! throttles its rasters until its fence closes, and a table at the tail waits
//! for its last row.
//!
//! Deviations: this crate's [`Markdown`] has no transient render cache, so the
//! flag lives here and only changes how the tail is split. There are no table
//! locks, so [`FigureMarkdown::lock_open_table`] reports `false` and the
//! leading prose is its whole first run.

use std::rc::Rc;

use crate::chat::svg_figure::{svg_figure_rendering, SvgFigure, SvgFigureOptions, SvgRasterizer};
use crate::chat::svg_source::{split_svg_fences, FigureSegment};
use crate::chat::table_chart::{has_chart_table, split_table_charts, ChartSegment, TableChartFigure, TableChartFigureOptions};
use crate::component::Component;
use crate::components::markdown::Markdown;

/// Options of a [`FigureMarkdown`].
#[derive(Clone)]
pub struct FigureMarkdownOptions {
    /// Markdown for one prose run (and a failed figure's code fallback);
    /// `FigureMarkdown` drives its transience.
    pub markdown: Rc<dyn Fn(&str) -> Markdown>,
    /// Draw charts under numeric tables; off for transcripts that are not the
    /// main session's.
    pub charts: bool,
    /// Rows each figure may fill, read on every render; callers reserve prose,
    /// chrome and separators themselves. Unset takes a share of the terminal
    /// viewport.
    pub max_rows: Option<Rc<dyn Fn() -> Option<usize>>>,
    /// A figure's rows changed outside a text update (raster landed, fell back
    /// to code).
    pub on_change: Option<Rc<dyn Fn()>>,
    /// Rasterizer for the svg figures.
    pub rasterizer: Option<SvgRasterizer>,
}

/// One run of the block.
enum Part {
    Markdown {
        md: Box<Markdown>,
        text: String,
    },
    Svg {
        figure: Box<SvgFigure>,
        source: String,
        closed: bool,
    },
    Chart {
        figure: Box<TableChartFigure>,
        table: String,
    },
}

impl Part {
    fn is_markdown(&self) -> bool {
        matches!(self, Part::Markdown { .. })
    }
}

/// A Markdown block with its figures lifted out.
pub struct FigureMarkdown {
    options: FigureMarkdownOptions,
    text: String,
    transient: bool,
    parts: Vec<Part>,
}

impl FigureMarkdown {
    /// A block for `text`.
    pub fn new(text: &str, options: FigureMarkdownOptions) -> Self {
        let mut block = FigureMarkdown {
            options,
            text: String::new(),
            transient: false,
            parts: Vec::new(),
        };
        block.set_text(text);
        block
    }

    /// Replace the source; returns whether it changed.
    pub fn set_text(&mut self, text: &str) -> bool {
        if text == self.text {
            return false;
        }
        self.text = text.to_string();
        self.reconcile();
        true
    }

    /// The source as fed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the block's tail is still streaming.
    pub fn transient_render_cache(&self) -> bool {
        self.transient
    }

    /// Mark the block's tail as still streaming.
    pub fn set_transient_render_cache(&mut self, transient: bool) {
        if self.transient == transient {
            return;
        }
        self.transient = transient;
        // The end of the stream completes a tail table, which may now get its
        // chart.
        self.reconcile();
    }

    /// The trimmed prose ahead of the first figure, or `""` when the block
    /// opens with one. While that prose is still the streaming tail (no figure
    /// follows it yet), only its settled prefix qualifies; this crate's
    /// [`Markdown`] settles the whole run, so it is the whole run.
    pub fn leading_prose(&self) -> String {
        match self.parts.first() {
            Some(Part::Markdown { text, .. }) => text.trim().to_string(),
            _ => String::new(),
        }
    }

    /// Whether [`leading_prose`](Self::leading_prose) ends inside an open
    /// table; this crate has no table locks, so never.
    pub fn leading_prose_ends_in_open_table(&self) -> bool {
        false
    }

    /// Lock a table streaming at the tail of the leading prose; unavailable
    /// here, so `false`.
    pub fn lock_open_table(&mut self) -> bool {
        false
    }

    /// Whether a figure's final source still waits for its raster, or a chart
    /// for its pick or raster.
    pub fn pending(&self) -> bool {
        self.parts.iter().any(|part| match part {
            Part::Markdown { .. } => false,
            Part::Svg { figure, .. } => figure.pending(),
            Part::Chart { figure, .. } => figure.pending(),
        })
    }

    /// Rebuild the prose runs through the Markdown factory after its theme
    /// changed; figures keep their rasters.
    pub fn restyle(&mut self) {
        let options = self.options.clone();
        self.parts = std::mem::take(&mut self.parts)
            .into_iter()
            .map(|part| match part {
                Part::Markdown { text, .. } => {
                    let md = (options.markdown)(&text);
                    Part::Markdown { md: Box::new(md), text }
                }
                other => other,
            })
            .collect();
    }

    fn prose_run(&self, text: &str) -> Markdown {
        (self.options.markdown)(text)
    }

    /// The block as prose, ```svg fences (when figures are on) and charts after
    /// complete tables (when charts are on); only the last run can still
    /// stream.
    fn segments(&self) -> Vec<Segment> {
        let fences: Vec<Segment> = if svg_figure_rendering() {
            split_svg_fences(&self.text)
                .into_iter()
                .map(Segment::from)
                .collect()
        } else {
            vec![Segment::Markdown(self.text.clone())]
        };
        if !self.options.charts || !has_chart_table(&self.text) {
            return fences;
        }
        let last = fences.len().saturating_sub(1);
        fences
            .into_iter()
            .enumerate()
            .flat_map(|(index, segment)| match segment {
                Segment::Markdown(text) => split_table_charts(&text, self.transient && index == last)
                    .into_iter()
                    .map(Segment::from)
                    .collect::<Vec<_>>(),
                other => vec![other],
            })
            .collect()
    }

    fn reconcile(&mut self) {
        let segments = self.segments();
        let mut previous: Vec<Option<Part>> = std::mem::take(&mut self.parts)
            .into_iter()
            .map(Some)
            .collect();
        let reshaped = previous.len() != segments.len();
        let mut parts = Vec::with_capacity(segments.len());
        for (index, segment) in segments.into_iter().enumerate() {
            let old = previous.get_mut(index).and_then(|slot| slot.take());
            parts.push(match (segment, old) {
                (Segment::Chart(table), Some(Part::Chart { figure, .. })) => Part::Chart { figure, table },
                (Segment::Chart(table), _) => {
                    let options = &self.options;
                    Part::Chart {
                        figure: Box::new(TableChartFigure::new(TableChartFigureOptions {
                            markdown: Some(options.markdown.clone()),
                            max_rows: options.max_rows.clone(),
                            on_change: options.on_change.clone(),
                        })),
                        table,
                    }
                }
                (Segment::Markdown(text), Some(Part::Markdown { mut md, .. })) => {
                    md.set_text(text.clone());
                    Part::Markdown { md, text }
                }
                (Segment::Markdown(text), _) => {
                    let md = self.prose_run(&text);
                    Part::Markdown { md: Box::new(md), text }
                }
                (Segment::Svg { source, closed }, Some(Part::Svg { mut figure, .. })) => {
                    // The figure keeps its raster; only the source moves.
                    figure.update(&source, closed);
                    Part::Svg { figure, source, closed }
                }
                (Segment::Svg { source, closed }, _) => {
                    let options = &self.options;
                    let mut figure = SvgFigure::new(SvgFigureOptions {
                        markdown: Some(options.markdown.clone()),
                        max_rows: options.max_rows.clone(),
                        on_change: options.on_change.clone(),
                        rasterizer: options.rasterizer.clone(),
                        cell: Default::default(),
                    });
                    figure.update(&source, closed);
                    Part::Svg {
                        figure: Box::new(figure),
                        source,
                        closed,
                    }
                }
            });
        }
        if reshaped {
            for stale in previous.into_iter().flatten() {
                self.dispose_part(stale);
            }
        }
        self.parts = parts;
    }

    fn dispose_part(&self, part: Part) {
        match part {
            Part::Markdown { .. } => {}
            Part::Svg { mut figure, .. } => figure.dispose(),
            Part::Chart { mut figure, .. } => figure.dispose(),
        }
    }

    /// Push sources and streaming state down: only the last part can still be
    /// streaming.
    fn sync_tails(&mut self) {
        let last = self.parts.len().saturating_sub(1);
        for (index, part) in self.parts.iter_mut().enumerate() {
            let streaming = self.transient && index == last;
            match part {
                Part::Markdown { .. } => {
                    // This crate's Markdown has no transient render cache.
                }
                Part::Svg { figure, source, closed } => figure.update(source, *closed || !streaming),
                Part::Chart { figure, table } => figure.update(table),
            }
        }
    }
}

/// One run of the block after splitting.
enum Segment {
    Markdown(String),
    Svg {
        source: String,
        closed: bool,
    },
    Chart(String),
}

impl From<FigureSegment> for Segment {
    fn from(segment: FigureSegment) -> Self {
        match segment {
            FigureSegment::Markdown { text } => Segment::Markdown(text),
            FigureSegment::Svg { source, closed } => Segment::Svg { source, closed },
        }
    }
}

impl From<ChartSegment> for Segment {
    fn from(segment: ChartSegment) -> Self {
        match segment {
            ChartSegment::Markdown(text) => Segment::Markdown(text),
            ChartSegment::Chart(table) => Segment::Chart(table),
        }
    }
}

impl Component for FigureMarkdown {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.sync_tails();
        let mut rows = Vec::new();
        for (index, part) in self.parts.iter_mut().enumerate() {
            // A chart draws its own blank row when it has rows.
            if index > 0 && !matches!(part, Part::Chart { .. }) {
                rows.push(String::new());
            }
            match part {
                Part::Markdown { md, .. } => rows.extend(md.render(width)),
                Part::Svg { figure, .. } => rows.extend(figure.render(width)),
                Part::Chart { figure, .. } => rows.extend(figure.render(width)),
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::svg_figure::{default_markdown_factory, set_svg_figure_rendering};
    use crate::chat::table_chart::{set_table_charts, TableChartMode};

    fn options() -> FigureMarkdownOptions {
        FigureMarkdownOptions {
            markdown: default_markdown_factory(),
            charts: false,
            max_rows: None,
            on_change: None,
            rasterizer: None,
        }
    }

    #[test]
    fn prose_only_blocks_render_as_one_run() {
        set_svg_figure_rendering(true);
        let mut block = FigureMarkdown::new("Hello **there**", options());
        assert_eq!(block.leading_prose(), "Hello **there**");
        assert!(!block.pending());
        let rows = block.render(40);
        assert!(!rows.is_empty());
    }

    #[test]
    fn set_text_reports_changes_and_keeps_parts() {
        set_svg_figure_rendering(true);
        let mut block = FigureMarkdown::new("one", options());
        assert!(!block.set_text("one"));
        assert!(block.set_text("one and two"));
        assert_eq!(block.text(), "one and two");
    }

    #[test]
    fn leading_prose_stops_at_the_first_figure() {
        set_svg_figure_rendering(true);
        let text = "Intro line.\n\n```svg\n<svg/>\n```\n\nOutro.\n";
        let block = FigureMarkdown::new(text, options());
        assert_eq!(block.leading_prose(), "Intro line.");
    }

    #[test]
    fn blocks_opening_with_a_figure_have_no_leading_prose() {
        set_svg_figure_rendering(true);
        let block = FigureMarkdown::new("```svg\n<svg/>\n```\n", options());
        assert_eq!(block.leading_prose(), "");
    }

    #[test]
    fn figures_are_turned_down_when_rendering_is_off() {
        set_svg_figure_rendering(false);
        let text = "```svg\n<svg/>\n```\n";
        let block = FigureMarkdown::new(text, options());
        assert_eq!(block.leading_prose(), text.trim());
        set_svg_figure_rendering(true);
    }

    #[test]
    fn charts_split_the_prose_when_enabled() {
        let table = "| Quarter | Revenue |\n|---|---|\n| Q1 | 10 |\n| Q2 | 14 |\n";
        let text = format!("Before.\n\n{table}\nAfter.\n");
        set_table_charts(
            TableChartMode::Always,
            None,
            Some(Rc::new(|_spec| Some("<svg/>".to_string()))),
        );
        let mut with_charts = options();
        with_charts.charts = true;
        let mut block = FigureMarkdown::new(&text, with_charts);
        block.set_transient_render_cache(false);
        let rows = block.render(60);
        assert!(!rows.is_empty());
        let without = FigureMarkdown::new(&text, options());
        assert!(without.leading_prose().contains("After."));
        assert_eq!(block.leading_prose(), "Before.");
        set_table_charts(TableChartMode::Off, None, None);
    }

    #[test]
    fn transient_tails_are_marked_for_figures() {
        set_svg_figure_rendering(true);
        let mut block = FigureMarkdown::new("```svg\n<svg/>\n```\n", options());
        block.set_transient_render_cache(true);
        assert!(block.transient_render_cache());
        assert!(!block.lock_open_table());
        assert!(!block.leading_prose_ends_in_open_table());
        block.set_transient_render_cache(false);
        assert!(!block.transient_render_cache());
    }
}
