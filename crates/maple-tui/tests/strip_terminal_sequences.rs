//! Port of `l/test/strip-terminal-sequences.test.ts`.
//!
//! `stripTerminalSequences` is not a public crate API (`ansi_scan` is private),
//! so the exact-string assertions are ignored and the visible-width contract
//! they pin is asserted through `plain_width`.

use maple_tui::text_shape::{plain_width, visible_width};

#[test]
fn strips_csi_styling_sequences() {
	assert_eq!(plain_width("\x1b[31mred\x1b[0m"), 3);
	assert_eq!(visible_width("red"), 3);
}

#[test]
fn strips_osc_8_hyperlinks_terminated_by_bel() {
	let link = "\x1b]8;;https://example.com\x07click\x1b]8;;\x07";
	assert_eq!(plain_width(link), 3);
}

#[test]
fn strips_osc_8_hyperlinks_terminated_by_st() {
	let link = "\x1b]8;;https://example.com\x1b\\click\x1b]8;;\x1b\\";
	assert_eq!(plain_width(link), 3);
}

#[test]
fn strips_apc_sequences() {
	assert_eq!(plain_width("\x1b_pi:c\x07"), 0);
	assert_eq!(plain_width("before\x1b_pi:c\x07after"), 11);
}

#[test]
fn strips_mixed_sequences_while_preserving_visible_text() {
	let input = "\x1b[1mbold \x1b]8;;https://pi.dev\x07link\x1b]8;;\x07\x1b[0m \x1b_pi:c\x07plain";
	assert_eq!(plain_width(input), "bold link plain".len());
}

#[test]
#[ignore = "missing facility: stripTerminalSequences is not public (ansi_scan is a private module), so exact stripped strings cannot be asserted"]
fn exact_stripped_strings() {
	assert_eq!(plain_width("\x1b[31mred\x1b[0m"), 3);
}
