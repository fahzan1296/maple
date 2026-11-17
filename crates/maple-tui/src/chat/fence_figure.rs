//! A tool call's [`ToolFigure`] drawn under its card, the way assistant
//! Markdown draws that fence: ```svg as an inline image ([`SvgFigure`]),
//! redrawn as the body streams; ```mermaid as ASCII art once the body is
//! final. The card already shows the source, so unlike the assistant's fence it
//! never falls back to code: [`draws`] turns down fences this terminal shows as
//! code, and an svg that does not rasterize stays blank.
//!
//! Reference: `l/src/chat/fence-figure.ts`.
//!
//! Deviations: this crate has no terminal capability probe and no graphics
//! protocol, so callers pass `show_images`/`image_protocol`, and mermaid support
//! is a probe the host injects ([`MermaidProbe`]).

use std::rc::Rc;

use crate::chat::svg_figure::{set_svg_figure_rendering, svg_figure_rendering, SvgFigure, SvgFigureOptions, SvgRasterizer};
use crate::component::Component;
use crate::components::markdown::Markdown;
use crate::tools::native_view::ToolFigure;

/// Fence languages a [`FenceFigure`] draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceFigureLang {
    /// An SVG drawing.
    Svg,
    /// A mermaid diagram.
    Mermaid,
}

impl FenceFigureLang {
    /// The fence language, or `None` for fences this figure leaves alone.
    pub fn parse(lang: &str) -> Option<Self> {
        match lang {
            "svg" => Some(FenceFigureLang::Svg),
            "mermaid" => Some(FenceFigureLang::Mermaid),
            _ => None,
        }
    }

    /// The fence language.
    pub fn as_str(self) -> &'static str {
        match self {
            FenceFigureLang::Svg => "svg",
            FenceFigureLang::Mermaid => "mermaid",
        }
    }
}

/// Whether a mermaid source renders as ASCII here; absent means it never does.
pub type MermaidProbe = Rc<dyn Fn(&str) -> bool>;

/// What the terminal can show.
#[derive(Clone, Copy, Debug, Default)]
pub struct FenceDrawOptions {
    /// Inline images are visible.
    pub show_images: bool,
    /// The terminal has a graphics protocol.
    pub image_protocol: bool,
}

/// Whether this terminal draws `figure` now: an svg where inline images show,
/// a closed mermaid fence when diagram rendering is on and the source renders.
pub fn draws(
    figure: &ToolFigure,
    options: FenceDrawOptions,
    mermaid: Option<&MermaidProbe>,
) -> Option<FenceFigureLang> {
    match FenceFigureLang::parse(&figure.lang)? {
        FenceFigureLang::Svg => {
            (options.show_images && options.image_protocol && svg_figure_rendering())
                .then_some(FenceFigureLang::Svg)
        }
        FenceFigureLang::Mermaid => {
            if !figure.closed {
                return None;
            }
            let source = figure.source.trim_end();
            let renders = mermaid.map(|probe| probe(source)).unwrap_or(false);
            renders.then_some(FenceFigureLang::Mermaid)
        }
    }
}

/// Options of a [`FenceFigure`].
#[derive(Clone, Default)]
pub struct FenceFigureOptions {
    /// Markdown for a mermaid fence's ASCII art.
    pub markdown: Option<Rc<dyn Fn(&str) -> Markdown>>,
    /// Rasterizer for an svg's rasters.
    pub rasterizer: Option<SvgRasterizer>,
    /// The figure's rows changed outside a render: a raster landed.
    pub on_change: Option<Rc<dyn Fn()>>,
}

/// The figure drawn under a tool card.
pub struct FenceFigure {
    lang: FenceFigureLang,
    options: FenceFigureOptions,
    svg: Option<SvgFigure>,
    source: String,
    /// The mermaid fence as assistant Markdown renders it; rebuilt after a
    /// theme change.
    diagram: Option<Markdown>,
}

impl FenceFigure {
    /// A figure for `lang` fences.
    pub fn new(lang: FenceFigureLang, options: FenceFigureOptions) -> Self {
        let svg = (lang == FenceFigureLang::Svg).then(|| {
            SvgFigure::new(SvgFigureOptions {
                // The card already shows the source: no code fallback.
                markdown: None,
                max_rows: None,
                on_change: options.on_change.clone(),
                rasterizer: options.rasterizer.clone(),
                cell: Default::default(),
            })
        });
        FenceFigure {
            lang,
            options,
            svg,
            source: String::new(),
            diagram: None,
        }
    }

    /// The fence language drawn.
    pub fn lang(&self) -> FenceFigureLang {
        self.lang
    }

    /// Feed the fence as it stands; its language is this figure's.
    pub fn update(&mut self, figure: &ToolFigure) {
        if figure.source != self.source {
            self.diagram = None;
        }
        self.source = figure.source.clone();
        if let Some(svg) = &mut self.svg {
            svg.update(&figure.source, figure.closed);
        }
    }

    /// Whether a closed svg's raster is still on its way.
    pub fn pending(&self) -> bool {
        self.svg.as_ref().map(|svg| svg.pending()).unwrap_or(false)
    }

    /// Drop caches a theme change invalidates.
    pub fn invalidate(&mut self) {
        if let Some(svg) = &mut self.svg {
            svg.invalidate();
        }
        self.diagram = None;
    }

    /// Retire drawn rasters.
    pub fn release_render_caches(&mut self) {
        if let Some(svg) = &mut self.svg {
            svg.release();
        }
        self.diagram = None;
    }

    /// Stop future rasterization.
    pub fn dispose(&mut self) {
        if let Some(svg) = &mut self.svg {
            svg.dispose();
        }
    }
}

impl Component for FenceFigure {
    /// The drawing; no rows while an svg draws, or when it does not.
    fn render(&mut self, width: u16) -> Vec<String> {
        if let Some(svg) = &mut self.svg {
            return svg.render(width);
        }
        let Some(markdown) = self.options.markdown.clone() else {
            return Vec::new();
        };
        if self.diagram.is_none() {
            self.diagram = Some(markdown(&fenced("mermaid", &self.source)));
        }
        self.diagram
            .as_mut()
            .map(|diagram| diagram.render(width))
            .unwrap_or_default()
    }
}

/// `source` as a fenced code block in `lang`.
fn fenced(lang: &str, source: &str) -> String {
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
    format!("{fence}{lang}\n{source}\n{fence}")
}

/// Turn ```svg fences into images on or off, re-exported for hosts wiring
/// figures.
pub fn set_fence_images_rendering(enabled: bool) {
    set_svg_figure_rendering(enabled);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::svg_figure::default_markdown_factory;

    fn figure(lang: &str, source: &str, closed: bool) -> ToolFigure {
        ToolFigure {
            lang: lang.to_string(),
            source: source.to_string(),
            closed,
        }
    }

    #[test]
    fn language_parsing() {
        assert_eq!(FenceFigureLang::parse("svg"), Some(FenceFigureLang::Svg));
        assert_eq!(
            FenceFigureLang::parse("mermaid"),
            Some(FenceFigureLang::Mermaid)
        );
        assert_eq!(FenceFigureLang::parse("rust"), None);
        assert_eq!(FenceFigureLang::Mermaid.as_str(), "mermaid");
    }

    #[test]
    fn draws_needs_images_and_a_protocol() {
        set_svg_figure_rendering(true);
        let svg = figure("svg", "<svg/>", false);
        assert_eq!(
            draws(&svg, FenceDrawOptions { show_images: true, image_protocol: true }, None),
            Some(FenceFigureLang::Svg)
        );
        assert_eq!(
            draws(&svg, FenceDrawOptions { show_images: true, image_protocol: false }, None),
            None
        );
    }

    #[test]
    fn drawing_mermaid_requires_a_closed_rendering_source() {
        let open = figure("mermaid", "graph TD\n", false);
        let probe: MermaidProbe = Rc::new(|_source| true);
        assert_eq!(draws(&open, FenceDrawOptions::default(), Some(&probe)), None);
        let closed = figure("mermaid", "graph TD\nA-->B\n", true);
        assert_eq!(
            draws(&closed, FenceDrawOptions::default(), Some(&probe)),
            Some(FenceFigureLang::Mermaid)
        );
        let refusing: MermaidProbe = Rc::new(|_source| false);
        assert_eq!(draws(&closed, FenceDrawOptions::default(), Some(&refusing)), None);
    }

    #[test]
    fn svg_figures_render_blank_without_a_rasterizer() {
        let mut fence = FenceFigure::new(
            FenceFigureLang::Svg,
            FenceFigureOptions {
                markdown: Some(default_markdown_factory()),
                ..FenceFigureOptions::default()
            },
        );
        fence.update(&figure("svg", "<svg/>", true));
        assert!(fence.render(60).is_empty(), "the card already shows the source");
        assert!(!fence.pending());
    }

    #[test]
    fn mermaid_renders_through_markdown() {
        let mut fence = FenceFigure::new(
            FenceFigureLang::Mermaid,
            FenceFigureOptions {
                markdown: Some(default_markdown_factory()),
                ..FenceFigureOptions::default()
            },
        );
        fence.update(&figure("mermaid", "graph TD\nA-->B\n", true));
        let rows = fence.render(60);
        assert!(!rows.is_empty());
        fence.invalidate();
        assert_eq!(fence.render(60), rows);
    }
}
