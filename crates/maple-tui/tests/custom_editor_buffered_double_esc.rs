//! Port of `l/test/custom-editor-buffered-double-esc.test.ts`.
//!
//! The reference wires `StdinBuffer` chunks into `CustomEditor.handleInput`
//! and counts `onEscape` calls. The crate `CustomEditor` has no key-input
//! path, so the same stdin contract is asserted at the buffer boundary: a
//! fast double-Esc chunk flushes as two ESC sequences, a follower stays
//! attached to the second ESC, and a meta-CSI arrow is one sequence.

use maple_tui::stdin_buffer::{StdinBuffer, StdinBufferEvent, StdinBufferOptions};

fn buffer() -> StdinBuffer {
	StdinBuffer::new(StdinBufferOptions { timeout: 5, partial_hold_timeout: 5, ..Default::default() })
}

#[test]
fn splits_a_fast_double_esc_chunk_into_two_esc_sequences() {
	let mut stdin = buffer();
	assert!(stdin.process("\x1b\x1b").is_empty());
	assert_eq!(stdin.flush(), vec!["\x1b".to_string(), "\x1b".to_string()]);
}

#[test]
fn preserves_a_legacy_alt_chord_batched_after_a_bare_esc() {
	let mut stdin = buffer();
	assert_eq!(
		stdin.process("\x1b\x1b\x7f"),
		vec![
			StdinBufferEvent::Data("\x1b".to_string()),
			StdinBufferEvent::Data("\x1b\x7f".to_string()),
		]
	);
}

#[test]
fn does_not_split_a_meta_csi_arrow_into_two_esc_sequences() {
	let mut stdin = buffer();
	let events = stdin.process("\x1b\x1b[A");
	assert_eq!(events, vec![StdinBufferEvent::Data("\x1b\x1b[A".to_string())]);
}
