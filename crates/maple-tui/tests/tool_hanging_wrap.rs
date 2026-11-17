//! Port of `l/test/tool-hanging-wrap.test.ts`.
//!
//! The reference exercises hanging/guide preservation through high-level tool
//! components and rendered tree rows. Rust exposes the underlying helpers in
//! `render::hanging_wrap`, so this asserts the same prefix/continuation and
//! guide-column invariants against that public API.

use maple_tui::render::hanging_wrap::{wrap_guided_line, wrap_prefixed_line, wrap_tree_line};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn theme() -> Theme {
	create_theme(builtin_theme_json("dark").expect("dark theme json"), CreateThemeOptions::default())
		.expect("dark theme")
}

#[test]
fn prefixed_wrap_keeps_first_prefix_and_hangs_continuations() {
	let rows = wrap_prefixed_line("> ", "  ", "hello world wraps onto another line", 12);
	assert!(rows.len() >= 2, "{rows:?}");
	assert!(rows[0].starts_with("> "), "{rows:?}");
	for row in rows.iter().skip(1) {
		assert!(row.starts_with("  "), "{row:?}");
	}
	for row in &rows {
		assert!(visible_width(row) <= 12, "{row:?} width={}", visible_width(row));
	}
}

#[test]
fn prefixed_wrap_falls_back_when_width_too_narrow_for_columns() {
	let rows = wrap_prefixed_line(">>>> ", ".... ", "content", 4);
	assert!(!rows.is_empty());
	// Narrow width: helper falls back to wrapping the joined string.
	assert!(rows.join("\n").contains("content"));
}

#[test]
fn tree_wrap_preserves_branch_guide_on_continuations() {
	let theme = theme();
	let branch = theme.symbol("tree.branch");
	let label = format!("   {branch} this is a long branch label that must wrap inside the tree");
	let rows = wrap_tree_line(&label, 24, &theme).expect("tree line wraps");
	assert!(rows.len() >= 2, "{rows:?}");
	assert!(rows[0].contains(branch), "{rows:?}");
	// Continuation keeps ancestor/guide indentation rather than returning to column 0.
	let cont = &rows[1];
	assert!(cont.starts_with("   "), "{cont:?}");
	assert!(!cont.trim_start().is_empty(), "{cont:?}");
	assert!(visible_width(cont) <= 24 || cont.trim().len() <= 24, "{cont:?}");
}

#[test]
fn tree_wrap_preserves_last_connector_spacing() {
	let theme = theme();
	let last = theme.symbol("tree.last");
	let label = format!("   {last} final child with a long wrapped description text");
	let rows = wrap_tree_line(&label, 22, &theme).expect("last branch wraps");
	assert!(rows.len() >= 2, "{rows:?}");
	assert!(rows[0].contains(last), "{rows:?}");
	for row in rows.iter().skip(1) {
		assert!(row.starts_with("   "), "{row:?}");
	}
}

#[test]
fn guided_wrap_hangs_detail_rows_beneath_leading_guide_prefix() {
	let theme = theme();
	let line = "  detail content that is long enough to wrap beneath its guide";
	let rows = wrap_guided_line(line, 20, &theme);
	assert!(rows.len() >= 2, "{rows:?}");
	for row in &rows {
		assert!(row.starts_with("  "), "{row:?}");
		assert!(visible_width(row) <= 20 || row.trim().len() <= 20, "{row:?}");
	}
}

#[test]
fn guided_wrap_prefers_tree_connector_handling_when_present() {
	let theme = theme();
	let branch = theme.symbol("tree.branch");
	let line = format!("│  {branch} a wrapped tree child line under an ancestor rail");
	let guided = wrap_guided_line(&line, 26, &theme);
	let tree = wrap_tree_line(&line, 26, &theme).expect("tree form");
	assert_eq!(guided, tree);
}
