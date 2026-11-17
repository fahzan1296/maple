//! Port of `l/test/cursor-visibility-dedupe.test.ts`.
//!
//! Deviations: the reference mocks process stdio and drives `ProcessTerminal`;
//! the crate exercises the same tracker through `VirtualTerminal`, which
//! captures outgoing bytes without a PTY. `ProcessTerminal` carries the same
//! `CursorVisibilityTracker` wiring.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;

const HIDE: &str = "\x1b[?25l";
const SHOW: &str = "\x1b[?25h";

fn terminal() -> VirtualTerminal {
	VirtualTerminal::new(20, 4)
}

#[test]
fn writes_each_visibility_change_once_and_skips_same_state_repeats() {
	let mut term = terminal();
	term.hide_cursor(false);
	term.hide_cursor(false);
	term.hide_cursor(false);
	assert_eq!(term.output(), HIDE);

	term.show_cursor(false);
	term.show_cursor(false);
	assert_eq!(term.output(), format!("{HIDE}{SHOW}"));

	term.hide_cursor(false);
	assert_eq!(term.output(), format!("{HIDE}{SHOW}{HIDE}"));
}

#[test]
fn tracks_cursor_sequences_embedded_in_frame_writes() {
	let mut term = terminal();
	term.write(&format!("\x1b[2Bframe content\x1b[5G{SHOW}\x1b[?2026l"));

	term.show_cursor(false);
	assert_eq!(term.output(), format!("\x1b[2Bframe content\x1b[5G{SHOW}\x1b[?2026l"));

	term.hide_cursor(false);
	assert!(term.output().ends_with(HIDE), "state change must write");
}

#[test]
fn honors_the_last_of_multiple_cursor_sequences_in_one_write() {
	let mut term = terminal();
	term.write(&format!("{SHOW}overlay paint{HIDE}"));

	term.hide_cursor(false);
	assert_eq!(term.output(), format!("{SHOW}overlay paint{HIDE}"));

	term.show_cursor(false);
	assert!(term.output().ends_with(SHOW));
}

#[test]
fn force_writes_regardless_of_tracked_state() {
	let mut term = terminal();
	term.show_cursor(false);
	term.show_cursor(true);
	term.show_cursor(true);
	assert_eq!(
		term.output(),
		format!("{SHOW}{SHOW}{SHOW}"),
		"force restores must always emit"
	);

	term.hide_cursor(false);
	term.hide_cursor(true);
	assert_eq!(term.output(), format!("{SHOW}{SHOW}{SHOW}{HIDE}{HIDE}"));
}

#[test]
fn resets_tracking_to_unknown_when_an_alt_screen_switch_follows_the_last_cursor_sequence() {
	let mut term = terminal();
	term.hide_cursor(false);
	term.write("\x1b[?1049h\x1b[2J");

	term.hide_cursor(false);
	assert!(
		term.output().ends_with(&format!("{HIDE}\x1b[?1049h\x1b[2J{HIDE}")),
		"alt-screen switch must invalidate the tracked state"
	);
}

#[test]
fn does_not_confuse_other_private_modes_with_cursor_visibility() {
	let mut term = terminal();
	term.hide_cursor(false);
	let after_hide = term.output().len();

	term.write("\x1b[?25$p\x1b[?2026h");
	term.hide_cursor(false);
	assert_eq!(
		term.output().len(),
		after_hide + "\x1b[?25$p\x1b[?2026h".len(),
		"DECRQM/unrelated private modes are not visibility changes"
	);
}
