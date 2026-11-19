//! Port of `l/test/annotation-editor-boundary.test.ts`.
//!
//! The reference types into the overlay's software-cursor editor and checks
//! wrapping at the input capacity (`overlay width - 4 frame cells - 2 gutter`
//! cells). `editor_rows` ports that wrapping and the software caret.

use maple_tui::overlays::annotation_overlay::{editor_content_width, editor_rows};
use maple_tui::strip_ansi;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};

const WIDTH: usize = 40;
/// Four frame cells and the two-cell prompt gutter.
const INPUT_CAPACITY: usize = WIDTH - 6;

fn theme() -> Theme {
	create_theme(&builtin_theme_json("dark").expect("dark theme json"), CreateThemeOptions::default())
		.expect("dark theme")
}

fn input_rows(text: &str, theme: &Theme) -> Vec<String> {
	editor_rows(text, WIDTH, theme)
}

#[test]
fn preserves_full_cell_input_and_shows_caret_after_wrapping() {
	let theme = theme();
	assert_eq!(editor_content_width(WIDTH), INPUT_CAPACITY);
	let cursor = theme.nav().cursor;

	let exact = "Q".repeat(INPUT_CAPACITY);
	let rows = input_rows(&exact, &theme);
	assert_eq!(rows.len(), 1);
	assert_eq!(strip_ansi(&rows[0]).matches('Q').count(), INPUT_CAPACITY);

	let rows = input_rows(&format!("{exact}Q"), &theme);
	assert_eq!(rows.len(), 2);
	assert_eq!(strip_ansi(&rows[1]).matches('Q').count(), 1);
	assert!(strip_ansi(&rows[1]).contains(&format!("Q{cursor}")));

	let rows = input_rows(&format!("{exact}QQ"), &theme);
	assert_eq!(strip_ansi(&rows[1]).matches('Q').count(), 2);
}

#[test]
fn keeps_wide_trailing_glyph_visible_when_cursor_borrows_cells() {
	let theme = theme();
	let exact = format!("{}{}", "Q".repeat(INPUT_CAPACITY - 2), "界");
	let rows = input_rows(&exact, &theme);
	assert_eq!(rows.len(), 1);
	assert!(strip_ansi(&rows[0]).contains(&exact));
}
