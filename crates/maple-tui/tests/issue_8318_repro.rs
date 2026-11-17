//! Port of `l/test/issue-8318-repro.test.ts`.
//!
//! Deviations:
//! - The reference drives a full TUI render scheduler and resize settle path
//!   (Bun timers + `VirtualTerminal`); the crate's `Screen` has no OSC 66
//!   spacer-aware repaint yet. The pure reservation math and the spacer rewrite
//!   sequence are ported here, plus a `Screen` pass-through check.

use maple_tui::screen::{
	is_osc66_line, osc66_max_scale, osc66_spacer_glyph_width, spacer_row_rewrite, Screen,
};
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::text_shape::visible_width;

const OSC66: &str = "\x1b]66;";
const ST: &str = "\x1b\\";

fn heading(scale: u32, text: &str) -> String {
	format!("{OSC66}s={scale};{text}{ST}")
}

#[test]
fn recognizes_osc66_lines_and_scales() {
	assert!(is_osc66_line(&heading(2, "Heading")));
	assert!(!is_osc66_line("plain text"));
	assert_eq!(osc66_max_scale(&heading(2, "Heading")), 2);
	assert_eq!(osc66_max_scale(&heading(3, "Big")), 3);
	assert_eq!(osc66_max_scale(&heading(7, "Max")), 7);
	assert_eq!(osc66_max_scale(&format!("{OSC66}Plain{ST}")), 1);
	// Scales above the legal maximum are ignored.
	assert_eq!(osc66_max_scale(&heading(9, "TooBig")), 1);
}

#[test]
fn finds_the_reserved_lower_rows_of_a_scaled_heading() {
	let lines = vec![heading(2, "Heading"), String::new(), "Body".to_string()];
	assert_eq!(osc66_spacer_glyph_width(&lines, 1), visible_width(&lines[0]) as i64);
	assert_eq!(osc66_spacer_glyph_width(&lines, 1), 7);
	assert_eq!(osc66_spacer_glyph_width(&lines, 2), -1);

	let deep = vec![
		heading(3, "Big"),
		String::new(),
		String::new(),
		"Body".to_string(),
	];
	assert_eq!(osc66_spacer_glyph_width(&deep, 1), 3);
	assert_eq!(osc66_spacer_glyph_width(&deep, 2), 3);
	assert_eq!(osc66_spacer_glyph_width(&deep, 3), -1);

	// A blank run longer than `scale - 1` does not belong to the heading.
	let too_far = vec![heading(2, "Heading"), String::new(), String::new(), "X".to_string()];
	assert_eq!(osc66_spacer_glyph_width(&too_far, 1), 7);
	assert_eq!(osc66_spacer_glyph_width(&too_far, 2), -1);

	// The heading row itself and index zero are never spacers.
	assert_eq!(osc66_spacer_glyph_width(&lines, 0), -1);
}

#[test]
fn rewrite_preserves_the_glyph_columns_and_erases_only_to_their_right() {
	assert_eq!(spacer_row_rewrite(14, 80), "\x1b[0m\x1b[14C\x1b[K");
	assert_eq!(spacer_row_rewrite(14, 14), "");
	assert_eq!(spacer_row_rewrite(14, 5), "");
	assert_eq!(spacer_row_rewrite(-1, 80), "");
}

#[test]
fn screen_passes_a_scaled_heading_through_untouched() {
	let mut terminal = VirtualTerminal::new(80, 6);
	let mut screen = Screen::new();
	let rows = vec![
		heading(2, "Heading"),
		String::new(),
		"Body".to_string(),
	];
	screen.flush(&mut terminal, &rows, 80);
	let painted = terminal.output();
	assert!(painted.contains("Heading"), "painted: {painted:?}");
	assert!(painted.contains("Body"));
	assert_eq!(screen.rows().len(), 3);
}