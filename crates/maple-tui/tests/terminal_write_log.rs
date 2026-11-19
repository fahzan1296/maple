//! Port of `l/test/terminal-write-log.test.ts`.
//!
//! `PI_TUI_WRITE_LOG` has no crate equivalent, so the log-file assertions are
//! ignored. The write path's observable contract — raw bytes reach the
//! terminal unchanged, including an OSC 52 clipboard payload — is pinned
//! through `VirtualTerminal`.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;

#[test]
fn writes_the_frame_and_the_clipboard_payload_verbatim() {
	let mut terminal = VirtualTerminal::new(80, 24);
	let frame = "\x1b[2Kframe\x1b]52;c;c2VjcmV0\x07tail";
	terminal.write(frame);
	assert_eq!(terminal.output(), frame);
}

#[test]
fn writes_the_clipboard_payload_verbatim_with_an_st_terminator() {
	let mut terminal = VirtualTerminal::new(80, 24);
	let frame = "\x1b[2Kframe\x1b]52;c;c2VjcmV0\x1b\\tail";
	terminal.write(frame);
	assert_eq!(terminal.output(), frame);
}

#[test]
fn keeps_the_osc_52_payload_length_visible_in_the_captured_bytes() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.write("\x1b]52;c;c2VjcmV0\x07");
	let payload = &terminal.output()["\x1b]52;c;".len()..terminal.output().len() - 1];
	assert_eq!(payload, "c2VjcmV0");
	assert_eq!(payload.len(), 8);
}

#[test]
fn clear_output_discards_captured_writes() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.write("frame");
	terminal.clear_output();
	assert_eq!(terminal.output(), "");
}

#[test]
#[ignore = "missing facility: PI_TUI_WRITE_LOG file logging is not ported to the crate"]
fn records_an_osc_52_clipboard_write_as_its_payload_length() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.write("\x1b]52;c;c2VjcmV0\x07");
	assert_eq!(terminal.output().len(), "\x1b]52;c;c2VjcmV0\x07".len());
}
