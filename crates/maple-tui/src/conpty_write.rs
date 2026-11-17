//! Windows ConPTY write chunking.
//!
//! Reference: `l/src/terminal.ts` (`chunkForConPTY`).
//!
//! Windows ConPTY ties viewport tracking to per-`WriteFile` boundaries: a
//! single write above roughly 32-64 KiB stops the pseudo-console following the
//! cursor, so a full paint of a long session parks the host viewport. The cap
//! is on **encoded UTF-8 bytes**, not code units, because a CJK row encodes to
//! about three bytes per character.

/// Maximum encoded UTF-8 bytes per `stdout` write on Windows.
pub const MAX_CONPTY_WRITE_CHUNK_BYTES: usize = 16 * 1024;

/// Split `data` into chunks whose encoded UTF-8 byte length is no greater than
/// `max_chunk_bytes`, preferring a line boundary (`\n`, or a bare `\r` in a diff
/// paint) as the cut point so escape sequences and rows stay intact. A buffer
/// with no terminator in range falls back to a hard cut at the last code point
/// boundary that still fits, which still makes forward progress.
pub fn chunk_for_conpty(data: &str, max_chunk_bytes: usize) -> Vec<String> {
	if data.len() <= max_chunk_bytes {
		return vec![data.to_string()];
	}
	let bytes = data.as_bytes();
	let len = bytes.len();
	let mut chunks = Vec::new();
	let mut pos = 0usize;
	while pos < len {
		let mut used = 0usize;
		// Byte index just past the most recent line terminator consumed in this
		// window: the natural cut point that leaves rows intact.
		let mut last_newline_end: Option<usize> = None;
		let mut last_carriage_return_end: Option<usize> = None;
		let mut index = pos;
		while index < len {
			let unit = data[index..].chars().next().expect("char boundary").len_utf8();
			if used + unit > max_chunk_bytes && index > pos {
				let cut = last_newline_end.into_iter().chain(last_carriage_return_end).max();
				let mut end = match cut {
					Some(cut) if cut > pos => cut,
					_ => index,
				};
				// A hard cut landing between `\r` and `\n` moves back before the `\r`.
				if end == index && bytes[index] == b'\n' && index > pos && bytes[index - 1] == b'\r' && index - 1 > pos {
					end = index - 1;
				}
				chunks.push(data[pos..end].to_string());
				pos = end;
				break;
			}
			used += unit;
			index += unit;
			// A `\r` directly followed by `\n` is half of a `\r\n` pair, even when
			// the `\n` lies past the cap, so it is never a cut point on its own.
			if unit == 1 && bytes[index - 1] == b'\n' {
				last_newline_end = Some(index);
			} else if unit == 1 && bytes[index - 1] == b'\r' && bytes.get(index) != Some(&b'\n') {
				last_carriage_return_end = Some(index);
			}
		}
		if index >= len {
			chunks.push(data[pos..].to_string());
			pos = len;
		}
	}
	chunks
}
