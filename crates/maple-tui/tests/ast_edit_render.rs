//! Port of `l/test/ast-edit-render.test.ts` (collapsed preview and header).
//!
//! Hyperlink/OSC-8 scenarios stay with the host renderer; the preview window,
//! per-file cut, and framed header/footer text are asserted here.

use maple_tui::tools::ast_edit::{
	AstEditOp, AstEditRenderArgs, AstEditToolDetails, ast_edit_footer, ast_edit_header, render_result,
	render_result_collapsed,
};

fn args() -> AstEditRenderArgs {
	AstEditRenderArgs {
		ops: vec![AstEditOp {
			pat: Some("oldApi($$$A)".to_string()),
			out: Some("newApi($$$A)".to_string()),
		}],
		paths: vec!["src/**/*.ts".to_string()],
	}
}

#[test]
fn collapsed_preview_cuts_first_diff_and_counts_files_left_out() {
	let diff_rows: Vec<String> = (0..10).map(|i| format!("{}{}│x", if i % 2 == 0 { "-" } else { "+" }, i + 1)).collect();
	let mut display = vec!["# src/".to_string(), "## a.ts (5 replacements)".to_string()];
	display.extend(diff_rows);
	display.push("## b.ts (1 replacement)".to_string());
	display.push("-1│y".to_string());
	display.push("+1│z".to_string());
	let output = display.join("\n");
	let details = AstEditToolDetails {
		total_replacements: 6,
		files_touched: 2,
		files_searched: 2,
		display_content: Some(output.clone()),
		..AstEditToolDetails::default()
	};
	let body = render_result_collapsed(Some(&details), &output, false);
	assert_eq!(body.len(), 6);
	assert!(body.last().expect("summary").contains("… 1 more file"));
}

#[test]
fn header_names_scope_and_footer_carries_proposed_counts() {
	let header = ast_edit_header(Some(&args()), None);
	assert_eq!(header, "AST Edit: oldApi($$$A) in src/**/*.ts");
	let details = AstEditToolDetails {
		total_replacements: 2,
		files_touched: 1,
		files_searched: 3,
		..AstEditToolDetails::default()
	};
	assert_eq!(ast_edit_footer(Some(&details)), "proposed · 2 replacements · 1 file · searched 3");
}

#[test]
fn footer_drops_proposed_once_applied() {
	let details = AstEditToolDetails {
		total_replacements: 2,
		files_touched: 1,
		files_searched: 3,
		applied: true,
		..AstEditToolDetails::default()
	};
	assert_eq!(ast_edit_footer(Some(&details)), "2 replacements · 1 file · searched 3");
}

#[test]
fn no_match_footer_keeps_zero_replacements_and_search_count() {
	let details = AstEditToolDetails {
		total_replacements: 0,
		files_touched: 0,
		files_searched: 3,
		parse_errors: vec!["src/a.ts: parse error".to_string()],
		..AstEditToolDetails::default()
	};
	assert_eq!(ast_edit_footer(Some(&details)), "0 replacements · searched 3");
}

#[test]
fn error_keeps_subject_and_message() {
	assert_eq!(ast_edit_header(Some(&args()), None), "AST Edit: oldApi($$$A) in src/**/*.ts");
	let lines = render_result(None, "Pattern parse error", true);
	assert!(lines.iter().any(|line| line.contains("Pattern parse error")));
}
