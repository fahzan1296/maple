//! Port of `l/test/expanded-reveals-text.test.ts`.
//!
//! The reference drives tool-result renderers (`renderLspResult`,
//! `renderDefaultToolExecution`, `renderTaskResult`, `debugToolRenderer`,
//! `renderMCPResult`, `webSearchToolRenderer`, `yieldToolRenderer`) that the
//! crate does not expose under those names. The shared contract they defend —
//! a collapsed view hides the tail of a long row, an expanded view shows every
//! character wrapped to the frame, and the expand hint appears only when
//! something is hidden — is ported against the crate's bounded output pane
//! (`format_output_pane_lines`) and `format_expand_hint`.

use maple_tui::render::output_pane::{format_output_pane_lines, OutputPaneFormatOptions};
use maple_tui::render::render_utils::{
	format_expand_hint, get_tree_continue_prefix, replace_tabs, sanitize_display_lines, TAB_WIDTH,
};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};
use maple_tui::theme::theme_class::Theme;

const WIDTH: usize = 100;
const MARK: &str = "ENDMARK";
const LONG: &str = "wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content wide-content ENDMARK";

fn theme() -> Theme {
	let json = builtin_theme_json("dark").expect("dark theme json");
	create_theme(json, CreateThemeOptions::default()).expect("dark theme builds")
}

fn marks(text: &str) -> usize {
	text.split(MARK).count() - 1
}

fn pane_lines(theme: &Theme, lines: &[&str], expanded: bool, collapsed_max_lines: usize) -> Vec<String> {
	let owned: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
	let options = OutputPaneFormatOptions {
		lines: &owned,
		expanded,
		collapsed_max_lines,
		expanded_max_lines: None,
		edge: None,
		visual: true,
		width: Some(WIDTH),
		style_line: None,
		uncap_sixel: false,
		show_hidden_marker: None,
		show_expand_hint: None,
		show_expand_hint_when_uncapped: None,
		format_hidden: None,
	};
	format_output_pane_lines(&options, theme).lines
}

#[test]
fn collapsed_output_hides_the_tail_once_expanded_it_is_whole() {
	let theme = theme();
	let collapsed = pane_lines(&theme, &[LONG], false, 1);
	assert_eq!(marks(&collapsed.join("\n")), 0, "collapsed view leaks the tail");
	assert!(collapsed.iter().all(|row| visible_width(row) <= WIDTH));

	let expanded = pane_lines(&theme, &[LONG], true, 1);
	assert_eq!(marks(&expanded.join("\n")), 1, "expanded view drops the tail");
	assert!(
		expanded.iter().all(|row| visible_width(row) <= WIDTH),
		"expanded rows exceed the frame width"
	);
}

#[test]
fn expanded_output_reveals_every_mark_of_a_multi_section_result() {
	let theme = theme();
	let section = format!("{MARK} {}", LONG);
	let lines = [section.as_str(), section.as_str(), section.as_str()];

	let collapsed = pane_lines(&theme, &lines, false, 1);
	assert_eq!(marks(&collapsed.join("\n")), 0);

	let expanded = pane_lines(&theme, &lines, true, 1);
	assert_eq!(marks(&expanded.join("\n")), 3);
}

#[test]
fn expanded_view_lists_every_reference_location_without_a_more_marker() {
	let theme = theme();
	let references: Vec<String> = (1..=5)
		.map(|index| format!("src/a.ts:{index}:1"))
		.collect();
	let borrowed: Vec<&str> = references.iter().map(String::as_str).collect();

	let collapsed = pane_lines(&theme, &borrowed, false, 2);
	assert!(collapsed.join("\n").contains("more"));
	assert!(!collapsed.join("\n").contains("src/a.ts:5:1"));

	let expanded = pane_lines(&theme, &borrowed, true, 2);
	let text = expanded.join("\n");
	assert!(text.contains("src/a.ts:5:1"), "expanded view drops a reference");
	assert!(!text.contains("more"), "expanded view still advertises hidden rows");
}

#[test]
fn expand_hint_appears_only_when_the_collapsed_view_hides_something() {
	let theme = theme();
	let one = pane_lines(&theme, &["src/a.ts:1:1"], false, 2);
	assert_eq!(one.join("\n").matches("Expand").count(), 0);

	let three = pane_lines(
		&theme,
		&["src/a.ts:1:1", "src/a.ts:2:1", "src/a.ts:3:1"],
		false,
		2,
	);
	assert!(three.join("\n").contains("Expand"));

	let long = pane_lines(&theme, &[LONG], false, 1);
	assert!(long.join("\n").contains("Expand"));

	// The hint helper itself: already expanded or nothing more to show.
	assert!(format_expand_hint(&theme, Some(true), Some(true)).is_empty());
	assert!(format_expand_hint(&theme, Some(false), Some(false)).is_empty());
	assert!(format_expand_hint(&theme, Some(false), Some(true)).contains("Expand"));
}

#[test]
fn debug_rows_sanitize_tabs_out_of_model_args() {
	for (raw, fragments) in [
		("bin\tary", ["bin", "ary"]),
		("run\ttests", ["run", "tests"]),
	] {
		let rows = sanitize_display_lines(raw);
		assert_eq!(rows.len(), 1);
		assert!(!rows[0].contains('\t'), "tab survives: {}", rows[0]);
		for fragment in fragments {
			assert!(rows[0].contains(fragment), "fragment lost: {}", rows[0]);
		}
	}
	assert_eq!(replace_tabs("a\tb"), format!("a{}b", " ".repeat(TAB_WIDTH)));
}

#[test]
fn debug_header_sanitizes_tabs_and_osc_title_sequences() {
	let action = "launch\t\u{1b}]0;pwned\u{7}X";
	let header = format!("Debug {action}");

	let rendered = maple_tui::strip_ansi(&replace_tabs(&header));

	assert!(!rendered.contains('\t'), "tab survives: {rendered}");
	assert!(
		!rendered.contains("\u{1b}]0;pwned"),
		"raw OSC bytes survive: {rendered}"
	);
	assert!(rendered.contains("Debug launch"), "action lost: {rendered}");
	assert!(rendered.contains('X'), "trailing text lost: {rendered}");
}

#[test]
#[ignore = "missing facility: the crate has no LSP result renderer that adds a tree rail to wrapped detail rows"]
fn expanded_lsp_detail_rows_keep_tree_rail() {
	// The rail primitive the renderer would compose exists; the LSP detail
	// renderer itself is not ported.
	let theme = theme();
	let _ = get_tree_continue_prefix(false, &theme);
}
