//! Port of `l/test/tool-execution-custom-repaint.test.ts`.
//!
//! The reference drives streamed/provisional tool output through the TUI's
//! injected frame provider, asserting when a viewport repaint is forced and when
//! placeholder rows are dropped from the terminal buffer. The crate has no chat
//! tool-execution runtime, frame provider or placeholder buffer, so all cases
//! stay ignored; the bodies exercise the headless render seam such a port would
//! reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no chat tool-execution frame provider in the crate"]
fn forces_a_viewport_repaint_when_a_painted_streamed_placeholder_receives_its_first_result() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no chat tool-execution frame provider in the crate"]
fn does_not_repaint_when_the_streamed_placeholder_never_reaches_the_terminal() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no streamed placeholder model in the crate"]
fn does_not_repaint_complete_args_on_the_first_result() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no provisional partial result model in the crate"]
fn forces_a_viewport_repaint_when_a_painted_provisional_partial_result_settles() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no provisional partial result model in the crate"]
fn does_not_repaint_when_the_provisional_partial_result_never_reaches_the_terminal() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no terminal buffer placeholder removal in the crate"]
fn removes_streamed_placeholder_rows_from_the_terminal_buffer_when_the_first_result_arrives() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no terminal buffer chrome removal in the crate"]
fn removes_provisional_partial_chrome_from_the_terminal_buffer_when_the_result_settles() {
	render_headless();
}
