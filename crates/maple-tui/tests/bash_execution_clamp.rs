//! Port of `l/test/bash-execution-clamp.test.ts`.
//!
//! The reference drives `BashExecutionComponent.getOutput()`; the crate exposes
//! the same clamp as the pure `clamp_display_line`, so the assertions call it
//! directly.

use maple_tui::chat::execution_shared::{clamp_display_line, MAX_DISPLAY_LINE_CHARS};
use maple_tui::text_shape::visible_width;

#[test]
fn truncates_cjk_over_limit_and_calculates_omitted_from_visible_width() {
	let cjk = "日本語".repeat(2500);
	let expected_visible = visible_width(&cjk);
	let output = clamp_display_line(&cjk);

	assert!(output.contains("…"));
	assert!(output.contains(&format!(
		"[{} visible columns omitted]",
		expected_visible - MAX_DISPLAY_LINE_CHARS
	)));
}

#[test]
fn calculates_omitted_from_visible_width_not_raw_length_for_ansi() {
	let ansi = "\x1b[1;31;47mbold red on white\x1b[0m".repeat(1000);
	let expected_visible = visible_width(&ansi);
	let output = clamp_display_line(&ansi);

	assert!(expected_visible > MAX_DISPLAY_LINE_CHARS);
	assert!(output.contains(&format!(
		"[{} visible columns omitted]",
		expected_visible - MAX_DISPLAY_LINE_CHARS
	)));
}

#[test]
fn truncates_long_ascii_with_ellipsis() {
	let long_ascii = "a".repeat(5000);
	let output = clamp_display_line(&long_ascii);

	assert!(output.contains("…"));
	assert!(output.contains("visible columns omitted"));
	assert!(output.chars().count() < 5000);
}

#[test]
fn truncated_portion_stays_within_the_visible_budget() {
	let long = "hello world ".repeat(1000);
	let output = clamp_display_line(&long);

	assert!(output.contains("omitted"));
	let truncated = output.split(" [").next().unwrap_or_default();
	assert!(visible_width(truncated) <= MAX_DISPLAY_LINE_CHARS + 10);
}

#[test]
fn omitted_is_visible_width_minus_limit() {
	let text = "test".repeat(1500);
	let expected_visible = visible_width(&text);
	let output = clamp_display_line(&text);

	assert!(output.contains(&format!(
		"[{} visible columns omitted]",
		expected_visible - MAX_DISPLAY_LINE_CHARS
	)));
}

#[test]
fn handles_mixed_ascii_cjk_emoji_and_ansi() {
	let mixed = "abc日本語😀\x1b[34mblue\x1b[0m".repeat(500);
	let expected_visible = visible_width(&mixed);
	let output = clamp_display_line(&mixed);

	assert!(expected_visible > MAX_DISPLAY_LINE_CHARS);
	assert!(output.contains(&format!(
		"[{} visible columns omitted]",
		expected_visible - MAX_DISPLAY_LINE_CHARS
	)));
}

#[test]
fn returns_original_at_exactly_the_limit() {
	let at_limit = "a".repeat(MAX_DISPLAY_LINE_CHARS);
	let output = clamp_display_line(&at_limit);

	assert_eq!(output, at_limit);
	assert!(!output.contains("omitted"));
}

#[test]
fn returns_original_just_below_the_limit() {
	let below = "a".repeat(MAX_DISPLAY_LINE_CHARS - 1);
	let output = clamp_display_line(&below);

	assert_eq!(output, below);
	assert!(!output.contains("omitted"));
}

#[test]
fn truncates_just_above_the_limit() {
	let above = "a".repeat(MAX_DISPLAY_LINE_CHARS + 1);
	let output = clamp_display_line(&above);

	assert!(output.contains("omitted"));
	assert!(output.contains("[1 visible columns omitted]"));
}

#[test]
fn handles_empty_string() {
	assert_eq!(clamp_display_line(""), "");
}
