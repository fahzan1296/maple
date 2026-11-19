//! Port of `l/test/hotkeys-markdown.test.ts`.
//!
//! The reference injects display strings through a stub manager and a global
//! `setKeyHintPlatform`. The crate's `build_hotkeys_markdown` takes a real
//! `KeybindingsManager` plus a `KeyHintFormatter`, so the display strings are
//! reproduced by user bindings and the platform travels on the formatter.

use maple_tui::hotkeys_markdown::build_hotkeys_markdown;
use maple_tui::key_hint_format::{KeyHintFormatter, Platform};
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};

fn theme() -> maple_tui::theme::theme_class::Theme {
	let json = builtin_theme_json("dark").expect("dark theme json");
	create_theme(json, CreateThemeOptions::default()).expect("dark theme builds")
}

fn linux_formatter(theme: &maple_tui::theme::theme_class::Theme) -> KeyHintFormatter<'_> {
	KeyHintFormatter::with_platform(theme, Platform::Other)
}

fn darwin_formatter(theme: &maple_tui::theme::theme_class::Theme) -> KeyHintFormatter<'_> {
	KeyHintFormatter::with_platform(theme, Platform::Darwin)
}

fn bind(manager: &mut KeybindingsManager, id: &str, keys: &[&str]) {
	manager.set_user_binding(id, Some(keys.iter().map(|key| key.to_string()).collect()));
}

#[test]
fn emits_flush_left_markdown_and_uses_the_configured_temporary_selector_hint() {
	let theme = theme();
	let formatter = linux_formatter(&theme);
	let mut manager = KeybindingsManager::new();
	bind(&mut manager, "app.clipboard.copyPrompt", &["ctrl+shift+p"]);
	bind(&mut manager, "app.model.selectTemporary", &["ctrl+shift+l"]);
	bind(&mut manager, "app.model.select", &["alt+m"]);
	bind(&mut manager, "app.display.reset", &["alt+l"]);
	bind(&mut manager, "app.live.toggle", &["ctrl+l"]);
	bind(&mut manager, "app.retry", &["alt+r"]);
	bind(&mut manager, "app.plan.toggle", &["alt+shift+p"]);
	bind(&mut manager, "app.tools.toggleVisibility", &["ctrl+shift+o"]);

	let markdown = build_hotkeys_markdown(&manager, &formatter);

	let lines: Vec<&str> = markdown.split('\n').collect();
	assert_eq!(lines[0], "**Navigation**");
	for expected in [
		"| `Ctrl+Shift+P` | Copy whole prompt |",
		"| `Ctrl+Shift+L` | Select model (temporary) |",
		"| `Alt+M` | Select model (set roles) |",
		"| `Alt+L` | Reset terminal display |",
		"| `Ctrl+L` | Start/stop live voice mode (/live) |",
		"| `Alt+R` | Retry last failed assistant turn |",
		"| `Alt+Shift+P` | Toggle plan mode |",
		"| `Ctrl+Shift+O` | Toggle tool activity visibility |",
	] {
		assert!(markdown.contains(expected), "missing row: {expected}");
	}
	assert!(markdown.contains("| `#<number>` | GitHub issue/PR reference"));
	assert!(markdown.contains("| `#` / `#<text>` | Prompt actions"));
	for line in lines {
		if line.is_empty() {
			continue;
		}
		assert!(!line.starts_with(' '), "indented line: {line}");
		assert!(!line.starts_with('\t'), "tab-indented line: {line}");
	}
}

#[test]
fn renders_the_temporary_selector_row_as_disabled_when_no_display_string_is_configured() {
	let theme = theme();
	let formatter = linux_formatter(&theme);
	let mut manager = KeybindingsManager::new();
	manager.set_user_binding("app.model.selectTemporary", None);
	bind(&mut manager, "app.model.select", &["alt+m"]);

	let markdown = build_hotkeys_markdown(&manager, &formatter);

	assert!(markdown.contains("| `Disabled` | Select model (temporary) |"));
	assert!(markdown.contains("| `Alt+M` | Select model (set roles) |"));
}

#[test]
fn drops_option_cmd_static_navigation_labels_off_darwin() {
	let theme = theme();
	let linux = build_hotkeys_markdown(&KeybindingsManager::new(), &linux_formatter(&theme));
	let darwin = build_hotkeys_markdown(&KeybindingsManager::new(), &darwin_formatter(&theme));

	assert!(!linux.contains("Option+"), "linux sheet carries Option+");
	assert!(!linux.contains("Cmd+"), "linux sheet carries Cmd+");
	assert!(linux.contains("| `Ctrl+A/Home` | Start of line |"));
	assert!(
		darwin.contains("| `Ctrl+A/Home/"),
		"darwin start-of-line row drops the super key: {}",
		darwin
			.lines()
			.find(|line| line.contains("Start of line"))
			.unwrap_or("<missing>")
	);
}

#[test]
fn renders_the_effective_push_to_talk_remap_without_borrowing_the_separate_toggle_binding() {
	let theme = theme();
	let formatter = linux_formatter(&theme);
	let mut manager = KeybindingsManager::new();
	bind(&mut manager, "app.stt.pushToTalk", &["x", "ctrl+x"]);

	let markdown = build_hotkeys_markdown(&manager, &formatter);

	let row = markdown
		.lines()
		.find(|line| line.contains("Speech-to-text (push-to-talk)"))
		.expect("push-to-talk row");
	assert!(row.contains("X/Ctrl+X"), "row: {row}");
	assert!(row.contains("Hold "), "row: {row}");
	assert!(!row.contains("Alt+H"), "row borrows the toggle binding: {row}");
	assert!(!markdown.contains("Alt+H"), "sheet borrows the toggle binding");
}

#[test]
fn renders_push_to_talk_as_disabled_when_its_effective_key_list_is_empty() {
	let theme = theme();
	let formatter = linux_formatter(&theme);
	let mut manager = KeybindingsManager::new();
	manager.set_user_binding("app.stt.pushToTalk", None);

	let markdown = build_hotkeys_markdown(&manager, &formatter);

	let row = markdown
		.lines()
		.find(|line| line.contains("Speech-to-text (push-to-talk)"))
		.expect("push-to-talk row");
	assert!(row.contains("Disabled"), "row: {row}");
	assert!(!row.contains("Hold"), "row: {row}");
}
