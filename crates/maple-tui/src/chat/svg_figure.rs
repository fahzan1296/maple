//! A ```svg fence drawn as an inline terminal image.
//!
//! Reference: `l/src/chat/svg-figure.ts`.
//!
//! The source is rasterized off-thread and shown through an image component;
//! while the fence streams, each throttled revision repairs the partial source
//! and replaces the previous raster, which stays on screen until the next one
//! lands.
//!
//! Sizing is natural rather than preview-capped: one terminal row spans
//! [`UNITS_PER_ROW`] SVG user units, so `font-size="14"` text lands near the
//! terminal's own glyph size and the figure takes as many columns as its
//! drawing needs — up to the full width, and a viewport-relative height.
//!
//! Deviations: this crate has neither a rasterizer nor graphics-protocol image
//! display, so the raster callback is injected ([`SvgRasterizer`]) and the rows
//! are the fenced-code fallback (or a dim placeholder when no Markdown factory
//! was injected). The throttle timer collapses into "rasterize on the first
//! render after the input changed", since a renderer here cannot be woken by a
//! timer.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::component::Component;
use crate::components::markdown::{Markdown, MarkdownTheme};
use crate::theme::theme_class::Theme;

/// Rows assumed for the viewport share when the host does not report a size.
const DEFAULT_TERMINAL_ROWS: usize = 24;

/// SVG user units per terminal row; a 1000-unit-wide drawing spans ~125
/// columns of 1:2 cells.
pub const UNITS_PER_ROW: f64 = 16.0;

/// Least time between rasters of a fence that is still streaming.
pub const STREAM_INTERVAL_MS: u64 = 200;

/// Longest raster edge; native rasterizers cap the surface at 4096² pixels.
pub const MAX_EDGE_PX: u32 = 4096;

/// Share of the terminal's rows one figure may fill, so it fits on screen whole.
pub const MAX_VIEWPORT_SHARE: f64 = 0.8;

/// Theme colors behind the figure tokens (`var(--name)`).
pub const PALETTE_TOKENS: [(&str, &str); 7] = [
    ("fg", "text"),
    ("muted", "muted"),
    ("border", "borderMuted"),
    ("accent", "accent"),
    ("success", "success"),
    ("warning", "warning"),
    ("error", "error"),
];

/// Series colors for `c1`…`c6`, taken from the theme's own series tokens.
const SERIES_TOKENS: [&str; 6] = ["accent", "success", "warning", "error", "info", "muted"];

/// The active theme's color per figure token, for resolving a figure with
/// `prepare_svg`.
pub fn svg_figure_palette(theme: &Theme) -> HashMap<String, String> {
    let mut palette = HashMap::new();
    palette.insert(
        "surface".to_string(),
        theme.get_bg_hex("selectedBg").unwrap_or_default(),
    );
    for (token, theme_key) in PALETTE_TOKENS {
        if let Ok(hex) = theme.get_color_hex(theme_key) {
            palette.insert(token.to_string(), hex);
        }
    }
    for (index, token) in SERIES_TOKENS.iter().enumerate() {
        if let Ok(hex) = theme.get_color_hex(token) {
            palette.insert(format!("c{}", index + 1), hex);
        }
    }
    palette
}

static FIGURES_ENABLED: AtomicBool = AtomicBool::new(true);

/// Turn lifting ```svg fences into images on or off (the host's
/// `tui.renderSvg`); the host rebuilds the transcript after.
pub fn set_svg_figure_rendering(enabled: bool) {
    FIGURES_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether assistant ```svg fences render as images where the terminal can show
/// them.
pub fn svg_figure_rendering() -> bool {
    FIGURES_ENABLED.load(Ordering::Relaxed)
}

/// Terminal cell size in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellDimensions {
    /// Cell width in pixels.
    pub width_px: u32,
    /// Cell height in pixels.
    pub height_px: u32,
}

impl Default for CellDimensions {
    fn default() -> Self {
        CellDimensions {
            width_px: 8,
            height_px: 16,
        }
    }
}

/// Cells a figure may fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Columns.
    pub columns: usize,
    /// Rows.
    pub rows: usize,
}

/// One rasterized revision and the inputs that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    /// Rasterized source.
    pub source: String,
    /// Cell size the raster was padded to.
    pub cell: CellDimensions,
    /// Room it was drawn for.
    pub limits: Limits,
    /// Base64 PNG.
    pub data: String,
    /// Pixel width.
    pub width_px: u32,
    /// Pixel height.
    pub height_px: u32,
    /// Budget key; one per revision so a new raster is a new terminal image.
    pub key: String,
}

/// Rasterizes prepared SVG into a PNG payload and its pixel size.
pub type SvgRasterizer = Rc<dyn Fn(&str, Limits, CellDimensions) -> Option<(String, u32, u32)>>;

/// Options of a [`SvgFigure`].
#[derive(Clone, Default)]
pub struct SvgFigureOptions {
    /// Markdown for the fenced-code fallback shown when the final source does
    /// not render.
    pub markdown: Option<Rc<dyn Fn(&str) -> Markdown>>,
    /// Rows the figure may fill, fallback code included, read on every render;
    /// zero hides it.
    pub max_rows: Option<Rc<dyn Fn() -> Option<usize>>>,
    /// The figure's rows changed outside a text update.
    pub on_change: Option<Rc<dyn Fn()>>,
    /// Rasterizer; absent means the figure can only show its fallback.
    pub rasterizer: Option<SvgRasterizer>,
    /// Cell size source.
    pub cell: CellDimensions,
}

/// Cells a figure rendered at `width` may fill.
pub fn limits_for(width: u16, cell: CellDimensions, max_rows: Option<usize>) -> Limits {
    let max_rows = max_rows.unwrap_or_else(|| {
        ((DEFAULT_TERMINAL_ROWS as f64) * MAX_VIEWPORT_SHARE).floor() as usize
    });
    Limits {
        columns: (width as usize).min((MAX_EDGE_PX / cell.width_px.max(1)) as usize),
        rows: max_rows
            .clamp(1, (MAX_EDGE_PX / cell.height_px.max(1)) as usize),
    }
}

/// The ```svg fence as a figure.
pub struct SvgFigure {
    options: SvgFigureOptions,
    id: u64,
    source: String,
    final_source: bool,
    revision: u64,
    raster: Option<Raster>,
    attempt: Option<(String, Limits, CellDimensions)>,
    failed: Option<String>,
    fallback: Option<(String, Markdown)>,
    placeholder: Option<Vec<String>>,
    width: Option<u16>,
    disposed: bool,
}

static NEXT_FIGURE_ID: AtomicU64 = AtomicU64::new(0);

impl SvgFigure {
    /// An empty figure.
    pub fn new(options: SvgFigureOptions) -> Self {
        SvgFigure {
            options,
            id: NEXT_FIGURE_ID.fetch_add(1, Ordering::Relaxed),
            source: String::new(),
            final_source: false,
            revision: 0,
            raster: None,
            attempt: None,
            failed: None,
            fallback: None,
            placeholder: None,
            width: None,
            disposed: false,
        }
    }

    /// The fence body fed so far.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Width of the last render.
    pub fn width(&self) -> Option<u16> {
        self.width
    }

    /// Feed the fence body; `final` once the fence closed or the message
    /// stopped streaming.
    pub fn update(&mut self, source: &str, final_source: bool) {
        if source == self.source && final_source == self.final_source {
            return;
        }
        self.source = source.to_string();
        self.final_source = final_source;
        self.fallback = None;
    }

    /// Whether the final source still waits for its raster (or for failing to
    /// render).
    pub fn pending(&self) -> bool {
        let max_rows = self.max_rows();
        max_rows != Some(0)
            && self.final_source
            && self
                .raster
                .as_ref()
                .map(|raster| raster.source != self.source)
                .unwrap_or(true)
            && self.failed.as_deref() != Some(self.source.as_str())
    }

    /// The raster currently on screen.
    pub fn raster(&self) -> Option<&Raster> {
        self.raster.as_ref()
    }

    /// A snapshot for tests and debug overlays.
    pub fn debug_state(&self) -> serde_json::Value {
        serde_json::json!({
            "final": self.final_source,
            "failed": self.failed.as_deref() == Some(self.source.as_str()),
            "raster": self.raster.as_ref().map(|raster| format!("{}x{}", raster.width_px, raster.height_px)),
            "current": self.raster.as_ref().map(|raster| raster.source == self.source).unwrap_or(false),
        })
    }

    /// Drop caches a theme or layout change invalidates.
    pub fn invalidate(&mut self) {
        self.placeholder = None;
        self.fallback = None;
    }

    /// Retire any drawn raster: nothing to release without an image store.
    pub fn release(&mut self) {}

    /// Stop future rasterization.
    pub fn dispose(&mut self) {
        self.disposed = true;
    }

    fn max_rows(&self) -> Option<usize> {
        self.options
            .max_rows
            .as_ref()
            .and_then(|max_rows| max_rows())
            .map(|rows| rows)
    }

    /// Rasterize when the current source, room or cell size has not been
    /// attempted yet.
    fn schedule(&mut self, width: u16) {
        if self.disposed {
            return;
        }
        let max_rows = self.max_rows();
        if max_rows == Some(0) {
            return;
        }
        let cell = self.options.cell;
        let limits = limits_for(width, cell, max_rows);
        let stale = self
            .attempt
            .as_ref()
            .map(|(source, attempted, attempted_cell)| {
                source != &self.source
                    || *attempted != limits
                    || *attempted_cell != cell
                    // A streaming attempt that failed is retried once as final.
                    || (self.final_source
                        && self
                            .raster
                            .as_ref()
                            .map(|raster| raster.source != self.source)
                            .unwrap_or(true))
            })
            .unwrap_or(true);
        if !stale || self.failed.as_deref() == Some(self.source.as_str()) {
            return;
        }
        self.rasterize(limits, cell);
    }

    fn rasterize(&mut self, limits: Limits, cell: CellDimensions) {
        let Some(rasterizer) = self.options.rasterizer.clone() else {
            return;
        };
        self.attempt = Some((self.source.clone(), limits, cell));
        let prepared = match crate::chat::svg_source::close_partial_svg(&self.source) {
            Some(svg) => svg,
            None => {
                if self.final_source {
                    self.failed = Some(self.source.clone());
                    self.notify_change();
                }
                return;
            }
        };
        match rasterizer(&prepared, limits, cell) {
            Some((data, width_px, height_px)) => self.show(data, width_px, height_px, limits, cell),
            None if self.final_source => {
                self.failed = Some(self.source.clone());
                self.notify_change();
            }
            None => {}
        }
    }

    fn show(&mut self, data: String, width_px: u32, height_px: u32, limits: Limits, cell: CellDimensions) {
        self.revision += 1;
        self.raster = Some(Raster {
            source: self.source.clone(),
            cell,
            limits,
            data,
            width_px,
            height_px,
            key: format!("svg{}:{}", self.id, self.revision),
        });
        self.notify_change();
    }

    fn notify_change(&self) {
        if let Some(on_change) = &self.options.on_change {
            on_change();
        }
    }

    /// The source as the fenced code block it was written as.
    fn fallback_rows(&mut self, width: u16, max_rows: Option<usize>) -> Vec<String> {
        let Some(markdown) = self.options.markdown.clone() else {
            return Vec::new();
        };
        if self
            .fallback
            .as_ref()
            .map(|(source, _)| source != &self.source)
            .unwrap_or(true)
        {
            let fenced = fenced_svg(&self.source);
            let rendered = markdown(&fenced);
            self.fallback = Some((self.source.clone(), rendered));
        }
        let Some((_, rendered)) = self.fallback.as_mut() else {
            return Vec::new();
        };
        let mut rows = rendered.render(width);
        if let Some(max_rows) = max_rows {
            rows.truncate(max_rows);
        }
        rows
    }
}

impl Component for SvgFigure {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.width = Some(width);
        self.schedule(width);
        let max_rows = self.max_rows();
        if max_rows == Some(0) {
            return Vec::new();
        }
        if self.failed.as_deref() == Some(self.source.as_str()) {
            return self.fallback_rows(width, max_rows);
        }
        if self.raster.is_some() {
            // Without a graphics protocol the raster cannot be displayed; the
            // fence's code fallback is the honest stand-in.
            return self.fallback_rows(width, max_rows);
        }
        if self.options.markdown.is_none() {
            return Vec::new();
        }
        self.placeholder.clone().unwrap_or_else(|| {
            let rows = vec![" Drawing SVG…".to_string()];
            self.placeholder = Some(rows.clone());
            rows
        })
    }
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

/// `source` as the fenced code block it was written as.
fn fenced_svg(source: &str) -> String {
    // Fence longer than any backtick run inside the body.
    let mut longest = 0usize;
    let mut run = 0usize;
    for ch in source.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}svg\n{source}\n{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin::builtin_theme_json;
    use crate::theme::loader::{create_theme, CreateThemeOptions};
    use crate::theme::theme_class::ColorMode;

    fn theme() -> Theme {
        create_theme(
            builtin_theme_json("dark").unwrap(),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn limits_clamp_to_the_caller_and_the_edge_cap() {
        let cell = CellDimensions::default();
        let limits = limits_for(80, cell, Some(10));
        assert_eq!(limits.columns, 80);
        assert_eq!(limits.rows, 10);
        let tall = limits_for(80, cell, Some(0));
        assert_eq!(tall.rows, 1, "a zero ceiling still keeps one row");
        let wide = limits_for(u16::MAX, cell, None);
        assert!(wide.columns <= (MAX_EDGE_PX / cell.width_px) as usize);
    }

    #[test]
    fn rendering_flag_round_trips() {
        set_svg_figure_rendering(false);
        assert!(!svg_figure_rendering());
        set_svg_figure_rendering(true);
        assert!(svg_figure_rendering());
    }

    #[test]
    fn palette_resolves_tokens_and_series() {
        let palette = svg_figure_palette(&theme());
        for token in ["fg", "muted", "accent"] {
            assert!(palette.contains_key(token), "missing {token}");
        }
        for series in ["c1", "c6"] {
            assert!(palette.contains_key(series));
        }
    }

    #[test]
    fn pending_until_a_final_raster_lands() {
        let rasterizer: SvgRasterizer =
            Rc::new(|_svg, _limits, _cell| Some(("png".into(), 8, 16)));
        let mut figure = SvgFigure::new(SvgFigureOptions {
            markdown: Some(default_markdown_factory()),
            rasterizer: Some(rasterizer),
            max_rows: Some(Rc::new(|| Some(10))),
            ..SvgFigureOptions::default()
        });
        figure.update("<svg>", false);
        assert!(!figure.pending(), "a streaming fence is not pending");
        let rows = figure.render(60);
        assert!(!rows.is_empty());
        assert!(figure.raster().is_some());
        figure.update("<svg><rect/></svg>", true);
        figure.render(60);
        assert!(!figure.pending());
        assert_eq!(figure.raster().unwrap().source, "<svg><rect/></svg>");
        assert_eq!(figure.debug_state()["current"], true);
    }

    #[test]
    fn failed_final_sources_fall_back_to_code() {
        let mut figure = SvgFigure::new(SvgFigureOptions {
            markdown: Some(default_markdown_factory()),
            rasterizer: Some(Rc::new(|_svg, _limits, _cell| None)),
            max_rows: Some(Rc::new(|| Some(10))),
            ..SvgFigureOptions::default()
        });
        figure.update("<svg/>", true);
        let rows = figure.render(60);
        assert!(!rows.is_empty());
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("svg"));
        assert!(!figure.pending());
    }

    #[test]
    fn zero_rows_hide_the_figure() {
        let mut figure = SvgFigure::new(SvgFigureOptions {
            markdown: Some(default_markdown_factory()),
            max_rows: Some(Rc::new(|| Some(0))),
            ..SvgFigureOptions::default()
        });
        figure.update("<svg/>", true);
        assert!(figure.render(60).is_empty());
    }
}
