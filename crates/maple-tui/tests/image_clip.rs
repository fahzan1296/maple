//! Port of `l/test/image-clip.test.ts`.
//!
//! The direct-placement wire format is ported into `kitty_graphics.rs`
//! (`encode_kitty_placement_line`, `parse_kitty_direct_placement_line`). The
//! lifecycle and uncroppable-block cases need the crate's frame writer plus a
//! live terminal engine, so they stay ignored.

use maple_tui::kitty_graphics::{
	encode_kitty_placement_line, parse_kitty_direct_placement_line, KittyPlacementLine,
};
use maple_tui::terminal_capabilities::{wrap_tmux_passthrough, TestEnv};

#[test]
fn round_trips_an_encoded_direct_placement_line() {
	let env = TestEnv::new();
	let line = encode_kitty_placement_line(7, 1, 4, 6, 9, 60, &env);
	assert_eq!(
		parse_kitty_direct_placement_line(&line),
		Some(KittyPlacementLine { image_id: 7, placement_id: Some(1), columns: 4, rows: 6 })
	);
}

#[test]
fn rejects_non_placement_image_lines() {
	// Placeholder virtual placement (`a=p,U=1` lead) is not a direct placement.
	assert_eq!(
		parse_kitty_direct_placement_line("\x1b_Ga=p,U=1,q=2,i=5,p=5,c=4,r=4\x1b\\"),
		None
	);
	// Transmit-and-display and plain text never match.
	assert_eq!(
		parse_kitty_direct_placement_line("\x1b_Ga=T,f=100,q=2,C=1,c=4,r=4;AAAA\x1b\\"),
		None
	);
	assert_eq!(parse_kitty_direct_placement_line("plain text"), None);
}

#[test]
fn recognizes_passthrough_placements() {
	let apc = wrap_tmux_passthrough("\x1b_Ga=p,q=2,C=1,i=5,p=5,c=4,r=4\x1b\\");
	let line = format!("\x1b7\x1b[3A{apc}\x1b8");
	assert_eq!(
		parse_kitty_direct_placement_line(&line),
		Some(KittyPlacementLine { image_id: 5, placement_id: Some(5), columns: 4, rows: 4 })
	);
}

#[test]
fn encodes_the_anchored_clipped_and_last_row_only_placement_forms() {
	let env = TestEnv::new();
	// Whole block visible (last line at viewport row 9): full anchored form.
	assert_eq!(
		encode_kitty_placement_line(7, 1, 4, 6, 9, 60, &env),
		"\x1b7\x1b[5A\x1b_Ga=p,q=2,C=1,i=7,p=1,c=4,r=6\x1b\\\x1b8"
	);
	// Straddling (last line at row 3): two rows hidden above, four visible —
	// the source slice starts at 60*2/6 = 20px.
	assert_eq!(
		encode_kitty_placement_line(7, 2, 4, 6, 3, 60, &env),
		"\x1b7\x1b[3A\x1b_Ga=p,q=2,C=1,i=7,p=2,c=4,r=4,y=20,h=40\x1b\\\x1b8"
	);
	// Only the last row visible: no cursor movement, bottom slice only.
	assert_eq!(
		encode_kitty_placement_line(7, 3, 4, 6, 0, 60, &env),
		"\x1b_Ga=p,q=2,C=1,i=7,p=3,c=4,r=1,y=50,h=10\x1b\\"
	);
}

#[test]
#[ignore = "missing harness: frame writer + live terminal graphics placement lifecycle"]
fn removes_moved_and_hidden_images_from_the_viewport() {
}

#[test]
#[ignore = "missing harness: frame writer + live terminal graphics placement lifecycle"]
fn deletes_a_live_copy_without_deleting_the_scrollback_copy() {
}

#[test]
#[ignore = "missing harness: frame writer + live terminal graphics placement lifecycle"]
fn preserves_replay_history_split_into_the_viewport() {
}

#[test]
#[ignore = "missing harness: frame writer + live terminal graphics placement lifecycle"]
fn does_not_draw_a_top_clipped_image_over_following_text() {
}
