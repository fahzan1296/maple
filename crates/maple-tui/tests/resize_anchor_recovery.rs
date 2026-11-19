//! Port of `l/test/resize-anchor-recovery.test.ts`.
//!
//! The reference drives kitty's WASM VT engine directly (resize, CPR probe,
//! scrollback) and checks the anchor-recovery formula `top = min(R, height -
//! staleReflowedRows)`. The crate has no VT engine, CPR probe or scrollback, so
//! every assertion is ignored; the bodies pin the headless render seam.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(40, 12)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no kitty VT engine, CPR probe or scrollback in the crate"]
fn height_shrink_with_blank_rows_below_the_viewport() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no VT engine resize/CPR round trip in the crate"]
fn recovery_formula_on_full_screen_height_shrink() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no VT engine width+height resize in the crate"]
fn recovery_formula_on_combined_width_height_resize() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no VT engine width-grow unwrap in the crate"]
fn recovery_formula_on_width_grow_unwrap() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no multi-step drag or alt-screen borrow in the crate"]
fn recovery_formula_across_a_multi_step_drag_with_an_alt_screen_borrow() {
	render_headless();
}
