//! Port of `l/test/issue-2034-repro.test.ts`.
//!
//! `chunk_for_conpty` is a pure function and is ported into `conpty_write.rs`.
//! The `ProcessTerminal#write` platform-gate cases stay ignored: the crate's
//! `terminal::process::ProcessTerminal` has no stdout write path to intercept.

use maple_tui::conpty_write::{chunk_for_conpty, MAX_CONPTY_WRITE_CHUNK_BYTES};

const ESC: char = '\x1b';

fn build_full_paint(lines: usize, line_length: usize) -> String {
	let mut buf = format!("{ESC}[2J{ESC}[H{ESC}[3J");
	for index in 0..lines {
		if index > 0 {
			buf.push_str("\r\n");
		}
		buf.push_str(&format!(
			"{ESC}[38;5;{}mrow-{:04}: {}{ESC}[0m",
			index % 256,
			index,
			"x".repeat(line_length)
		));
	}
	buf.push_str(&format!("{ESC}[H{ESC}[?25h"));
	buf
}

/// Mirrors the `diffable` paint branch: each changed row is addressed with
/// `CSI row;1H`, painted, and closed with a bare `\r`, so the frame has no `\n`.
fn build_diff_paint(rows: usize, line_length: usize) -> String {
	let mut buf = String::new();
	for index in 0..rows {
		buf.push_str(&format!(
			"{ESC}[{};1H{ESC}[38;5;{}mrow-{:04}: {}{ESC}[0m\r",
			index + 1,
			index % 256,
			index,
			"x".repeat(line_length)
		));
	}
	buf.push_str(&format!("{ESC}[?25h"));
	buf
}

#[test]
fn splits_a_large_multi_line_buffer_into_pieces_no_larger_than_the_byte_cap() {
	let data = build_full_paint(2000, 60);
	let max = MAX_CONPTY_WRITE_CHUNK_BYTES;
	assert!(data.len() > max);

	let chunks = chunk_for_conpty(&data, max);
	assert!(chunks.len() > 1);
	for chunk in &chunks {
		assert!(chunk.len() <= max, "chunk of {} bytes", chunk.len());
	}
}

#[test]
fn preserves_the_full_payload_across_chunks() {
	let data = build_full_paint(500, 120);
	let chunks = chunk_for_conpty(&data, 4 * 1024);
	assert_eq!(chunks.concat(), data);
}

#[test]
fn splits_at_line_boundaries_so_escapes_are_never_sliced_apart() {
	let data = build_full_paint(400, 80);
	let chunks = chunk_for_conpty(&data, 4 * 1024);
	for chunk in chunks.iter().skip(1) {
		let starts_with_esc = chunk.starts_with(ESC);
		let starts_with_row_text = chunk.starts_with("row-");
		assert!(starts_with_esc || starts_with_row_text, "chunk starts with {chunk:?}");
		if !starts_with_esc {
			assert!(chunk.as_bytes()[0] >= 0x20);
		}
	}
}

#[test]
fn makes_forward_progress_on_a_single_line_longer_than_the_chunk_size() {
	let data = format!("{}\nshort\n", "a".repeat(20_000));
	let chunks = chunk_for_conpty(&data, 4 * 1024);
	assert!(chunks.len() >= 2);
	assert_eq!(chunks.concat(), data);
}

#[test]
fn cuts_a_row_diff_paint_at_row_boundaries() {
	let data = build_diff_paint(400, 80);
	let max = 4 * 1024;
	assert!(data.len() > max);
	assert!(!data.contains('\n'));

	let chunks = chunk_for_conpty(&data, max);
	assert!(chunks.len() > 1);
	for chunk in chunks.iter().skip(1) {
		assert!(chunk.starts_with("\x1b["), "chunk starts with {chunk:?}");
	}
	for chunk in &chunks {
		assert!(chunk.len() <= max);
	}
	assert_eq!(chunks.concat(), data);
}

#[test]
fn never_splits_a_crlf_pair_whose_newline_falls_just_past_the_cap() {
	let data = format!("{}\r\n{}\r\n", "a".repeat(9), "b".repeat(9));
	let chunks = chunk_for_conpty(&data, 10);
	assert_eq!(chunks.concat(), data);
	for chunk in &chunks {
		assert!(!chunk.starts_with('\n'));
		assert!(!chunk.ends_with('\r'));
	}
}

#[test]
fn falls_back_to_a_raw_split_when_the_buffer_has_no_newlines() {
	let data = "x".repeat(20_000);
	let chunks = chunk_for_conpty(&data, 4 * 1024);
	assert_eq!(chunks.concat(), data);
	assert!(chunks.len() > 1);
	for chunk in chunks.iter().take(chunks.len() - 1) {
		assert_eq!(chunk.len(), 4 * 1024);
	}
}

#[test]
fn caps_by_encoded_utf8_bytes_so_cjk_transcripts_stay_under_the_threshold() {
	let cjk_line = "字".repeat(200);
	let rows: Vec<String> = (0..200).map(|_| cjk_line.clone()).collect();
	let data = format!("{}\n", rows.join("\n"));
	let max = 4 * 1024;
	assert!(data.len() > max * 4);

	let chunks = chunk_for_conpty(&data, max);
	assert!(chunks.len() > 1);
	assert_eq!(chunks.concat(), data);
	for chunk in &chunks {
		assert!(chunk.len() <= max, "chunk of {} bytes", chunk.len());
	}
}

#[test]
fn keeps_multi_byte_code_points_intact_when_cutting_at_the_byte_cap() {
	let data = "😀".repeat(1024);
	let chunks = chunk_for_conpty(&data, 1024);
	assert_eq!(chunks.concat(), data);
	for chunk in &chunks {
		assert!(chunk.len() <= 1024);
		assert!(chunk.is_char_boundary(0) && chunk.is_char_boundary(chunk.len()));
	}
}

#[test]
#[ignore = "missing API: ProcessTerminal#write platform gate (win32/WSL chunking of stdout writes)"]
fn splits_large_writes_into_chunks_on_win32() {
}

#[test]
#[ignore = "missing API: ProcessTerminal#write platform gate (WSL chunking)"]
fn splits_large_writes_inside_wsl() {
}

#[test]
#[ignore = "missing API: ProcessTerminal#write platform gate (single-write fast path on non-ConPTY platforms)"]
fn keeps_the_single_write_fast_path_off_conpty() {
}

#[test]
#[ignore = "missing API: ProcessTerminal#write platform gate (small writes stay whole on win32)"]
fn does_not_chunk_small_writes() {
}

#[test]
#[ignore = "missing API: ProcessTerminal#write platform gate (CJK payload chunking)"]
fn chunks_a_cjk_payload_on_win32() {
}

#[test]
#[ignore = "missing API: ProcessTerminal stdout error handling (EIO marks the terminal dead)"]
fn marks_the_terminal_dead_on_stdout_eio() {
}

#[test]
#[ignore = "missing API: ProcessTerminal stdout error handling after stop"]
fn keeps_stdout_error_events_handled_after_stop() {
}
