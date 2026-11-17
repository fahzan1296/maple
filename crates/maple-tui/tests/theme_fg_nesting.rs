//! Port of `l/test/theme-fg-nesting.test.ts`.

use maple_tui::strip_ansi;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn dark() -> Theme {
	create_theme(
		builtin_theme_json("dark").expect("embedded dark theme"),
		CreateThemeOptions::default(),
	)
	.expect("dark theme resolves")
}

#[test]
fn keeps_its_color_on_the_text_after_a_nested_color_ends() {
	let theme = dark();
	let dim = theme.get_fg_ansi("dim").expect("dim color");
	let footer = theme
		.fg("dim", &theme.fg("success", "+2").expect("success color"))
		.expect("outer fg");
	let tail = &footer[footer.find("+2").expect("nested text") + "+2".len()..];
	assert!(tail.starts_with(&format!("\x1b[39m{dim}")), "{tail:?}");
	assert_eq!(strip_ansi(&footer), "+2 · 3 more lines");
}

#[test]
fn restores_its_color_after_a_full_reset_inside_the_text() {
	let theme = dark();
	let accent = theme.get_fg_ansi("accent").expect("accent color");
	let styled = theme.fg("accent", "a\x1b[0mb").expect("accent fg");
	assert_eq!(styled, format!("{accent}a\x1b[0m{accent}b\x1b[39m"));
}
