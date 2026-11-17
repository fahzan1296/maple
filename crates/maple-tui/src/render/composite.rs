//! Overlay line compositing.
//! Reference: `l/src/render/composite.ts`.
//!
//! Image-marker handling lives here; the column splice itself is
//! [`crate::render::render_utils::composite_line_at`].

use crate::render::render_utils::{composite_line_at as splice_line, slice_by_column};
use crate::terminal_capabilities::terminal;

const SEGMENT_RESET: &str = "\x1b[0m";

/// Splice overlay content into a base line at a specific column.
pub fn composite_line_at(
	base_line: &str,
	overlay_line: &str,
	start_col: usize,
	overlay_width: usize,
	total_width: usize,
) -> String {
	let caps = terminal();
	// A full-width image row owns its terminal row; the writer must see its exact placement shape.
	if start_col == 0 && overlay_width >= total_width && caps.is_image_line(overlay_line) {
		return overlay_line.to_string();
	}
	if caps.is_image_line(base_line) {
		// Full-width overlays such as /switch are opaque: replace the
		// Unicode placeholder cells so the image cannot cover the modal.
		// Partial overlays cannot safely splice placement control sequences.
		if start_col != 0 || overlay_width < total_width {
			return base_line.to_string();
		}
		let overlay = slice_by_column(overlay_line, 0, total_width, true);
		return format!(
			"{SEGMENT_RESET}{}{}",
			overlay.text,
			" ".repeat(total_width.saturating_sub(overlay.width))
		);
	}

	splice_line(base_line, overlay_line, start_col, overlay_width, total_width)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::text_shape::visible_width;

	#[test]
	fn basic_overlay_splice() {
		let out = composite_line_at("hello world", "XX", 6, 2, 11);
		assert!(out.contains("XX"));
		assert!(visible_width(&out) <= 11);
	}

	#[test]
	fn clamps_width() {
		let out = composite_line_at("abcdef", "ZZZZ", 2, 4, 6);
		assert!(visible_width(&out) <= 6);
	}
}
