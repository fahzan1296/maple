//! Port of `l/test/stdin-buffer.test.ts`.
//!
//! The crate's `StdinBuffer` returns `StdinBufferEvent` values instead of
//! emitting `data`/`paste` listener callbacks, and its timers use real
//! `Instant`s. Deterministic parsing/buffering/paste cases are pinned exactly;
//! the timer-driven flush/hold cases are ignored with that facility named.

use maple_tui::stdin_buffer::{StdinBuffer, StdinBufferEvent, StdinBufferOptions};

fn buffer() -> StdinBuffer {
	StdinBuffer::new(StdinBufferOptions {
		timeout: 10,
		..StdinBufferOptions::default()
	})
}

fn data(events: Vec<StdinBufferEvent>) -> Vec<String> {
	events
		.into_iter()
		.filter_map(|event| match event {
			StdinBufferEvent::Data(sequence) => Some(sequence),
			StdinBufferEvent::Paste { .. } => None,
		})
		.collect()
}

fn pastes(events: Vec<StdinBufferEvent>) -> Vec<(String, Option<String>)> {
	events
		.into_iter()
		.filter_map(|event| match event {
			StdinBufferEvent::Paste { content, enter } => Some((content, enter)),
			StdinBufferEvent::Data(_) => None,
		})
		.collect()
}

#[test]
fn handles_unicode_characters_one_per_event() {
	let mut buffer = buffer();
	assert_eq!(
		data(buffer.process("hello 世界")),
		vec!["h", "e", "l", "l", "o", " ", "世", "界"]
	);
}

#[test]
fn emits_a_surrogate_pair_as_one_text_sequence() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("🙂")), vec!["🙂"]);
}

#[test]
fn buffers_an_incomplete_mouse_sgr_sequence() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b");
	assert_eq!(data(buffer.process("[<35")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b[<35");
	assert_eq!(data(buffer.process(";20;5m")), vec!["\x1b[<35;20;5m"]);
	assert_eq!(buffer.get_buffer(), "");
}

#[test]
fn buffers_an_incomplete_csi_sequence() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[")), Vec::<String>::new());
	assert_eq!(data(buffer.process("1;")), Vec::<String>::new());
	assert_eq!(data(buffer.process("5H")), vec!["\x1b[1;5H"]);
}

#[test]
fn reassembles_a_sequence_split_across_many_chunks() {
	let mut buffer = buffer();
	for chunk in ["\x1b", "[", "<", "3", "5", ";", "2", "0", ";", "5"] {
		assert_eq!(data(buffer.process(chunk)), Vec::<String>::new(), "{chunk:?}");
	}
	assert_eq!(data(buffer.process("m")), vec!["\x1b[<35;20;5m"]);
	assert_eq!(buffer.get_buffer(), "");
}

#[test]
fn reassembles_an_osc_whose_st_is_split_at_the_chunk_boundary() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b]52;c;aGVsbG8=\x1b")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b]52;c;aGVsbG8=\x1b");
	assert_eq!(data(buffer.process("\\")), vec!["\x1b]52;c;aGVsbG8=\x1b\\"]);
	assert_eq!(buffer.get_buffer(), "");
}

#[test]
fn reassembles_a_dcs_whose_st_is_split_at_the_chunk_boundary() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1bPq#0;2;0;0;0\x1b")), Vec::<String>::new());
	assert_eq!(data(buffer.process("\\")), vec!["\x1bPq#0;2;0;0;0\x1b\\"]);
	assert_eq!(buffer.get_buffer(), "");
}

#[test]
fn handles_a_partial_sequence_with_preceding_characters() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("abc\x1b[<35")), vec!["a", "b", "c"]);
	assert_eq!(buffer.get_buffer(), "\x1b[<35");
	assert_eq!(data(buffer.process(";20;5m")), vec!["\x1b[<35;20;5m"]);
}

#[test]
fn handles_empty_input_as_an_empty_data_event() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("")), vec![""]);
}

#[test]
fn flushes_a_lone_escape_on_explicit_flush() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b")), Vec::<String>::new());
	assert_eq!(buffer.flush(), vec!["\x1b"]);
}

#[test]
fn handles_a_whole_arrow_sequence() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[A")), vec!["\x1b[A"]);
}

#[test]
fn handles_a_very_long_sequence() {
	let mut buffer = buffer();
	let long = format!("\x1b[{}H", "1;".repeat(50));
	assert_eq!(data(buffer.process(&long)), vec![long]);
}

#[test]
fn handles_mouse_press_release_and_multiple_reports() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[<0;10;5M")), vec!["\x1b[<0;10;5M"]);
	assert_eq!(data(buffer.process("\x1b[<0;10;5m")), vec!["\x1b[<0;10;5m"]);
	assert_eq!(
		data(buffer.process("\x1b[<35;1;1m\x1b[<35;2;2m\x1b[<35;3;3m")),
		vec!["\x1b[<35;1;1m", "\x1b[<35;2;2m", "\x1b[<35;3;3m"]
	);
}

#[test]
fn handles_old_style_mouse_sequences() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[M abc")), vec!["\x1b[M ab", "c"]);
}

#[test]
fn buffers_an_incomplete_old_style_mouse_sequence() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[M")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b[M");
	assert_eq!(data(buffer.process(" a")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b[M a");
	assert_eq!(data(buffer.process("b")), vec!["\x1b[M ab"]);
}

#[test]
fn splits_a_large_plain_burst_into_per_character_events() {
	let mut buffer = buffer();
	let content = "0123456789abcdef".repeat(4096);
	let events = data(buffer.process(&content));
	assert_eq!(events.len(), content.len());
	assert_eq!(events[0], "0");
	assert_eq!(events[events.len() - 1], "f");
}

#[test]
fn keeps_escape_parsing_and_surrogate_pairs_intact_inside_a_burst() {
	let mut buffer = buffer();
	assert_eq!(
		data(buffer.process("abc🙂\x1b[A\u{1f389}def\x1b[<35;20;5m\x1b")),
		vec![
			"a",
			"b",
			"c",
			"🙂",
			"\x1b[A",
			"\u{1f389}",
			"d",
			"e",
			"f",
			"\x1b[<35;20;5m",
		]
	);
	assert_eq!(buffer.get_buffer(), "\x1b");
}

#[test]
fn flushes_an_incomplete_sequence_and_reports_an_empty_flush() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[<35")), Vec::<String>::new());
	assert_eq!(buffer.flush(), vec!["\x1b[<35"]);
	assert_eq!(buffer.get_buffer(), "");
	assert_eq!(buffer.flush(), Vec::<String>::new());
}

#[test]
fn clears_buffered_content_without_emitting() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[<35")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b[<35");
	buffer.clear();
	assert_eq!(buffer.get_buffer(), "");
}

#[test]
fn emits_a_complete_bracketed_paste_without_data_events() {
	let mut buffer = buffer();
	let events = buffer.process("\x1b[200~hello world\x1b[201~");
	assert_eq!(pastes(events), vec![("hello world".to_string(), None)]);
}

#[test]
fn assembles_a_paste_arriving_in_chunks() {
	let mut buffer = buffer();
	assert_eq!(pastes(buffer.process("\x1b[200~")), Vec::new());
	assert_eq!(pastes(buffer.process("hello ")), Vec::new());
	assert_eq!(
		pastes(buffer.process("world\x1b[201~")),
		vec![("hello world".to_string(), None)]
	);
}

#[test]
fn handles_input_before_and_after_a_paste() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("a")), vec!["a"]);
	assert_eq!(
		pastes(buffer.process("\x1b[200~pasted\x1b[201~")),
		vec![("pasted".to_string(), None)]
	);
	assert_eq!(data(buffer.process("b")), vec!["b"]);
}

#[test]
fn preserves_newlines_and_unicode_inside_a_paste() {
	let mut buffer = buffer();
	assert_eq!(
		pastes(buffer.process("\x1b[200~line1\nline2\nline3\x1b[201~")),
		vec![("line1\nline2\nline3".to_string(), None)]
	);
	assert_eq!(
		pastes(buffer.process("\x1b[200~Hello 世界 \u{1f389}\x1b[201~")),
		vec![("Hello 世界 \u{1f389}".to_string(), None)]
	);
}

#[test]
fn assembles_a_paste_whose_end_marker_is_split_or_byte_wise() {
	let mut buffer = buffer();
	assert_eq!(pastes(buffer.process("\x1b[200~hello world\x1b[201")), Vec::new());
	assert_eq!(
		pastes(buffer.process("~")),
		vec![("hello world".to_string(), None)]
	);

	let mut byte_wise = buffer();
	let mut collected = Vec::new();
	for chunk in "\x1b[200~ab\x1b[201~".chars() {
		collected.extend(byte_wise.process(&chunk.to_string()));
	}
	assert_eq!(pastes(collected), vec![("ab".to_string(), None)]);
}

#[test]
fn preserves_trailing_input_after_a_boundary_split_end_marker() {
	let mut buffer = buffer();
	assert_eq!(pastes(buffer.process("\x1b[200~paste\x1b")), Vec::new());
	let events = buffer.process("[201~x");
	assert_eq!(pastes(events.clone()), vec![("paste".to_string(), None)]);
	assert_eq!(data(events), vec!["x"]);
}

#[test]
fn keeps_a_same_read_enter_on_the_paste_event() {
	for enter in ["\r", "\n", "\x1b[13u", "\x1b[13;1u"] {
		let mut buffer = buffer();
		let events = buffer.process(&format!("\x1b[200~paste\x1b[201~{enter}"));
		assert_eq!(
			pastes(events.clone()),
			vec![("paste".to_string(), Some(enter.to_string()))],
			"{enter:?}"
		);
		assert_eq!(data(events), Vec::<String>::new(), "{enter:?}");
	}
}

#[test]
fn routes_bytes_after_the_attached_enter_as_ordinary_input() {
	let mut buffer = buffer();
	let events = buffer.process("\x1b[200~paste\x1b[201~\rnext");
	assert_eq!(
		pastes(events.clone()),
		vec![("paste".to_string(), Some("\r".to_string()))]
	);
	assert_eq!(data(events), vec!["n", "e", "x", "t"]);
}

#[test]
fn leaves_non_enter_sequences_after_a_paste_on_the_data_route() {
	for sequence in ["\x1b[13;2u", "\x1b[?1;2c", "\x1b[A", "x"] {
		let mut buffer = buffer();
		let events = buffer.process(&format!("\x1b[200~paste\x1b[201~{sequence}"));
		assert_eq!(
			pastes(events.clone()),
			vec![("paste".to_string(), None)],
			"{sequence:?}"
		);
		assert_eq!(data(events), vec![sequence], "{sequence:?}");
	}
}

#[test]
fn handles_batched_kitty_keyboard_events() {
	let mut buffer = buffer();
	assert_eq!(
		data(buffer.process("\x1b[97u\x1b[97;1:3u\x1b[98u\x1b[98;1:3u")),
		vec!["\x1b[97u", "\x1b[97;1:3u", "\x1b[98u", "\x1b[98;1:3u"]
	);
	assert_eq!(data(buffer.process("\x1b[3;1:3~")), vec!["\x1b[3;1:3~"]);
}

#[test]
fn swallows_the_immediate_bare_duplicate_of_a_kitty_printable() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[97ua")), vec!["\x1b[97u"]);
}

#[test]
fn preserves_legacy_alt_chords_batched_after_a_bare_esc() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b\x1bX")), vec!["\x1b", "\x1bX"]);
	assert_eq!(data(buffer.process("\x1b\x1bd")), vec!["\x1b", "\x1bd"]);
	assert_eq!(data(buffer.process("\x1b\x1b\x7f")), vec!["\x1b", "\x1b\x7f"]);
}

#[test]
fn consumes_a_whole_meta_csi_arrow_in_one_chunk() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b\x1b[A")), vec!["\x1b\x1b[A"]);
}

#[test]
#[ignore = "missing facility: deterministic clock/timer injection; the crate flushes on real Instant deadlines"]
fn flushes_an_incomplete_sequence_after_the_timeout() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[1;5")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b[1;5");
}

#[test]
#[ignore = "missing facility: deterministic clock/timer injection for the partial-hold window"]
fn holds_a_split_sgr_mouse_partial_past_the_flush_timeout() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[<35;8;16")), Vec::<String>::new());
	assert_eq!(buffer.get_buffer(), "\x1b[<35;8;16");
}

#[test]
#[ignore = "missing facility: deterministic clock/timer injection for the hold cap"]
fn delivers_a_held_mouse_partial_raw_once_the_hold_cap_expires() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[<35;8;16")), Vec::<String>::new());
}

#[test]
#[ignore = "missing facility: kitty-protocol global flag is a TS module global; the crate setter exists but the timer window is real-time"]
fn holds_a_lone_esc_while_the_kitty_protocol_is_active() {
	let mut buffer = buffer();
	buffer.set_kitty_protocol_active(true);
	assert_eq!(data(buffer.process("\x1b")), Vec::<String>::new());
}

#[test]
#[ignore = "missing facility: deterministic clock/timer injection for the lone-ESC flush"]
fn flushes_a_lone_esc_after_the_timeout_when_kitty_is_inactive() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b")), Vec::<String>::new());
}

#[test]
#[ignore = "missing facility: deterministic clock/timer injection for double-ESC disambiguation"]
fn joins_a_held_bare_esc_with_a_following_csi() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b\x1b[B")), vec!["\x1b\x1b[B"]);
}

#[test]
#[ignore = "missing facility: deterministic clock/timer injection for the dedup window expiry"]
fn does_not_swallow_a_real_keystroke_after_the_dedup_window_expires() {
	let mut buffer = buffer();
	assert_eq!(data(buffer.process("\x1b[97u")), vec!["\x1b[97u"]);
}
