//! Port of `l/test/dynamic-border.test.ts`.
//!
//! The crate's `DynamicBorder` takes an explicit `&Theme` instead of reading a
//! module-level global, so the "uninitialized theme" regression has no Rust
//! equivalent and stays ignored.

use maple_tui::chrome::dynamic_border::{BorderLabel, DynamicBorder};
use maple_tui::component::Component;
use maple_tui::theme::default_theme;
use std::cell::RefCell;

#[test]
fn paints_a_custom_styler_over_the_rule() {
	let theme = default_theme();
	let mut border = DynamicBorder::with_color(&theme, Box::new(|text| format!("<{text}>")));
	assert_eq!(border.render(4), ["<────>"]);
}

#[test]
fn paints_plain_glyphs_without_a_styler() {
	let theme = default_theme();
	let horizontal = theme.symbol("boxRound.horizontal");
	let mut border = DynamicBorder::with_color(&theme, Box::new(|text| text.to_string()));
	assert_eq!(border.render(3), [horizontal.repeat(3)]);
}

#[test]
fn sets_a_label_and_rereads_a_dynamic_label_every_render() {
	let theme = default_theme();
	let horizontal = theme.symbol("boxRound.horizontal");
	let tick = RefCell::new("a".to_string());
	let mut border = DynamicBorder::with_color(&theme, Box::new(|text| text.to_string()));
	border.set_label(Some(BorderLabel::dynamic(|| tick.borrow().clone())));
	assert_eq!(border.render(10), [format!("{} a {}", horizontal.repeat(3), horizontal.repeat(4))]);
	*tick.borrow_mut() = "b".to_string();
	assert_eq!(border.render(10), [format!("{} b {}", horizontal.repeat(3), horizontal.repeat(4))]);
	// Too narrow for the lead, padded label and one trailing cell: a plain rule.
	assert_eq!(border.render(6), [horizontal.repeat(6)]);
	border.set_label(None);
	assert_eq!(border.render(10), [horizontal.repeat(10)]);
}

#[test]
#[ignore = "missing API: the crate holds an explicit &Theme; no uninitialized-theme state exists"]
fn renders_plain_glyphs_when_the_module_theme_is_uninitialized() {}
