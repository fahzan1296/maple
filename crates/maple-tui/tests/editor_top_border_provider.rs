//! Port of `l/test/editor-top-border-provider.test.ts`.
//!
//! The reference defends the `Editor` lazy top-border provider: provider
//! precedence over eager content, one provider call per render, current state
//! at render time, fallback when cleared, and the visually-available width.
//! The crate's `Editor` has no top border; its top chrome lives in the composer
//! styles (`l/src/components/composer/box.ts` → `BOX_COMPOSER_STYLE`). The
//! contract is therefore asserted against the composer's top-border row: the
//! injected `EditorTopBorder` is embedded within `width - side chrome`, an
//! oversized border is truncated to that budget, and no border falls back to a
//! plain frame.

use std::sync::Arc;

use maple_tui::components::composer::r#box::BOX_COMPOSER_STYLE;
use maple_tui::components::composer::types::{ComposerChromeContext, EditorTopBorder};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};
use maple_tui::theme::theme_class::Theme;
use maple_tui::theme::tui_adapters::get_symbol_theme;

fn theme() -> Theme {
	let json = builtin_theme_json("dark").expect("dark theme json");
	create_theme(json, CreateThemeOptions::default()).expect("dark theme builds")
}

fn chrome(theme: &Theme, width: usize, padding_x: usize) -> ComposerChromeContext {
	ComposerChromeContext {
		width,
		padding_x,
		border_color: Arc::new(|text| text.to_string()),
		accent_color: Arc::new(|text| text.to_string()),
		surface_color: Arc::new(|text| text.to_string()),
		r#box: get_symbol_theme(theme).box_round.clone(),
		top_border: None,
	}
}

fn top_border(content: &str) -> EditorTopBorder {
	EditorTopBorder {
		content: content.to_string(),
		width: visible_width(content),
		revision: None,
	}
}

#[test]
fn top_border_content_is_embedded_within_the_visually_available_width() {
	let theme = theme();
	for width in [80usize, 120usize] {
		let mut ctx = chrome(&theme, width, 1);
		ctx.top_border = Some(top_border("status"));
		let rendered = (BOX_COMPOSER_STYLE.render_top)(&ctx).expect("top border row");
		assert!(rendered.contains("status"), "width {width}: {rendered}");
		assert_eq!(visible_width(&rendered), width, "width {width}");
		let box_glyphs = get_symbol_theme(&theme).box_round.clone();
		assert!(rendered.starts_with(&box_glyphs.top_left));
		assert!(rendered.ends_with(&box_glyphs.top_right));
	}
}

#[test]
fn oversized_top_border_is_truncated_to_the_available_width() {
	let theme = theme();
	let width = 80usize;
	let mut ctx = chrome(&theme, width, 1);
	ctx.top_border = Some(top_border(&"x".repeat(width * 2)));
	let rendered = (BOX_COMPOSER_STYLE.render_top)(&ctx).expect("top border row");
	assert_eq!(visible_width(&rendered), width, "{rendered}");
	assert!(rendered.contains('…'), "no ellipsis: {rendered}");
}

#[test]
fn missing_top_border_falls_back_to_a_plain_frame() {
	let theme = theme();
	let width = 80usize;
	let ctx = chrome(&theme, width, 1);
	let rendered = (BOX_COMPOSER_STYLE.render_top)(&ctx).expect("top border row");
	let box_glyphs = get_symbol_theme(&theme).box_round.clone();
	let side = (BOX_COMPOSER_STYLE.side_chrome_width)(1);
	let fill = width - side * 2;
	assert_eq!(
		rendered,
		format!(
			"{}{}{}",
			box_glyphs.top_left,
			box_glyphs.horizontal.repeat(fill),
			box_glyphs.top_right
		)
	);
	assert!(!rendered.contains("status"));
}

#[test]
fn each_render_embeds_the_border_supplied_for_that_render() {
	let theme = theme();
	let mut first = chrome(&theme, 80, 1);
	first.top_border = Some(top_border("first"));
	let mut second = chrome(&theme, 120, 1);
	second.top_border = Some(top_border("second"));

	let first_row = (BOX_COMPOSER_STYLE.render_top)(&first).expect("first row");
	let second_row = (BOX_COMPOSER_STYLE.render_top)(&second).expect("second row");

	assert!(first_row.contains("first"));
	assert!(!first_row.contains("second"));
	assert!(second_row.contains("second"));
	assert!(!second_row.contains("first"));

	// The row is a pure function of its context: no state leaks between renders.
	assert_eq!(first_row, (BOX_COMPOSER_STYLE.render_top)(&first).unwrap());
}
