//! Line-based status cards shared by the internal-URL renderers (`proc://`,
//! `agent://`, `cfg://`): a header plus sanitized, width-truncated body lines.
//!
//! Ported from `l/src/tools/result-card.ts`.
//!
//! Deviation: the reference builds the card with `createCachedComponent`,
//! closing over the caller's mutable `RenderResultOptions`; Rust has no shared
//! mutable options object, so [`CardLines`] snapshots `expanded` at creation
//! and the caller re-states it with [`CardLines::set_expanded`] when the
//! transcript toggles expansion. The cache key (expansion + width) is the same.

use crate::component::Component;
use crate::keys::Key;
use crate::render::render_utils::replace_tabs;
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::tools::native_view::{ContentBlock, RenderResultOptions};

/// Tool result fields the card renderers read.
#[derive(Clone, Debug, Default)]
pub struct CardToolResult {
    /// Result content blocks.
    pub content: Vec<ContentBlock>,
    /// Whether the tool reported a failure.
    pub is_error: Option<bool>,
}

/// First text block of a tool result, or `""`.
pub fn first_text(result: &CardToolResult) -> String {
    result
        .content
        .iter()
        .find(|item| item.type_ == "text")
        .and_then(|item| item.text.clone())
        .unwrap_or_default()
}

/// Sanitize one display line: tabs become spaces, carriage returns are dropped.
pub fn safe(value: &str) -> String {
    replace_tabs(value).replace('\r', "")
}

/// Card whose lines are sanitized and truncated, re-rendered when the expanded
/// state flips.
pub struct CardLines {
    lines: Box<dyn Fn(usize, bool) -> Vec<String>>,
    expanded: bool,
    cached_expanded: Option<bool>,
    cached_width: Option<usize>,
    cache: Vec<String>,
}

impl CardLines {
    /// A card rendering `lines(width, expanded)`, starting at `options.expanded`.
    pub fn new(lines: Box<dyn Fn(usize, bool) -> Vec<String>>, options: &RenderResultOptions) -> Self {
        CardLines {
            lines,
            expanded: options.expanded,
            cached_expanded: None,
            cached_width: None,
            cache: Vec::new(),
        }
    }

    /// Re-state the expansion flag captured from `RenderResultOptions`.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    /// The expansion flag this card currently renders.
    pub fn expanded(&self) -> bool {
        self.expanded
    }
}

impl Component for CardLines {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if self.cached_expanded != Some(self.expanded) || self.cached_width != Some(width) {
            let raw = (self.lines)(width, self.expanded);
            self.cache = raw
                .into_iter()
                .flat_map(|line| line.split('\n').map(|part| part.to_string()).collect::<Vec<_>>())
                .map(|line| truncate_to_width(&safe(&line), width, Ellipsis::Unicode))
                .collect();
            self.cached_expanded = Some(self.expanded);
            self.cached_width = Some(width);
        }
        self.cache.clone()
    }

    fn handle_input(&mut self, key: &Key) {
        let _ = key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(expanded: bool) -> RenderResultOptions {
        RenderResultOptions {
            expanded,
            ..RenderResultOptions::default()
        }
    }

    #[test]
    fn first_text_picks_the_first_text_block() {
        let result = CardToolResult {
            content: vec![
                ContentBlock::text("first"),
                ContentBlock::text("second"),
            ],
            is_error: None,
        };
        assert_eq!(first_text(&result), "first");
    }

    #[test]
    fn first_text_is_empty_without_text_blocks() {
        let result = CardToolResult::default();
        assert_eq!(first_text(&result), "");
    }

    #[test]
    fn safe_replaces_tabs_and_drops_carriage_returns() {
        assert_eq!(safe("a\tb\rc"), "a   bc");
    }

    #[test]
    fn card_splits_lines_and_truncates_to_width() {
        let mut card = CardLines::new(
            Box::new(|width, expanded| {
                if expanded {
                    vec!["short\na much longer line that must be cut".to_string()]
                } else {
                    vec![format!("width={width}")]
                }
            }),
            &options(false),
        );
        assert_eq!(card.render(80), vec!["width=80"]);
        card.set_expanded(true);
        let rows = card.render(10);
        assert_eq!(rows[0], "short");
        assert!(rows[1].chars().count() <= 10);
        assert!(rows[1].ends_with('…'));
    }
}
