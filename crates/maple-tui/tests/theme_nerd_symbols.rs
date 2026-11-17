//! Port of `l/test/theme-nerd-symbols.test.ts`.
//!
//! The reference installs a custom theme file with `symbols.preset: "nerd"` on
//! disk; the crate resolves the same preset from the embedded dark JSON, so the
//! preset is applied to the parsed theme document before creation.

use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn nerd_theme(name: &str) -> Theme {
	let mut json = builtin_theme_json("dark").expect("embedded dark theme");
	json["name"] = serde_json::json!(name);
	json["symbols"]["preset"] = serde_json::json!("nerd");
	create_theme(json, CreateThemeOptions::default()).expect("nerd theme resolves")
}

#[test]
fn uses_the_nerd_fonts_v3_session_and_csharp_icons() {
	let theme = nerd_theme("nerd-symbols");
	assert_eq!(theme.symbol("icon.session"), "\u{f0051}");
	assert_eq!(theme.get_lang_icon(Some("csharp")), "\u{e7b2}");
}

#[test]
fn resolves_subscription_and_advisor_icons_in_nerd_mode() {
	let theme = nerd_theme("nerd-annotation-symbols");
	assert_eq!(theme.symbol("icon.subscription"), "\u{f067a}");
	assert_eq!(theme.symbol("icon.advisor"), "\u{ea70}");
	assert_eq!(theme.symbol("icon.advisorClosed"), "\u{eae7}");
	let icons = theme.icon();
	assert_eq!(icons.subscription, "\u{f067a}");
	assert_eq!(icons.advisor, "\u{ea70}");
	assert_eq!(icons.advisor_closed, "\u{eae7}");
}
