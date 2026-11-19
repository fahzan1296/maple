//! Port of `l/test/resize-settle-fused-exit.test.ts`.
//!
//! The reference asserts the alternate-buffer enter/exit rides in the same
//! terminal write as the first resize frame and the settled rebuild. The crate's
//! `ProcessTerminal` has no alternate-buffer tracking, no resize settle window
//! and no replay provider, so all cases stay ignored; the bodies exercise the
//! headless render seam such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no borrowed alternate buffer or fused write batching in the crate"]
fn switches_to_the_borrowed_buffer_in_the_same_write_as_the_first_resize_frame() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no alternate-buffer restore or settled rebuild write in the crate"]
fn restores_the_normal_buffer_in_the_same_write_as_the_settled_rebuild() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no resize settle window or borrow resumption in the crate"]
fn resumes_the_borrow_when_a_resize_lands_before_the_fused_rebuild_is_written() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no fullscreen overlay opened during settling in the crate"]
fn hands_the_borrowed_buffer_to_a_fullscreen_overlay_opened_before_the_rebuild_is_written() {
	render_headless();
}
