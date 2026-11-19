//! Port of `l/test/apply-patch-preview-render.test.ts`.

use maple_tui::ansi_scan::strip_ansi;
use maple_tui::theme::default_theme;
use maple_tui::tools::edit::{render_per_file_diff_preview, PerFileDiffPreview};

fn preview(path: &str, diff: Option<&str>, error: Option<&str>, first_changed_line: Option<usize>) -> PerFileDiffPreview {
	PerFileDiffPreview {
		path: path.to_string(),
		diff: diff.map(str::to_string),
		first_changed_line,
		error: error.map(str::to_string),
	}
}

fn render(previews: &[PerFileDiffPreview]) -> String {
	strip_ansi(&render_per_file_diff_preview(previews, 160, false, &default_theme()).join("\n"))
}

#[test]
fn renders_each_file_from_structured_per_file_diff_previews() {
	let rendered = render(&[
		preview(
			"src/a.ts",
			Some("@@ -1,1 +1,1 @@\n-1|old a\n+1|new a"),
			None,
			Some(1),
		),
		preview(
			"src/b.ts",
			Some("@@ -2,1 +2,1 @@\n-2|old b\n+2|new b"),
			None,
			Some(2),
		),
	]);

	assert!(rendered.contains("src/a.ts"), "rows: {rendered:?}");
	assert!(rendered.contains("new a"), "rows: {rendered:?}");
	assert!(rendered.contains("src/b.ts"), "rows: {rendered:?}");
	assert!(rendered.contains("new b"), "rows: {rendered:?}");
}

#[test]
fn renders_a_structured_per_file_preview_error_beside_successful_siblings() {
	let rendered = render(&[
		preview(
			"src/good.ts",
			Some("@@ -1,1 +1,1 @@\n-1|old\n+1|new"),
			None,
			Some(1),
		),
		preview("src/missing.ts", None, Some("File not found: src/missing.ts"), None),
	]);

	assert!(rendered.contains("src/good.ts"), "rows: {rendered:?}");
	assert!(rendered.contains("new"), "rows: {rendered:?}");
	assert!(rendered.contains("src/missing.ts"), "rows: {rendered:?}");
	assert!(
		rendered.contains("File not found: src/missing.ts"),
		"rows: {rendered:?}"
	);
}

#[test]
fn skips_previews_without_a_diff_or_error_and_keeps_the_file_header() {
	let rendered = render(&[
		preview("src/unchanged.ts", None, None, None),
		preview("src/missing.ts", None, Some("File not found: src/missing.ts"), None),
	]);

	assert!(!rendered.contains("src/unchanged.ts"), "rows: {rendered:?}");
	assert!(rendered.contains("── src/missing.ts ──"), "rows: {rendered:?}");
	assert!(rendered.contains("File not found: src/missing.ts"), "rows: {rendered:?}");
	assert!(!rendered.contains("@@"), "an error-only preview draws no diff rows: {rendered:?}");
}