//! Port of `l/test/sixel-block-repaint.test.ts`.
//!
//! The reference paints real sixels through `VirtualTerminal` + native
//! `encodeSixelAsync`, tracking raster rows and stale paints across frames. The
//! crate's `render::sixel` and `components::image` are pure encoders with no
//! terminal raster tracking or native image surface, so all cases stay ignored;
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
#[ignore = "missing facility: no raster paint tracking or native sixel encoder in the crate"]
fn erases_a_moved_blocks_previous_raster_before_painting_it_again() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no top-clipping raster budget in the crate"]
fn keeps_a_real_image_within_its_3_visible_rows_after_top_clipping() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no top-clipping raster budget in the crate"]
fn keeps_a_real_image_within_its_2_visible_rows_after_top_clipping() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no top-clipping raster budget in the crate"]
fn keeps_a_real_image_within_its_1_visible_row_after_top_clipping() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no history-row raster separation in the crate"]
fn keeps_a_top_clipped_blocks_raster_out_of_the_history_rows_written_above_it() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no unparseable-sixel crop fallback in the crate"]
fn drops_a_top_clipped_sixel_the_cropper_cannot_parse_instead_of_failing_the_frame() {
	render_headless();
}
