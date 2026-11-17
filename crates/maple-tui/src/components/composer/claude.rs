//! Claude Code-like composer: full-width horizontal rules above and below a
//! borderless `❯ ` prompt. The right status group rides the top rule as a
//! bg chip near the right edge (`─────── hi ─`); the left group renders as a
//! plain standalone bottom bar that yields its row to the autocomplete menu.
//!
//! Reference: `l/src/components/composer/claude.ts`.

use std::sync::{Arc, LazyLock};

use super::rule::render_top_rule;
use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

fn render_top(ctx: &ComposerChromeContext) -> Option<String> {
	Some(render_top_rule(ctx))
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	vec![format!("{}{}{}", ctx.gutter, ctx.text, ctx.pad)]
}

fn render_bottom(ctx: &ComposerChromeContext) -> Option<String> {
	let horizontal = ctx.r#box.horizontal.as_str();
	Some((ctx.border_color)(&horizontal.repeat(ctx.width)))
}

/// Claude Code-like composer style.
pub static CLAUDE_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "claude".to_string(),
	filled_surface: None,
	side_borders: false,
	vertical_chrome: VerticalChrome::Two,
	status_attachment: StatusAttachment::TopRuleChip,
	bottom_bar: BottomBar::Left,
	bottom_bar_gap: false,
	default_prompt_gutter: Some("❯ ".to_string()),
	default_padding_x: Arc::new(|_| 0),
	side_chrome_width: Arc::new(|padding_x| padding_x),
	render_top: Arc::new(render_top),
	render_row: Arc::new(render_row),
	render_bottom: Arc::new(render_bottom),
});

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{test_chrome, test_row, EditorTopBorder};

	#[test]
	fn top_rule_docks_the_status_chip() {
		let mut ctx = test_chrome(12, 0);
		ctx.top_border = Some(EditorTopBorder {
			content: "hi".to_string(),
			width: 2,
			revision: None,
		});
		assert_eq!(
			(CLAUDE_COMPOSER_STYLE.render_top)(&ctx).as_deref(),
			Some("─────────hi─")
		);
	}

	#[test]
	fn row_is_the_plain_prompt_line() {
		let ctx = test_chrome(10, 0);
		let mut row = test_row(&ctx, "hi");
		row.gutter = "❯ ".to_string();
		row.pad = " ".to_string();
		assert_eq!(
			(CLAUDE_COMPOSER_STYLE.render_row)(&row),
			vec!["❯ hi ".to_string()]
		);
	}

	#[test]
	fn bottom_is_a_full_width_rule() {
		let mut ctx = test_chrome(5, 0);
		ctx.border_color = Arc::new(|text| format!("[{text}]"));
		assert_eq!(
			(CLAUDE_COMPOSER_STYLE.render_bottom)(&ctx).as_deref(),
			Some("[─────]")
		);
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*CLAUDE_COMPOSER_STYLE;
		assert_eq!(style.id, "claude");
		assert_eq!(style.filled_surface, None);
		assert!(!style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::Two);
		assert_eq!(style.status_attachment, StatusAttachment::TopRuleChip);
		assert_eq!(style.bottom_bar, BottomBar::Left);
		assert!(!style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter.as_deref(), Some("❯ "));
		assert_eq!((style.default_padding_x)(None), 0);
		assert_eq!((style.default_padding_x)(Some(4)), 0);
		assert_eq!((style.side_chrome_width)(4), 4);
	}
}
