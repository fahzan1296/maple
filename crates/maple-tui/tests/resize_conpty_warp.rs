//! Port of `l/test/resize-conpty-warp.test.ts`.
//!
//! Windows conhost/Windows Terminal overwrite the repaint region on resize;
//! the reference rebuilds the transcript, borrows the alternate buffer once and
//! drives a DSR probe. The crate's `ProcessTerminal` has no conhost detection,
//! alternate-buffer borrow tracking or DSR probe, so these stay ignored; the
//! bodies exercise the headless render seam such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no conhost overwrite detection or transcript rebuild in the crate"]
fn rebuilds_the_transcript_instead_of_anchoring_a_repaint_conhost_already_overwrote() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no alternate-buffer borrow tracking in the crate"]
fn borrows_the_alt_buffer_exactly_once_so_the_warp_toggle_echo_cannot_loop() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no DSR probe in the crate's terminal layer"]
fn keeps_the_dsr_probe_inside_a_multiplexer_which_answers_it_from_its_own_grid() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no PI_TUI_RESIZE_IN_PLACE in-place repaint mode in the crate"]
fn keeps_the_dsr_probe_when_pi_tui_resize_in_place_forces_in_place_repaint() {
	render_headless();
}
