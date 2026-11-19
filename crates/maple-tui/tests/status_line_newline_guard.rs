//! Port of `l/test/status-line-newline-guard.test.ts`.

use maple_tui::render::render_utils::ToolUiStatus;
use maple_tui::render::status_line::{render_status_line, StatusLineOptions};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn ui_theme() -> Theme {
	let json = builtin_theme_json("dark").expect("dark theme");
	create_theme(json, CreateThemeOptions::default()).expect("valid theme")
}

#[test]
fn flattens_newlines_in_description_so_a_tool_cannot_break_the_header() {
	let rendered = render_status_line(
		&StatusLineOptions {
			icon: Some(ToolUiStatus::Success),
			title: "SSH".to_string(),
			description: Some("[router] $ set -e\ncat > /etc/apt/sources.list".to_string()),
			..Default::default()
		},
		&ui_theme(),
	);
	assert!(!rendered.contains('\n'), "{rendered:?}");
	assert!(rendered.contains("[router]"), "{rendered:?}");
	assert!(rendered.contains("set -e"), "{rendered:?}");
	assert!(rendered.contains("cat > /etc/apt/sources.list"), "{rendered:?}");
}

#[test]
fn flattens_newlines_in_meta_entries() {
	let rendered = render_status_line(
		&StatusLineOptions {
			icon: Some(ToolUiStatus::Success),
			title: "X".to_string(),
			meta: Some(vec!["first\nsecond".to_string(), "third".to_string()]),
			..Default::default()
		},
		&ui_theme(),
	);
	assert!(!rendered.contains('\n'), "{rendered:?}");
	assert!(rendered.contains("first second"), "{rendered:?}");
	assert!(rendered.contains("third"), "{rendered:?}");
}

#[test]
fn flattens_crlf_the_same_way_as_lf() {
	let rendered = render_status_line(
		&StatusLineOptions {
			icon: Some(ToolUiStatus::Success),
			title: "X".to_string(),
			description: Some("a\r\nb\rc".to_string()),
			..Default::default()
		},
		&ui_theme(),
	);
	assert!(!rendered.contains('\r'), "{rendered:?}");
	assert!(!rendered.contains('\n'), "{rendered:?}");
	assert!(rendered.contains("a b c"), "{rendered:?}");
}
