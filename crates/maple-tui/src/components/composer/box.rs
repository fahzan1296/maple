//! The classic maple composer: rounded frame, status line embedded in the top
//! border, and the last content row merged into the bottom border
//! (`╰─ text … ─╯`), keeping a one-line prompt at two rows total.
//!
//! Reference: `l/src/components/composer/box.ts`.

use std::sync::{Arc, LazyLock};

use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

fn spaces(cells: usize) -> String {
	" ".repeat(cells)
}

fn render_top(ctx: &ComposerChromeContext) -> Option<String> {
	let side_chrome_width = (ctx.side_chrome_width)(ctx.padding_x);
	let top_left = (ctx.border_color)(&format!(
		"{}{}",
		ctx.r#box.top_left,
		ctx.r#box.horizontal.repeat(ctx.padding_x)
	));
	let top_right = (ctx.border_color)(&format!(
		"{}{}",
		ctx.r#box.horizontal.repeat(ctx.padding_x),
		ctx.r#box.top_right
	));
	let top_fill_width = ctx.width.saturating_sub(side_chrome_width * 2);
	let Some(top_border) = ctx.top_border.as_ref() else {
		let fill = (ctx.border_color)(&ctx.r#box.horizontal.repeat(top_fill_width));
		return Some(format!("{top_left}{fill}{top_right}"));
	};
	if top_border.width <= top_fill_width {
		// Status fits - add fill after it.
		let fill_width = top_fill_width - top_border.width;
		let fill = (ctx.border_color)(&ctx.r#box.horizontal.repeat(fill_width));
		return Some(format!("{top_left}{}", top_border.content) + &fill + &top_right);
	}
	// Status too long - truncate it.
	let truncate_width = top_fill_width.saturating_sub(1);
	let truncated = if truncate_width == 0 {
		String::new()
	} else {
		truncate_to_width(&top_border.content, truncate_width, Ellipsis::Unicode)
	};
	let truncated_width = visible_width(&truncated);
	let fill_width = top_fill_width.saturating_sub(truncated_width);
	let fill = (ctx.border_color)(&ctx.r#box.horizontal.repeat(fill_width));
	Some(format!("{top_left}{truncated}{fill}{top_right}"))
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	// When the end-of-line cursor glyph (or a wide trailing grapheme) extends
	// past the content width, shrink the right chrome by the exact overflow
	// count: drop padding spaces first, then the trailing `─`, but never the
	// corner/vertical bar itself.
	let right_chrome_cells = (ctx.padding_x + 1)
		.saturating_sub(ctx.cursor_overflow)
		.max(1);
	if ctx.is_last_row && ctx.ime_safe_cursor_tail {
		// Terminal frontends render IME marked text locally before committed
		// bytes reach the application. Keep the end-of-input cursor row empty
		// to its right so insertion cannot shift box chrome onto the next row.
		let left_border = (ctx.border_color)(&format!(
			"{}{}",
			ctx.r#box.vertical,
			spaces(ctx.padding_x)
		));
		let bottom_border = (ctx.border_color)(&format!(
			"{}{}{}",
			ctx.r#box.bottom_left,
			ctx.r#box.horizontal.repeat(ctx.width.saturating_sub(2)),
			ctx.r#box.bottom_right
		));
		return vec![format!("{left_border}{}", ctx.text), bottom_border];
	}
	if ctx.is_last_row {
		let bottom_left = (ctx.border_color)(&format!(
			"{}{}{}",
			ctx.r#box.bottom_left,
			ctx.r#box.horizontal,
			spaces(ctx.padding_x.saturating_sub(1))
		));
		let right_pad = right_chrome_cells.saturating_sub(2);
		let include_horizontal = right_chrome_cells >= 2;
		let bottom_right_adjusted = (ctx.border_color)(&format!(
			"{}{}{}",
			spaces(right_pad),
			if include_horizontal {
				ctx.r#box.horizontal.as_str()
			} else {
				""
			},
			ctx.r#box.bottom_right
		));
		return vec![format!(
			"{bottom_left}{}{}{bottom_right_adjusted}",
			ctx.text, ctx.pad
		)];
	}
	let left_border = (ctx.border_color)(&format!(
		"{}{}",
		ctx.r#box.vertical,
		spaces(ctx.padding_x)
	));
	// When the scrollbar is active, replace the right border vertical with a
	// thumb glyph (█) inside the thumb range, keeping the track (│) elsewhere.
	let right_glyph = if ctx.scrollbar_thumb {
		"█"
	} else {
		ctx.r#box.vertical.as_str()
	};
	let right_border = (ctx.border_color)(&format!(
		"{}{}",
		spaces(right_chrome_cells.saturating_sub(1)),
		right_glyph
	));
	vec![format!(
		"{left_border}{}{}{right_border}",
		ctx.text, ctx.pad
	)]
}

fn render_bottom(_ctx: &ComposerChromeContext) -> Option<String> {
	// The bottom border is merged into the last content row.
	None
}

/// The classic maple composer style.
pub static BOX_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "box".to_string(),
	filled_surface: None,
	side_borders: true,
	vertical_chrome: VerticalChrome::Two,
	status_attachment: StatusAttachment::TopBorder,
	bottom_bar: BottomBar::None,
	bottom_bar_gap: false,
	default_prompt_gutter: None,
	default_padding_x: Arc::new(|theme_padding_x| theme_padding_x.unwrap_or(2)),
	side_chrome_width: Arc::new(|padding_x| padding_x + 1),
	render_top: Arc::new(render_top),
	render_row: Arc::new(render_row),
	render_bottom: Arc::new(render_bottom),
});

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{test_chrome, test_row, EditorTopBorder};

	fn mark(text: &str) -> String {
		if text.is_empty() {
			String::new()
		} else {
			format!("<{text}>")
		}
	}

	fn marked(width: usize, padding_x: usize) -> ComposerChromeContext {
		let mut ctx = test_chrome(width, padding_x);
		ctx.border_color = Arc::new(mark);
		ctx
	}

	fn top_border(content: &str, width: usize) -> EditorTopBorder {
		EditorTopBorder {
			content: content.to_string(),
			width,
			revision: None,
		}
	}

	#[test]
	fn top_border_embeds_status_or_truncates_it() {
		let cases = [
			(10, 2, None, "╭────────╮"),
			(10, 2, Some(("hi", 2)), "╭──hi────╮"),
			(10, 2, Some(("hello", 5)), "╭──he…───╮"),
			(4, 0, None, "╭──╮"),
			(4, 0, Some(("ab", 2)), "╭ab╮"),
		];
		for (width, padding_x, top_border, expected) in cases {
			let mut ctx = marked(width, padding_x);
			ctx.top_border = top_border.map(|(content, width)| top_border(content, width));
			assert_eq!(
				(BOX_COMPOSER_STYLE.render_top)(&ctx).as_deref(),
				Some(expected),
				"width {width}"
			);
		}
	}

	#[test]
	fn top_border_paints_each_segment_once() {
		let mut ctx = marked(10, 2);
		ctx.top_border = Some(top_border("hi", 2));
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_top)(&ctx).as_deref(),
			Some("<╭──>hi<──><──╮>")
		);
	}

	#[test]
	fn plain_rows_shrink_right_chrome_by_cursor_overflow() {
		let mut ctx = marked(10, 1);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<│ >hi  < │>".to_string()]
		);

		row.cursor_overflow = 1;
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<│ >hi  <│>".to_string()]
		);

		row.cursor_overflow = 2;
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<│ >hi  <│>".to_string()]
		);
	}

	#[test]
	fn scrollbar_thumb_replaces_only_the_right_track() {
		let ctx = marked(10, 1);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		row.scrollbar_thumb = true;
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<│ >hi  < █>".to_string()]
		);
	}

	#[test]
	fn last_row_merges_the_bottom_border() {
		let mut ctx = marked(10, 1);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		row.is_last_row = true;
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<╰─>hi  <─╯>".to_string()]
		);

		row.cursor_overflow = 2;
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<╰─>hi  <╯>".to_string()]
		);
	}

	#[test]
	fn ime_safe_last_row_emits_two_rows() {
		let ctx = marked(6, 1);
		let mut row = test_row(&ctx, "hi");
		row.is_last_row = true;
		row.ime_safe_cursor_tail = true;
		assert_eq!(
			(BOX_COMPOSER_STYLE.render_row)(&row),
			vec!["<│ >hi".to_string(), "<╰────╯>".to_string()]
		);
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*BOX_COMPOSER_STYLE;
		assert_eq!(style.id, "box");
		assert_eq!(style.filled_surface, None);
		assert!(style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::Two);
		assert_eq!(style.status_attachment, StatusAttachment::TopBorder);
		assert_eq!(style.bottom_bar, BottomBar::None);
		assert!(!style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter, None);
		assert_eq!((style.default_padding_x)(None), 2);
		assert_eq!((style.default_padding_x)(Some(0)), 0);
		assert_eq!((style.default_padding_x)(Some(7)), 7);
		assert_eq!((style.side_chrome_width)(2), 3);
		assert_eq!((style.render_bottom)(&marked(4, 0)), None);
	}
}
