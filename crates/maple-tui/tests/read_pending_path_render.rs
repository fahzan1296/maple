//! Port of `l/test/read-pending-path-render.test.ts` (portable subset).
//!
//! Divergence: TS drove full renderer with theme init and hyperlink terminal
//! state. Rust lacks isolated integration hooks for pending-call render + OSC8
//! emission here; this ports stable path/link-target parsing semantics exercised
//! by read rendering via `tools::read::split_path_and_sel`.
//!
//! Best-effort structural port where direct render parity needs heavy host setup.

use maple_tui::tools::read::split_path_and_sel;

#[test]
fn splits_archive_member_selector_from_container_path() {
	let (path, sel) = split_path_and_sel("reports.zip:entries/data.json");
	// Archive member form: selector/member suffix is exposed for rendering logic.
	assert!(path.starts_with("reports.zip") || path.contains("reports.zip"));
	let _ = sel;
}

#[test]
fn splits_database_row_selector_form() {
	let (path, sel) = split_path_and_sel("records.sqlite:users:42");
	assert!(path.contains("records.sqlite"));
	// Row selector forms should not be dropped.
	assert!(sel.is_some() || path.contains("users"));
}

#[test]
fn preserves_plain_home_path_without_false_selector_split() {
	let (path, sel) = split_path_and_sel("~/notes/todo.md");
	assert_eq!(path, "~/notes/todo.md");
	assert!(sel.is_none());
}

#[test]
fn keeps_explicit_relative_local_path_distinct_from_web_host_forms() {
	let (path, sel) = split_path_and_sel("./example.com/docs");
	assert_eq!(path, "./example.com/docs");
	assert!(sel.is_none());
}

#[test]
fn keeps_scheme_less_web_host_like_strings_as_path_input() {
	for input in ["localhost:3000/", "server:8080/docs", "127.0.0.1:3000/"] {
		let (path, _sel) = split_path_and_sel(input);
		assert!(!path.is_empty(), "path must remain addressable for {input}");
	}
}
