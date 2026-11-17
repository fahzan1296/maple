//! Port of `l/test/resize-preserved-clear.test.ts`.
//!
//! The reference asserts that a height resize never erases live rows into
//! scrollback and never issues a screen-wide clear. The crate's `Tui` has no
//! scrollback model, erase strategy or cursor-forward-only single-column
//! erase, so these stay ignored; the bodies exercise the headless render seam
//! such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no scrollback model or erase strategy in the crate's Tui"]
fn keeps_live_rows_out_of_scrollback_when_a_height_grow_erases_a_screen_filling_viewport() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no screen-wide clear suppression in the crate's Tui"]
fn never_issues_a_screen_wide_clear_when_a_height_shrink_erases_a_screen_filling_viewport() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no anchored-viewport resize erase in the crate's Tui"]
fn keeps_a_short_viewport_anchored_after_a_height_shrink() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no cursor-forward-only erase path in the crate's Tui"]
fn keeps_the_erase_off_the_first_cell_at_a_one_column_viewport_where_cuf_cannot_move() {
	render_headless();
}
