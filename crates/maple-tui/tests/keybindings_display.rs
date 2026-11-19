//! Port of `l/test/keybindings-display.test.ts`.
//!
//! The reference's `getDisplayString` lives on the TS `KeybindingsManager`; the
//! crate splits it into `get_keys` plus `KeyHintFormatter::format_key_hints`, so
//! display strings are rebuilt from those two primitives.

use maple_tui::key_hint_format::{KeyHintFormatter, Platform};
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn theme_with_preset(preset: &str) -> Theme {
	let mut json = builtin_theme_json("dark").expect("embedded dark theme");
	json["symbols"]["preset"] = serde_json::json!(preset);
	create_theme(json, CreateThemeOptions::default()).expect("theme resolves")
}

fn linux_formatter(theme: &Theme) -> KeyHintFormatter<'_> {
	KeyHintFormatter::with_platform(theme, Platform::Other)
}

#[test]
fn defaults_retry_to_f5_with_the_alt_r_fallback() {
	let manager = KeybindingsManager::new();
	assert_eq!(manager.get_keys("app.retry"), vec!["f5".to_string(), "alt+r".to_string()]);
	let formatter = linux_formatter(&theme_with_preset("unicode"));
	assert_eq!(
		formatter.format_key_hints(&["f5", "alt+r"]),
		"F5/Alt+R"
	);
}

#[test]
fn returns_an_empty_string_when_the_action_has_no_binding() {
	let mut manager = KeybindingsManager::new();
	manager.set_user_binding("app.clipboard.copyPrompt", Some(Vec::new()));
	assert!(manager.get_keys("app.clipboard.copyPrompt").is_empty());
	let formatter = linux_formatter(&theme_with_preset("unicode"));
	assert_eq!(formatter.format_key_hints(&[]), "");
}

#[test]
fn keeps_alt_and_super_labels_as_words_off_macos() {
	let mut manager = KeybindingsManager::new();
	manager.set_user_binding("app.display.reset", Some(vec!["alt+l".to_string()]));
	manager.set_user_binding(
		"app.clipboard.pasteImage",
		Some(vec!["ctrl+v".to_string(), "super+v".to_string()]),
	);
	let formatter = linux_formatter(&theme_with_preset("unicode"));
	let display_reset = manager.get_keys("app.display.reset");
	let display_reset: Vec<&str> = display_reset.iter().map(String::as_str).collect();
	assert_eq!(formatter.format_key_hints(&display_reset), "Alt+L");
	let paste = manager.get_keys("app.clipboard.pasteImage");
	let paste: Vec<&str> = paste.iter().map(String::as_str).collect();
	assert_eq!(formatter.format_key_hints(&paste), "Ctrl+V/Super+V");
}

#[test]
fn abuts_macos_glyph_modifiers_and_keeps_words_elsewhere() {
	let theme = theme_with_preset("unicode");
	let mac = KeyHintFormatter::with_platform(&theme, Platform::Darwin);
	assert_eq!(mac.format_key_hint("ctrl+shift+c"), "⌃⇧C");
	assert_eq!(mac.format_key_hint("alt+up"), "⌥↑");
	assert_eq!(mac.format_key_hint("super+v"), "⌘V");

	let linux = linux_formatter(&theme);
	assert_eq!(linux.format_key_hint("ctrl+shift+c"), "Ctrl+⇧C");
	assert_eq!(linux.format_key_hint("alt+up"), "Alt+↑");
	assert_eq!(linux.format_key_hint("super+v"), "Super+V");
}

#[test]
fn orders_modifiers_canonically_regardless_of_binding_order() {
	let mac = KeyHintFormatter::with_platform(&theme_with_preset("unicode"), Platform::Darwin);
	assert_eq!(mac.format_key_hint("shift+ctrl+p"), "⌃⇧P");
	assert_eq!(mac.format_key_hint("super+shift+alt+k"), "⌥⇧⌘K");
}

#[test]
fn keeps_a_bare_letter_lowercase_but_capitalizes_it_inside_a_chord() {
	let formatter = linux_formatter(&theme_with_preset("unicode"));
	assert_eq!(formatter.format_key_hint("q"), "q");
	assert_eq!(formatter.format_key_hint("shift+g"), "⇧G");
}

#[test]
fn renders_a_bare_modifier_and_the_plus_key_itself() {
	let formatter = linux_formatter(&theme_with_preset("unicode"));
	assert_eq!(formatter.format_key_hint("shift"), "⇧");
	assert_eq!(formatter.format_key_hint("ctrl++"), "Ctrl++");
	assert_eq!(formatter.format_key_hint("+"), "+");
}

#[test]
fn spaces_a_double_tap_only_when_the_key_renders_as_a_word() {
	let unicode = linux_formatter(&theme_with_preset("unicode"));
	assert_eq!(unicode.format_double_tap("left"), "←←");
	let ascii = linux_formatter(&theme_with_preset("ascii"));
	assert_eq!(ascii.format_double_tap("left"), "Left Left");
}

#[test]
fn separates_nerd_icons_so_adjacent_keycaps_stay_legible() {
	let nerd = KeyHintFormatter::with_platform(&theme_with_preset("nerd"), Platform::Darwin);
	assert_eq!(nerd.format_key_hint("shift+tab"), "\u{f0636} \u{f0312}");
	assert_eq!(nerd.format_key_hint("ctrl+shift+c"), "\u{f0634} \u{f0636} C");
}

#[test]
fn keeps_tooltip_keys_abutting_under_the_nerd_preset() {
	let nerd = KeyHintFormatter::with_platform(&theme_with_preset("nerd"), Platform::Darwin);
	assert_eq!(nerd.format_tooltip_key("shift+tab"), "⇧⇥");
	assert_eq!(nerd.format_tooltip_key("ctrl+shift+c"), "⌃⇧C");
	assert_eq!(nerd.format_tooltip_key("escape"), "esc");
}

#[test]
#[ignore = "missing facility: getDefaultPasteImageKeys is an app-keybindings helper that is not ported"]
fn keeps_ctrl_v_registered_for_image_paste_on_windows_alongside_the_fallback() {
	let theme = theme_with_preset("unicode");
	let formatter = linux_formatter(&theme);
	assert_eq!(formatter.format_key_hints(&["ctrl+v", "alt+v"]), "Ctrl+V/Alt+V");
}
