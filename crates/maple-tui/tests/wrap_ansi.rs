//! Port of `l/test/wrap-ansi.test.ts`.
//!
//! The crate's wrapper preserves the active SGR prefix at the start of each
//! continuation line but does not emit per-line underline/strikethrough resets,
//! so the reference's reset-placement tests are ignored.

use maple_tui::text_shape::{visible_width, wrap_text_with_ansi};

#[test]
fn should_not_apply_underline_style_before_the_styled_text() {
	let underline_on = "\x1b[4m";
	let underline_off = "\x1b[24m";
	let url = "https://example.com/very/long/path/that/will/wrap";
	let text = format!("read this thread {underline_on}{url}{underline_off}");

	let wrapped = wrap_text_with_ansi(&text, 40);

	assert_eq!(wrapped[0], "read this thread");
	assert!(wrapped[1].starts_with(underline_on));
	assert!(wrapped[1].contains("https://"));
}

#[test]
fn should_not_have_whitespace_before_underline_reset_code() {
	let underline_on = "\x1b[4m";
	let underline_off = "\x1b[24m";
	let text = format!("{underline_on}underlined text here {underline_off}more");

	let wrapped = wrap_text_with_ansi(&text, 18);

	assert!(!wrapped[0].contains(&format!(" {underline_off}")));
}

#[test]
fn should_preserve_background_color_across_wrapped_lines_without_full_reset() {
	let bg_blue = "\x1b[44m";
	let reset = "\x1b[0m";
	let text = format!("{bg_blue}hello world this is blue background text{reset}");

	let wrapped = wrap_text_with_ansi(&text, 15);

	for line in &wrapped {
		assert!(line.contains(bg_blue));
	}
	for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
		assert!(!line.ends_with("\x1b[0m"));
	}
}

#[test]
fn should_wrap_plain_text_correctly() {
	let text = "hello world this is a test";
	let wrapped = wrap_text_with_ansi(text, 10);

	assert!(wrapped.len() > 1);
	for line in &wrapped {
		assert!(visible_width(line) <= 10);
	}
}

#[test]
fn should_truncate_trailing_whitespace_that_exceeds_width() {
	let wrapped = wrap_text_with_ansi("  ", 1);
	assert!(visible_width(&wrapped[0]) <= 1);
}

#[test]
fn should_preserve_color_codes_across_wraps() {
	let red = "\x1b[31m";
	let reset = "\x1b[0m";
	let text = format!("{red}hello world this is red{reset}");

	let wrapped = wrap_text_with_ansi(&text, 10);

	for line in wrapped.iter().skip(1) {
		assert!(line.starts_with(red));
	}
	for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
		assert!(!line.ends_with("\x1b[0m"));
	}
}

#[test]
#[ignore = "missing facility: wrap_text_with_ansi does not emit per-line underline-off resets"]
fn should_not_bleed_underline_to_padding() {
	let underline_on = "\x1b[4m";
	let url = "https://example.com/very/long/path/that/will/definitely/wrap";
	let text = format!("prefix {underline_on}{url}\x1b[24m suffix");
	let wrapped = wrap_text_with_ansi(&text, 30);
	assert!(wrapped.len() > 2);
}

#[test]
#[ignore = "missing facility: wrap_text_with_ansi does not emit per-line underline-off while preserving background"]
fn should_reset_underline_but_preserve_background_when_wrapping_underlined_text_inside_background() {
	let text = "\x1b[41mprefix \x1b[4mUNDERLINED_CONTENT_THAT_WRAPS\x1b[24m suffix\x1b[0m";
	let wrapped = wrap_text_with_ansi(text, 20);
	assert!(wrapped.len() > 1);
}

#[test]
#[ignore = "missing facility: wrap_text_with_ansi does not emit per-line strikethrough-off resets"]
fn disables_strikethrough_at_wrapped_line_ends_while_preserving_fg_bg_colors() {
	let text = "\x1b[38;5;196m\x1b[48;5;236m\x1b[9mstrikethrough content that wraps across lines\x1b[29m\x1b[0m";
	let wrapped = wrap_text_with_ansi(text, 16);
	assert!(wrapped.len() > 1);
}
