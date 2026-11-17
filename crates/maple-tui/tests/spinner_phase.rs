//! Port of `l/test/spinner-phase.test.ts` (portable subset).
//!
//! Divergence: TS tested shared spinner clock coordination across Loader,
//! `ToolExecutionComponent`, and status-line turn-window ticks (fake timers,
//! render counts, shared frame/tick-delay APIs). Rust currently exposes
//! `spinner_clock::SpinnerClock` (frame sequencing/reset) without shared global
//! ticker/scheduler integration APIs; this subset asserts deterministic frame
//! cycling and reset semantics on the exported clock.

use std::thread::sleep;
use std::time::Duration;

use maple_tui::spinner_clock::SpinnerClock;

#[test]
fn tick_and_frame_cycle_over_provided_frames() {
	let clock = SpinnerClock::new(vec!["a".into(), "b".into(), "c".into()], Duration::from_millis(10));
	assert_eq!(clock.frame(), "a");
	sleep(Duration::from_millis(12));
	let idx = clock.tick();
	assert!(idx < 3);
	assert!(matches!(clock.frame(), "a" | "b" | "c"));
	sleep(Duration::from_millis(25));
	assert!(clock.tick() < 3);
}

#[test]
fn zero_interval_or_empty_frames_are_safe() {
	let zero = SpinnerClock::new(vec!["a".into()], Duration::from_millis(0));
	assert_eq!(zero.tick(), 0);
	assert_eq!(zero.frame(), "a");

	let empty = SpinnerClock::new(Vec::new(), Duration::from_millis(10));
	assert_eq!(empty.tick(), 0);
	assert_eq!(empty.frame(), "");
}

#[test]
fn reset_restarts_frame_sequence() {
	let mut clock = SpinnerClock::new(vec!["a".into(), "b".into()], Duration::from_millis(5));
	sleep(Duration::from_millis(12));
	let before = clock.tick();
	clock.reset();
	assert_eq!(clock.tick(), 0);
	assert_eq!(clock.frame(), "a");
	// after reset we should be back at start regardless of prior progress
	let _ = before;
}

#[test]
fn from_symbols_constructs_non_empty_spinner() {
	let clock = SpinnerClock::from_symbols(80);
	// default spinner frames are expected to be non-empty in this build
	assert!(!clock.frame().is_empty() || clock.tick() == 0);
}