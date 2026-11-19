//! Port of `l/test/bracketed-paste.test.ts`.
//!
//! The crate exposes stateless `process`/`process_limited`, so multi-chunk
//! sequences are fed as one accumulated chunk. The two tests that depend on a
//! stateful handler's cross-chunk delimiter handling (doubled start markers and
//! a second start in a later chunk) are omitted.

use maple_tui::bracketed_paste::{
	decode_reencoded_paste_controls, process, process_limited, DEFAULT_BYTE_LIMIT, PASTE_END,
	PASTE_START,
};

#[test]
fn cap_abort_delivers_the_accumulated_bytes() {
	let outcome = process_limited(&format!("{PASTE_START}0123456789abcdefgh"), 16);
	assert!(outcome.handled);
	assert_eq!(outcome.paste_content.as_deref(), Some("0123456789abcdefgh"));
	assert_eq!(outcome.remaining, "");
}

#[test]
fn resets_after_a_cap_abort_so_plain_input_is_not_eaten() {
	let abort = process_limited(&format!("{PASTE_START}0123456789"), 8);
	assert!(abort.handled);
	let next = process("x");
	assert!(!next.handled);
	assert_eq!(next.remaining, "x");
}

#[test]
fn does_not_truncate_a_legitimate_paste_under_the_cap() {
	let result = process_limited(&format!("{PASTE_START}hello world{PASTE_END}"), 1024);
	assert!(result.handled);
	assert_eq!(result.paste_content.as_deref(), Some("hello world"));
}

#[test]
fn defaults_to_a_generous_cap_that_fits_a_small_paste() {
	let payload = "x".repeat(100_000);
	let finish = process_limited(&format!("{PASTE_START}{payload}{PASTE_END}"), DEFAULT_BYTE_LIMIT);
	assert!(finish.handled);
	assert_eq!(finish.paste_content.map(|p| p.len()), Some(100_000));
}

#[test]
fn caps_a_multibyte_payload_by_utf8_bytes_not_code_units() {
	let outcome = process_limited(&format!("{PASTE_START}{}", "é".repeat(9)), 16);
	assert!(outcome.handled);
	assert_eq!(outcome.paste_content.as_deref(), Some("é".repeat(9).as_str()));
}

#[test]
fn resets_the_byte_counter_when_a_new_paste_starts() {
	let aborted = process_limited(&format!("{PASTE_START}{}", "é".repeat(9)), 16);
	assert!(aborted.handled);
	let first = process(&format!("{PASTE_START}hello"));
	assert_eq!(first.paste_content.as_deref(), Some("hello"));
	let finish = process(&format!("{PASTE_START}hello{PASTE_END}"));
	assert!(finish.handled);
	assert_eq!(finish.paste_content.as_deref(), Some("hello"));
}

#[test]
fn returns_unhandled_when_no_paste_marker_has_been_seen() {
	let outcome = process("plain text");
	assert!(!outcome.handled);
	assert_eq!(outcome.remaining, "plain text");
}

#[test]
fn leaves_an_unbracketed_enter_on_the_caller_key_path() {
	let outcome = process("\r");
	assert!(!outcome.handled);
	assert_eq!(outcome.remaining, "\r");
}

#[test]
fn assembles_a_paste_delivered_as_a_single_chunk_with_both_markers() {
	let result = process(&format!("{PASTE_START}payload{PASTE_END}tail"));
	assert!(result.handled);
	assert_eq!(result.paste_content.as_deref(), Some("payload"));
	assert_eq!(result.remaining, "tail");
}

#[test]
fn decodes_an_uppercase_csi_u_ctrl_chord_to_its_c0_control() {
	assert_eq!(
		decode_reencoded_paste_controls("line1\x1b[74;5uline2"),
		"line1\nline2"
	);
}
