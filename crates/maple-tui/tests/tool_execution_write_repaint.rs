//! Port of `l/test/tool-execution-write-repaint.test.ts`.
//!
//! The reference drives collapsed/expanded tail previews of a Write tool call,
//! asserting which states force a viewport repaint. The crate has no chat
//! tool-execution runtime or preview state machine, so all cases stay ignored;
//! the bodies exercise the headless render seam such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no collapsed tail-window preview in the crate"]
fn forces_a_viewport_repaint_when_a_painted_collapsed_tail_window_receives_its_first_result() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no pending tail-window preview in the crate"]
fn does_not_repaint_when_the_pending_tail_window_never_reaches_the_terminal() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no streaming-window preview fit model in the crate"]
fn does_not_repaint_a_collapsed_preview_that_fits_the_streaming_window() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no expanded pending preview model in the crate"]
fn does_not_repaint_an_expanded_pending_preview() {
	render_headless();
}
