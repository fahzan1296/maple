//! Shared viewport geometry: scroll bounds, visible ranges, and scrollbar thumbs.
//! Reference: `l/src/components/scroll-viewport.ts`.
//!
//! These helpers are the single source of truth for `ScrollView`, `ScrollViewport`,
//! and any other component that windows a logical row buffer.

use crate::ansi_scan::{scan, Token};
use crate::component::Component;
use crate::keys::{is_arrow_down, is_arrow_up, is_end, is_home, is_page_down, is_page_up, Key};
use crate::mouse::{MouseKind, MouseEvent};
use crate::style::{Color, Style};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// How a revealed row is aligned inside the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewportAlignment {
    /// Keep the current offset while the row is already visible.
    #[default]
    Nearest,
    /// Place the row at the top edge.
    Start,
    /// Place the row in the middle.
    Center,
    /// Place the row at the bottom edge.
    End,
}

/// Logical half-open row range visible in a viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewportRange {
    /// First visible logical row.
    pub start: usize,
    /// Exclusive end of the visible logical rows.
    pub end: usize,
}

/// Local viewport rows occupied by a scrollbar thumb.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollbarThumbRange {
    /// First local viewport row occupied by the thumb.
    pub start: usize,
    /// Exclusive end of the thumb.
    pub end: usize,
}

/// Horizontally windowed text plus the cursor position inside the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorColumnWindow {
    /// Visible source slice.
    pub text: String,
    /// First visible terminal column in the source.
    pub start_column: usize,
    /// Grapheme index of the cursor within `text`.
    pub cursor_index: usize,
}

/// Maximum legal row offset for a bounded viewport.
pub fn max_scroll_offset(total_rows: usize, viewport_rows: usize) -> usize {
    total_rows.saturating_sub(viewport_rows)
}

/// Clamp a row offset to the bounds of a viewport.
pub fn clamp_scroll_offset(offset: usize, total_rows: usize, viewport_rows: usize) -> usize {
    offset.min(max_scroll_offset(total_rows, viewport_rows))
}

/// Logical half-open row range visible at an offset.
pub fn viewport_range(total_rows: usize, viewport_rows: usize, offset: usize) -> ViewportRange {
    let start = clamp_scroll_offset(offset, total_rows, viewport_rows);
    ViewportRange {
        start,
        end: (start + viewport_rows).min(total_rows),
    }
}

/// Whether the logical row set is taller than its viewport.
pub fn viewport_overflows(total_rows: usize, viewport_rows: usize) -> bool {
    total_rows > viewport_rows
}

/// Offset that keeps `row` visible under the requested alignment.
pub fn scroll_offset_for_row(
    current_offset: usize,
    row: usize,
    total_rows: usize,
    viewport_rows: usize,
    alignment: ViewportAlignment,
) -> usize {
    if total_rows == 0 || viewport_rows == 0 {
        return 0;
    }
    let selected = row.min(total_rows - 1);
    let current = clamp_scroll_offset(current_offset, total_rows, viewport_rows);
    let next = match alignment {
        ViewportAlignment::Start => selected,
        ViewportAlignment::Center => selected.saturating_sub(viewport_rows / 2),
        ViewportAlignment::End => selected.saturating_sub(viewport_rows.saturating_sub(1)),
        ViewportAlignment::Nearest => {
            if selected < current {
                selected
            } else if selected >= current + viewport_rows {
                selected.saturating_sub(viewport_rows.saturating_sub(1))
            } else {
                current
            }
        }
    };
    clamp_scroll_offset(next, total_rows, viewport_rows)
}

/// Selection-centered logical row window.
pub fn centered_viewport_range(
    selected_row: usize,
    total_rows: usize,
    viewport_rows: usize,
) -> ViewportRange {
    let start = scroll_offset_for_row(
        0,
        selected_row,
        total_rows,
        viewport_rows,
        ViewportAlignment::Center,
    );
    viewport_range(total_rows, viewport_rows, start)
}

/// Scrollbar thumb geometry local to the viewport rows.
pub fn scrollbar_thumb_range(
    viewport_rows: usize,
    total_rows: usize,
    scroll_offset: usize,
) -> ScrollbarThumbRange {
    if viewport_rows == 0 {
        return ScrollbarThumbRange { start: 0, end: 0 };
    }
    if total_rows <= viewport_rows {
        return ScrollbarThumbRange {
            start: 0,
            end: viewport_rows,
        };
    }
    let size = ((viewport_rows * viewport_rows) / total_rows).clamp(1, viewport_rows);
    let travel = viewport_rows - size;
    let max_offset = max_scroll_offset(total_rows, viewport_rows);
    let offset = clamp_scroll_offset(scroll_offset, total_rows, viewport_rows);
    let start = if max_offset == 0 {
        0
    } else {
        ((offset as f64 / max_offset as f64) * travel as f64).round() as usize
    };
    let start = start.min(travel);
    ScrollbarThumbRange {
        start,
        end: start + size,
    }
}

/// One measurable fragment of a string: either an escape sequence or a grapheme.
struct Cell {
    width: usize,
    offset: usize,
    is_text: bool,
}

/// Split `text` into escape and grapheme cells with byte offsets.
fn cells(text: &str) -> Vec<Cell> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for token in scan(text) {
        match token {
            Token::Escape(seq) => {
                out.push(Cell {
                    width: 0,
                    offset: pos,
                    is_text: false,
                });
                pos += seq.len();
            }
            Token::Text(chunk) => {
                let mut offset = pos;
                for grapheme in chunk.graphemes(true) {
                    out.push(Cell {
                        width: UnicodeWidthStr::width(grapheme),
                        offset,
                        is_text: true,
                    });
                    offset += grapheme.len();
                }
                pos += chunk.len();
            }
        }
    }
    out
}

/// Horizontally window a single rendered line around its cursor.
///
/// `cursor_index` counts graphemes from the start of `text`. The returned
/// `cursor_index` is the cursor's grapheme index inside the returned slice.
pub fn cursor_column_window(text: &str, cursor_index: usize, width: usize) -> CursorColumnWindow {
    if width == 0 {
        return CursorColumnWindow {
            text: String::new(),
            start_column: 0,
            cursor_index: 0,
        };
    }

    let cell_list = cells(text);
    let total_columns: usize = cell_list.iter().map(|cell| cell.width).sum();
    let grapheme_count = cell_list.iter().filter(|cell| cell.is_text).count();
    let cursor = cursor_index.min(grapheme_count);

    let mut cursor_columns = 0usize;
    let mut cursor_width = 1usize;
    let mut seen = 0usize;
    for cell in &cell_list {
        if !cell.is_text {
            continue;
        }
        if seen == cursor {
            cursor_width = cell.width;
            break;
        }
        cursor_columns += cell.width;
        seen += 1;
    }

    let max_start = total_columns.saturating_sub(width);
    let mut start_column = 0usize;
    if total_columns > width {
        start_column = cursor_columns.saturating_sub(width / 2).min(max_start);
        let max_cursor_column = width.saturating_sub(cursor_width);
        let relative_cursor_column = cursor_columns.saturating_sub(start_column);
        if relative_cursor_column > max_cursor_column {
            start_column = cursor_columns
                .saturating_sub(max_cursor_column)
                .min(max_start);
        }
    }

    let mut skip_bytes = text.len();
    let mut column = 0usize;
    for cell in &cell_list {
        if column >= start_column {
            skip_bytes = cell.offset;
            break;
        }
        column += cell.width;
    }

    let mut prefix_count = 0usize;
    let mut prefix_column = 0usize;
    for cell in &cell_list {
        if !cell.is_text {
            continue;
        }
        if prefix_column >= cursor_columns {
            break;
        }
        if cell.offset >= skip_bytes {
            prefix_count += 1;
        }
        prefix_column += cell.width;
    }

    CursorColumnWindow {
        text: truncate_to_width(&text[skip_bytes..], width, Ellipsis::Omit),
        start_column,
        cursor_index: prefix_count,
    }
}

/// Scrollbar visibility policy shared by viewport components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarMode {
    /// Show only when content overflows the viewport.
    Auto,
    /// Always show the scrollbar column.
    Always,
    /// Never show the scrollbar column.
    Never,
}

impl Default for ScrollbarMode {
    fn default() -> Self {
        ScrollbarMode::Auto
    }
}

/// Styling for a scrollbar track and thumb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollViewTheme {
    /// Style applied to the track glyph.
    pub track: Style,
    /// Style applied to the thumb glyph.
    pub thumb: Style,
}

impl Default for ScrollViewTheme {
    fn default() -> Self {
        ScrollViewTheme {
            track: Style::fg(Color::DarkGray),
            thumb: Style::fg(Color::Gray),
        }
    }
}

/// Scrolling text viewport with no child component.
pub struct ScrollViewport {
    lines: Vec<String>,
    height: usize,
    scroll_offset: usize,
    scrollbar: ScrollbarMode,
    theme: ScrollViewTheme,
    track_char: String,
    thumb_char: String,
    ellipsis: Ellipsis,
    fast_scroll_lines: usize,
    cursor_line: Option<usize>,
    cursor_style: Style,
    on_scroll: Option<Box<dyn FnMut(usize) + 'static>>,
}

impl Default for ScrollViewport {
    fn default() -> Self {
        ScrollViewport::new(Vec::new(), 0)
    }
}

impl ScrollViewport {
    /// Create a viewport over an owned row buffer.
    pub fn new(lines: Vec<String>, height: usize) -> Self {
        ScrollViewport {
            lines,
            height,
            scroll_offset: 0,
            scrollbar: ScrollbarMode::Auto,
            theme: ScrollViewTheme::default(),
            track_char: "│".to_string(),
            thumb_char: "█".to_string(),
            ellipsis: Ellipsis::default(),
            fast_scroll_lines: 5,
            cursor_line: None,
            cursor_style: Style::default().reverse(),
            on_scroll: None,
        }
    }

    /// Replace the row buffer.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.scroll_offset = clamp_scroll_offset(self.scroll_offset, self.lines.len(), self.height);
    }

    /// Replace the row buffer from a single text block.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.set_lines(text.into().split('\n').map(str::to_string).collect());
    }

    /// Current text as a single block.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Set the viewport height in rows.
    pub fn set_viewport_height(&mut self, height: usize) {
        self.height = height;
        self.scroll_offset = clamp_scroll_offset(self.scroll_offset, self.lines.len(), self.height);
    }

    /// Viewport height in rows.
    pub fn viewport_height(&self) -> usize {
        self.height
    }

    /// Number of content rows.
    pub fn content_height(&self) -> usize {
        self.lines.len()
    }

    /// Current scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Maximum legal scroll offset.
    pub fn max_scroll_offset(&self) -> usize {
        max_scroll_offset(self.lines.len(), self.height)
    }

    /// Current half-open visible row range.
    pub fn visible_range(&self) -> ViewportRange {
        viewport_range(self.lines.len(), self.height, self.scroll_offset)
    }

    /// Scroll to an absolute offset, clamped to the content bounds.
    pub fn scroll_to(&mut self, offset: usize) {
        self.apply_offset(offset);
    }

    /// Scroll by `delta` rows, clamped to the content bounds.
    pub fn scroll_by(&mut self, delta: i64) {
        let max = self.max_scroll_offset() as i64;
        let next = (self.scroll_offset as i64 + delta).clamp(0, max);
        self.apply_offset(next as usize);
    }

    /// Scroll by one page (height minus one, at least one row).
    pub fn page(&mut self, delta: i64) {
        let step = self.height.saturating_sub(1).max(1) as i64;
        self.scroll_by(step * delta);
    }

    /// Scroll to the first content row.
    pub fn scroll_to_top(&mut self) {
        self.apply_offset(0);
    }

    /// Scroll to the last content row.
    pub fn scroll_to_bottom(&mut self) {
        self.apply_offset(self.max_scroll_offset());
    }

    /// Set the highlighted cursor line, keeping it visible.
    pub fn set_cursor_line(&mut self, line: Option<usize>) {
        self.cursor_line = line;
        if let Some(line) = line {
            let next = scroll_offset_for_row(
                self.scroll_offset,
                line,
                self.lines.len(),
                self.height,
                ViewportAlignment::Nearest,
            );
            self.apply_offset(next);
        }
    }

    /// Highlighted cursor line, if any.
    pub fn cursor_line(&self) -> Option<usize> {
        self.cursor_line
    }

    /// Style applied to the cursor line.
    pub fn set_cursor_style(&mut self, style: Style) {
        self.cursor_style = style;
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

    /// Register a callback fired with the new offset after every scroll.
    pub fn set_on_scroll<F: FnMut(usize) + 'static>(&mut self, callback: F) {
        self.on_scroll = Some(Box::new(callback));
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

    fn apply_offset(&mut self, next: usize) {
        let next = clamp_scroll_offset(next, self.lines.len(), self.height);
        if next == self.scroll_offset {
            return;
        }
        self.scroll_offset = next;
        if let Some(callback) = self.on_scroll.as_mut() {
            callback(next);
        }
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
}

/// First character of `value`, or `fallback` when empty.
fn first_cell_glyph(value: &str, fallback: &str) -> String {
    value
        .chars()
        .next()
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

impl Component for ScrollViewport {
    fn render(&mut self, width: u16) -> Vec<String> {
        let safe_width = width as usize;
        if self.height == 0 || safe_width == 0 {
            return Vec::new();
        }
        let row_count = self.lines.len();
        let show_scrollbar = self.should_render_scrollbar(row_count, safe_width);
        let content_width = safe_width.saturating_sub(show_scrollbar as usize);
        self.scroll_offset = clamp_scroll_offset(self.scroll_offset, row_count, self.height);
        let thumb = if show_scrollbar {
            Some(scrollbar_thumb_range(
                self.height,
                row_count,
                self.scroll_offset,
            ))
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
            let line = self.lines.get(index).map(String::as_str).unwrap_or("");
            let styled = if self.cursor_line == Some(index) {
                self.cursor_style.paint(line)
            } else {
                line.to_string()
            };
            let truncated = if content_width == 0 {
                String::new()
            } else {
                truncate_to_width(&styled, content_width, self.ellipsis)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn offset_bounds_never_exceed_content() {
        assert_eq!(max_scroll_offset(10, 3), 7);
        assert_eq!(max_scroll_offset(2, 3), 0);
        assert_eq!(clamp_scroll_offset(99, 10, 3), 7);
        assert_eq!(clamp_scroll_offset(0, 10, 3), 0);
        assert_eq!(clamp_scroll_offset(99, 0, 3), 0);
    }

    #[test]
    fn viewport_range_is_half_open() {
        assert_eq!(viewport_range(10, 3, 0), ViewportRange { start: 0, end: 3 });
        assert_eq!(viewport_range(10, 3, 7), ViewportRange { start: 7, end: 10 });
        assert_eq!(viewport_range(10, 3, 99), ViewportRange { start: 7, end: 10 });
        assert_eq!(viewport_range(2, 3, 0), ViewportRange { start: 0, end: 2 });
    }

    #[test]
    fn overflow_requires_more_rows_than_height() {
        assert!(viewport_overflows(4, 3));
        assert!(!viewport_overflows(3, 3));
        assert!(!viewport_overflows(0, 0));
    }

    #[test]
    fn row_alignment_matches_requested_edge() {
        assert_eq!(scroll_offset_for_row(0, 5, 10, 3, ViewportAlignment::Start), 5);
        assert_eq!(scroll_offset_for_row(0, 5, 10, 3, ViewportAlignment::End), 3);
        assert_eq!(scroll_offset_for_row(0, 5, 10, 4, ViewportAlignment::Center), 3);
        assert_eq!(scroll_offset_for_row(2, 1, 10, 3, ViewportAlignment::Nearest), 1);
        assert_eq!(scroll_offset_for_row(2, 2, 10, 3, ViewportAlignment::Nearest), 2);
        assert_eq!(scroll_offset_for_row(0, 9, 10, 3, ViewportAlignment::Nearest), 7);
        assert_eq!(scroll_offset_for_row(0, 0, 0, 3, ViewportAlignment::Start), 0);
    }

    #[test]
    fn centered_range_wraps_the_selection() {
        assert_eq!(
            centered_viewport_range(5, 10, 4),
            ViewportRange { start: 3, end: 7 }
        );
    }

    #[test]
    fn thumb_fills_viewport_when_content_fits() {
        assert_eq!(
            scrollbar_thumb_range(3, 3, 0),
            ScrollbarThumbRange { start: 0, end: 3 }
        );
    }

    #[test]
    fn thumb_travels_with_offset() {
        let thumb = scrollbar_thumb_range(3, 10, 0);
        assert_eq!(thumb, ScrollbarThumbRange { start: 0, end: 1 });
        let thumb = scrollbar_thumb_range(3, 10, 7);
        assert_eq!(thumb, ScrollbarThumbRange { start: 2, end: 3 });
        let thumb = scrollbar_thumb_range(0, 10, 0);
        assert_eq!(thumb, ScrollbarThumbRange { start: 0, end: 0 });
    }

    #[test]
    fn cursor_window_centers_cursor_and_reports_index() {
        let window = cursor_column_window("abcdef", 3, 4);
        assert_eq!(window.text, "bcde");
        assert_eq!(window.start_column, 1);
        assert_eq!(window.cursor_index, 2);
    }

    #[test]
    fn cursor_window_keeps_cursor_at_end() {
        let window = cursor_column_window("abcdef", 6, 4);
        assert_eq!(window.text, "cdef");
        assert_eq!(window.cursor_index, 4);
    }

    #[test]
    fn cursor_window_handles_zero_width_and_overflow() {
        let window = cursor_column_window("abc", 1, 0);
        assert_eq!(window.text, "");
        assert_eq!(window.cursor_index, 0);
        let window = cursor_column_window("abcdef", 99, 2);
        assert_eq!(window.text, "ef");
        assert_eq!(window.cursor_index, 2);
    }

    fn ten_rows() -> Vec<String> {
        (0..10).map(|index| format!("r{index}")).collect()
    }

    #[test]
    fn viewport_renders_exact_rows_and_clamps() {
        let mut view = ScrollViewport::new(ten_rows(), 3);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        assert_eq!(view.render(4), vec!["r0", "r1", "r2"]);
        view.scroll_by(5);
        assert_eq!(view.scroll_offset(), 5);
        assert_eq!(view.render(4), vec!["r5", "r6", "r7"]);
        view.scroll_by(100);
        assert_eq!(view.scroll_offset(), 7);
        view.scroll_by(-100);
        assert_eq!(view.scroll_offset(), 0);
    }

    #[test]
    fn viewport_text_and_height() {
        let mut view = ScrollViewport::new(Vec::new(), 2);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        view.set_text("a\nb\nc");
        assert_eq!(view.content_height(), 3);
        assert_eq!(view.text(), "a\nb\nc");
        view.set_viewport_height(1);
        assert_eq!(view.viewport_height(), 1);
        assert_eq!(view.render(3), vec!["a"]);
    }

    #[test]
    fn viewport_cursor_line_scrolls_into_view() {
        let mut view = ScrollViewport::new(ten_rows(), 3);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        view.set_cursor_line(Some(8));
        assert_eq!(view.scroll_offset(), 6);
        assert_eq!(view.cursor_line(), Some(8));
        view.set_cursor_line(None);
        assert_eq!(view.cursor_line(), None);
    }

    #[test]
    fn viewport_on_scroll_fires_once_per_change() {
        let mut view = ScrollViewport::new(ten_rows(), 3);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        view.set_on_scroll(move |offset| sink.borrow_mut().push(offset));
        view.scroll_by(2);
        assert_eq!(*seen.borrow(), vec![2]);
        view.scroll_by(0);
        assert_eq!(*seen.borrow(), vec![2]);
    }

    #[test]
    fn viewport_empty_and_zero_width_are_safe() {
        let mut view = ScrollViewport::new(Vec::new(), 3);
        view.set_scrollbar_mode(ScrollbarMode::Never);
        assert_eq!(view.render(4), vec!["", "", ""]);
        assert!(view.render(0).is_empty());
    }

    #[test]
    fn viewport_scrollbar_and_keyboard() {
        let mut view = ScrollViewport::new(ten_rows(), 3);
        view.set_scrollbar_style(ScrollViewTheme {
            track: Style::default(),
            thumb: Style::default(),
        });
        assert_eq!(view.render(4), vec!["r0 █", "r1 │", "r2 │"]);
        assert!(view.handle_scroll_key(&Key::End));
        assert_eq!(view.scroll_offset(), 7);
        assert!(view.handle_scroll_key(&Key::Home));
        assert_eq!(view.scroll_offset(), 0);
        assert!(!view.handle_scroll_key(&Key::Tab));
    }
}
