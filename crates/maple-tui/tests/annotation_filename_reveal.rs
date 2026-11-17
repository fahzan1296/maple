//! Port of `l/test/annotation-filename-reveal.test.ts`.
//!
//! The reference floats a multi-row filename reveal over the diff body while
//! the file sidebar has focus. `reveal_selected_file` ports that overlay; the
//! scenarios drive it directly with a bordered canvas.

use maple_tui::overlays::annotation_overlay::{
	FocusRegion, reveal_selected_file, sidebar_badge, sidebar_label_width, sidebar_rows, sidebar_width,
};
use maple_tui::overlays::annotation_types::ReviewDiffFile;
use maple_tui::strip_ansi;
use maple_tui::text_shape::{pad_to_width, visible_width};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};

fn theme() -> Theme {
	create_theme(&builtin_theme_json("dark").expect("dark theme json"), CreateThemeOptions::default())
		.expect("dark theme")
}

fn file(path: &str) -> ReviewDiffFile {
	ReviewDiffFile {
		path: path.to_string(),
		..ReviewDiffFile::default()
	}
}

/// Bordered canvas: row 0 is the frame, rows 1.. are body rows.
fn canvas(width: usize, body: &[&str]) -> Vec<String> {
	let mut out = vec![pad_to_width("┌", width)];
	for line in body {
		out.push(pad_to_width(line, width));
	}
	out
}

fn reveal(
	out: &mut Vec<String>,
	files: &[ReviewDiffFile],
	index: usize,
	focus: FocusRegion,
	width: usize,
	body_height: usize,
	theme: &Theme,
) {
	reveal_selected_file(out, files, index, focus, false, true, body_height, width, theme);
}

fn plain(out: &[String]) -> Vec<String> {
	out.iter().map(|line| strip_ansi(line)).collect()
}

#[test]
fn shows_full_selected_filepath_past_truncated_sidebar_label() {
	let path = format!("src/{}selected-file.ts", "deep-module/".repeat(5));
	let files = vec![file(&path)];
	let mut out = canvas(90, &["body row"]);
	reveal(&mut out, &files, 0, FocusRegion::Files, 90, 40, &theme());
	assert!(plain(&out).iter().any(|line| line.contains(&path)));
}

#[test]
fn reveals_exactly_fitting_filename_when_note_reduces_label_budget() {
	let path = "src/boundaries.ts";
	let width = 90;
	assert_eq!(sidebar_width(width), 25);
	let mut annotated = file(path);
	annotated.annotation_count = 1;
	let bare = file(path);
	assert_eq!(sidebar_label_width(&bare, 0, sidebar_width(width), &theme()), 17);
	assert_eq!(visible_width(&sidebar_badge(&bare, 0, &theme())), 6);
	assert_eq!(sidebar_label_width(&annotated, 1, sidebar_width(width), &theme()), 13);

	let mut out = canvas(width, &["body row"]);
	reveal(&mut out, &[bare.clone()], 0, FocusRegion::Files, width, 40, &theme());
	assert!(!plain(&out).iter().any(|line| line.contains(path)));

	let mut out = canvas(width, &["body row"]);
	reveal(&mut out, &[annotated.clone()], 0, FocusRegion::Files, width, 40, &theme());
	assert!(plain(&out).iter().any(|line| line.contains(path)));

	let rows = sidebar_rows(&[annotated], 0, FocusRegion::Files, 2, sidebar_width(width), &theme());
	let row = strip_ansi(&rows[0]);
	assert!(row.contains("…"));
	assert!(row.contains("✎1"));
}

#[test]
fn keeps_file_rows_stable_while_selection_moves() {
	let path = format!("src/{}selected-file.ts", "deep-module/".repeat(5));
	let files = vec![file(&path), file("second.ts"), file("third.ts")];
	let mut out = canvas(90, &["second.ts", "third.ts"]);
	let before = out.len();
	reveal(&mut out, &files, 1, FocusRegion::Files, 90, 40, &theme());
	assert_eq!(out.len(), before);
	assert!(plain(&out).iter().any(|line| line.contains("second.ts")));
	assert!(!plain(&out).iter().any(|line| line.contains(&path)));
}

#[test]
fn removes_floating_reveal_when_file_focus_exits() {
	let path = format!("src/{}selected-file.ts", "deep-module/".repeat(5));
	let files = vec![file(&path)];
	let mut out = canvas(90, &["body row"]);
	reveal(&mut out, &files, 0, FocusRegion::Files, 90, 40, &theme());
	assert!(plain(&out).iter().any(|line| line.contains(&path)));

	let mut out = canvas(90, &["body row"]);
	reveal(&mut out, &files, 0, FocusRegion::Diff, 90, 40, &theme());
	assert!(!plain(&out).iter().any(|line| line.contains(&path)));
}

#[test]
fn wraps_very_long_filepaths_without_overflowing() {
	let path = format!(
		"src/{}target.ts",
		(0..11).map(|i| format!("folder-{i:03}")).collect::<Vec<_>>().join("/")
	);
	let width = 65;
	let files = vec![file(&path)];
	let body = vec!["body row"; 41];
	let mut out = canvas(width, &body.iter().map(String::as_str).collect::<Vec<_>>());
	reveal(&mut out, &files, 0, FocusRegion::Files, width, 40, &theme());

	let path_column = 4;
	let interior_width = width - path_column - 1;
	let reveal_rows = path.len().div_ceil(interior_width);
	let revealed: String = plain(&out)[1..1 + reveal_rows]
		.iter()
		.map(|line| line[path_column..width - 1].trim_end())
		.collect();
	assert!(revealed.contains(&path));
	assert!(plain(&out).iter().all(|line| visible_width(line) <= width));
}

#[test]
fn floats_bottom_selection_upward_without_changing_rows() {
	let width = 65;
	let path = format!("src/{}BOTTOM_FILENAME_TAIL.ts", "nested-directory/".repeat(7));
	let mut paths: Vec<String> = (0..80).map(|index| format!("file-{index}.ts")).collect();
	paths[79] = path.clone();
	let files: Vec<ReviewDiffFile> = paths.iter().map(|p| file(p)).collect();
	let body = vec!["body row"; 41];
	let mut out = canvas(width, &body.iter().map(String::as_str).collect::<Vec<_>>());
	let before = out.len();
	reveal(&mut out, &files, files.len() - 1, FocusRegion::Files, width, 40, &theme());

	let path_column = 4;
	let interior_width = width - path_column - 1;
	let reveal_rows = path.len().div_ceil(interior_width);
	let plain = plain(&out);
	let reveal_start = plain
		.iter()
		.position(|line| line[path_column..width - 1].starts_with("src/"))
		.expect("reveal start");
	assert_eq!(reveal_rows, 3);
	assert!(reveal_start > 1);
	let revealed: String = plain[reveal_start..reveal_start + reveal_rows]
		.iter()
		.map(|line| line[path_column..width - 1].trim_end())
		.collect();
	assert_eq!(revealed, path);
	assert!(plain.iter().any(|line| line.contains("BOTTOM_FILENAME_TAIL.ts")));
	assert_eq!(out.len(), before);
	assert!(plain.iter().all(|line| visible_width(line) <= width));
}

#[test]
fn marks_filepath_longer_than_frame_while_preserving_tail() {
	let width = 65;
	let path = format!("src/{}BOTTOM_FILENAME_TAIL.ts", "long-directory/".repeat(500));
	let files = vec![file(&path)];
	let body = vec!["body row"; 41];
	let mut out = canvas(width, &body.iter().map(String::as_str).collect::<Vec<_>>());
	reveal(&mut out, &files, 0, FocusRegion::Files, width, 40, &theme());
	let plain = plain(&out);
	assert!(plain.iter().any(|line| line[4..width - 1].starts_with('…')));
	assert!(plain.iter().any(|line| line.contains("BOTTOM_FILENAME_TAIL.ts")));
	assert!(plain.iter().all(|line| visible_width(line) <= width));
}

#[test]
fn keeps_wrapped_path_segments_separate_from_diff_text() {
	let path = format!("src/{}target.ts", "nested-path-segment/".repeat(6));
	let files = vec![file(&path)];
	let body = ["background-line-0", "background-line-1", "background-line-2"];
	let mut out = canvas(65, &body);
	reveal(&mut out, &files, 0, FocusRegion::Files, 65, 40, &theme());
	let plain = plain(&out);
	assert!(plain[1].contains("src/"));
	assert!(!plain[3].contains("background-line-1"));
	assert!(plain[4].contains("background-line-2"));
}
