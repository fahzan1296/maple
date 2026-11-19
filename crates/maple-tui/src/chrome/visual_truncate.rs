//! Shared utility for truncating text to visual lines (accounting for line
//! wrapping). Used by tool-execution and bash-execution rendering for
//! consistent behavior.
//!
//! Reference: `l/src/chrome/visual-truncate.ts`.
//!
//! Deviation: the reference keeps a module-level cache of `Text` instances
//! keyed by a `Bun.hash` of the input. Rust has no global cache here, so every
//! call builds a fresh [`Text`]; the returned window and skip count are
//! unchanged. The reference's default `paddingX = 0` is an explicit
//! `padding_x` argument.

use crate::component::Component;
use crate::components::scroll_viewport::viewport_range;
use crate::components::text::Text;

/// The visual lines to display plus how many were skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisualTruncateResult {
	/// The visual lines to display.
	pub visual_lines: Vec<String>,
	/// Number of visual lines that were skipped (hidden).
	pub skipped_count: usize,
}

/// Truncate `text` to at most `max_visual_lines` visual lines, keeping the last
/// ones. Wrapping follows the terminal `width`; `padding_x` is the horizontal
/// padding of the [`Text`] component (0 inside a box, 1 in a plain container).
pub fn truncate_to_visual_lines(
	text: &str,
	max_visual_lines: usize,
	width: u16,
	padding_x: u16,
) -> VisualTruncateResult {
	if text.is_empty() {
		return VisualTruncateResult::default();
	}
	let mut component = Text::padded("", padding_x, 0);
	component.set_text(text);
	let all_visual_lines = component.render(width);
	if all_visual_lines.len() <= max_visual_lines {
		return VisualTruncateResult {
			visual_lines: all_visual_lines,
			skipped_count: 0,
		};
	}
	let range = viewport_range(
		all_visual_lines.len(),
		max_visual_lines,
		all_visual_lines.len(),
	);
	let visual_lines = all_visual_lines[range.start..range.end].to_vec();
	VisualTruncateResult {
		visual_lines,
		skipped_count: range.start,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_text_has_no_visual_lines() {
		let result = truncate_to_visual_lines("", 3, 10, 0);
		assert_eq!(result.visual_lines, Vec::<String>::new());
		assert_eq!(result.skipped_count, 0);
	}

	#[test]
	fn truncates_to_the_last_visual_lines() {
		let result = truncate_to_visual_lines("one\ntwo\nthree\nfour", 2, 10, 0);
		assert_eq!(result.visual_lines, vec!["three     ", "four      "]);
		assert_eq!(result.skipped_count, 2);
	}

	#[test]
	fn keeps_every_line_at_the_boundary() {
		let result = truncate_to_visual_lines("one\ntwo\nthree\nfour", 4, 10, 0);
		assert_eq!(
			result.visual_lines,
			vec!["one       ", "two       ", "three     ", "four      "]
		);
		assert_eq!(result.skipped_count, 0);
	}

	#[test]
	fn wraps_overlong_words_before_windowing() {
		let result = truncate_to_visual_lines("aaaaaaaaaaaa", 1, 5, 0);
		assert_eq!(result.visual_lines, vec!["aa   "]);
		assert_eq!(result.skipped_count, 2);
	}

	#[test]
	fn padding_narrows_the_wrap_column() {
		let result = truncate_to_visual_lines("a\nb", 1, 5, 1);
		assert_eq!(result.visual_lines, vec![" b   "]);
		assert_eq!(result.skipped_count, 1);
	}
}
