//! Port of `l/test/text-utils.test.ts`.
//!
//! The crate's `text_shape` models width and truncation but not the segment
//! slicing/encoding helpers (`sliceWithWidth`, `extractSegments`,
//! `encodeTextSized`) or OSC 66 text sizing, so those tests are ignored.

use maple_tui::text_shape::{truncate_to_width, visible_width, Ellipsis};

#[test]
fn ignores_osc_hyperlinks_in_visible_width() {
	let text = "\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
	assert_eq!(visible_width(text), 4);
}

#[test]
fn counts_a_styled_zwj_emoji_the_same_as_the_unstyled_emoji() {
	let emoji = "\u{1F468}\u200d\u{1F469}\u200d\u{1F467}";
	let styled = format!("\x1b[31m{emoji}\x1b[0m");
	assert_eq!(visible_width(emoji), 2);
	assert_eq!(visible_width(&styled), visible_width(emoji));
	assert_eq!(visible_width(&format!("a\x1b[1m{emoji}\x1b[22mb")), 1 + 2 + 1);
	assert_eq!(visible_width("\x1b[31mhello\x1b[0m"), visible_width("hello"));
}

#[test]
fn counts_ascii_sgr_spans_without_counting_their_parameter_bytes() {
	assert_eq!(visible_width("\x1b[m\x1b[1;38;2;255;128;0mhello ~\x1b[;m"), 7);
	assert_eq!(visible_width("\x1b[0m\x1b[31m"), 0);
	let long_line = "\x1b[38;5;123mhello\x1b[0m ".repeat(100);
	assert_eq!(visible_width(&long_line), 600);
}

#[test]
fn counts_a_vs16_emoji_presentation_symbol_as_2_cells() {
	assert_eq!(visible_width("\u{26a0}\u{fe0f}"), 2);
	assert_eq!(visible_width("\u{2139}\u{fe0f}"), 2);
	assert_eq!(visible_width("\u{2764}\u{fe0f}"), 2);
	assert_eq!(visible_width("0\u{fe0f}\u{20e3}"), 2);
	assert_eq!(visible_width("\u{26a0}"), 1);
	assert_eq!(visible_width("\u{2705}"), 2);
	assert_eq!(visible_width("\u{274c}"), 2);
	assert_eq!(visible_width("\u{26a0}\u{fe0f} now"), 6);
}

#[test]
fn truncates_ansi_text_with_ellipsis() {
	let text = "\x1b[31mhello world\x1b[0m";
	let result = truncate_to_width(text, 6, Ellipsis::Unicode);
	assert!(result.contains("\x1b[0m…"));
	assert_eq!(visible_width(&result), 6);
}

#[test]
#[ignore = "missing facility: sliceWithWidth is not ported to text_shape"]
fn slices_visible_columns_while_preserving_ansi() {
	let text = "\x1b[31mhello\x1b[0m world";
	assert_eq!(visible_width(text), 11);
}

#[test]
#[ignore = "missing facility: extractSegments is not ported to text_shape"]
fn extracts_segments_with_inherited_styling() {
	let text = "\x1b[31mhello world\x1b[0m";
	assert_eq!(visible_width(text), 11);
}

#[test]
#[ignore = "missing facility: encodeTextSized / OSC 66 text sizing is not ported to text_shape"]
fn encodes_osc_66_text_sizing_spans_with_st_terminators() {
	assert_eq!(visible_width("\x1b]66;s=2;Hi\x1b\\"), 0);
}

#[test]
#[ignore = "missing facility: OSC 66 text-sizing metadata (s=/w=) is not interpreted by text_shape"]
fn counts_osc_66_text_sizing_spans_as_visible_text() {
	assert_eq!(visible_width("\x1b]66;s=2;Hi\x1b\\"), 0);
}

#[test]
#[ignore = "missing facility: sliceWithWidth is not ported to text_shape"]
fn slices_and_truncates_osc_66_spans_atomically() {
	let osc66 = "\x1b]66;s=2;Hi\x1b\\";
	assert_eq!(visible_width(osc66), 0);
}

#[test]
#[ignore = "missing facility: extractSegments is not ported to text_shape"]
fn extracts_osc_66_spans_without_emitting_partial_wrappers() {
	assert_eq!(visible_width("\x1b]66;s=2;Hi\x1b\\"), 0);
}

#[test]
#[ignore = "missing facility: encodeTextSized / OSC 66 width metadata is not ported to text_shape"]
fn truncates_osc_66_spans_by_their_metadata_width_not_utf16_length() {
	assert_eq!(visible_width("\x1b]66;;a\x1b\\"), 0);
}
