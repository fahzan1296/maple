//! Reference: `l/test/native-content.test.ts`.
//!
//! The reference drives `Markdown`, `Box`, `Table`, `Text` and `Disclosure`
//! descriptions. The crate describes `Box` natively but has no
//! `NativeDescribe` for `Markdown`, `Table` or `Disclosure`, and `Text`
//! describes as plain text without ANSI tokenization, so those cases are
//! ignored with the absent facility named.

use std::sync::Arc;

use maple_tui::components::r#box::{Box, BoxBorder, BoxBorderChars, BoxStyler};
use maple_tui::components::Text;
use maple_tui::native::node::{DescribeContext, NodeKind};
use maple_tui::theme::{builtin, create_theme, ColorMode, CreateThemeOptions, Theme};

fn theme() -> Theme {
	create_theme(
		builtin::builtin_theme_json("dark").expect("built-in dark theme"),
		CreateThemeOptions { mode: Some(ColorMode::TrueColor), ..Default::default() },
	)
	.expect("valid theme")
}

fn bg_styler(theme: &Theme, token: &str) -> BoxStyler {
	let ansi = theme.get_bg_ansi(token).unwrap_or_default();
	Arc::new(move |text| format!("{ansi}{text}"))
}

fn cx() -> DescribeContext {
	DescribeContext::new(80)
}

#[test]
fn box_background_fills_map_to_card_tone_and_role() {
	let theme = theme();
	let mut error_box = Box::styled(1, 1, Some(bg_styler(&theme, "toolErrorBg")), None);
	error_box.add_child(Text::new("boom"));
	let described = error_box.describe(&cx(), &theme);
	assert_eq!(described.kind, NodeKind::Card);
	assert_eq!(described.props.get_str("tone"), Some("error"));
	assert_eq!(described.props.get_str("role"), Some("omp.tool"));
	assert_eq!(described.props.get_bool("inset"), Some(true));

	let mut user_box = Box::styled(1, 1, Some(bg_styler(&theme, "userMessageBg")), None);
	user_box.add_child(Text::new("hi"));
	let described = user_box.describe(&cx(), &theme);
	assert_eq!(described.kind, NodeKind::Card);
	assert_eq!(described.props.get_str("tone"), Some("user"));
	assert_eq!(described.props.get_str("role"), Some("omp.user"));
}

#[test]
fn box_border_colour_maps_to_ring_tone_and_drops_glyph_chrome() {
	let theme = theme();
	let color_theme = theme.clone();
	let chars = BoxBorderChars::new("╭", "╮", "╰", "╯", "─", "│");
	let accent = color_theme.get_fg_ansi("borderAccent").unwrap_or_default();
	let border = BoxBorder {
		chars,
		color: Some(Arc::new(move |text| format!("{accent}{text}"))),
	};
	let mut bordered = Box::styled(1, 0, None, Some(border));
	bordered.add_child(Text::new("hi"));
	let described = bordered.describe(&cx(), &theme);
	assert_eq!(described.kind, NodeKind::Card);
	assert_eq!(described.props.get_str("tone"), Some("accent"));
	assert_eq!(described.props.get_bool("inset"), None);
	let json = serde_json::to_string(&described.to_wire("n")).expect("serializable node");
	for glyph in ['╭', '╮', '╰', '╯', '│', '─'] {
		assert!(!json.contains(glyph), "described node leaked border glyph {glyph}: {json}");
	}
}

#[test]
fn plain_box_is_a_col_and_rebuilds_only_when_children_change() {
	let mut boxed = Box::new(1, 1);
	boxed.add_child(Text::new("hello"));
	let first = boxed.describe(&cx(), &theme());
	assert_eq!(first.kind, NodeKind::Col);
	assert_eq!(first.children.len(), 1);
	assert_eq!(boxed.describe(&cx(), &theme()), first);

	boxed.add_child(Text::new("more"));
	assert_ne!(boxed.describe(&cx(), &theme()), first);
}

#[test]
#[ignore = "missing facility: Markdown has no NativeDescribe (no describe_native / md streaming node), so the append-only stream description cannot be asserted"]
fn markdown_describe_streaming_keeps_the_node_and_grows_append_only() {}

#[test]
#[ignore = "missing facility: Table has no NativeDescribe (no describe_native table node), so column priority/alignment/truncation props cannot be asserted"]
fn table_describe_keeps_column_priorities_alignment_and_truncation() {}

#[test]
#[ignore = "missing facility: Text::describe_native emits plain text and no ANSI-to-token-span conversion exists, so styled-ANSI spans cannot be asserted"]
fn text_describe_turns_theme_styled_ansi_into_token_spans() {}

#[test]
#[ignore = "missing facility: Disclosure has no NativeDescribe (no describe_native collapsed/body node), so the described expanded section cannot be asserted"]
fn disclosure_toggle_mirrors_into_expanded_state_and_described_section() {}
