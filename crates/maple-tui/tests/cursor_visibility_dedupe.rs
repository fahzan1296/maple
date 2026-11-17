//! Port of `l/test/cursor-visibility-dedupe.test.ts`.
//!
//! Deviations: the reference drives a real `ProcessTerminal` and inspects its
//! captured stdout writes. The crate's write dedupe lives in the pure
//! [`CursorVisibilityTracker`]; these tests exercise that tracker's decisions
//! and sniffing rules directly. The `ProcessTerminal` emit path itself is not
//! asserted without a TTY.

use maple_tui::terminal::cursor_visibility::CursorVisibilityTracker;

const HIDE: &str = "\x1b[?25l";
const SHOW: &str = "\x1b[?25h";

#[test]
fn writes_each_visibility_change_once_and_skips_same_state_repeats() {
	let mut tracker = CursorVisibilityTracker::new();
	assert!(tracker.should_hide(false));
	tracker.record_hide();
	assert!(!tracker.should_hide(false), "second hide must be skipped");
	assert!(tracker.should_show(false));
	tracker.record_show();
	assert!(!tracker.should_show(false), "second show must be skipped");
	assert!(tracker.should_hide(false));
	tracker.record_hide();
	assert_eq!(tracker.is_visible(), Some(false));
}

#[test]
fn force_writes_always_emit() {
	let mut tracker = CursorVisibilityTracker::new();
	tracker.record_hide();
	assert!(tracker.should_show(true));
	assert!(tracker.should_hide(true));
}

#[test]
fn tracks_cursor_sequences_embedded_in_frame_writes() {
	let mut tracker = CursorVisibilityTracker::new();
	tracker.record_hide();
	tracker.track_write(&format!("frame{SHOW}"));
	assert_eq!(tracker.is_visible(), Some(true));
	tracker.track_write(&format!("frame{HIDE}"));
	assert_eq!(tracker.is_visible(), Some(false));
}

#[test]
fn resets_to_unknown_when_an_alt_screen_switch_is_newer_than_the_last_cursor_sequence() {
	let mut tracker = CursorVisibilityTracker::new();
	tracker.record_show();
	tracker.track_write(SHOW);
	assert_eq!(tracker.is_visible(), Some(true));
	tracker.track_write("\x1b[?1049h");
	assert_eq!(tracker.is_visible(), None, "alt-screen switch resets tracking");
	tracker.track_write("\x1b[?1049l");
	assert_eq!(tracker.is_visible(), None, "leaving the alt screen stays unknown");
}