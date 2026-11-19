//! Shared framed viewport for transcript browsers.
//!
//! Reference: `l/src/chat/transcript-browser.ts`.
//!
//! Domain controllers retain loading, actions, and branch/block presentation;
//! this component owns outline row caching, frame sizing, scroll state,
//! selection anchoring, append-follow behavior, and frame-to-body coordinates.
//!
//! Deviations:
//! - The border is built per frame from an owned [`Theme`], because
//!   [`DynamicBorder`] borrows the theme and the browser must not.
//! - The scrollbar uses [`ScrollView`]'s default track/thumb styling: this
//!   crate's [`ScrollViewTheme`] takes [`Style`]s, not token painters.
//! - [`TranscriptBrowser::handle_scroll_key`] takes a decoded [`Key`], as the
//!   crate's [`ScrollView`] does, instead of raw key data.

use crate::chat::transcript_outline::{
    compose_outline_column, outline_visibility, ComposedColumn, OutlineRowCache, OutlineStyle,
    OutlineTarget,
};
use crate::chrome::dynamic_border::DynamicBorder;
use crate::component::Component;
use crate::components::scroll_view::{ScrollRangeAnchor, ScrollView};
use crate::keys::Key;
use crate::native::state::same_items;
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::theme::Theme;

/// Logical body rows and optional selection supplied by a transcript browser
/// controller.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranscriptBrowserBody {
    /// The body's rows.
    pub lines: Vec<String>,
    /// A selection to reveal.
    pub anchor: Option<ScrollRangeAnchor>,
}

/// One complete browser frame. Header and footer rows are inset by one column.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranscriptBrowserFrame {
    /// Rows above the body.
    pub header: Vec<String>,
    /// The scrollable body.
    pub body: TranscriptBrowserBody,
    /// Rows below the body.
    pub footer: Vec<String>,
}

/// Widths available while composing a transcript browser frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TranscriptBrowserRenderContext {
    /// Width passed to [`TranscriptBrowser::render`].
    pub frame_width: usize,
    /// Stable transcript width, reserving the final column for an automatic
    /// scrollbar.
    pub content_width: usize,
    /// Width for inset header/footer children such as an editor.
    pub chrome_width: usize,
}

/// Construction options for the shared transcript browser viewport and frame.
pub struct TranscriptBrowserOptions {
    /// Explicit height budget supplied by the fullscreen host.
    pub get_height: Box<dyn Fn() -> f64>,
    /// Domain controller which supplies header, body, footer, and optional
    /// selection.
    pub frame: Box<dyn Fn(&TranscriptBrowserRenderContext) -> TranscriptBrowserFrame>,
    /// Preserve the historical three-row minimum used by transcript overlays.
    pub minimum_body_rows: Option<usize>,
    /// Start in append-follow mode; manual scrolling updates the mode
    /// automatically.
    pub follow_bottom: bool,
}

impl Default for TranscriptBrowserOptions {
    fn default() -> Self {
        TranscriptBrowserOptions {
            get_height: Box::new(|| 0.0),
            frame: Box::new(|_context| TranscriptBrowserFrame::default()),
            minimum_body_rows: None,
            follow_bottom: false,
        }
    }
}

/// Prepared transcript rows, visibility, and nearest valid selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreparedTranscriptOutline {
    /// Every rendered child's rows.
    pub child_rows: Vec<Vec<String>>,
    /// Whether each child is visible.
    pub visible: Vec<bool>,
    /// The nearest visible selection.
    pub selected: usize,
}

/// Options for composing the standard single-column dotted transcript outline.
pub struct TranscriptOutlineComposition<'a> {
    /// The transcript's children.
    pub children: &'a mut [Box<dyn Component>],
    /// One target per selectable entry.
    pub targets: &'a [OutlineTarget],
    /// The selected target.
    pub selected: usize,
    /// The outline's width.
    pub column_width: usize,
    /// Rows already prepared for custom branch/block composition in the same
    /// frame.
    pub prepared: Option<PreparedTranscriptOutline>,
    /// First child to compose.
    pub from: Option<usize>,
    /// One past the last child to compose.
    pub to: Option<usize>,
    /// Rows leading the column.
    pub header: Option<Vec<String>>,
    /// Stroke colour and caption.
    pub style: OutlineStyle,
}

/// Which rows of the frame hold the body.
struct BrowserLayout {
    body_top: usize,
    body_height: usize,
}

/// Nearest visible target, preferring the preceding item just like the legacy
/// selectors.
pub fn nearest_visible_outline_target(visible: &[bool], selected: isize) -> usize {
    if selected < 0 || selected as usize >= visible.len() {
        return visible.iter().position(|visible| *visible).unwrap_or(0);
    }
    let selected = selected as usize;
    if visible[selected] {
        return selected;
    }
    for above in (0..selected).rev() {
        if visible[above] {
            return above;
        }
    }
    for below in selected + 1..visible.len() {
        if visible[below] {
            return below;
        }
    }
    selected
}

/// Shared framed viewport for transcript browsers.
pub struct TranscriptBrowser {
    options: TranscriptBrowserOptions,
    theme: Theme,
    scroll_view: ScrollView,
    row_cache: OutlineRowCache,
    follow_bottom: bool,
    layout: Option<BrowserLayout>,
    cached_lines: Option<Vec<String>>,
}

impl TranscriptBrowser {
    /// A browser over `options`, drawn in `theme`.
    pub fn new(options: TranscriptBrowserOptions, theme: Theme) -> Self {
        let follow_bottom = options.follow_bottom;
        let mut scroll_view = ScrollView::new(Vec::new(), 0);
        scroll_view.set_follow_tail(follow_bottom);
        TranscriptBrowser {
            options,
            theme,
            scroll_view,
            row_cache: OutlineRowCache::new(),
            follow_bottom,
            layout: None,
            cached_lines: None,
        }
    }

    /// Prompt-zone-safe rendered rows for outline composition at
    /// `column_width`.
    pub fn render_outline_rows(
        &mut self,
        children: &mut [Box<dyn Component>],
        column_width: usize,
    ) -> Vec<Vec<String>> {
        self.row_cache
            .rows(children, column_width.saturating_sub(4).max(10) as u16)
    }

    /// Compute target visibility and snap a collapsed selection to the nearest
    /// rendered item.
    pub fn prepare_outline(
        &mut self,
        children: &mut [Box<dyn Component>],
        targets: &[OutlineTarget],
        selected: usize,
        column_width: usize,
    ) -> PreparedTranscriptOutline {
        let child_rows = self.render_outline_rows(children, column_width);
        let visible = outline_visibility(&child_rows, targets);
        PreparedTranscriptOutline {
            selected: nearest_visible_outline_target(&visible, selected as isize),
            child_rows,
            visible,
        }
    }

    /// Compose the standard single-column outline using the browser-owned row
    /// cache.
    pub fn compose_outline(
        &mut self,
        options: TranscriptOutlineComposition<'_>,
    ) -> (PreparedTranscriptOutline, ComposedColumn) {
        let prepared = options.prepared.unwrap_or_else(|| {
            self.prepare_outline(
                options.children,
                options.targets,
                options.selected,
                options.column_width,
            )
        });
        let column = compose_outline_column(
            &self.theme,
            &prepared.child_rows,
            options.from.unwrap_or(0),
            options.to.unwrap_or(options.children.len()),
            options.targets,
            prepared.selected,
            options.column_width,
            options.header.as_deref(),
            &options.style,
        );
        (prepared, column)
    }

    /// Scroll logical body rows. Returns whether the offset changed.
    pub fn scroll(&mut self, delta: i64) -> bool {
        let before = self.scroll_view.scroll_offset();
        self.scroll_view.scroll_by(delta);
        self.sync_follow_bottom();
        self.scroll_view.scroll_offset() != before
    }

    /// Apply the scroll-view navigation key set.
    pub fn handle_scroll_key(&mut self, key: &Key) -> bool {
        if !self.scroll_view.handle_scroll_key(key) {
            return false;
        }
        self.sync_follow_bottom();
        true
    }

    /// Scroll to the first body row.
    pub fn scroll_to_top(&mut self) {
        self.scroll_view.scroll_to_top();
        self.sync_follow_bottom();
    }

    /// Scroll to the last body row.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_view.scroll_to_bottom();
        self.sync_follow_bottom();
    }

    /// Whether the view shows the first body row.
    pub fn at_top(&self) -> bool {
        self.scroll_view.scroll_offset() == 0
    }

    /// One past the last logical body row in view.
    pub fn visible_end(&self) -> usize {
        self.scroll_view.visible_range().end
    }

    /// Whether an anchor identity has successfully been applied to the
    /// viewport.
    pub fn has_anchored(&self, id: &str) -> bool {
        self.scroll_view.has_revealed_range(id)
    }

    /// What to add to a body row of the last frame to get its logical body row,
    /// as [`to_content_point`](Self::to_content_point) maps it; it follows the
    /// scroll, so a content row keeps its sum.
    pub fn frame_row_offset(&self) -> isize {
        self.scroll_view.visible_range().start as isize
            - self.layout.as_ref().map(|layout| layout.body_top).unwrap_or(0) as isize
    }

    /// Translate a 0-based frame mouse point into the logical, scrolled body.
    /// Returns `None` for borders, header/footer rows, and unpainted body rows.
    pub fn to_content_point(&self, frame_row: isize, frame_col: isize) -> Option<(usize, usize)> {
        let layout = self.layout.as_ref()?;
        let local_row = frame_row - layout.body_top as isize;
        if local_row < 0 || local_row as usize >= layout.body_height {
            return None;
        }
        let row = self.scroll_view.logical_row_at(local_row as usize)?;
        Some((row, frame_col.max(0) as usize))
    }

    /// The theme the browser draws in.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    fn sync_follow_bottom(&mut self) {
        if !self.options.follow_bottom {
            return;
        }
        self.follow_bottom = self.scroll_view.scroll_offset() >= self.scroll_view.max_scroll_offset();
        self.scroll_view.set_follow_tail(self.follow_bottom);
    }

    /// One inset chrome row, truncated one column short of `width`.
    fn chrome_line(text: &str, width: usize) -> String {
        if width == 0 {
            return String::new();
        }
        format!(" {}", truncate_to_width(text, width - 1, Ellipsis::Unicode))
    }
}

impl Component for TranscriptBrowser {
    fn render(&mut self, width: u16) -> Vec<String> {
        let frame_width = width as usize;
        let context = TranscriptBrowserRenderContext {
            frame_width,
            content_width: frame_width.saturating_sub(1).max(1),
            chrome_width: frame_width.saturating_sub(2).max(1),
        };
        let frame = (self.options.frame)(&context);
        let height_value = (self.options.get_height)();
        let height = if height_value.is_finite() {
            height_value.max(0.0).trunc() as usize
        } else {
            0
        };
        let border_rows = if frame_width > 0 { 3 } else { 0 };
        let chrome_rows = frame.header.len() + frame.footer.len() + border_rows;
        let minimum_body_rows = self.options.minimum_body_rows.unwrap_or(3);
        let body_height = minimum_body_rows.max(height.saturating_sub(chrome_rows));
        let body_top = frame.header.len() + if frame_width > 0 { 2 } else { 0 };
        self.layout = Some(BrowserLayout {
            body_top,
            body_height,
        });

        self.scroll_view.set_lines(frame.body.lines);
        self.scroll_view.set_height(body_height);
        if frame
            .body
            .anchor
            .is_some_and(|anchor| self.scroll_view.reveal_range(Some(anchor), context.content_width as u16))
        {
            self.follow_bottom = false;
            self.scroll_view.set_follow_tail(false);
        }

        let border = if frame_width > 0 {
            DynamicBorder::new(&self.theme).render(frame_width as u16)
        } else {
            Vec::new()
        };
        let mut output = Vec::new();
        output.extend(border.iter().cloned());
        for line in &frame.header {
            output.push(Self::chrome_line(line, frame_width));
        }
        output.extend(border.iter().cloned());
        output.extend(self.scroll_view.render(frame_width as u16));
        for line in &frame.footer {
            output.push(Self::chrome_line(line, frame_width));
        }
        output.extend(border.iter().cloned());
        if let Some(cached) = &self.cached_lines {
            if same_items(cached, &output) {
                return cached.clone();
            }
        }
        self.cached_lines = Some(output.clone());
        output
    }

    fn invalidate(&mut self) {
        self.cached_lines = None;
        self.scroll_view.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;
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

    fn browser(
        frame: Box<dyn Fn(&TranscriptBrowserRenderContext) -> TranscriptBrowserFrame>,
        height: f64,
    ) -> TranscriptBrowser {
        TranscriptBrowser::new(
            TranscriptBrowserOptions {
                get_height: Box::new(move || height),
                frame,
                minimum_body_rows: Some(3),
                follow_bottom: false,
            },
            theme(),
        )
    }

    fn lines(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("row {index}")).collect()
    }

    #[test]
    fn nearest_visible_prefers_the_preceding_item() {
        let visible = [true, false, true, false, true];
        assert_eq!(nearest_visible_outline_target(&visible, 2), 2);
        assert_eq!(nearest_visible_outline_target(&visible, 3), 2);
        assert_eq!(nearest_visible_outline_target(&visible, 1), 0);
        assert_eq!(nearest_visible_outline_target(&visible, -1), 0);
        assert_eq!(nearest_visible_outline_target(&visible, 99), 0);
        assert_eq!(nearest_visible_outline_target(&[false, false], 0), 0);
    }

    #[test]
    fn frames_compose_borders_header_and_footer() {
        let mut browser = browser(
            Box::new(|_| TranscriptBrowserFrame {
                header: vec!["head".to_string()],
                body: TranscriptBrowserBody {
                    lines: lines(5),
                    anchor: None,
                },
                footer: vec!["foot".to_string()],
            }),
            12.0,
        );
        let rows = browser.render(20);
        assert!(rows.len() > 5);
        let plain: Vec<String> = rows.iter().map(|row| crate::ansi_scan::strip_ansi(row)).collect();
        assert!(plain[1].contains("head"));
        assert!(plain.iter().any(|row| row.contains("row 0")));
        assert!(plain.last().unwrap().trim().is_empty(), "the bottom border closes the frame");
    }

    #[test]
    fn rendering_is_cached_while_the_frame_is_unchanged() {
        let mut browser = browser(
            Box::new(|_| TranscriptBrowserFrame {
                body: TranscriptBrowserBody {
                    lines: lines(3),
                    anchor: None,
                },
                ..TranscriptBrowserFrame::default()
            }),
            10.0,
        );
        let first = browser.render(20);
        let second = browser.render(20);
        assert_eq!(first, second);
    }

    #[test]
    fn scrolling_moves_the_body_and_respects_the_top() {
        let mut browser = browser(
            Box::new(|_| TranscriptBrowserFrame {
                body: TranscriptBrowserBody {
                    lines: lines(40),
                    anchor: None,
                },
                ..TranscriptBrowserFrame::default()
            }),
            6.0,
        );
        browser.render(20);
        assert!(browser.at_top());
        assert!(browser.scroll(3));
        assert!(!browser.at_top());
        assert!(!browser.scroll(-100) || browser.at_top());
        assert!(browser.at_top());
    }

    #[test]
    fn content_points_skip_chrome_rows() {
        let mut browser = browser(
            Box::new(|_| TranscriptBrowserFrame {
                header: vec!["head".to_string()],
                body: TranscriptBrowserBody {
                    lines: lines(10),
                    anchor: None,
                },
                ..TranscriptBrowserFrame::default()
            }),
            12.0,
        );
        browser.render(20);
        assert!(browser.to_content_point(0, 0).is_none(), "border");
        assert_eq!(browser.to_content_point(2, 4), Some((0, 4)));
        assert_eq!(browser.frame_row_offset(), 0);
    }

    #[test]
    fn anchors_are_revealed_and_reported() {
        let anchor = ScrollRangeAnchor {
            id: "row-30".to_string(),
            start: 30,
            end: 31,
            margin: None,
            alignment: None,
            mode: None,
            oversized: None,
        };
        let mut browser = browser(
            Box::new(move |_| TranscriptBrowserFrame {
                body: TranscriptBrowserBody {
                    lines: lines(40),
                    anchor: Some(anchor.clone()),
                },
                ..TranscriptBrowserFrame::default()
            }),
            6.0,
        );
        browser.render(20);
        assert!(browser.has_anchored("row-30"));
        assert!(!browser.at_top());
        assert_eq!(browser.visible_end(), 40);
    }

    #[test]
    fn outline_rows_render_children() {
        let mut browser = browser(Box::new(|_| TranscriptBrowserFrame::default()), 10.0);
        let mut children: Vec<Box<dyn Component>> = vec![Box::new(Text::new("child"))];
        let rows = browser.render_outline_rows(&mut children, 40);
        assert_eq!(rows.len(), 1);
        assert!(crate::ansi_scan::strip_ansi(&rows[0][0]).contains("child"));
    }
}
