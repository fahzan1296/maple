//! Port of `l/test/log-formatting.test.ts`.

use maple_tui::apps::debug::log_formatting::{
	format_debug_log_expanded_lines, parse_debug_log_timestamp_ms,
};
use maple_tui::overlays::extensions::display_text::sanitize_display_text;
use maple_tui::text_shape::{Ellipsis, truncate_to_width, visible_width};

fn display_truncate(input: &str, width: usize) -> String {
	truncate_to_width(&sanitize_display_text(input), width, Ellipsis::Unicode)
}

#[test]
fn strips_ansi_codes_and_carriage_returns() {
	assert_eq!(display_truncate("\u{1b}[31merror\r\u{1b}[0m", 80), "error");
}

#[test]
fn replaces_tabs_with_spaces() {
	assert_eq!(display_truncate("col1\tcol2", 80), "col1   col2");
}

#[test]
fn removes_unsafe_control_characters() {
	assert_eq!(display_truncate("ok\u{7}bad", 80), "okbad");
}

#[test]
fn truncates_long_lines() {
	let result = display_truncate("0123456789ABCDEFGHIJ", 10);
	assert!(visible_width(&result) <= 10);
	assert!(result.starts_with("012345"));
}

#[test]
fn wraps_expanded_log_lines_without_dropping_content() {
	let lines = format_debug_log_expanded_lines("0123456789ABCDEFGHIJ", 6);
	assert!(lines.len() > 1);
	for line in &lines {
		assert!(visible_width(line) <= 6);
	}
	assert_eq!(lines.concat(), "0123456789ABCDEFGHIJ");
}
#[test]
fn parses_timestamp_from_json_log_lines() {
	let input = r#"{"timestamp":"2026-02-14T12:34:56.000Z","level":"info","message":"ok"}"#;
	assert_eq!(
		parse_debug_log_timestamp_ms(input),
		Some(1_771_072_496_000)
	);
}

#[test]
fn returns_none_when_timestamp_is_missing_or_invalid() {
	assert_eq!(parse_debug_log_timestamp_ms(r#"{"message":"ok"}"#), None);
	assert_eq!(parse_debug_log_timestamp_ms(r#"{"timestamp":"not-a-date"}"#), None);
	assert_eq!(parse_debug_log_timestamp_ms("not-json"), None);
}
