//! Upstream-pi composer: full-width horizontal rules above and below plain
//! padded text — no side borders, no prompt gutter. The status bar renders as
//! a plain standalone bottom bar with both segment groups.
//!
//! Reference: `l/src/components/composer/pi.ts`.

use std::sync::{Arc, LazyLock};

use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

fn render_top(ctx: &ComposerChromeContext) -> Option<String> {
	let horizontal = ctx.r#box.horizontal.as_str();
	Some((ctx.border_color)(&horizontal.repeat(ctx.width)))
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	let side = (ctx.side_chrome_width)(ctx.padding_x);
	vec![format!(
		"{}{}{}{}",
		" ".repeat(side),
		ctx.gutter,
		ctx.text,
		ctx.pad
	)]
}

fn render_bottom(ctx: &ComposerChromeContext) -> Option<String> {
	let horizontal = ctx.r#box.horizontal.as_str();
	Some((ctx.border_color)(&horizontal.repeat(ctx.width)))
}

/// Upstream-pi composer style.
pub static PI_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "pi".to_string(),
	filled_surface: None,
	side_borders: false,
	vertical_chrome: VerticalChrome::Two,
	status_attachment: StatusAttachment::None,
	bottom_bar: BottomBar::Full,
	bottom_bar_gap: false,
	default_prompt_gutter: None,
	default_padding_x: Arc::new(|_| 1),
	side_chrome_width: Arc::new(|padding_x| padding_x),
	render_top: Arc::new(render_top),
	render_row: Arc::new(render_row),
	render_bottom: Arc::new(render_bottom),
});

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{test_chrome, test_row};

	#[test]
	fn top_and_bottom_are_full_width_rules() {
		let mut ctx = test_chrome(6, 1);
		ctx.border_color = Arc::new(|text| format!("[{text}]"));
		assert_eq!(
			(PI_COMPOSER_STYLE.render_top)(&ctx).as_deref(),
			Some("[──────]")
		);
		assert_eq!(
			(PI_COMPOSER_STYLE.render_bottom)(&ctx).as_deref(),
			Some("[──────]")
		);
	}

	#[test]
	fn rows_pad_once_per_side_without_chrome() {
		let ctx = test_chrome(10, 1);
		let mut row = test_row(&ctx, "hi");
		row.pad = "  ".to_string();
		assert_eq!(
			(PI_COMPOSER_STYLE.render_row)(&row),
			vec![" hi  ".to_string()]
		);

		let ctx = test_chrome(10, 2);
		let row = test_row(&ctx, "hi");
		assert_eq!(
			(PI_COMPOSER_STYLE.render_row)(&row),
			vec!["  hi".to_string()]
		);
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*PI_COMPOSER_STYLE;
		assert_eq!(style.id, "pi");
		assert_eq!(style.filled_surface, None);
		assert!(!style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::Two);
		assert_eq!(style.status_attachment, StatusAttachment::None);
		assert_eq!(style.bottom_bar, BottomBar::Full);
		assert!(!style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter, None);
		assert_eq!((style.default_padding_x)(None), 1);
		assert_eq!((style.default_padding_x)(Some(8)), 1);
		assert_eq!((style.side_chrome_width)(3), 3);
	}
}
