//! Text whose content is reformatted against the actual render width.
//!
//! A plain [`Text`](crate::components::text::Text) receives an already
//! formatted string and only wraps it at render time, so width-dependent layout
//! (per-line truncation, gutters, inline previews) has to be decided before the
//! width is known. [`WidthAwareText`] defers that decision to `render(width)`:
//! it computes the same content width the inner `Text` uses, hands it to a
//! formatter closure, and delegates margins and wrapping to `Text`.

use crate::component::Component;
use crate::components::text::Text;
use crate::render::render_utils::get_padding_x;
use crate::text_shape::{visible_width, wrap_text_with_ansi};

/// Text reformatted for the width it is about to be drawn at.
pub struct WidthAwareText {
    format: Box<dyn FnMut(usize) -> String>,
    padding_x: usize,
    padding_y: usize,
    ignore_tight: bool,
    inner: Text,
    cached_content_width: Option<usize>,
    cached_text: String,
}

impl WidthAwareText {
    /// Create a width-aware block from a formatter that receives the content
    /// width (viewport minus horizontal padding).
    pub fn new(
        format: impl FnMut(usize) -> String + 'static,
        padding_x: usize,
        padding_y: usize,
    ) -> Self {
        WidthAwareText {
            format: Box::new(format),
            padding_x,
            padding_y,
            ignore_tight: false,
            inner: Text::padded("", padding_x as u16, padding_y as u16),
            cached_content_width: None,
            cached_text: String::new(),
        }
    }

    /// When true, configured padding is used as-is instead of the tight-mode
    /// reduction.
    pub fn set_ignore_tight(&mut self, ignore: bool) {
        self.ignore_tight = ignore;
        self.cached_content_width = None;
    }

    /// Drop derived caches; the next render rebuilds from state.
    pub fn invalidate(&mut self) {
        self.cached_content_width = None;
        self.cached_text.clear();
        self.inner.invalidate();
    }

    /// Re-run the formatter on the next render while keeping the inner wrap
    /// cache, so a reformat that yields the same string skips re-wrapping.
    pub fn reformat(&mut self) {
        self.cached_content_width = None;
    }

    /// The formatter's output for a content width, recomputed only when the
    /// width changes.
    pub fn formatted(&mut self, content_width: usize) -> &str {
        if self.cached_content_width != Some(content_width) || self.cached_text.is_empty() {
            self.cached_content_width = Some(content_width);
            self.cached_text = (self.format)(content_width);
        }
        &self.cached_text
    }
}

impl Component for WidthAwareText {
    fn render(&mut self, width: u16) -> Vec<String> {
        let padding_x = if self.ignore_tight {
            self.padding_x
        } else {
            get_padding_x(self.padding_x)
        };
        self.inner.set_padding(padding_x as u16, self.padding_y as u16);
        let content_width = (width as usize).saturating_sub(padding_x * 2).max(1);
        let content = self.formatted(content_width).to_string();
        self.inner.set_text(content);
        self.inner.render(width)
    }
}

/// Wrap `content` under a distinct first-row prefix, keeping its column on
/// continuation rows. When the prefix cannot fit, the prefix and content wrap
/// together.
pub fn wrap_prefixed_line(
    prefix: &str,
    continuation_prefix: &str,
    content: &str,
    width: usize,
) -> Vec<String> {
    let prefix_width = visible_width(prefix);
    let continuation_width = visible_width(continuation_prefix);
    let gutter = prefix_width.max(continuation_width);
    if width <= gutter {
        return wrap_text_with_ansi(&format!("{prefix}{content}"), width);
    }
    let rows = wrap_text_with_ansi(content, width - gutter);
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            if index == 0 {
                format!("{prefix}{row}")
            } else {
                format!("{continuation_prefix}{row}")
            }
        })
        .collect()
}

/// Wrap `content` under a fixed-width gutter, padding continuation rows with
/// spaces so the text column never shifts.
pub fn wrap_with_gutter(content: &str, gutter: usize, width: usize) -> Vec<String> {
    let continuation = " ".repeat(gutter);
    wrap_prefixed_line(&continuation, &continuation, content, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_aware_text_formats_once_per_width() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let counted = std::rc::Rc::clone(&calls);
        let mut text = WidthAwareText::new(
            move |width| {
                counted.set(counted.get() + 1);
                format!("w{width}")
            },
            1,
            0,
        );
        assert_eq!(text.render(9), vec![" w7      "]);
        assert_eq!(text.render(9), vec![" w7      "]);
        assert_eq!(text.render(10), vec![" w8       "]);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn width_aware_text_wraps_to_content_width() {
        let mut text = WidthAwareText::new(|width| "x".repeat(width + 5), 1, 0);
        let rows = text.render(10);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| visible_width(row) <= 10));
    }

    #[test]
    fn prefixed_wrap_keeps_column_on_continuation() {
        let rows = wrap_prefixed_line(">>> ", "    ", "aaaa bbbb cccc", 10);
        assert_eq!(rows, vec![">>> aaaa", "    bbbb", "    cccc"]);
    }

    #[test]
    fn prefixed_wrap_falls_back_when_prefix_does_not_fit() {
        let rows = wrap_prefixed_line("long-prefix ", "  ", "abc", 4);
        assert_eq!(rows, vec!["long", "-pre", "fix", "abc"]);
    }

    #[test]
    fn gutter_wrap_pads_continuation_rows() {
        assert_eq!(wrap_with_gutter("abcdef", 2, 5), vec!["  abc", "  def"]);
        assert_eq!(wrap_with_gutter("", 2, 5), vec!["  "]);
    }

    #[test]
    fn narrow_widths_never_overflow() {
        let rows = wrap_prefixed_line(">>> ", "    ", "abc", 0);
        assert_eq!(rows, vec![""]);
    }
}
