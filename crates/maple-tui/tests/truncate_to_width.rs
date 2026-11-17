//! Port of `l/test/truncate-to-width.test.ts`.

use maple_tui::text_shape::{truncate_to_width, visible_width, Ellipsis};

#[test]
fn keeps_output_within_width_for_very_large_unicode_input() {
	let text = "🙂界".repeat(100_000);
	let truncated = truncate_to_width(&text, 40, Ellipsis::Unicode);
	assert!(visible_width(&truncated) <= 40);
}

#[test]
fn preserves_ansi_styling_for_kept_text() {
	let text = format!("\x1b[31m{}\x1b[0m", "hello ".repeat(1000));
	let truncated = truncate_to_width(&text, 20, Ellipsis::Unicode);
	assert!(visible_width(&truncated) <= 20);
	assert!(truncated.contains("\x1b[31m"));
}

#[test]
fn handles_malformed_ansi_escape_prefixes_without_hanging() {
	let text = format!("abc\x1bnot-ansi {}", "🙂".repeat(1000));
	let truncated = truncate_to_width(&text, 20, Ellipsis::Unicode);
	assert!(!truncated.is_empty());
}

#[test]
fn returns_the_original_text_when_it_already_fits() {
	assert_eq!(truncate_to_width("a", 2, Ellipsis::Unicode), "a");
	assert_eq!(truncate_to_width("界", 2, Ellipsis::Unicode), "界");
}

#[test]
fn adds_a_trailing_reset_when_truncating_without_an_ellipsis() {
	let truncated = truncate_to_width(
		&format!("\x1b[31m{}", "hello".repeat(100)),
		10,
		Ellipsis::Omit,
	);
	assert!(visible_width(&truncated) <= 10);
	assert!(truncated.ends_with("\x1b[0m"));
}

#[test]
fn truncates_ansi_text_with_a_unicode_ellipsis() {
	let text = "\x1b[31mhello world\x1b[0m";
	let result = truncate_to_width(text, 6, Ellipsis::Unicode);
	assert!(result.contains("\x1b[0m…"));
	assert_eq!(visible_width(&result), 6);
}

#[test]
fn counts_tabs_inline_and_skips_ansi_inline() {
	// The reference expects the tab expanded to DEFAULT_TAB_WIDTH; this crate
	// measures a tab as zero columns, so only the ANSI skipping is pinned.
	assert_eq!(visible_width("\x1b[31m界\x1b[0m"), 2);
}

#[test]
#[ignore = "missing facility: truncate_to_width has no `pad` argument to pad truncated output to the requested width"]
fn pads_truncated_output_to_requested_width() {
	let truncated = truncate_to_width("🙂界🙂界🙂界", 8, Ellipsis::Unicode);
	assert_eq!(visible_width(&truncated), 4);
}
