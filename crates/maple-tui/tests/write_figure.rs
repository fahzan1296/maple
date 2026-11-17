//! Port of `l/test/write-figure.test.ts` (pure figure-classification subset).
//!
//! The reference streams SVG/mermaid/OBJ content through a live tool card with
//! terminal graphics protocol switching. The crate exposes the fence-language
//! table and write classification/summary helpers; those are asserted here.
//! Raster/protocol painting is host concern.

use maple_tui::tools::write::{
	count_lines, figure_fence_lang, is_figure_write, normalize_display_text, render_call,
	render_result, summarize_write, WriteRenderArgs, WriteToolDetails, FIGURE_FENCES,
};

fn args(path: &str, content: &str) -> WriteRenderArgs {
	WriteRenderArgs {
		path: Some(path.to_string()),
		content: Some(content.to_string()),
		..Default::default()
	}
}

#[test]
fn maps_figure_extensions_to_fence_languages() {
	assert_eq!(figure_fence_lang("diagram.svg"), Some("svg"));
	assert_eq!(figure_fence_lang("flow.mmd"), Some("mermaid"));
	assert_eq!(figure_fence_lang("flow.mermaid"), Some("mermaid"));
	assert_eq!(figure_fence_lang("mesh.obj"), Some("obj"));
	assert_eq!(figure_fence_lang("model.ply"), Some("ply"));
	assert_eq!(figure_fence_lang("scene.gltf"), Some("gltf"));
	assert_eq!(figure_fence_lang("notes.txt"), None);
	assert_eq!(figure_fence_lang("no-extension"), None);
	assert!(FIGURE_FENCES.contains(&("svg", "svg")));
}

#[test]
fn classifies_figure_writes_and_disables_them_on_errors_or_xdev() {
	let svg = args("chart.svg", "<svg viewBox='0 0 16 4'><rect width='16' height='4'/></svg>");
	assert!(is_figure_write(Some(&svg), false, false));
	assert!(!is_figure_write(Some(&svg), true, false));
	assert!(!is_figure_write(Some(&svg), false, true));

	// Whitespace-only content is not drawn as a figure.
	assert!(!is_figure_write(Some(&args("chart.svg", " \n\t")), false, false));
	// Non-figure extensions never qualify.
	assert!(!is_figure_write(Some(&args("notes.txt", "hello")), false, false));
	// No path/content means no figure.
	assert!(!is_figure_write(None, false, false));
}

#[test]
fn counts_normalized_lines_for_the_summary() {
	assert_eq!(normalize_display_text("a\r\nb\rc"), "a\nbc");
	assert_eq!(count_lines(""), 0);
	assert_eq!(count_lines("a"), 1);
	assert_eq!(count_lines("a\nb\n"), 3);

	let summary = summarize_write(
		Some(&args("chart.svg", "<svg>\n<rect/>\n</svg>")),
		None,
		false,
	);
	assert_eq!(summary.path.as_deref(), Some("chart.svg"));
	assert_eq!(summary.line_count, 3);
	assert_eq!(summary.figure_lang.as_deref(), Some("svg"));

	let plain = summarize_write(Some(&args("notes.txt", "one\ntwo")), None, false);
	assert_eq!(plain.figure_lang, None);
}

#[test]
fn labels_calls_with_the_path_and_singular_plural_line_counts() {
	assert_eq!(
		render_call(Some(&args("chart.svg", "<svg/>"))),
		"Write: chart.svg · 1 line"
	);
	assert_eq!(
		render_call(Some(&args("notes.txt", "a\nb"))),
		"Write: notes.txt · 2 lines"
	);
	assert_eq!(render_call(None), "Write");
}

#[test]
fn renders_preview_rows_with_a_collapsed_overflow_marker() {
	let content: String = (1..=9).map(|n| format!("line {n}\n")).collect();
	let rows = render_result(
		Some(&args("notes.txt", &content)),
		None,
		false,
		true,
		"",
		false,
	);
	assert_eq!(rows[0], "Write: notes.txt · 10 lines");
	assert_eq!(rows.len(), 1 + 6 + 1);
	assert_eq!(rows.last().unwrap(), "… 4 more lines");

	let expanded = render_result(
		Some(&args("notes.txt", &content)),
		None,
		false,
		false,
		"",
		true,
	);
	assert_eq!(expanded.len(), 1 + 10);
	assert!(!expanded.iter().any(|row| row.contains("more lines")));
}

#[test]
fn renders_errors_progress_and_executable_chip() {
	let failed = render_result(
		Some(&args("notes.txt", "body")),
		None,
		true,
		false,
		"disk full",
		true,
	);
	assert_eq!(failed, vec!["Write failed: disk full".to_string()]);
	assert_eq!(
		render_result(Some(&args("notes.txt", "x")), None, true, false, "  ", true),
		vec!["Write failed: Unknown error".to_string()]
	);

	let mut details = WriteToolDetails {
		made_executable: true,
		..Default::default()
	};
	let rows = render_result(
		Some(&args("run.sh", "#!/bin/sh\necho hi")),
		Some(&details),
		false,
		true,
		"writing…",
		false,
	);
	assert_eq!(rows[0], "Write: run.sh · 2 lines");
	assert_eq!(rows[1], "made executable!");
	assert_eq!(rows[2], "writing…");
	details.made_executable = false;
	let settled = render_result(
		Some(&args("run.sh", "#!/bin/sh\necho hi")),
		Some(&details),
		false,
		false,
		"writing…",
		false,
	);
	assert!(!settled.iter().any(|row| row == "writing…"));
}
