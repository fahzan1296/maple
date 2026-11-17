use maple_tui::tools::lsp::{
	parse_lsp_diagnostics, parse_status_row, render_result_expanded, LspAction, LspParams, LspServerState,
};

fn diagnostics_args() -> LspParams {
	LspParams { action: Some(LspAction::Diagnostics), file: Some("src/a.ts".into()), ..Default::default() }
}

fn request_args() -> LspParams {
	LspParams { action: Some(LspAction::Request), ..Default::default() }
}

#[test]
fn request_text_starting_with_a_glyph_stays_request_text() {
	let text = "server ← custom/method:\n✘ literal\nkeep-me";
	let rows = render_result_expanded(Some(&request_args()), text, false, false).join("\n");
	assert!(!rows.contains("No issues"));
	assert!(rows.contains("✘ literal"));
	assert!(rows.contains("keep-me"));
}

#[test]
fn glyph_mid_line_does_not_turn_a_request_into_diagnostics() {
	let text = "{\"items\": [\"a ✘ b\"]}\nsecond line";
	let rows = render_result_expanded(Some(&request_args()), text, false, false).join("\n");
	assert!(!rows.contains("No issues"));
	assert!(rows.contains("{\"items\": [\"a ✘ b\"]}"));
	assert!(rows.contains("second line"));
}

#[test]
fn grouped_diagnostics_are_counted_and_prefixed_with_their_file() {
	let text = [
		"1 error(s), 1 warning(s):",
		"# src/",
		"## a.ts",
		"  3:5 [error] [ts] Cannot find name 'x'. (2304)",
		"  9:1 [warning] [ts] Unused 'y'. (6133)",
	]
	.join("\n");
	let parsed = parse_lsp_diagnostics(&text);
	assert_eq!(parsed.diagnostics.len(), 2);
	assert_eq!(parsed.diagnostics[0].file_path, "src/a.ts");
	let rows = render_result_expanded(Some(&diagnostics_args()), &text, false, false);
	assert_eq!(rows[0], "LSP: diagnostics src/a.ts 1 error · 1 warning");
	assert!(rows.iter().any(|row| row == "src/a.ts:3:5 [error] [ts] Cannot find name 'x'. (2304)"));
	assert!(rows.iter().any(|row| row == "src/a.ts:9:1 [warning] [ts] Unused 'y'. (6133)"));
}

#[test]
fn failed_server_is_counted_instead_of_clean() {
	let text = "✘ src/a.ts: all language servers failed (tsserver)";
	let rows = render_result_expanded(Some(&diagnostics_args()), text, false, false);
	assert!(rows[0].ends_with("1 server failure"));
	assert!(!rows[0].contains("No issues"));
	assert!(rows.iter().any(|row| row == "src/a.ts: all language servers failed (tsserver)"));
}

#[test]
fn partial_failure_remains_a_warning_without_a_clean_claim() {
	let text = "OK\n⚠ src/a.ts: some servers failed (tsserver)";
	let rows = render_result_expanded(Some(&diagnostics_args()), text, false, false);
	assert_eq!(rows[0], "LSP: diagnostics src/a.ts");
	assert!(rows.iter().any(|row| row == "src/a.ts: some servers failed (tsserver)"));
	assert!(!rows.join("\n").contains("No issues"));
	let clean = render_result_expanded(Some(&diagnostics_args()), "OK", false, false);
	assert_eq!(clean, vec!["LSP: diagnostics src/a.ts No issues"]);
}

#[test]
fn diagnostics_count_every_file_including_info() {
	let text = [
		"✘ src/a.ts: 2 error(s)",
		"# src/",
		"## a.ts",
		"  1:1 [error] one",
		"  2:1 [error] two",
		"✘ src/b.ts: 1 error(s), 1 warning(s), 1 info(s)",
		"# src/",
		"## b.ts",
		"  1:1 [error] three",
		"  2:1 [warning] four",
		"  3:1 [info] five",
	]
	.join("\n");
	let rows = render_result_expanded(Some(&diagnostics_args()), &text, false, true);
	assert_eq!(rows[0], "LSP: diagnostics src/a.ts 3 errors · 1 warning · 1 info");
	let info_only = "1 info(s):\n# src/\n## a.ts\n  3:5 [info] Prefer const";
	assert!(render_result_expanded(Some(&diagnostics_args()), info_only, false, false)[0].ends_with("1 info"));
}

#[test]
fn status_rows_expose_pending_ready_and_error_plus_servers() {
	let pending = parse_status_row("⚠ src/a.ts: no diagnostics report yet (tsserver)", "⚠").unwrap();
	assert_eq!(pending.state, LspServerState::Pending);
	assert_eq!(pending.file, "src/a.ts");
	assert_eq!(pending.servers, vec!["tsserver"]);

	let ready = parse_status_row("✓ src/b.ts: no issues", "✓").unwrap();
	assert_eq!(ready.state, LspServerState::Ready);

	let failed = parse_status_row("✘ src/c.ts: all language servers failed (tsserver, biome)", "✘").unwrap();
	assert_eq!(failed.state, LspServerState::Error);
	assert_eq!(failed.servers, vec!["tsserver", "biome"]);

	let counted = parse_status_row("✘ src/d.ts: 2 error(s)", "✘").unwrap();
	assert_eq!(counted.state, LspServerState::Error);
	assert!(parse_status_row("mid ✘ line", "✘").is_none());
}
