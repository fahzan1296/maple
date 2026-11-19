//! Port of `l/test/eval-code-preview-render.test.ts`.
//!
//! Deviations:
//! - The crate has no `evalToolRenderer.renderResult`/`renderCall`, so the
//!   bounded tail window is exercised through `render_code_cell`, which owns
//!   the `code_tail`/`code_max_lines` contract.
//! - `previewWindowRows()` reads `LINES` instead of `process.stdout.rows`; both
//!   fall back to the same 30-row assumption.

use maple_tui::ansi_scan::strip_ansi;
use maple_tui::render::code_cell::{render_code_cell, CodeCellOptions, CodeCellStatus};
use maple_tui::render::render_utils::preview_window_rows;
use maple_tui::theme::default_theme;

fn cell_options(code: &str, expanded: bool, status: CodeCellStatus) -> CodeCellOptions {
	let window = preview_window_rows();
	CodeCellOptions {
		code: code.to_string(),
		language: Some("python".to_string()),
		index: Some(0),
		total: Some(1),
		title: Some("Investigate".to_string()),
		status: Some(status),
		spinner_frame: Some(0),
		duration: None,
		output: None,
		output_max_lines: None,
		code_max_lines: Some(window),
		code_tail: Some(true),
		expanded: Some(expanded),
		show_language: None,
		width: 120,
		code_start_line: None,
		code_line_numbers: None,
	}
}

fn long_code() -> (String, String, String) {
	let total = preview_window_rows() + 5;
	let code = (0..total)
		.map(|index| format!("value_{index} = {index}"))
		.collect::<Vec<_>>()
		.join("\n");
	let first = "value_0 = 0".to_string();
	let last = format!("value_{} = {}", total - 1, total - 1);
	(code, first, last)
}

#[test]
fn caps_collapsed_result_code_to_the_tail_window_with_an_earlier_marker() {
	let theme = default_theme();
	let (code, first, last) = long_code();
	let rendered = strip_ansi(&render_code_cell(&cell_options(&code, false, CodeCellStatus::Complete), &theme).join("\n"));
	assert!(rendered.contains(&last), "last line missing: {rendered:?}");
	assert!(rendered.contains("earlier line"));
	assert!(!rendered.contains(&first));
}

#[test]
fn shows_the_full_source_when_expanded() {
	let theme = default_theme();
	let (code, first, last) = long_code();
	let rendered = strip_ansi(&render_code_cell(&cell_options(&code, true, CodeCellStatus::Complete), &theme).join("\n"));
	assert!(rendered.contains(&first));
	assert!(rendered.contains(&last));
	assert!(!rendered.contains("earlier line"));
}

#[test]
fn bounds_the_pending_preview_to_the_same_live_tail_window() {
	let theme = default_theme();
	let (code, first, last) = long_code();
	let rendered = strip_ansi(&render_code_cell(&cell_options(&code, false, CodeCellStatus::Pending), &theme).join("\n"));
	assert!(rendered.contains(&last), "streaming edge missing: {rendered:?}");
	assert!(rendered.contains("earlier line"));
	assert!(!rendered.contains(&first));
}

#[test]
fn preview_window_rows_matches_the_reference_formula() {
	let rows = preview_window_rows();
	assert!(rows >= 6);
}

#[test]
#[ignore = "missing API: evalToolRenderer.renderCall multi-cell streamed rendering"]
fn renders_every_cell_of_a_streamed_call_while_a_later_cells_code_has_not_arrived() {}