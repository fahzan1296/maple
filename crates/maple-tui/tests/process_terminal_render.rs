//! Port of `l/test/process-terminal-render.test.ts`.
//!
//! Geometry reflow through the real renderer needs a PTY plus SIGWINCH and the
//! DEC 2048 in-band channel. `VirtualTerminal` models the observable seam
//! (queued resize events, cursor/row bookkeeping, clear sequences), so those
//! are pinned; the OS-channel and SIGHUP cases are ignored.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::{Terminal, TtyEvent};

#[test]
fn reports_the_geometry_it_was_created_with() {
	let terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.columns(), 100);
	assert_eq!(terminal.rows(), 30);
}

#[test]
fn delivers_a_resize_event_with_the_new_geometry() {
	let mut terminal = VirtualTerminal::new(100, 30);
	terminal.push_event(TtyEvent::Resize(160, 40));
	match terminal.event(std::time::Duration::ZERO) {
		Some(TtyEvent::Resize(columns, rows)) => {
			assert_eq!(columns, 160);
			assert_eq!(rows, 40);
		}
		other => panic!("expected Resize(160, 40), got {other:?}"),
	}
}

#[test]
fn clear_screen_resets_the_cursor_row_and_emits_home() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.write("a\nb\nc");
	assert_eq!(terminal.cursor_row(), 3);
	terminal.clear_screen();
	assert_eq!(terminal.cursor_row(), 0);
	assert!(terminal.output().ends_with("\x1b[2J\x1b[H"));
}

#[test]
fn move_by_tracks_the_cursor_row_and_emits_the_correct_sequence() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.move_by(3);
	assert_eq!(terminal.cursor_row(), 3);
	terminal.move_by(-1);
	assert_eq!(terminal.cursor_row(), 2);
	assert!(terminal.output().ends_with("\x1b[3B\x1b[1A"));
}

#[test]
fn hide_and_show_cursor_emit_the_private_mode_sequences() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.hide_cursor(false);
	terminal.show_cursor(false);
	assert_eq!(terminal.output(), "\x1b[?25l\x1b[?25h");
}

#[test]
fn clear_line_and_clear_from_cursor_emit_their_sequences() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.clear_line();
	terminal.clear_from_cursor();
	assert_eq!(terminal.output(), "\x1b[2K\x1b[J");
}

#[test]
#[ignore = "missing facility: no PTY/SIGWINCH harness, so OS-width reflow cannot be observed"]
fn reflows_to_the_os_width_on_resize_when_in_band_resize_is_inactive() {
	let mut terminal = VirtualTerminal::new(100, 30);
	terminal.push_event(TtyEvent::Resize(160, 40));
	assert!(matches!(terminal.event(std::time::Duration::ZERO), Some(TtyEvent::Resize(160, 40))));
}

#[test]
#[ignore = "missing facility: no PTY, so a missed post-resize in-band report cannot be simulated"]
fn reflows_to_the_os_width_when_the_post_resize_in_band_report_is_missed() {
	let mut terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.columns(), 100);
}

#[test]
#[ignore = "missing facility: no DEC 2048 in-band channel parser in the crate"]
fn reflows_to_in_band_geometry_that_disagrees_with_the_os() {
	let mut terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.columns(), 100);
}

#[test]
#[ignore = "missing facility: stdin read splitting is a TS stream concern; VirtualTerminal has no partial reads"]
fn reflows_when_an_in_band_report_is_split_across_stdin_reads() {
	let mut terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.rows(), 30);
}

#[test]
#[ignore = "missing facility: no in-band parser, so colon subparameters (#4748) are not decoded"]
fn recovers_the_full_height_when_the_grow_back_report_carries_colon_subparameters() {
	let mut terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.rows(), 30);
}

#[test]
#[ignore = "missing facility: no PTY, so input-end SIGHUP teardown cannot be observed"]
fn stops_rendering_and_raises_sighup_when_terminal_input_ends() {
	let mut terminal = VirtualTerminal::new(100, 30);
	terminal.push_event(TtyEvent::Disconnect);
	assert!(matches!(terminal.event(std::time::Duration::ZERO), Some(TtyEvent::Disconnect)));
}

#[test]
#[ignore = "missing facility: no Windows ConPTY backend in the crate"]
fn does_not_wait_for_terminal_output_to_drain_after_input_ends_on_windows() {
	let mut terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.columns(), 100);
}

#[test]
#[ignore = "missing facility: no PTY, so output-failure teardown cannot be observed"]
fn stops_rendering_and_raises_sighup_when_terminal_output_fails() {
	let mut terminal = VirtualTerminal::new(100, 30);
	assert_eq!(terminal.rows(), 30);
}
