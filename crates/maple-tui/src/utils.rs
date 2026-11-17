//! Shared text and terminal helpers (port of `l/src/utils.ts`).
//!
//! Reference: `l/src/utils.ts`.
//!
//! The reference module is a facade over the native `pi-natives` text engine.
//! This crate reimplements that engine in [`crate::text_shape`] and
//! [`crate::render::render_utils`], so several exports already have a crate
//! home and are deliberately not duplicated here:
//!
//! - `visibleWidth`, `truncateToWidth`, `wrapTextWithAnsi`, `pad_to_width`,
//!   `Ellipsis` → [`crate::text_shape`].
//! - `replaceTabs`, `padding`, `sliceWithWidth`/`sliceByColumn`,
//!   `extractSegments`, `setTuiTight`/`isTuiTight`, `getPaddingX`
//!   → [`crate::render::render_utils`] (`replace_tabs`, `padding`,
//!   `slice_by_column`, `extract_segments`, `set_tight_layout`,
//!   `is_tight_layout`, `get_padding_x`).
//! - `stripTerminalSequences` is subsumed by [`crate::ansi_scan::strip_ansi`],
//!   which also drops the CSI forms the reference parser leaves alone.
//! - Word navigation (`getWordNavKind`, `moveWordLeft`, `moveWordRight`,
//!   `isWordNavJoiner`) is reimplemented in [`crate::vim`], its only consumer.
//! - The width-config sidecar (`publishLineWidths`,
//!   `getPublishedLineWidths`) has no crate equivalent: memos key on
//!   `terminal().hangul_jamo_width` instead (see `render/output_pane.rs`).
//!
//! Deviations forced by Rust:
//!
//! - `Bun.stringWidth` is not available; width comes from `unicode-width` via
//!   [`crate::text_shape::visible_width`].
//! - The native Hangul-Jamo override is not forwarded to a native engine; the
//!   runtime value is process-global state read by callers.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::render::render_utils::{padding, replace_tabs, slice_by_column};
use crate::terminal_capabilities::HangulCompatibilityJamoWidth;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// Display columns one tab expands to. Mirrors `DEFAULT_TAB_WIDTH` from
/// `@oh-my-pi/pi-utils` (3).
pub const DEFAULT_TAB_WIDTH: usize = 3;

static JAMO_WIDTH: Mutex<HangulCompatibilityJamoWidth> =
	Mutex::new(HangulCompatibilityJamoWidth::Platform);
static WIDTH_CONFIG_EPOCH: AtomicUsize = AtomicUsize::new(0);

/// Runtime Hangul Compatibility Jamo width, as last set.
pub fn get_hangul_compatibility_jamo_width() -> HangulCompatibilityJamoWidth {
	*JAMO_WIDTH.lock().unwrap()
}

/// Monotonic epoch for width-affecting runtime configuration. Caches derived
/// from a width are stamped with it and discarded on mismatch.
pub fn get_width_config_epoch() -> usize {
	WIDTH_CONFIG_EPOCH.load(Ordering::Relaxed)
}

/// Set the runtime Hangul Compatibility Jamo width. Returns whether it changed.
pub fn set_hangul_compatibility_jamo_width(width: HangulCompatibilityJamoWidth) -> bool {
	let mut guard = JAMO_WIDTH.lock().unwrap();
	let changed = *guard != width;
	if changed {
		*guard = width;
		WIDTH_CONFIG_EPOCH.fetch_add(1, Ordering::Relaxed);
	}
	changed
}

/// Restore the platform default and invalidate width caches.
pub fn reset_hangul_compatibility_jamo_width_for_tests() {
	let mut guard = JAMO_WIDTH.lock().unwrap();
	if *guard != HangulCompatibilityJamoWidth::Platform {
		WIDTH_CONFIG_EPOCH.fetch_add(1, Ordering::Relaxed);
	}
	*guard = HangulCompatibilityJamoWidth::Platform;
}

/// Kitty OSC 66 text-sizing scale.
pub type TextSizingScale = u8;

/// Vertical placement of an OSC 66 text-sizing span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSizingVerticalAlign {
	Top,
	Bottom,
	Center,
}

impl TextSizingVerticalAlign {
	fn code(self) -> &'static str {
		match self {
			TextSizingVerticalAlign::Top => "0",
			TextSizingVerticalAlign::Bottom => "1",
			TextSizingVerticalAlign::Center => "2",
		}
	}
}

/// Horizontal placement of an OSC 66 text-sizing span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSizingHorizontalAlign {
	Left,
	Right,
	Center,
}

impl TextSizingHorizontalAlign {
	fn code(self) -> &'static str {
		match self {
			TextSizingHorizontalAlign::Left => "0",
			TextSizingHorizontalAlign::Right => "1",
			TextSizingHorizontalAlign::Center => "2",
		}
	}
}

/// Options for [`encode_text_sized`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSizingOptions {
	pub scale: Option<TextSizingScale>,
	pub width_cells: Option<usize>,
	pub vertical_align: Option<TextSizingVerticalAlign>,
	pub horizontal_align: Option<TextSizingHorizontalAlign>,
}

fn is_osc66_unsafe(ch: char) -> bool {
	matches!(ch as u32, 0..=0x1f | 0x7f..=0x9f)
}

/// Encode a plain-text span using Kitty's OSC 66 text-sizing protocol.
///
/// Emits safe UTF-8 payloads and an ST terminator so the ANSI parser and the
/// terminal agree on span boundaries.
pub fn encode_text_sized(text: &str, options: TextSizingOptions) -> String {
	let mut metadata: Vec<String> = Vec::new();
	if let Some(scale) = options.scale {
		metadata.push(format!("s={scale}"));
	}
	if let Some(width_cells) = options.width_cells {
		metadata.push(format!("w={width_cells}"));
	}
	if let Some(vertical) = options.vertical_align {
		metadata.push(format!("v={}", vertical.code()));
	}
	if let Some(horizontal) = options.horizontal_align {
		metadata.push(format!("h={}", horizontal.code()));
	}
	let safe = if text.chars().any(is_osc66_unsafe) {
		text.chars()
			.map(|ch| if is_osc66_unsafe(ch) { ' ' } else { ch })
			.collect()
	} else {
		text.to_string()
	};
	format!("\x1b]66;{};{}\x1b\\", metadata.join(":"), safe)
}

/// A non-negative integer cell width; non-finite input becomes `fallback`.
pub fn finite_width(width: f64, fallback: usize) -> usize {
	if width.is_finite() {
		width.max(0.0).trunc() as usize
	} else {
		fallback
	}
}

/// Whether `line` carries a Kitty OSC 66 text-sizing span.
pub fn is_osc66_line(line: &str) -> bool {
	line.contains("\x1b]66;")
}

/// Largest `s=` scale among the OSC 66 spans in `line` (1 when none is scaled).
pub fn osc66_max_scale(line: &str) -> u8 {
	let mut max = 1u8;
	let mut cursor = 0usize;
	while let Some(relative) = line[cursor..].find("\x1b]66;") {
		let metadata_start = cursor + relative + "\x1b]66;".len();
		let Some(metadata_relative) = line[metadata_start..].find(';') else {
			break;
		};
		let metadata = &line[metadata_start..metadata_start + metadata_relative];
		let body_start = metadata_start + metadata_relative + 1;
		let bel = line[body_start..].find('\u{7}');
		let st = line[body_start..].find("\x1b\\");
		let terminator = match (bel, st) {
			(Some(bel), Some(st)) => bel.min(st),
			(Some(bel), None) => bel,
			(None, Some(st)) => st,
			(None, None) => break,
		};
		for part in metadata.split(':') {
			if let Some(value) = part.strip_prefix("s=") {
				if let Ok(value) = value.parse::<i64>() {
					if value > i64::from(max) && value <= 7 {
						max = value as u8;
					}
				}
			}
		}
		cursor = body_start + terminator + 1;
	}
	max
}

/// Whether `ch` is one of the ASCII whitespace characters.
pub fn is_whitespace_char(ch: char) -> bool {
	matches!(ch, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ')
}

/// Whether `ch` is one of the ASCII punctuation characters.
pub fn is_punctuation_char(ch: char) -> bool {
	matches!(
		ch,
		'(' | ')'
			| '{' | '}' | '[' | ']' | '<' | '>'
			| '.' | ',' | ';' | ':' | '\'' | '"' | '!' | '?'
			| '+' | '-' | '=' | '*' | '/' | '\\' | '|'
			| '&' | '%' | '^' | '$' | '#' | '@' | '~' | '`'
	)
}

/// Normalize text for terminal output without changing logical editor content.
///
/// Precomposed Thai/Lao AM vowels decompose to their compatibility forms,
/// which render without stale-cell artifacts during differential repaint.
pub fn normalize_terminal_output(input: &str) -> String {
	if !input.contains('\u{e33}') && !input.contains('\u{eb3}') {
		return input.to_string();
	}
	input
		.replace('\u{e33}', "\u{e4d}\u{e32}")
		.replace('\u{eb3}', "\u{ecd}\u{eb2}")
}

/// Length of the ANSI/OSC/APC sequence at `pos`, if one starts there.
///
/// Mirrors the reference parser: CSI is only recognized when it ends in one of
/// `m`, `G`, `K`, `H`, `J`; OSC and APC run to BEL or ST.
fn ansi_code_len(input: &str, pos: usize) -> Option<usize> {
	let bytes = input.as_bytes();
	if bytes.get(pos) != Some(&0x1b) {
		return None;
	}
	match bytes.get(pos + 1)? {
		b'[' => {
			let mut index = pos + 2;
			while let Some(byte) = bytes.get(index) {
				if matches!(byte, b'm' | b'G' | b'K' | b'H' | b'J') {
					return Some(index + 1 - pos);
				}
				index += 1;
			}
			None
		}
		b']' | b'_' => {
			let mut index = pos + 2;
			while let Some(byte) = bytes.get(index) {
				if *byte == 0x07 {
					return Some(index + 1 - pos);
				}
				if *byte == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
					return Some(index + 2 - pos);
				}
				index += 1;
			}
			None
		}
		_ => None,
	}
}

/// Remove ANSI, OSC, and APC control sequences while preserving visible text.
pub fn strip_terminal_sequences(input: &str) -> String {
	if !input.contains('\x1b') {
		return input.to_string();
	}
	let mut out = String::with_capacity(input.len());
	let mut index = 0usize;
	while index < input.len() {
		if let Some(len) = ansi_code_len(input, index) {
			index += len;
			continue;
		}
		let ch = input[index..].chars().next().expect("index is on a char boundary");
		out.push(ch);
		index += ch.len_utf8();
	}
	out
}

/// Hard-wrap one literal terminal row without trimming spaces or splitting
/// graphemes. An oversized grapheme is emitted intact even when it exceeds the
/// requested width; a zero-cell row stays one empty row.
pub fn wrap_literal_line(line: &str, width: usize) -> Vec<String> {
	let columns = width.max(1);
	let line_width = visible_width(line);
	if line_width == 0 {
		return vec![String::new()];
	}
	let mut rows = Vec::new();
	let mut column = 0usize;
	while column < line_width {
		let mut slice = slice_by_column(line, column, columns, true);
		if slice.width == 0 {
			slice = slice_by_column(line, column, columns, false);
		}
		rows.push(slice.text);
		column += slice.width.max(1);
	}
	rows
}

/// Center `line` in a field of `width` columns, truncating when too wide.
pub fn center_line(line: &str, width: usize) -> String {
	let line_width = visible_width(line);
	if line_width >= width {
		return truncate_to_width(line, width, Ellipsis::Unicode);
	}
	let left = (width - line_width) / 2;
	let right = width - left - line_width;
	format!("{}{}{}", padding(left), line, padding(right))
}

/// Expand tabs and collapse line breaks to single spaces.
pub fn single_line(text: &str) -> String {
	let expanded = replace_tabs(text);
	let mut out = String::with_capacity(expanded.len());
	let mut in_break = false;
	for ch in expanded.chars() {
		if ch == '\r' || ch == '\n' {
			if !in_break {
				out.push(' ');
				in_break = true;
			}
		} else {
			out.push(ch);
			in_break = false;
		}
	}
	out
}

/// Apply a background color to `line`, padding it to `width` columns.
///
/// Nested background resets (`\x1b[49m`) would terminate a plain open…close
/// wrapper early, so the line background is re-opened after each one.
pub fn apply_background_to_line(line: &str, width: usize, bg_fn: &dyn Fn(&str) -> String) -> String {
	let padding_needed = width.saturating_sub(visible_width(line));
	let mut with_padding = String::with_capacity(line.len() + padding_needed);
	with_padding.push_str(line);
	with_padding.push_str(&padding(padding_needed));
	if line.contains("\x1b[49m") {
		let probe = bg_fn("\x01");
		if let Some(probe_index) = probe.find('\x01') {
			if probe_index > 0 {
				let open = &probe[..probe_index];
				if !open.is_empty() {
					with_padding = with_padding.replace("\x1b[49m", &format!("\x1b[49m{open}"));
				}
			}
		}
	}
	bg_fn(&with_padding)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tab_width_matches_the_native_engine() {
		assert_eq!(DEFAULT_TAB_WIDTH, 3);
	}

	#[test]
	fn text_sizing_metadata_is_ordered_and_sanitized() {
		let options = TextSizingOptions {
			scale: Some(2),
			width_cells: Some(5),
			vertical_align: Some(TextSizingVerticalAlign::Center),
			horizontal_align: Some(TextSizingHorizontalAlign::Left),
		};
		assert_eq!(
			encode_text_sized("hi", options),
			"\x1b]66;s=2:w=5:v=2:h=0;hi\x1b\\"
		);
		assert_eq!(
			encode_text_sized("a\nb", TextSizingOptions::default()),
			"\x1b]66;;a b\x1b\\"
		);
	}

	#[test]
	fn finite_width_clamps_and_falls_back() {
		assert_eq!(finite_width(3.9, 0), 3);
		assert_eq!(finite_width(-2.0, 7), 0);
		assert_eq!(finite_width(f64::NAN, 7), 7);
		assert_eq!(finite_width(f64::INFINITY, 4), 4);
	}

	#[test]
	fn hangul_width_override_bumps_the_epoch() {
		reset_hangul_compatibility_jamo_width_for_tests();
		let epoch = get_width_config_epoch();
		assert_eq!(
			get_hangul_compatibility_jamo_width(),
			HangulCompatibilityJamoWidth::Platform
		);
		assert!(!set_hangul_compatibility_jamo_width(
			HangulCompatibilityJamoWidth::Platform
		));
		assert!(set_hangul_compatibility_jamo_width(
			HangulCompatibilityJamoWidth::One
		));
		assert_eq!(
			get_hangul_compatibility_jamo_width(),
			HangulCompatibilityJamoWidth::One
		);
		assert!(get_width_config_epoch() > epoch);
		reset_hangul_compatibility_jamo_width_for_tests();
		assert_eq!(
			get_hangul_compatibility_jamo_width(),
			HangulCompatibilityJamoWidth::Platform
		);
	}

	#[test]
	fn osc66_scales_are_reported_per_line() {
		assert!(is_osc66_line("\x1b]66;s=2;hi\x1b\\"));
		assert!(!is_osc66_line("plain"));
		assert_eq!(osc66_max_scale("plain"), 1);
		assert_eq!(osc66_max_scale("\x1b]66;s=2;a\x1b\\\x1b]66;s=5;b\x07"), 5);
		assert_eq!(osc66_max_scale("\x1b]66;s=9;a\x1b\\"), 1);
	}

	#[test]
	fn ascii_character_classes_match_the_reference_tables() {
		assert!(is_whitespace_char('\t'));
		assert!(is_whitespace_char(' '));
		assert!(!is_whitespace_char('a'));
		assert!(is_punctuation_char('!'));
		assert!(is_punctuation_char('\\'));
		assert!(!is_punctuation_char('a'));
	}

	#[test]
	fn thai_lao_am_vowels_decompose() {
		assert_eq!(normalize_terminal_output("a\u{e33}b"), "a\u{e4d}\u{e32}b");
		assert_eq!(normalize_terminal_output("a\u{eb3}b"), "a\u{ecd}\u{eb2}b");
		assert_eq!(normalize_terminal_output("plain"), "plain");
	}

	#[test]
	fn terminal_sequences_are_stripped_but_text_survives() {
		assert_eq!(strip_terminal_sequences("\x1b[31mhi\x1b[0m"), "hi");
		assert_eq!(strip_terminal_sequences("\x1b]8;;https://x\x1b\\link\x1b\\"), "link");
		assert_eq!(strip_terminal_sequences("\x1b]8;;u\x07x"), "x");
		assert_eq!(strip_terminal_sequences("\x1b_APC\x07y"), "y");
		assert_eq!(strip_terminal_sequences("plain"), "plain");
	}

	#[test]
	fn literal_wrapping_keeps_oversized_graphemes() {
		assert_eq!(wrap_literal_line("abcdef", 2), vec!["ab", "cd", "ef"]);
		assert_eq!(wrap_literal_line("", 4), vec![""]);
		assert_eq!(wrap_literal_line("한", 1), vec!["한"]);
	}

	#[test]
	fn centering_and_single_lining_match_the_reference() {
		assert_eq!(center_line("ab", 6), "  ab  ");
		assert_eq!(center_line("abcdef", 3), "abc");
		assert_eq!(single_line("a\tb\r\nc"), "a   b c");
	}

	#[test]
	fn background_wraps_the_padding_and_reopens_after_resets() {
		let bg = |text: &str| format!("\x1b[44m{text}\x1b[0m");
		assert_eq!(apply_background_to_line("ab", 4, &bg), "\x1b[44mab  \x1b[0m");
		assert_eq!(
			apply_background_to_line("a\x1b[49mb", 4, &bg),
			"\x1b[44ma\x1b[49m\x1b[44mb  \x1b[0m"
		);
	}
}
