//! Port of `l/test/terminal-oversized-frame.test.ts`.
//!
//! The reference asserts the render loop does not self-terminate when one frame
//! overflows the backlog cap. The crate's `Tui` has no render backlog or frame
//! budget, so the case stays ignored; the body exercises the headless render
//! seam such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

#[test]
#[ignore = "missing facility: no render backlog cap or frame budget in the crate's Tui"]
fn does_not_self_terminate_when_a_single_frame_overflows_the_backlog_cap() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}
