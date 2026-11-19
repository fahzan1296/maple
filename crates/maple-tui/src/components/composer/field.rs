//! Compact filled composer: every input row is a one-line field with accent end
//! caps and a subtle surface fill. The complete status line remains below it.
//!
//! Reference: `l/src/components/composer/field.ts`.

use std::sync::{Arc, LazyLock};

use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

const LEFT_CAP: &str = "▐";
const RIGHT_CAP: &str = "▌";

fn render_top(_ctx: &ComposerChromeContext) -> Option<String> {
	None
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	let left = (ctx.accent_color)(LEFT_CAP);
	let left_fill = format!("{}{}{}", " ".repeat(ctx.padding_x), ctx.gutter, ctx.text);
	if ctx.ime_safe_cursor_tail {
		return vec![format!("{left}{}", (ctx.surface_color)(&left_fill))];
	}
	let right_chrome_cells = (ctx.padding_x + 1)
		.saturating_sub(ctx.cursor_overflow)
		.max(1);
	let interior = format!(
		"{}{}{}",
		left_fill,
		ctx.pad,
		" ".repeat(right_chrome_cells - 1)
	);
	let right_glyph = if ctx.scrollbar_thumb { "█" } else { RIGHT_CAP };
	vec![format!(
		"{left}{}{}",
		(ctx.surface_color)(&interior),
		(ctx.accent_color)(right_glyph)
	)]
}

fn render_bottom(_ctx: &ComposerChromeContext) -> Option<String> {
	None
}

/// One-row filled field with accent caps.
pub static FIELD_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "field".to_string(),
	filled_surface: Some(true),
	side_borders: true,
	vertical_chrome: VerticalChrome::Zero,
	status_attachment: StatusAttachment::None,
	bottom_bar: BottomBar::Full,
	bottom_bar_gap: true,
	default_prompt_gutter: None,
	default_padding_x: Arc::new(|_| 1),
	side_chrome_width: Arc::new(|padding_x| padding_x + 1),
	render_top: Arc::new(render_top),
	render_row: Arc::new(render_row),
	render_bottom: Arc::new(render_bottom),
});

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{test_chrome, test_row};

	fn paints(ctx: &mut ComposerChromeContext) {
		ctx.accent_color = Arc::new(|text| format!("<{text}>"));
		ctx.surface_color = Arc::new(|text| format!("<{text}>"));
	}

	#[test]
	fn row_wraps_the_surface_between_accent_caps() {
		let mut ctx = test_chrome(10, 1);
		paints(&mut ctx);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		assert_eq!(
			(FIELD_COMPOSER_STYLE.render_row)(&row),
			vec!["<▐>< hi   ><▌>".to_string()]
		);
	}

	#[test]
	fn cursor_overflow_drops_padding_cells_before_the_cap() {
		let mut ctx = test_chrome(10, 1);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		row.cursor_overflow = 2;
		assert_eq!(
			(FIELD_COMPOSER_STYLE.render_row)(&row),
			vec!["▐ hi  ▌".to_string()]
		);

		row.cursor_overflow = 3;
		assert_eq!(
			(FIELD_COMPOSER_STYLE.render_row)(&row),
			vec!["▐ hi  ▌".to_string()]
		);
	}

	#[test]
	fn scrollbar_thumb_replaces_the_right_cap() {
		let mut ctx = test_chrome(10, 1);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		row.scrollbar_thumb = true;
		assert_eq!(
			(FIELD_COMPOSER_STYLE.render_row)(&row),
			vec!["▐ hi   █".to_string()]
		);
	}

	#[test]
	fn ime_safe_tail_keeps_the_row_open() {
		let mut ctx = test_chrome(10, 1);
		paints(&mut ctx);
		let mut row = test_row(&ctx, "hi");
		row.ime_safe_cursor_tail = true;
		assert_eq!(
			(FIELD_COMPOSER_STYLE.render_row)(&row),
			vec!["<▐>< hi>".to_string()]
		);
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*FIELD_COMPOSER_STYLE;
		assert_eq!(style.id, "field");
		assert_eq!(style.filled_surface, Some(true));
		assert!(style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::Zero);
		assert_eq!(style.status_attachment, StatusAttachment::None);
		assert_eq!(style.bottom_bar, BottomBar::Full);
		assert!(style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter, None);
		assert_eq!((style.default_padding_x)(None), 1);
		assert_eq!((style.default_padding_x)(Some(9)), 1);
		assert_eq!((style.side_chrome_width)(1), 2);
		assert_eq!((style.render_top)(&test_chrome(4, 1)), None);
		assert_eq!((style.render_bottom)(&test_chrome(4, 1)), None);
	}
}
