//! Soft status band composer: the full status line sits flush against the
//! terminal's left edge as a filled powerline band with a soft opening cap —
//! no frame, rules, or corners — above an unboxed prompt anchored by a single
//! curved `╰─ ` cue.
//!
//! Reference: `l/src/components/composer/band.ts`.

use std::sync::{Arc, LazyLock};

use crate::text_shape::{truncate_to_width, Ellipsis};

use super::types::{
	BottomBar, ComposerChromeContext, ComposerRowContext, ComposerStyle, StatusAttachment,
	VerticalChrome,
};

fn render_top(ctx: &ComposerChromeContext) -> Option<String> {
	let top_border = ctx.top_border.as_ref()?;
	if top_border.content.is_empty() {
		return Some(String::new());
	}
	// The band builder already sizes its groups + gauge to the full width;
	// truncation only guards against a stale provider during resize.
	if top_border.width > ctx.width {
		Some(if ctx.width == 0 {
			String::new()
		} else {
			truncate_to_width(&top_border.content, ctx.width, Ellipsis::Unicode)
		})
	} else {
		Some(top_border.content.clone())
	}
}

fn render_row(ctx: &ComposerRowContext) -> Vec<String> {
	let gutter = if ctx.gutter.is_empty() {
		String::new()
	} else {
		(ctx.border_color)(&ctx.gutter)
	};
	vec![format!("{}{}{}", gutter, ctx.text, ctx.pad)]
}

fn render_bottom(_ctx: &ComposerChromeContext) -> Option<String> {
	None
}

/// Soft status band composer.
pub static BAND_COMPOSER_STYLE: LazyLock<ComposerStyle> = LazyLock::new(|| ComposerStyle {
	id: "band".to_string(),
	filled_surface: None,
	side_borders: false,
	vertical_chrome: VerticalChrome::One,
	status_attachment: StatusAttachment::TopBand,
	bottom_bar: BottomBar::None,
	bottom_bar_gap: false,
	default_prompt_gutter: Some("╰─ ".to_string()),
	default_padding_x: Arc::new(|_| 0),
	side_chrome_width: Arc::new(|_| 0),
	render_top: Arc::new(render_top),
	render_row: Arc::new(render_row),
	render_bottom: Arc::new(render_bottom),
});

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{test_chrome, test_row, EditorTopBorder};

	fn top_border(content: &str, width: usize) -> EditorTopBorder {
		EditorTopBorder {
			content: content.to_string(),
			width,
			revision: None,
		}
	}

	#[test]
	fn top_row_reserves_its_chrome_row_until_content_exists() {
		let cases = [
			(None, 8, ""),
			(Some(("", 0)), 8, ""),
			(Some(("abc", 3)), 8, "abc"),
			(Some(("abcdef", 6)), 4, "abc…"),
			(Some(("abc", 10)), 8, "abc"),
		];
		for (top_border, width, expected) in cases {
			let mut ctx = test_chrome(width, 0);
			ctx.top_border = top_border.map(|(content, width)| top_border(content, width));
			assert_eq!(
				(BAND_COMPOSER_STYLE.render_top)(&ctx),
				Some(expected.to_string()),
				"width {width}"
			);
		}
	}

	#[test]
	fn row_paints_only_a_non_empty_gutter() {
		let mut ctx = test_chrome(10, 0);
		ctx.border_color = Arc::new(|text| format!("[{text}]"));
		let mut row = test_row(&ctx, "hi");
		row.pad = " ".to_string();

		assert_eq!(
			(BAND_COMPOSER_STYLE.render_row)(&row),
			vec!["hi ".to_string()]
		);

		row.gutter = "╰─ ".to_string();
		assert_eq!(
			(BAND_COMPOSER_STYLE.render_row)(&row),
			vec!["[╰─ ]hi ".to_string()]
		);
	}

	#[test]
	fn style_metadata_matches_the_reference() {
		let style = &*BAND_COMPOSER_STYLE;
		assert_eq!(style.id, "band");
		assert_eq!(style.filled_surface, None);
		assert!(!style.side_borders);
		assert_eq!(style.vertical_chrome, VerticalChrome::One);
		assert_eq!(style.status_attachment, StatusAttachment::TopBand);
		assert_eq!(style.bottom_bar, BottomBar::None);
		assert!(!style.bottom_bar_gap);
		assert_eq!(style.default_prompt_gutter.as_deref(), Some("╰─ "));
		assert_eq!((style.default_padding_x)(None), 0);
		assert_eq!((style.side_chrome_width)(9), 0);
	}

	#[test]
	fn bottom_chrome_is_never_rendered() {
		let ctx = test_chrome(6, 0);
		assert_eq!((BAND_COMPOSER_STYLE.render_bottom)(&ctx), None);
	}
}
