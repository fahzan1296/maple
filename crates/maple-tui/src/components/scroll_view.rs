//! Fixed-height viewport over supplied rows or a lazily rendered child.
//!
//! The viewport slices a logical row buffer at its own scroll offset, draws an
//! optional scrollbar column, and forwards scroll keys and wheel events.

use crate::component::Component;
use crate::components::scroll_viewport::{
    clamp_scroll_offset, max_scroll_offset, scrollbar_thumb_range, scroll_offset_for_row,
    viewport_overflows, viewport_range, ScrollbarMode, ScrollViewTheme, ViewportAlignment,
    ViewportRange,
};
use crate::keys::{
    is_arrow_down, is_arrow_up, is_end, is_home, is_page_down, is_page_up, Key,
};
use crate::mouse::{MouseKind, MouseEvent};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};

/// Which edge content sticks to when the viewport resizes or content changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAnchor {
    /// Keep the offset clamped toward the top.
    #[default]
    Start,
    /// Keep the trailing rows visible.
    End,
}
/// Which edge a revealed range prefers when it fills the whole viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollRangeOversized {
    /// Keep the nearest edge of the range visible.
    #[default]
    Nearest,
    /// Prefer the leading edge of the range.
    Start,
}

/// Whether a revealed range re-applies on every geometry change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollRangeMode {
    /// Reveal again whenever the range or viewport geometry changes.
    #[default]
    Selection,
    /// Reveal only the first time the keyed range is seen.
    Once,
}

/// A selected logical range revealed once or whenever its geometry changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollRangeAnchor {
    /// Key identifying the selection; unchanged geometry is not re-revealed.
    pub id: String,
    /// First logical row of the range.
    pub start: usize,
    /// Exclusive end of the range.
    pub end: usize,
    /// Rows of context kept around the range. Defaults to 1.
    pub margin: Option<usize>,
    /// How the range is aligned inside the viewport. Defaults to nearest.
    pub alignment: Option<ViewportAlignment>,
    /// Reveal policy. Defaults to [`ScrollRangeMode::Selection`].
    pub mode: Option<ScrollRangeMode>,
    /// Edge preference when the range is at least as tall as the viewport.
    pub oversized: Option<ScrollRangeOversized>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RangeSnapshot {
    id: String,
    start: usize,
    end: usize,
    margin: usize,
    alignment: ViewportAlignment,
    mode: ScrollRangeMode,
    oversized: ScrollRangeOversized,
    width: u16,
    height: usize,
    total_rows: usize,
}

/// Fixed-height, scrollable window over row content.
pub struct ScrollView {
    children: Vec<Box<dyn Component>>,
    child_index: usize,
    lines: Vec<String>,
    measured_rows: usize,
    height: usize,
    scroll_offset: usize,
    total_rows: Option<usize>,
    scrollbar: ScrollbarMode,
    theme: ScrollViewTheme,
    track_char: String,
    thumb_char: String,
    ellipsis: Ellipsis,
    fast_scroll_lines: usize,
    follow_tail: bool,
    anchor: ScrollAnchor,
    range_snapshot: Option<RangeSnapshot>,
    active_row: Option<usize>,
    active_alignment: ViewportAlignment,
}

impl Default for ScrollView {
    fn default() -> Self {
        ScrollView::new(Vec::new(), 0)
    }
}

impl ScrollView {
    /// Create a viewport over an owned row buffer.
    pub fn new(lines: Vec<String>, height: usize) -> Self {
        let measured_rows = lines.len();
        ScrollView {
            children: Vec::new(),
            child_index: 0,
            lines,
            measured_rows,
            height,
            scroll_offset: 0,
            total_rows: None,
            scrollbar: ScrollbarMode::Auto,
            theme: ScrollViewTheme::default(),
            track_char: "│".to_string(),
            thumb_char: "█".to_string(),
            ellipsis: Ellipsis::default(),
            fast_scroll_lines: 5,
            follow_tail: false,
            anchor: ScrollAnchor::Start,
            range_snapshot: None,
            active_row: None,
            active_alignment: ViewportAlignment::Nearest,
        }
    }

    /// Create a viewport that lazily renders a child component.
    pub fn from_child(child: Box<dyn Component>, height: usize) -> Self {
        let mut view = ScrollView::new(Vec::new(), height);
        view.set_child(child);
        view
    }

    /// Replace the row buffer and drop any child component.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        let old_total = self.row_count();
        let old_height = self.height;
        self.dispose_children();
        self.measured_rows = lines.len();
        self.lines = lines;
        self.reconcile_bounds(old_total, old_height);
    }

    /// Replace the rendered child component.
    pub fn set_child(&mut self, child: Box<dyn Component>) {
        self.dispose_children();
        self.children = vec![child];
        self.child_index = 0;
        self.lines = Vec::new();
        self.measured_rows = 0;
    }

    /// Append a child component and make it the rendered one.
    pub fn add_child(&mut self, child: Box<dyn Component>) {
        self.children.push(child);
        self.child_index = self.children.len() - 1;
        self.lines = Vec::new();
        self.measured_rows = 0;
    }

    /// The currently rendered child, if any.
    pub fn child(&self) -> Option<&dyn Component> {
        self.children.get(self.child_index).map(|child| &**child)
    }

    /// Mutable access to the currently rendered child.
    fn child_mut(&mut self) -> Option<&mut Box<dyn Component>> {
        self.children.get_mut(self.child_index)
    }

    /// Dispose and drop every stored child.
    fn dispose_children(&mut self) {
        for child in &mut self.children {
            child.dispose();
        }
        self.children.clear();
        self.child_index = 0;
    }

    /// Set the viewport height in rows.
    pub fn set_height(&mut self, height: usize) {
        if self.height == height {
            return;
        }
        let old_total = self.row_count();
        let old_height = self.height;
        self.height = height;
        self.reconcile_bounds(old_total, old_height);
    }

    /// Viewport height in rows.
    pub fn visible_height(&self) -> usize {
        self.height
    }

    /// Logical content height in rows.
    pub fn content_height(&self) -> usize {
        self.row_count()
    }

    /// Override the logical content height.
    pub fn set_total_rows(&mut self, total_rows: Option<usize>) {
        if self.total_rows == total_rows {
            return;
        }
        let old_total = self.row_count();
        let old_height = self.height;
        self.total_rows = total_rows;
        self.reconcile_bounds(old_total, old_height);
    }

    /// Current scroll offset in logical rows.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Maximum legal scroll offset.
    pub fn max_scroll_offset(&self) -> usize {
        max_scroll_offset(self.row_count(), self.height)
    }

    /// Current half-open visible row range.
    pub fn visible_range(&self) -> ViewportRange {
        viewport_range(self.row_count(), self.height, self.scroll_offset)
    }

    /// Scroll to an absolute offset, clamped to the content bounds.
    pub fn scroll_to(&mut self, offset: usize) {
        self.scroll_offset = clamp_scroll_offset(offset, self.row_count(), self.height);
    }

    /// Scroll by `delta` rows, clamped to the content bounds.
    pub fn scroll_by(&mut self, delta: i64) {
        let max = self.max_scroll_offset() as i64;
        let next = (self.scroll_offset as i64 + delta).clamp(0, max);
        self.scroll_offset = next as usize;
    }

    /// Scroll by one page (height minus one, at least one row).
    pub fn page(&mut self, delta: i64) {
        let step = self.height.saturating_sub(1).max(1) as i64;
        self.scroll_by(step * delta);
    }

    /// Scroll to the first content row.
    pub fn scroll_to_top(&mut self) {
        self.scroll_to(0);
    }

    /// Scroll to the last content row.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_to(self.max_scroll_offset());
    }

    /// Keep a logical row visible now and through later content changes.
    pub fn set_active_row(&mut self, row: Option<usize>, alignment: ViewportAlignment) {
        self.active_row = row;
        self.active_alignment = alignment;
        if let Some(row) = row {
            self.scroll_offset = scroll_offset_for_row(
                self.scroll_offset,
                row,
                self.row_count(),
                self.height,
                alignment,
            );
        }
    }

    /// Follow the content tail whenever it grows.
    pub fn set_follow_tail(&mut self, follow_tail: bool) {
        self.follow_tail = follow_tail;
    }

    /// Set the anchor used when content or height changes.
    pub fn set_anchor(&mut self, anchor: ScrollAnchor) {
        self.anchor = anchor;
    }


    /// Map a local viewport row to its logical content row.
    pub fn logical_row_at(&self, local_row: usize) -> Option<usize> {
        let range = self.visible_range();
        let logical = range.start.checked_add(local_row)?;
        (logical < range.end).then_some(logical)
    }

    /// Map a visible logical content row to its local viewport row.
    pub fn local_row_for(&self, logical_row: usize) -> Option<usize> {
        let range = self.visible_range();
        (logical_row >= range.start && logical_row < range.end).then(|| logical_row - range.start)
    }

    /// Whether a keyed range has already been revealed successfully.
    pub fn has_revealed_range(&self, id: &str) -> bool {
        self.range_snapshot
            .as_ref()
            .map_or(false, |snapshot| snapshot.id == id)
    }

    /// Reveal a changed selection without snapping manual scrolling back on
    /// unchanged renders. Passing `None` forgets the previous range.
    pub fn reveal_range(&mut self, anchor: Option<ScrollRangeAnchor>, width: u16) -> bool {
        let Some(anchor) = anchor else {
            self.range_snapshot = None;
            return false;
        };
        if anchor.end <= anchor.start {
            return false;
        }
        let mode = anchor.mode.unwrap_or_default();
        let alignment = anchor.alignment.unwrap_or_default();
        let margin = anchor.margin.unwrap_or(1);
        let oversized = anchor.oversized.unwrap_or_default();
        let total_rows = self.content_height();
        if let Some(previous) = &self.range_snapshot {
            let unchanged = previous.id == anchor.id
                && previous.mode == mode
                && previous.alignment == alignment
                && previous.margin == margin
                && previous.oversized == oversized
                && (mode == ScrollRangeMode::Once
                    || (previous.start == anchor.start
                        && previous.end == anchor.end
                        && previous.width == width
                        && previous.height == self.height
                        && previous.total_rows == total_rows));
            if unchanged {
                return false;
            }
        }
        let top = anchor.start.saturating_sub(margin);
        let bottom = (anchor.end + margin).min(total_rows);
        let visible = self.visible_range();
        let next = match alignment {
            ViewportAlignment::Start => scroll_offset_for_row(
                self.scroll_offset,
                top,
                total_rows,
                self.height,
                ViewportAlignment::Start,
            ),
            ViewportAlignment::End => scroll_offset_for_row(
                self.scroll_offset,
                top.max(bottom.saturating_sub(1)),
                total_rows,
                self.height,
                ViewportAlignment::End,
            ),
            ViewportAlignment::Center => scroll_offset_for_row(
                self.scroll_offset,
                (anchor.start + anchor.end - 1) / 2,
                total_rows,
                self.height,
                ViewportAlignment::Center,
            ),
            ViewportAlignment::Nearest => {
                if top < visible.start
                    || (oversized == ScrollRangeOversized::Start && bottom - top >= self.height)
                {
                    scroll_offset_for_row(
                        self.scroll_offset,
                        top,
                        total_rows,
                        self.height,
                        ViewportAlignment::Start,
                    )
                } else if bottom > visible.end {
                    scroll_offset_for_row(
                        self.scroll_offset,
                        top.max(bottom.saturating_sub(1)),
                        total_rows,
                        self.height,
                        ViewportAlignment::End,
                    )
                } else {
                    self.scroll_offset
                }
            }
        };
        self.scroll_to(next);
        self.range_snapshot = Some(RangeSnapshot {
            id: anchor.id,
            start: anchor.start,
            end: anchor.end,
            margin,
            alignment,
            mode,
            oversized,
            width,
            height: self.height,
            total_rows,
        });
        true
    }
    /// Show or hide the scrollbar column.
    pub fn set_show_scrollbar(&mut self, show: bool) {
        self.scrollbar = if show {
            ScrollbarMode::Always
        } else {
            ScrollbarMode::Never
        };
    }

    /// Set the scrollbar visibility policy.
    pub fn set_scrollbar_mode(&mut self, mode: ScrollbarMode) {
        self.scrollbar = mode;
    }

    /// Replace the scrollbar track and thumb styles.
    pub fn set_scrollbar_style(&mut self, theme: ScrollViewTheme) {
        self.theme = theme;
    }

    /// Current scrollbar track and thumb styles.
    pub fn scrollbar_style(&self) -> &ScrollViewTheme {
        &self.theme
    }

    /// Set the track and thumb glyphs (first character wins).
    pub fn set_scrollbar_chars(&mut self, track: &str, thumb: &str) {
        self.track_char = first_cell_glyph(track, "│");
        self.thumb_char = first_cell_glyph(thumb, "█");
    }

    /// Set the truncation marker for oversized rows.
    pub fn set_ellipsis(&mut self, ellipsis: Ellipsis) {
        self.ellipsis = ellipsis;
    }

    /// Rows moved per fast-scroll key or wheel notch.
    pub fn set_fast_scroll_lines(&mut self, lines: usize) {
        self.fast_scroll_lines = lines.max(1);
    }

    /// Apply arrow, page, home, and end keys. Returns whether the key was handled.
    pub fn handle_scroll_key(&mut self, key: &Key) -> bool {
        if matches!(key, Key::CtrlUp) {
            self.scroll_by(-(self.fast_scroll_lines as i64));
            return true;
        }
        if matches!(key, Key::CtrlDown) {
            self.scroll_by(self.fast_scroll_lines as i64);
            return true;
        }
        if is_arrow_up(key) {
            self.scroll_by(-1);
            return true;
        }
        if is_arrow_down(key) {
            self.scroll_by(1);
            return true;
        }
        if is_page_up(key) {
            self.page(-1);
            return true;
        }
        if is_page_down(key) {
            self.page(1);
            return true;
        }
        if is_home(key) {
            self.scroll_to_top();
            return true;
        }
        if is_end(key) {
            self.scroll_to_bottom();
            return true;
        }
        false
    }

    fn row_count(&self) -> usize {
        self.total_rows.unwrap_or(self.measured_rows)
    }

    fn should_render_scrollbar(&self, row_count: usize, width: usize) -> bool {
        if self.height == 0 || width == 0 || self.scrollbar == ScrollbarMode::Never {
            return false;
        }
        if self.scrollbar == ScrollbarMode::Always {
            return true;
        }
        viewport_overflows(row_count, self.height)
    }

    fn reconcile_bounds(&mut self, old_total: usize, old_height: usize) {
        let new_total = self.row_count();
        if let Some(active) = self.active_row {
            self.scroll_offset = scroll_offset_for_row(
                self.scroll_offset,
                active,
                new_total,
                self.height,
                self.active_alignment,
            );
            return;
        }
        let was_at_tail = self.scroll_offset >= max_scroll_offset(old_total, old_height);
        if self.anchor == ScrollAnchor::End {
            let trailing_rows = old_total.saturating_sub(self.scroll_offset + old_height);
            let target = new_total
                .saturating_sub(self.height)
                .saturating_sub(trailing_rows);
            self.scroll_offset = clamp_scroll_offset(target, new_total, self.height);
        } else if self.follow_tail && was_at_tail {
            self.scroll_offset = max_scroll_offset(new_total, self.height);
        } else {
            self.scroll_offset = clamp_scroll_offset(self.scroll_offset, new_total, self.height);
        }
    }
}

/// First character of `value`, or `fallback` when empty.
fn first_cell_glyph(value: &str, fallback: &str) -> String {
    value
        .chars()
        .next()
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

impl Component for ScrollView {
    fn render(&mut self, width: u16) -> Vec<String> {
        let safe_width = width as usize;
        if self.height == 0 || safe_width == 0 {
            return Vec::new();
        }

        let initial_total = self.total_rows.unwrap_or(self.measured_rows);
        let initial_show = self.should_render_scrollbar(initial_total, safe_width);
        let initial_content_width = safe_width.saturating_sub(initial_show as usize);

        let mut child_rows: Option<Vec<String>> = self
            .child_mut()
            .map(|child| child.render(initial_content_width as u16));
        if let Some(rows) = &child_rows {
            self.measured_rows = rows.len();
        }

        let row_count = self.row_count();
        let show_scrollbar = self.should_render_scrollbar(row_count, safe_width);
        let content_width = safe_width.saturating_sub(show_scrollbar as usize);
        if child_rows.is_some() && content_width != initial_content_width {
            child_rows = self
                .child_mut()
                .map(|child| child.render(content_width as u16));
            if let Some(rows) = &child_rows {
                self.measured_rows = rows.len();
            }
        }

        self.scroll_offset = clamp_scroll_offset(self.scroll_offset, row_count, self.height);
        let thumb = if show_scrollbar {
            Some(scrollbar_thumb_range(self.height, row_count, self.scroll_offset))
        } else {
            None
        };
        let track_sample = self.theme.track.paint(&self.track_char);
        let thumb_sample = self.theme.thumb.paint(&self.thumb_char);
        let track_width = visible_width(&track_sample);
        let thumb_width = visible_width(&thumb_sample);

        let mut out = Vec::with_capacity(self.height);
        for row in 0..self.height {
            let index = self.scroll_offset + row;
            let line = match &child_rows {
                Some(rows) => rows.get(index).map(String::as_str).unwrap_or(""),
                None => self.lines.get(index).map(String::as_str).unwrap_or(""),
            };
            let truncated = if content_width == 0 {
                String::new()
            } else {
                truncate_to_width(line, content_width, self.ellipsis)
            };
            if !show_scrollbar {
                out.push(truncated);
                continue;
            }
            let padded = pad_to_width(&truncated, content_width);
            let (glyph, glyph_width) = match thumb {
                Some(range) if row >= range.start && row < range.end => {
                    (&thumb_sample, thumb_width)
                }
                _ => (&track_sample, track_width),
            };
            let composed = format!("{padded}{glyph}");
            if content_width + glyph_width <= safe_width {
                out.push(composed);
            } else {
                out.push(truncate_to_width(&composed, safe_width, Ellipsis::Omit));
            }
        }
        out
    }

    fn handle_input(&mut self, key: &Key) {
        self.handle_scroll_key(key);
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        match event.kind {
            MouseKind::ScrollUp => self.scroll_by(-(self.fast_scroll_lines as i64)),
            MouseKind::ScrollDown => self.scroll_by(self.fast_scroll_lines as i64),
            _ => {}
        }
    }

    fn invalidate(&mut self) {
        if let Some(child) = self.child_mut() {
            child.invalidate();
        }
    }

    fn dispose(&mut self) {
        self.dispose_children();
        self.lines = Vec::new();
        self.measured_rows = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;
    use crate::components::scroll_viewport::ViewportRange;
    use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
    use crate::style::Style;

    struct Echo;

    impl Component for Echo {
        fn render(&mut self, width: u16) -> Vec<String> {
            vec![format!("w{width}")]
        }
    }

    fn ten_rows() -> Vec<String> {
        (0..10).map(|index| format!("row{index}")).collect()
    }

    fn plain_theme() -> ScrollViewTheme {
        ScrollViewTheme {
            track: Style::default(),
            thumb: Style::default(),
        }
    }

    fn mouse(kind: MouseKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    #[test]
    fn offset_never_leaves_content_bounds() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.scroll_by(100);
        assert_eq!(view.scroll_offset(), 7);
        view.scroll_by(-100);
        assert_eq!(view.scroll_offset(), 0);
        view.scroll_to(99);
        assert_eq!(view.scroll_offset(), 7);
        assert_eq!(view.visible_range(), ViewportRange { start: 7, end: 10 });
    }

    #[test]
    fn renders_exact_rows_at_fixed_width() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        assert_eq!(view.render(6), vec!["row0", "row1", "row2"]);
        view.scroll_to(2);
        assert_eq!(view.render(6), vec!["row2", "row3", "row4"]);
        view.scroll_to_bottom();
        assert_eq!(view.render(6), vec!["row7", "row8", "row9"]);
    }

    #[test]
    fn draws_scrollbar_track_and_thumb() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.set_scrollbar_style(plain_theme());
        assert_eq!(
            view.render(6),
            vec!["row0 █", "row1 │", "row2 │"]
        );
    }

    #[test]
    fn forced_scrollbar_uses_full_height() {
        let mut view = ScrollView::new(vec!["a".to_string()], 3);
        view.set_scrollbar_style(plain_theme());
        view.set_show_scrollbar(true);
        assert_eq!(view.render(3), vec!["a █", "  █", "  █"]);
    }

    #[test]
    fn empty_content_and_zero_width_are_safe() {
        let mut view = ScrollView::new(Vec::new(), 3);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        assert_eq!(view.render(5), vec!["", "", ""]);
        assert!(view.render(0).is_empty());
        assert_eq!(view.content_height(), 0);
        assert_eq!(view.max_scroll_offset(), 0);
    }

    #[test]
    fn renders_child_at_content_width() {
        let mut view = ScrollView::from_child(Box::new(Echo), 2);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        assert_eq!(view.render(4), vec!["w4", ""]);
        assert_eq!(view.content_height(), 1);
    }

    #[test]
    fn keyboard_scrolls_and_clamps() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        assert!(view.handle_scroll_key(&Key::Down));
        assert_eq!(view.scroll_offset(), 1);
        assert!(view.handle_scroll_key(&Key::PageDown));
        assert_eq!(view.scroll_offset(), 3);
        assert!(view.handle_scroll_key(&Key::End));
        assert_eq!(view.scroll_offset(), 7);
        assert!(view.handle_scroll_key(&Key::Home));
        assert_eq!(view.scroll_offset(), 0);
        assert!(view.handle_scroll_key(&Key::PageUp));
        assert_eq!(view.scroll_offset(), 0);
        assert!(!view.handle_scroll_key(&Key::Tab));
    }

    #[test]
    fn wheel_scrolls_by_fast_lines() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.handle_mouse(&mouse(MouseKind::ScrollDown));
        assert_eq!(view.scroll_offset(), 5);
        view.handle_mouse(&mouse(MouseKind::ScrollUp));
        assert_eq!(view.scroll_offset(), 0);
        view.handle_mouse(&mouse(MouseKind::Down(MouseButtonKind::Left)));
        assert_eq!(view.scroll_offset(), 0);
    }

    #[test]
    fn content_replacement_reconciles_offset() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.scroll_to(7);
        view.set_lines(vec!["x".to_string()]);
        assert_eq!(view.scroll_offset(), 0);
        assert_eq!(view.content_height(), 1);
    }

    #[test]
    fn follow_tail_keeps_last_rows_visible() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.set_follow_tail(true);
        view.scroll_to_bottom();
        let mut grown: Vec<String> = ten_rows();
        grown.extend((10..20).map(|index| format!("row{index}")));
        view.set_lines(grown);
        assert_eq!(view.scroll_offset(), 17);
    }

    #[test]
    fn logical_rows_map_through_offset() {
        let mut view = ScrollView::new(ten_rows(), 3);
        view.scroll_to(4);
        assert_eq!(view.logical_row_at(0), Some(4));
        assert_eq!(view.logical_row_at(2), Some(6));
        assert_eq!(view.logical_row_at(3), None);
        assert_eq!(view.local_row_for(4), Some(0));
        assert_eq!(view.local_row_for(3), None);
    }

    #[test]
    fn reveal_range_skips_unchanged_geometry() {
        let mut view = ScrollView::new(ten_rows(), 3);
        let anchor = || ScrollRangeAnchor {
            id: "sel".to_string(),
            start: 8,
            end: 10,
            margin: None,
            alignment: None,
            mode: None,
            oversized: None,
        };
        assert!(view.reveal_range(Some(anchor()), 40));
        assert_eq!(view.scroll_offset(), 7);
        assert!(view.has_revealed_range("sel"));
        assert!(!view.reveal_range(Some(anchor()), 40));
        assert_eq!(view.scroll_offset(), 7);
        assert!(view.reveal_range(Some(anchor()), 80));
    }

    #[test]
    fn reveal_range_prefers_leading_edge_when_oversized() {
        let mut view = ScrollView::new(ten_rows(), 3);
        let anchor = ScrollRangeAnchor {
            id: "big".to_string(),
            start: 2,
            end: 9,
            margin: None,
            alignment: None,
            mode: None,
            oversized: Some(ScrollRangeOversized::Start),
        };
        assert!(view.reveal_range(Some(anchor), 40));
        assert_eq!(view.scroll_offset(), 1);
        assert!(!view.reveal_range(None, 40));
        assert!(!view.has_revealed_range("big"));
    }
}
