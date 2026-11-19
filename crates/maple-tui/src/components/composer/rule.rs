//! Single-rule composer: a borderless prompt docked below one top rule. The
//! right status group rides the rule while the left group remains below the
//! editor, preserving the compact status split without a closing rule.
//!
//! Reference: `l/src/components/composer/rule.ts`.

use std::sync::{Arc, LazyLock};

use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

/// Draw a full-width rule with status content docked at its right edge.
/// Over-wide content is truncated (keeping one rule cell on each side) rather
/// than dropped, so the chip survives narrow terminals and previews.
pub fn render_top_rule(ctx: &ComposerChromeContext) -> String {
	if let Some(top_border) = ctx.top_border.as_ref() {
		if top_border.width > 0 && ctx.width > 2 {
			let mut content = top_border.content.clone();
			let mut chip_width = top_border.width;
			if chip_width > ctx.width - 2 {
				content = truncate_to_width(&content, ctx.width - 2, Ellipsis::Unicode);
				chip_width = visible_width(&content);
			}
			let left_fill = ctx.width.saturating_sub(chip_width + 1);
			let horizontal = ctx.r#box.horizontal.as_str();
			let left = (ctx.border_color)(&horizontal.repeat(left_fill));
			let tail = (ctx.border_color)(horizontal);
			return format!("{left}{content}{tail}");
		}
	}
	let horizontal = ctx.r#box.horizontal.as_str();
	(ctx.border_color)(&horizontal.repeat(ctx.width))
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	vec![format!("{}{}{}", ctx.gutter, ctx.text, ctx.pad)]
}

fn render_bottom(_ctx: &ComposerChromeContext) -> Option<String> {
	None
}

/// Composer style with one status-bearing top rule and no bottom chrome.
pub static RULE_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "rule".to_string(),
	filled_surface: None,
	side_borders: false,
	vertical_chrome: VerticalChrome::One,
	status_attachment: StatusAttachment::TopRuleChip,
	bottom_bar: BottomBar::Left,
	bottom_bar_gap: true,
	default_prompt_gutter: Some("❯ ".to_string()),
	default_padding_x: Arc::new(|_| 0),
	side_chrome_width: Arc::new(|padding_x| padding_x),
	render_top: Arc::new(|ctx| Some(render_top_rule(ctx))),
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

	fn marked(width: usize) -> ComposerChromeContext {
		let mut ctx = test_chrome(width, 0);
		ctx.border_color = Arc::new(mark);
		ctx
	}

	fn top_border(content: &str, width: usize) -> Option<EditorTopBorder> {
		Some(EditorTopBorder {
			content: content.to_string(),
			width,
			revision: None,
		})
	}

	#[test]
	fn docks_chip_on_the_rule_or_falls_back_to_a_plain_rule() {
		let cases = [
			(10, None, "──────────"),
			(12, Some(("hi", 2)), "─────────hi─"),
			(3, Some(("hi", 2)), "hi─"),
			(2, Some(("hi", 2)), "──"),
			(10, Some(("x", 0)), "──────────"),
		];
		for (width, top_border, expected) in cases {
			let mut ctx = marked(width);
			ctx.top_border = top_border.map(|(content, width)| top_border(content, width));
			assert_eq!(render_top_rule(&ctx), expected, "width {width}");
		}
	}

	#[test]
	fn truncates_an_overwide_chip_and_remeasures_it() {
		let mut ctx = marked(10);
		ctx.top_border = top_border("abcdefghijkl", 12);
		assert_eq!(render_top_rule(&ctx), "─abcdefg…─");

		let mut ctx = marked(10);
		ctx.top_border = top_border("abcdef", 10);
		assert_eq!(render_top_rule(&ctx), "───abcdef─");
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*RULE_COMPOSER_STYLE;
		assert_eq!(style.id, "rule");
		assert_eq!(style.filled_surface, None);
		assert!(!style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::One);
		assert_eq!(style.status_attachment, StatusAttachment::TopRuleChip);
		assert_eq!(style.bottom_bar, BottomBar::Left);
		assert!(style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter.as_deref(), Some("❯ "));
		assert_eq!((style.default_padding_x)(None), 0);
		assert_eq!((style.default_padding_x)(Some(9)), 0);
		assert_eq!((style.side_chrome_width)(4), 4);
	}

	#[test]
	fn style_render_members_match_the_reference() {
		let ctx = marked(8);
		let mut row_ctx = test_row(&ctx, "hi");
		row_ctx.gutter = "❯ ".to_string();
		assert_eq!(
			(RULE_COMPOSER_STYLE.render_top)(&ctx),
			Some(render_top_rule(&ctx))
		);
		assert_eq!(
			(RULE_COMPOSER_STYLE.render_row)(&row_ctx),
			vec!["❯ hi".to_string()]
		);
		assert_eq!((RULE_COMPOSER_STYLE.render_bottom)(&ctx), None);
	}
}
