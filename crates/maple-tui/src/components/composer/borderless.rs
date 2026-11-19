//! Chrome-free composer: a bare `❯ ` prompt with no rules or borders. Also the
//! effective style whenever a host calls `setBorderVisible(false)` (hook
//! editors, agents hub). The status bar renders as a plain standalone bottom
//! bar with both segment groups.
//!
//! Reference: `l/src/components/composer/borderless.ts`.

use std::sync::{Arc, LazyLock};

use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

fn render_top(_ctx: &ComposerChromeContext) -> Option<String> {
	None
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	vec![format!("{}{}{}", ctx.gutter, ctx.text, ctx.pad)]
}

fn render_bottom(_ctx: &ComposerChromeContext) -> Option<String> {
	None
}

/// Chrome-free composer style.
pub static BORDERLESS_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "borderless".to_string(),
	filled_surface: None,
	side_borders: false,
	vertical_chrome: VerticalChrome::Zero,
	status_attachment: StatusAttachment::None,
	bottom_bar: BottomBar::Full,
	bottom_bar_gap: false,
	default_prompt_gutter: Some("❯ ".to_string()),
	default_padding_x: Arc::new(|_| 0),
	side_chrome_width: Arc::new(|_| 0),
	render_top: Arc::new(render_top),
	render_row: Arc::new(render_row),
	render_bottom: Arc::new(render_bottom),
});

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{test_chrome, test_row};

	#[test]
	fn row_is_the_bare_prompt_line() {
		let ctx = test_chrome(10, 0);
		let mut row = test_row(&ctx, "hi");
		row.gutter = "❯ ".to_string();
		row.pad = " ".to_string();
		assert_eq!(
			(BORDERLESS_COMPOSER_STYLE.render_row)(&row),
			vec!["❯ hi ".to_string()]
		);
	}

	#[test]
	fn no_chrome_rows_are_rendered() {
		let ctx = test_chrome(10, 0);
		assert_eq!((BORDERLESS_COMPOSER_STYLE.render_top)(&ctx), None);
		assert_eq!((BORDERLESS_COMPOSER_STYLE.render_bottom)(&ctx), None);
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*BORDERLESS_COMPOSER_STYLE;
		assert_eq!(style.id, "borderless");
		assert_eq!(style.filled_surface, None);
		assert!(!style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::Zero);
		assert_eq!(style.status_attachment, StatusAttachment::None);
		assert_eq!(style.bottom_bar, BottomBar::Full);
		assert!(!style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter.as_deref(), Some("❯ "));
		assert_eq!((style.default_padding_x)(None), 0);
		assert_eq!((style.default_padding_x)(Some(5)), 0);
		assert_eq!((style.side_chrome_width)(5), 0);
	}
}
