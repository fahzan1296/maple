//! Port of `l/test/sgr-coalesce.test.ts`.
//!
//! Tests `maple_tui::coalesce_adjacent_sgr` (SGR merge/cap/boundary behavior).

use maple_tui::coalesce_adjacent_sgr;

fn sgr_sequences(s: &str) -> Vec<String> {
	let bytes = s.as_bytes();
	let mut out = Vec::new();
	let mut i = 0usize;
	while i < bytes.len() {
		if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
			let mut j = i + 2;
			while j < bytes.len() {
				let b = bytes[j];
				j += 1;
				if b == b'm' {
					out.push(s[i..j].to_string());
					break;
				}
				if !(b.is_ascii_digit() || b == b';' || b == b':') {
					break;
				}
			}
			i = j;
		} else {
			i += 1;
		}
	}
	out
}

fn max_param_tokens(s: &str) -> usize {
	let mut max = 0usize;
	for seq in sgr_sequences(s) {
		let inner = seq
			.strip_prefix("\x1b[")
			.and_then(|v| v.strip_suffix('m'))
			.unwrap_or("");
		let tokens = if inner.is_empty() {
			0
		} else {
			inner
				.chars()
				.filter(|c| *c == ';' || *c == ':')
				.count()
				+ 1
		};
		if tokens > max {
			max = tokens;
		}
	}
	max
}

#[test]
fn merges_byte_adjacent_sgr_sequences_into_one_csi() {
	let input = "\x1b[31m\x1b[1mhello\x1b[0m";
	let out = coalesce_adjacent_sgr(input);
	assert_eq!(out, "\x1b[31;1mhello\x1b[0m");
}

#[test]
fn normalizes_empty_parameter_reset_when_merging() {
	let input = "\x1b[m\x1b[31mhello";
	let out = coalesce_adjacent_sgr(input);
	assert_eq!(out, "\x1b[0;31mhello");
}

#[test]
fn does_not_merge_sgr_separated_by_visible_content() {
	let input = "\x1b[31mA\x1b[32mB";
	let out = coalesce_adjacent_sgr(input);
	assert_eq!(out, input);
	assert_eq!(sgr_sequences(&out).len(), 2);
}

#[test]
fn leaves_non_sgr_control_sequences_untouched() {
	let input = "\x1b[2K\x1b[31m\x1b[1mhi\x1b[0m\x1b[1A";
	let out = coalesce_adjacent_sgr(input);
	assert_eq!(out, "\x1b[2K\x1b[31;1mhi\x1b[0m\x1b[1A");
}

#[test]
fn caps_each_emitted_csi_for_long_adjacent_run() {
	// 25 adjacent single-param SGRs: should split so no emitted CSI exceeds cap.
	let input = (0..25)
		.map(|i| format!("\x1b[{}m", 30 + (i % 8)))
		.collect::<String>()
		+ "x";
	let out = coalesce_adjacent_sgr(&input);
	assert!(max_param_tokens(&out) <= 16, "max tokens {}", max_param_tokens(&out));
	// Still contains all params in order across emitted CSIs.
	let joined_params = sgr_sequences(&out)
		.into_iter()
		.map(|seq| {
			seq.strip_prefix("\x1b[")
				.and_then(|v| v.strip_suffix('m'))
				.unwrap_or("")
				.to_string()
		})
		.filter(|p| !p.is_empty())
		.collect::<Vec<_>>()
		.join(";");
	let expected = (0..25)
		.map(|i| (30 + (i % 8)).to_string())
		.collect::<Vec<_>>()
		.join(";");
	assert_eq!(joined_params, expected);
}

#[test]
fn does_not_merge_across_incomplete_truecolor_introducer_missing_channel() {
	// 38;2;255;0 is missing blue channel -> boundary preserved, next code not absorbed.
	let input = "\x1b[38;2;255;0m\x1b[31mX";
	let out = coalesce_adjacent_sgr(input);
	// Must not absorb 31 into the color channels.
	assert!(!sgr_sequences(&out).iter().any(|s| s.contains("38;2;255;0;31")));
	assert_ne!(out, "\x1b[38;2;255;0;31mX");
}

#[test]
fn does_not_merge_across_incomplete_indexed_color_introducer_missing_index() {
	let input = "\x1b[38;5m\x1b[31mX";
	let out = coalesce_adjacent_sgr(input);
	assert!(!sgr_sequences(&out).iter().any(|s| s.contains("38;5;31")));
}

#[test]
fn still_merges_complete_extended_color_followed_by_another_code() {
	let input = "\x1b[38;2;1;2;3m\x1b[1mX";
	let out = coalesce_adjacent_sgr(input);
	assert_eq!(out, "\x1b[38;2;1;2;3;1mX");
}

#[test]
fn preserves_colon_subparameters_without_splitting_complete_color() {
	let input = "\x1b[4:3m\x1b[58:2::255:0:0m\x1b[1mX";
	let out = coalesce_adjacent_sgr(input);
	assert_eq!(out, "\x1b[4:3;58:2::255:0:0;1mX");
}
