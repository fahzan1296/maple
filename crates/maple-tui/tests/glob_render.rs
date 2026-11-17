//! Port of `l/test/glob-render.test.ts`.
//!
//! The crate's glob renderer is plain-text plus a native describe view; the
//! themed terminal layout (tool column, accent headers, OSC-8 rows) is not
//! ported, so the ANSI row assertions are adapted to the plain renderer and the
//! native view's link targets.

use maple_tui::native::node::{NativeNode, Tone};
use maple_tui::tools::glob::{
	describe_result, empty_label, render_result, summarize_glob_result, GlobRenderArgs,
	GlobToolDetails,
};

fn args() -> Option<GlobRenderArgs> {
	Some(GlobRenderArgs {
		path: vec!["src/**/*.ts".to_string()],
		paths: vec!["src/**/*.ts".to_string()],
		limit: None,
	})
}

fn details(files: &[&str], truncated: bool, timed_out: bool, result_limit_reached: Option<usize>) -> GlobToolDetails {
	GlobToolDetails {
		files: files.iter().map(|file| file.to_string()).collect(),
		truncated,
		timed_out,
		result_limit_reached,
		..GlobToolDetails::default()
	}
}

fn head_texts(view: &maple_tui::tools::native_view::NativeToolView) -> Vec<String> {
	let mut out = Vec::new();
	let Some(head) = &view.tool else { return out };
	for part in head.meta.iter().flatten() {
		match part {
			maple_tui::native::node::Text::Plain(text) => out.push(text.clone()),
			maple_tui::native::node::Text::Spans(spans) => {
				out.push(spans.iter().map(|span| span.t.clone()).collect())
			}
		}
	}
	out
}

fn hrefs(node: &NativeNode, out: &mut Vec<String>) {
	if let Some(spans) = node.props.get("spans").and_then(|value| value.as_array()) {
		for span in spans {
			if let Some(href) = span.get("href").and_then(|value| value.as_str()) {
				out.push(href.to_string());
			}
		}
	}
	for child in &node.children {
		hrefs(child, out);
	}
}

#[test]
fn distinguishes_a_timed_out_scan_from_a_result_limit() {
	assert_eq!(empty_label(false, false), "No files found");
	assert_eq!(empty_label(false, true), "No matches before timeout (scan incomplete)");

	let timed_out = summarize_glob_result(
		args().as_ref(),
		Some(&details(&["src/a.ts", "src/b.ts"], true, true, None)),
		false,
		"src/a.ts\nglob timed out after 5s; returning 2 partial matches",
	);
	assert!(timed_out.timed_out);
	assert!(timed_out.truncated);
	assert!(timed_out.reasons.iter().any(|reason| reason == "timed out"));

	let limit_only = summarize_glob_result(
		args().as_ref(),
		Some(&details(&["src/a.ts", "src/b.ts"], true, false, Some(2))),
		"src/a.ts",
	);
	assert!(!limit_only.timed_out);
	assert!(limit_only.truncated);
	let reasons = limit_only.reasons.join(", ");
	assert!(reasons.contains("truncated"), "{reasons}");
	assert!(!reasons.contains("timed out"), "{reasons}");
}

#[test]
fn renders_empty_and_timed_out_results_without_claiming_no_files() {
	let empty = render_result(
		"No files found matching pattern",
		args().as_ref(),
		Some(&details(&[], false, false, None)),
		false,
		true,
	);
	assert!(empty.join("\n").contains("No files found"));
	assert!(!empty.join("\n").contains("incomplete"));

	let timed_out = render_result(
		"Glob timed out after 5s before finding any matches",
		args().as_ref(),
		Some(&details(&[], true, true, None)),
		false,
		true,
	);
	let text = timed_out.join("\n");
	assert!(text.contains("No matches before timeout (scan incomplete)"), "{text}");
	assert!(text.contains("timed out"), "{text}");
	assert!(!text.contains("No files found"), "{text}");
}

#[test]
fn plain_rows_start_at_the_left_edge_with_no_ansi() {
	let rows = render_result("", args().as_ref(), Some(&details(&["src/a.ts", "src/b.ts"], false, false, None)), false, true);
	assert_eq!(rows[0], "2 files");
	assert!(!rows[0].starts_with(' '));
	assert!(!rows.iter().any(|row| row.contains('\x1b')));
}

#[test]
fn native_result_lists_files_with_absolute_link_targets() {
	let mut meta_details = details(&["src/a.ts", "src/b.ts"], false, false, None);
	meta_details.cwd = Some("/repo".to_string());
	meta_details.file_count = Some(2);
	let view = describe_result("", args().as_ref(), Some(&meta_details), false);
	assert_eq!(head_texts(&view), ["2 files"]);
	let mut found = Vec::new();
	for node in view.body.iter().flatten() {
		hrefs(node, &mut found);
	}
	assert_eq!(found, ["file:///repo/src/a.ts", "file:///repo/src/b.ts"]);
}

#[test]
fn native_result_marks_an_incomplete_scan_with_a_warning_and_note() {
	let mut timed_out = details(&[], true, true, None);
	timed_out.file_count = Some(0);
	let view = describe_result("", args().as_ref(), Some(&timed_out), false);
	assert_eq!(view.tone, Some(Tone::Warning));
	let Some(head) = &view.tool else { panic!("head") };
	let note = match head.note.as_ref() {
		Some(maple_tui::native::node::Text::Plain(text)) => text.clone(),
		Some(maple_tui::native::node::Text::Spans(spans)) => spans.iter().map(|span| span.t.clone()).collect(),
		None => String::new(),
	};
	assert_eq!(note, "timed out");
	assert!(head_texts(&view).contains(&"0 files".to_string()));
}

#[test]
fn native_links_keep_control_bytes_in_the_target_only() {
	let control = "\x1b[?1049h";
	let cwd = format!("/tmp/glob{control}-project");
	let file = format!("src/fi{control}xture.ts");
	let mut raw = details(&[file.as_str()], false, false, None);
	raw.cwd = Some(cwd.clone());
	raw.file_count = Some(1);
	let view = describe_result("", args().as_ref(), Some(&raw), false);
	let mut found = Vec::new();
	for node in view.body.iter().flatten() {
		hrefs(node, &mut found);
	}
	assert_eq!(found, ["file:///tmp/glob%1B%5B%3F1049h-project/src/fi%1B%5B%3F1049hxture.ts"]);
}

#[test]
#[ignore = "missing API: no themed terminal renderer for glob rows (tool column, accent headers, OSC-8 rows)"]
fn starts_glob_output_at_the_tool_column_without_accent_headers() {}
