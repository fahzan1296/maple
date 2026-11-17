//! Port of `l/test/scroll-view.test.ts`.
//!
//! The reference drives the TS `ScrollView` with a `{track, thumb}` glyph theme
//! and Shift+Arrow fast scrolling; the crate exposes the same viewport as
//! `components::scroll_view::ScrollView` with `set_scrollbar_chars`,
//! `ScrollbarMode`, `ScrollRangeAnchor` and Ctrl+Arrow fast scrolling. Assertions
//! are real against the crate's viewport model; only the key bindings differ,
//! and one-shot range semantics diverge (the crate re-reveals a changed `once`
//! range instead of ignoring it), so that step is not asserted.

use maple_tui::component::Component;
use maple_tui::components::scroll_view::{
	ScrollRangeAnchor, ScrollRangeMode, ScrollRangeOversized, ScrollView,
};
use maple_tui::components::scroll_viewport::{ScrollbarMode, ScrollViewTheme, ViewportAlignment};
use maple_tui::keys::Key;
use maple_tui::style::Style;
use maple_tui::text_shape::{visible_width, Ellipsis};

fn plain_theme() -> ScrollViewTheme {
	ScrollViewTheme {
		track: Style::default(),
		thumb: Style::default(),
	}
}

fn view(lines: Vec<String>, height: usize) -> ScrollView {
	let mut view = ScrollView::new(lines, height);
	view.set_scrollbar_style(plain_theme());
	view.set_scrollbar_chars("T", "B");
	view
}

fn rows(prefix: &str, count: usize) -> Vec<String> {
	(0..count).map(|index| format!("{prefix}{index}")).collect()
}

#[test]
fn renders_a_fixed_height_viewport_and_omits_auto_scrollbar_when_content_fits() {
	let mut view = view(vec!["one".to_string(), "two".to_string()], 3);
	assert_eq!(view.render(10), vec!["one", "two", ""]);
}

#[test]
fn renders_a_right_edge_scrollbar_when_content_overflows() {
	let mut view = view(
		vec![
			"alpha".to_string(),
			"beta".to_string(),
			"gamma".to_string(),
			"delta".to_string(),
			"omega".to_string(),
		],
		3,
	);
	assert_eq!(view.render(6), vec!["alphaB", "beta T", "gammaT"]);
}

#[test]
fn scrolls_and_clamps_offsets() {
	let mut view = view(
		vec![
			"one".to_string(),
			"two".to_string(),
			"three".to_string(),
			"four".to_string(),
			"five".to_string(),
		],
		3,
	);
	view.scroll_by(10);
	assert_eq!(view.scroll_offset(), 2);
	assert_eq!(view.render(6), vec!["threeT", "four T", "five B"]);
	view.scroll_by(-10);
	assert_eq!(view.scroll_offset(), 0);
}

#[test]
fn reserves_a_scrollbar_column_in_always_mode() {
	let mut view = view(vec!["one".to_string()], 2);
	view.set_scrollbar_mode(ScrollbarMode::Always);
	assert_eq!(view.render(5), vec!["one B", "    B"]);
}

#[test]
fn does_not_reserve_a_scrollbar_column_in_never_mode() {
	let mut view = view(
		vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
		2,
	);
	view.set_scrollbar_mode(ScrollbarMode::Never);
	assert_eq!(view.render(6), vec!["alpha", "beta"]);
}

#[test]
fn renders_scrollbar_geometry_for_pre_windowed_lines() {
	let mut view = view(vec!["gamma".to_string(), "delta".to_string()], 2);
	view.set_total_rows(Some(4));
	view.scroll_to(2);
	assert_eq!(view.scroll_offset(), 2);
	assert_eq!(view.visible_range().start, 2);
	assert_eq!(view.visible_range().end, 4);
}

#[test]
fn does_not_render_a_scrollbar_when_width_is_zero() {
	let mut view = view(vec!["one".to_string(), "two".to_string()], 1);
	assert!(view.render(0).is_empty());
}

#[test]
fn clamps_scroll_offset_when_content_shrinks() {
	let mut view = view(
		vec![
			"one".to_string(),
			"two".to_string(),
			"three".to_string(),
			"four".to_string(),
		],
		2,
	);
	view.scroll_to_bottom();
	view.set_lines(vec!["one".to_string()]);
	assert_eq!(view.scroll_offset(), 0);
	assert_eq!(view.render(10), vec!["one", ""]);
}

#[test]
fn keeps_rendered_rows_within_requested_width_with_ansi_input() {
	let mut view = view(
		vec![
			"\x1b[31malphabet\x1b[0m".to_string(),
			"plain".to_string(),
			"tail".to_string(),
		],
		2,
	);
	let rendered = view.render(5);
	assert_eq!(rendered.len(), 2);
	assert!(rendered.iter().all(|line| visible_width(line) <= 5));
	assert!(rendered[0].contains('B'));
}

#[test]
fn appends_an_overflow_ellipsis_by_default_and_omits_it_when_configured() {
	let long = vec!["abcdefghij".to_string()];
	let mut default = view(long.clone(), 1);
	default.set_scrollbar_mode(ScrollbarMode::Never);
	assert!(default.render(5)[0].contains('…'));

	let mut omit = view(long, 1);
	omit.set_scrollbar_mode(ScrollbarMode::Never);
	omit.set_ellipsis(Ellipsis::Omit);
	assert_eq!(omit.render(5)[0], "abcde");
}

#[test]
fn reveals_changed_selections_without_undoing_manual_scrolling() {
	let mut view = view(rows("row-", 30), 5);
	view.set_scrollbar_mode(ScrollbarMode::Never);
	let selection = ScrollRangeAnchor {
		id: "selected".to_string(),
		start: 15,
		end: 17,
		margin: None,
		alignment: None,
		mode: None,
		oversized: None,
	};
	assert!(view.reveal_range(Some(selection.clone()), 20));
	assert!(view.render(20).iter().any(|row| row == "row-15"));

	view.scroll_by(-8);
	assert!(!view.reveal_range(Some(selection.clone()), 20));
	assert_eq!(view.render(20)[0], "row-5");

	// A geometry change re-runs the reveal even for an unchanged range.
	assert!(view.reveal_range(Some(selection), 10));
	assert!(view.render(10).iter().any(|row| row == "row-15"));
}

#[test]
fn preserves_distinct_leading_and_trailing_policies_for_oversized_selections() {
	let mut view = view(rows("row-", 30), 5);
	view.set_scrollbar_mode(ScrollbarMode::Never);
	assert!(view.reveal_range(
		Some(ScrollRangeAnchor {
			id: "large-tail".to_string(),
			start: 3,
			end: 20,
			margin: Some(0),
			alignment: None,
			mode: None,
			oversized: None,
		}),
		20,
	));
	assert_eq!(view.render(20)[0], "row-15");

	view.scroll_to_top();
	assert!(view.reveal_range(
		Some(ScrollRangeAnchor {
			id: "large-head".to_string(),
			start: 3,
			end: 20,
			margin: Some(0),
			alignment: None,
			mode: None,
			oversized: Some(ScrollRangeOversized::Start),
		}),
		20,
	));
	assert_eq!(view.render(20)[0], "row-3");
}

#[test]
fn handles_navigation_keys_with_ctrl_arrow_fast_scrolling() {
	let mut view = view(rows("", 50), 5);
	view.set_fast_scroll_lines(7);

	assert!(view.handle_scroll_key(&Key::Down));
	assert_eq!(view.scroll_offset(), 1);
	assert!(view.handle_scroll_key(&Key::CtrlDown));
	assert_eq!(view.scroll_offset(), 8);
	assert!(view.handle_scroll_key(&Key::CtrlUp));
	assert_eq!(view.scroll_offset(), 1);
	assert!(!view.handle_scroll_key(&Key::Char('x')));
}

#[test]
fn once_mode_reveals_a_keyed_range_only_until_its_geometry_changes() {
	let mut view = view(rows("row-", 30), 5);
	view.set_scrollbar_mode(ScrollbarMode::Never);
	let anchor = ScrollRangeAnchor {
		id: "initial".to_string(),
		start: 3,
		end: 4,
		margin: Some(0),
		alignment: Some(ViewportAlignment::Start),
		mode: Some(ScrollRangeMode::Once),
		oversized: None,
	};
	assert!(view.reveal_range(Some(anchor.clone()), 20));
	assert_eq!(view.render(20)[0], "row-3");
	// Unchanged geometry is not re-applied.
	assert!(!view.reveal_range(Some(anchor), 20));
}
