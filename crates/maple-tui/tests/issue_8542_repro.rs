//! Port of `l/test/issue-8542-repro.test.ts`.
//!
//! Deviations:
//! - The reference needs a real PTY and waits out the wall-clock partial-hold
//!   timeout; `ProcessTerminal::feed_raw_stdin` reassembles the split sequence
//!   synchronously through the crate's stdin buffer, so no delay is needed.
//! - `setTerminalHeadless`/process-stream stubs have no Rust equivalent; the
//!   probe seam is exercised directly without starting the terminal.

use maple_tui::stdin_buffer::StdinBufferEvent;
use maple_tui::terminal::process::ProcessTerminal;
use maple_tui::terminal::program_status::{is_da1_response, is_program_status_sequence};

const DA1_REPLY: &str = "\x1b[?1;22;23;24;28;32;42;52c";
const SENTINEL: &str = "\x1b[?62c";

#[test]
fn da1_sequences_are_recognized_as_terminal_reports() {
	assert!(is_da1_response(DA1_REPLY));
	assert!(is_da1_response(SENTINEL));
	assert!(!is_da1_response("\x1bP"));
	assert!(!is_da1_response("\x1b[?1;22"));
	assert!(is_program_status_sequence(DA1_REPLY));
}

#[test]
fn swallows_a_single_event_da_reply_that_arrives_after_the_sentinel_fifo_drains() {
	let mut terminal = ProcessTerminal::new();
	for _ in 0..32 {
		let events = terminal.feed_raw_stdin(DA1_REPLY);
		assert!(
			events.iter().all(|event| !matches!(event, StdinBufferEvent::Data(sequence) if sequence == DA1_REPLY)),
			"DA1 leaked: {events:?}"
		);
	}
	assert_eq!(terminal.program_status_support(), Some(false));
}

#[test]
fn reassembles_and_swallows_a_split_da_reply_with_an_empty_fifo() {
	let mut terminal = ProcessTerminal::new();
	for _ in 0..32 {
		terminal.feed_raw_stdin(SENTINEL);
	}
	assert_eq!(terminal.program_status_support(), Some(false));

	let prefix = terminal.feed_raw_stdin("\x1b[?1;22;23");
	assert!(prefix.is_empty(), "partial DA1 must be held, got: {prefix:?}");
	let tail = terminal.feed_raw_stdin(";24;28;32;42;52c");
	assert!(tail.is_empty(), "reassembled DA1 leaked: {tail:?}");
	assert_eq!(terminal.program_status_support(), Some(false));
}

#[test]
fn ordinary_keystrokes_still_forward_after_the_probe_resolves() {
	let mut terminal = ProcessTerminal::new();
	terminal.feed_raw_stdin(DA1_REPLY);
	let events = terminal.feed_raw_stdin("\x1b[A");
	assert!(
		events
			.iter()
			.any(|event| matches!(event, StdinBufferEvent::Data(sequence) if sequence == "\x1b[A")),
		"keystroke swallowed: {events:?}"
	);
}