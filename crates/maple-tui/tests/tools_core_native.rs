//! Port of `l/test/tools-core-native.test.ts`.
//!
//! The crate has no `describeResult` for edit/read/write (only grep), and no
//! native blob registry in test scope. Only the code-cell wrap case is expressible
//! via `render::code_cell::describe_code_cell`; the rest stay ignored.

use maple_tui::native::node::{NativeNode, NodeKind};
use maple_tui::render::code_cell::{describe_code_cell, CodeCellDescribeOptions};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn theme() -> Theme {
	create_theme(builtin_theme_json("dark").expect("embedded dark theme"), CreateThemeOptions::default())
		.expect("dark theme resolves")
}

fn find_kind(node: &NativeNode, kind: NodeKind) -> Option<&NativeNode> {
	if node.kind == kind {
		return Some(node);
	}
	node.children.iter().find_map(|child| find_kind(child, kind))
}

fn cell_options(code: String) -> CodeCellDescribeOptions {
	CodeCellDescribeOptions {
		code,
		language: Some("ts".to_string()),
		index: None,
		total: None,
		title: None,
		status: None,
		spinner_frame: None,
		duration: None,
		output: None,
		output_max_lines: None,
		code_max_lines: None,
		code_tail: None,
		expanded: None,
		show_language: None,
		code_start_line: Some(1),
		code_line_numbers: None,
	}
}

#[test]
fn wraps_long_code_cell_source_as_the_ansi_renderer_does() {
	let theme = theme();
	let long = format!("const x = \"{}\";", "y".repeat(300));
	let node = describe_code_cell(&cell_options(long.clone()), &theme);
	let code = find_kind(&node, NodeKind::Code).expect("code node");
	assert_eq!(code.props.get_str("text"), Some(long.as_str()));
	assert_eq!(code.props.get_bool("wrap"), Some(true));
}

#[test]
#[ignore = "missing facility: no edit describeResult (diff node/hunks)"]
fn describes_an_edit_result_as_a_path_headed_tool_over_a_headerless_diff() {}

#[test]
#[ignore = "missing facility: no edit describeResult (hunk grouping at real lines)"]
fn diffs_an_edit_as_hunks_at_their_real_lines() {}

#[test]
#[ignore = "missing facility: no read describeResult and no native blob registry in test scope"]
fn describes_an_image_read_as_an_image_node_backed_by_a_registered_blob() {}

#[test]
#[ignore = "missing facility: grep describe_result carries no per-line numbers/marks props"]
fn keeps_grep_hit_line_numbers_and_marks_match_lines() {}
