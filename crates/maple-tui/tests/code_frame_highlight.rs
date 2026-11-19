//! Port of `l/test/code-frame-highlight.test.ts`.
//!
//! The reference styles search rows through `createCodeFrameStyler`,
//! `classifyGroupedLines` and the theme's `highlightCode`. The crate ports only
//! the row classifiers (`is_match_row`/`is_grouped_header`) and has no
//! highlighter or styler, so all four assertions are ignored; the bodies pin
//! the classification seam those tests depend on.

use maple_tui::tools::grouped_file_output::{is_grouped_header, is_match_row};

#[test]
#[ignore = "missing facility: no createCodeFrameStyler/highlightCode, so per-run highlighting cannot be asserted"]
fn highlights_consecutive_rows_of_one_file_as_one_text() {
	assert!(is_grouped_header("## a.ts"));
	assert!(is_match_row("*2│   end */ const x = 1;"));
	assert!(!is_match_row(" 9│   end */ const y = 2;"));
}

#[test]
#[ignore = "missing facility: no highlighter, so the flat tint for an unknown language cannot be asserted"]
fn keeps_the_flat_tint_for_a_file_in_a_language_the_highlighter_lacks() {
	assert!(is_grouped_header("## notes.unknownext"));
	assert!(is_match_row("*3│plain text"));
}

#[test]
#[ignore = "missing facility: no bounded-chunk styler, so the highlighted row budget cannot be asserted"]
fn highlights_only_a_bounded_chunk_of_a_huge_match_for_the_rows_requested() {
	let mut lines = vec!["## big.ts".to_string()];
	for i in 0..2000 {
		lines.push(format!(" {}│const v{i} = {i};", i + 1));
	}
	assert!(lines.iter().all(|l| !is_match_row(l)));
	assert!(is_grouped_header(&lines[0]));
}

#[test]
#[ignore = "missing facility: no language override parameter, so ast_grep's searched language cannot be asserted"]
fn highlights_in_the_searched_language_when_ast_grep_overrode_the_files() {
	assert!(is_grouped_header("## a.h"));
	assert!(is_match_row("*1│class A {};"));
}
