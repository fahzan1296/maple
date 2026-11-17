//! Port of `l/test/write-pending-path-render.test.ts` (link-target subset).
//!
//! The reference renders a pending write call card and inspects its OSC 8
//! target under VS Code/auto link settings. The crate exposes the URI builder
//! directly; these tests assert the same `file:`/`vscode://file` targets for
//! relative, absolute, UNC, and positioned paths.

use std::path::Path;

use maple_tui::render::hyperlink::{file_hyperlink, file_uri_for_terminal, FilePosition};

const RELATIVE: &str = ".tricky/reports/pending/interval.json";

fn resolved() -> String {
	std::env::current_dir()
		.unwrap_or_default()
		.join(RELATIVE)
		.to_string_lossy()
		.into_owned()
}

#[test]
fn links_a_relative_path_before_the_write_result_exists() {
	let uri = file_uri_for_terminal(RELATIVE, FilePosition::default(), "auto").expect("uri");
	assert!(uri.starts_with("file://"), "{uri}");
	let path = uri.trim_start_matches("file://");
	assert!(
		Path::new(path).ends_with(Path::new(RELATIVE)),
		"{uri} does not end with {RELATIVE}"
	);
}

#[test]
fn uses_the_absolute_filesystem_target_in_vs_code() {
	let uri = file_uri_for_terminal(RELATIVE, FilePosition::default(), "vscode").expect("uri");
	assert!(uri.starts_with("vscode://file"), "{uri}");
	assert!(uri.ends_with(&resolved()), "{uri} does not end with {}", resolved());
}

#[test]
fn keeps_absolute_and_unc_paths_intact() {
	let absolute = "/tmp/maple/notes.txt";
	let uri = file_uri_for_terminal(absolute, FilePosition::default(), "vscode").expect("uri");
	assert_eq!(uri, "vscode://file/tmp/maple/notes.txt");

	let unc = r"\\server\share\notes.txt";
	let uri = file_uri_for_terminal(unc, FilePosition::default(), "auto").expect("uri");
	assert_eq!(uri, "file://server/share/notes.txt");

	assert_eq!(file_uri_for_terminal("", FilePosition::default(), "auto"), None);
}

#[test]
fn appends_cursor_position_to_the_link_target() {
	let position = FilePosition {
		line: Some(12),
		col: Some(4),
	};
	let uri = file_uri_for_terminal("/tmp/a.rs", position, "vscode").expect("uri");
	assert_eq!(uri, "vscode://file/tmp/a.rs:12:4");
	let line_only = FilePosition {
		line: Some(7),
		col: None,
	};
	assert_eq!(
		file_uri_for_terminal("/tmp/a.rs", line_only, "auto").expect("uri"),
		"file:///tmp/a.rs"
	);
}

#[test]
fn wraps_display_text_in_an_osc8_link() {
	let text = file_hyperlink(RELATIVE, "interval.json", FilePosition::default());
	assert!(text.contains("interval.json"));
	assert!(text.contains("\x1b]8;"), "{text:?}");
}
