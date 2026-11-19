//! Port of `l/test/inband-resize-introducer-split.test.ts` (portable subset).
//!
//! The reference feeds DEC 2048 in-band resize reports (`CSI 48;rows;cols;…t`)
//! through a real PTY, splits them at every field boundary, and asserts the
//! geometry applies while no fragment reaches the input handler. The crate has
//! no DEC 2048 parser, so geometry application has no counterpart; the portable
//! subset asserts the same stdin seam those cases exercise —
//! [`ProcessTerminal::feed_raw_stdin`] reassembles a report split at any byte
//! and never leaks a fragment as a keystroke.

use maple_tui::stdin_buffer::StdinBufferEvent;
use maple_tui::terminal::process::ProcessTerminal;

/// The reference report body: `CSI 48;rows;cols;yPx;xPx t`.
const REPORT_TAIL: &str = "[48;61;211;1837;3131t";
const REPORT: &str = "\x1b[48;61;211;1837;3131t";

fn data(sequence: &str) -> StdinBufferEvent {
	StdinBufferEvent::Data(sequence.to_string())
}

#[test]
fn reassembles_a_report_whose_introducer_arrived_alone() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.feed_raw_stdin("\x1b").is_empty());
	assert_eq!(terminal.feed_raw_stdin(REPORT_TAIL), vec![data(REPORT)]);
}

#[test]
fn reassembles_a_report_split_at_the_type_field() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.feed_raw_stdin("\x1b[4").is_empty());
	assert_eq!(
		terminal.feed_raw_stdin("8;61;211;1837;3131t"),
		vec![data(REPORT)]
	);
}

#[test]
fn reassembles_a_body_bundled_with_the_next_introducer() {
	let mut terminal = ProcessTerminal::new();
	assert_eq!(
		terminal.feed_raw_stdin("\x1b[48;61;211;1837;3131t\x1b["),
		vec![data(REPORT)]
	);
	assert_eq!(terminal.feed_raw_stdin("3~"), vec![data("\x1b[3~")]);
}

#[test]
fn reassembles_a_body_that_arrived_without_its_terminator() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.feed_raw_stdin("\x1b[48;61;211;1837;3131").is_empty());
	assert_eq!(terminal.feed_raw_stdin("t"), vec![data(REPORT)]);
}

#[test]
fn forwards_a_keystroke_after_a_complete_report() {
	let mut terminal = ProcessTerminal::new();
	assert_eq!(
		terminal.feed_raw_stdin("\x1b[48;61;211;1837;3131ta"),
		vec![data(REPORT), data("a")]
	);
}

#[test]
fn reassembles_a_private_csi_reply_split_across_reads() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.feed_raw_stdin("\x1b[?2026").is_empty());
	assert_eq!(terminal.feed_raw_stdin("$p"), vec![data("\x1b[?2026$p")]);
}

#[test]
fn holds_an_introducer_that_no_body_follows_until_more_arrives() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.feed_raw_stdin("\x1b[").is_empty());
	assert_eq!(terminal.feed_raw_stdin("A"), vec![data("\x1b[A")]);
}

#[test]
fn reassembles_a_split_csi_key_after_a_held_introducer() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.feed_raw_stdin("\x1b").is_empty());
	assert!(terminal.feed_raw_stdin("[").is_empty());
	assert_eq!(terminal.feed_raw_stdin("3~"), vec![data("\x1b[3~")]);
}
