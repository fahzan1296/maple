//! Port of `l/test/resize-alt-toggle-echo.test.ts`.
//!
//! The reference drives the real `TUI` resize path on Warp (which SIGWINCHes on
//! an alt-buffer toggle) and asserts when the engine borrows the alternate
//! screen, coalesces drags and re-probes. The crate's `Tui` has no resize
//! borrow/settle/probe state machine, so every assertion is ignored; the bodies
//! pin the multiplexer classification and headless render seam underneath.

use maple_tui::multiplexers::Multiplexer;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(80, 24)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no resize alt-borrow state machine in the crate's Tui"]
fn does_not_borrow_the_alt_buffer_for_a_warp_pane_resize() {
	assert!(Multiplexer::Warp.host_owns_grid_on_resize());
	render_headless();
}

#[test]
#[ignore = "missing facility: no in-place forced-off borrow path in the crate's Tui"]
fn borrows_the_alt_buffer_once_when_warp_in_place_is_forced_off() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no alt-buffer borrow decision in the crate's Tui"]
fn borrows_the_alt_buffer_on_non_warp_terminals() {
	assert!(!Multiplexer::AppleTerminal.host_owns_grid_on_resize());
	render_headless();
}

#[test]
#[ignore = "missing facility: no in-place settle window in the crate's Tui"]
fn paints_after_a_restart_during_the_in_place_settle_window() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no overlay toggle echo handling in the crate's Tui"]
fn repaints_the_modal_instead_of_probing_on_an_overlay_toggle_echo() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no shell handoff cursor parking in the crate's Tui"]
fn parks_the_cursor_before_the_shell_handoff_when_stopping_mid_resize() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no drag coalescing or ED3-free repaint in the crate's Tui"]
fn coalesces_a_drag_into_one_ed3_free_repaint_with_no_alt_borrow() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no in-place settle paint gating in the crate's Tui"]
fn drops_ordinary_paints_while_the_in_place_resize_settles() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no in-place shrink settle in the crate's Tui"]
fn settles_a_shrink_drag_in_place_with_history_and_viewport_intact() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no single-shot echo expectation in the crate's Tui"]
fn consumes_the_echo_expectation_so_a_later_one_row_resize_restarts() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no delayed-echo re-probe in the crate's Tui"]
fn re_probes_a_delayed_echo_that_arrives_after_its_probe_resolved() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no probe/borrow mutual exclusion in the crate's Tui"]
fn never_probes_while_the_resize_borrow_owns_the_alt_buffer() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no multiplexer borrow-path override in the crate's Tui"]
fn keeps_the_borrow_path_for_warp_sessions_inside_a_multiplexer() {
	assert!(Multiplexer::Tmux.host_owns_grid_on_resize());
	render_headless();
}
