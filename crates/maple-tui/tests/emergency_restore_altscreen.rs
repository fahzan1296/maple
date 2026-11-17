//! Port of `l/test/emergency-restore-altscreen.test.ts`.
//!
//! The reference captures the real process write path and asserts when
//! `emergencyTerminalRestore()` emits DECRST 1049, plus the keyboard
//! enhancement frame ordering around it. The crate's `ProcessTerminal` has no
//! alt-screen tracking, no emergency-restore entry point and no captured
//! stdout, so every assertion is ignored; the bodies pin the backend's
//! event seam that a future port of that path would build on.

use std::time::Duration;

use maple_tui::terminal::process::ProcessTerminal;
use maple_tui::terminal::Terminal;

#[test]
#[ignore = "missing facility: no emergencyTerminalRestore or alt-screen tracking in the crate"]
fn does_not_emit_decrst_1049_on_the_post_stop_path_when_the_alt_screen_was_never_entered() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no setAltScreenActive state, so the alt-screen-gated DECRST 1049 cannot be asserted"]
fn emits_decrst_1049_on_the_post_stop_path_while_the_alt_screen_is_tracked_active() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no live-terminal crash path or alt-screen state in the crate"]
fn emits_decrst_1049_on_the_live_terminal_crash_path_only_when_the_alt_screen_is_tracked_active() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no frame provider or deferred replacement paint, so a throwing repaint cannot be driven"]
fn restores_the_alternate_screen_when_a_deferred_replacement_paint_throws() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no kitty keyboard-enhancement push/pop tracking in the crate"]
fn pops_keyboard_enhancement_frames_on_both_screens_when_crashing_from_a_fullscreen_overlay() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}
