//! Port of `l/test/apply-patch-preview-render.test.ts`.
//!
//! Deviations: the reference drives the whole framed edit card through
//! `editToolRenderer.renderCall`; the crate splits that surface —
//! [`render_per_file_diff_preview`] ports the streaming multi-file preview body
//! (`formatMultiFileStreamingDiff`), while [`render_call`] keeps the one-line
//! head. The reference's viewport tail-slicing is left to the caller.

use maple_tui::theme::default_theme;
use maple_tui::tools::edit::{PerFileDiffPreview, render_per_file_diff_preview};

fn preview(path: &str, diff: &str) -> PerFileDiffPreview {
	PerFileDiffPreview {
		path: path.to_string(),
		diff: Some(diff.to_string()),
		first_changed_line: Some(1),
		error: None,
	}
}

fn render(previews: &[PerFileDiffPreview]) -> String {
	let theme = default_theme();
	maple_tui::strip_ansi(&render_per_file_diff_preview(previews, 160, false, &theme).join("\n"))
}

#[test]
fn renders_each_file_from_structured_per_file_diff_previews() {
	let rendered = render(&[
		preview(
			"src/a.ts",
			"@@ -1,1 +1,1 @@\n-1|old a\n+1|new a",
		),
		preview(
			"src/b.ts",
			"@@ -2,1 +2,1 @@\n-2|old b\n+2|new b",
		),
	]);

	assert!(rendered.contains("src/a.ts"), "header for first file");
	assert!(rendered.contains("new a"), "added row from first file");
	assert!(rendered.contains("src/b.ts"), "header for second file");
	assert!(rendered.contains("new b"), "added row from second file");
	assert!(rendered.contains("──"), "streaming file separators");
}

#[test]
fn renders_a_structured_per_file_preview_error_beside_successful_siblings() {
	let mut missing = preview("src/missing.ts", "");
	missing.diff = None;
	missing.error = Some("File not found: src/missing.ts".to_string());
	let rendered = render(&[preview("src/good.ts", "@@ -1,1 +1,1 @@\n-1|old\n+1|new"), missing]);

	assert!(rendered.contains("src/good.ts"));
	assert!(rendered.contains("new"));
	assert!(rendered.contains("src/missing.ts"));
	assert!(
		rendered.contains("File not found: src/missing.ts"),
		"the failed preview's error must render beside its siblings"
	);
}
