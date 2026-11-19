//! Port of `l/test/write-streaming-incremental-render.test.ts` (pure subset).
//!
//! The reference tracks an append-only highlight stream through a live card,
//! including CRLF normalization and trailing-newline line accounting. The crate
//! ports the payload normalization, line counting, and collapsed/expanded
//! preview rows; those are asserted here. Streaming highlight/tokenization is
//! host concern.

use maple_tui::tools::write::{
	count_lines, normalize_display_text, render_result, WriteRenderArgs,
};

fn args(content: &str) -> WriteRenderArgs {
	WriteRenderArgs {
		path: Some("notes.md".to_string()),
		content: Some(content.to_string()),
		..Default::default()
	}
}

#[test]
fn normalizes_crlf_and_cr_only_payloads_before_counting() {
	assert_eq!(normalize_display_text("a\r\nb\rc"), "a\nbc");
	assert_eq!(count_lines(&normalize_display_text("a\r\nb\rc")), 2);
	assert_eq!(count_lines(&normalize_display_text("\r\r\r")), 1);
	assert_eq!(count_lines(""), 0);
}

#[test]
fn counts_a_trailing_newline_as_a_final_empty_row() {
	let content = "one\ntwo\n";
	assert_eq!(count_lines(content), 3);
	let rows = render_result(Some(&args(content)), None, false, false, "", false);
	assert_eq!(rows[0], "Write: notes.md · 3 lines");
	// Collapsed preview takes the first WRITE_PREVIEW_LINES rows.
	assert_eq!(rows.len(), 1 + 3);
	assert_eq!(rows[1], "one");
	assert_eq!(rows[2], "two");
	assert_eq!(rows[3], "");
}

#[test]
fn shows_progress_while_partial_and_drops_it_once_settled() {
	let content = "first\nsecond";
	let partial = render_result(Some(&args(content)), None, false, true, "writing…", false);
	assert!(partial.iter().any(|row| row == "writing…"));
	let settled = render_result(Some(&args(content)), None, false, false, "writing…", false);
	assert!(!settled.iter().any(|row| row == "writing…"));
}

#[test]
fn collapsed_preview_caps_rows_and_expanded_shows_every_line() {
	let content: String = (1..=15).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
	let collapsed = render_result(Some(&args(&content)), None, false, false, "", false);
	assert_eq!(collapsed[0], "Write: notes.md · 15 lines");
	assert_eq!(collapsed.len(), 1 + 6 + 1);
	assert_eq!(collapsed.last().unwrap(), "… 9 more lines");

	let expanded = render_result(Some(&args(&content)), None, false, false, "", true);
	assert_eq!(expanded.len(), 1 + 15);
	for index in 1..=15 {
		assert!(expanded.iter().any(|row| row == &format!("line {index}")));
	}
	assert!(!expanded.iter().any(|row| row.contains("more lines")));
}
