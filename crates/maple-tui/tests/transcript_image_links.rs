//! Port of `l/test/transcript-image-links.test.ts`.
//!
//! A transcript seam must never cut through an inline image: the committed row
//! prefix stops below a whole image, and the image payload travels with the
//! seam. The crate keeps those rules in `chrome/transcript_container`, so the
//! port asserts payload extraction, direct-placement lift, and the per-protocol
//! cut rule.

use maple_tui::chrome::transcript_container::{
	cut_below_whole_image_with_protocol, direct_placement_lift, iterm2_image_payload,
};
use maple_tui::terminal_capabilities::{ImageProtocol, RuntimeTerminal, TerminalId};

const IMAGE: &str = "\x1b]1337;File=inline=1;width=2;preserveAspectRatio=1:QUJD\x07";

fn terminal(protocol: Option<ImageProtocol>) -> RuntimeTerminal {
	RuntimeTerminal::new(TerminalId::Base, protocol, true, true)
}

#[test]
fn iterm2_payload_is_the_body_between_the_marker_and_the_bell() {
	assert_eq!(iterm2_image_payload(IMAGE), Some("QUJD".to_string()));
	assert_eq!(
		iterm2_image_payload("\x1b]1337;File=inline=1:AAAA"),
		Some("AAAA".to_string())
	);
	assert_eq!(iterm2_image_payload("plain text"), None);
	assert_eq!(iterm2_image_payload("\x1b]1337;File=inline=1:\x07"), Some(String::new()));
}

#[test]
fn direct_placement_lift_counts_the_rows_an_image_paints_over() {
	assert_eq!(direct_placement_lift(&format!("\x1b7\x1b[3A{IMAGE}")), 3);
	assert_eq!(direct_placement_lift("\x1b7\x1b[2A"), 2);
	assert_eq!(direct_placement_lift("\x1b7\x1b[A"), 0);
	assert_eq!(direct_placement_lift("no lift"), 0);
	assert_eq!(direct_placement_lift(""), 0);
}

#[test]
fn a_cut_moves_below_a_whole_lifted_image() {
	let rows = vec![
		"a".to_string(),
		format!("\x1b7\x1b[2A{IMAGE}"),
		"b".to_string(),
	];
	let info = terminal(Some(ImageProtocol::Iterm2));

	assert!(info.is_image_line(&rows[1]));
	assert!(!info.is_image_line(&rows[0]));
	assert_eq!(cut_below_whole_image_with_protocol(&rows, 1, Some(ImageProtocol::Iterm2), &info), 2);
	assert_eq!(cut_below_whole_image_with_protocol(&rows, 2, Some(ImageProtocol::Iterm2), &info), 2);
	assert_eq!(cut_below_whole_image_with_protocol(&rows, 0, Some(ImageProtocol::Iterm2), &info), 0);
}

#[test]
fn protocols_that_draw_part_of_an_image_cut_where_asked() {
	let rows = vec![
		"a".to_string(),
		format!("\x1b7\x1b[2A{IMAGE}"),
		"b".to_string(),
	];
	for protocol in [None, Some(ImageProtocol::Kitty), Some(ImageProtocol::Sixel)] {
		let info = terminal(protocol);
		assert_eq!(cut_below_whole_image_with_protocol(&rows, 1, protocol, &info), 1);
	}
}

#[test]
fn an_image_carrying_row_is_only_recognised_through_its_protocol_marker() {
	let iterm = terminal(Some(ImageProtocol::Iterm2));
	assert!(iterm.is_image_line(IMAGE));
	assert!(!iterm.is_image_line("\x1bPq\"1;1;1;1"));
	assert!(!iterm.is_image_line("plain text"));

	let kitty = terminal(Some(ImageProtocol::Kitty));
	assert!(kitty.is_image_line("\x1b_Ga=T,f=10\x1b\\"));
	assert!(!kitty.is_image_line(IMAGE));
}
