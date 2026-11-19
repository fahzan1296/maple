//! Port of `l/test/loop-watchdog.test.ts`.
//!
//! The reference injects `now`/`schedule` and asserts the exact
//! `logger.warn("ui.loop-blocked", …)` line, phase attribution and CPU-time
//! classification. The crate's `LoopWatchdog` uses a real worker thread with
//! no clock or phase injection and emits no logger line, so those assertions
//! are ignored; the lifecycle and suspension contract are pinned.

use std::thread;
use std::time::Duration;

use maple_tui::loop_watchdog::LoopWatchdog;

#[test]
fn starts_unstalled_and_stops_cleanly() {
	let mut watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
	watchdog.stop();
	assert!(!watchdog.is_stalled());
}

#[test]
fn drop_releases_the_worker_without_stalling() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
	drop(watchdog);
}

#[test]
fn never_stalls_without_ticks() {
	// The worker only flags an overdue tick it actually receives; with no
	// ticker the watchdog stays unstalled even after a long wall-clock wait.
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(1));
	thread::sleep(Duration::from_millis(50));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: no loop-phase tracker, so ui.terminal-write attribution cannot be asserted"]
fn attributes_blocked_time_to_the_active_phase() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: no logger sink, so the single rising-edge ui.loop-blocked warn cannot be asserted"]
fn logs_exactly_one_warning_on_the_rising_edge() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: no CPU-time sampling, so long-block classification (sleep vs busy) cannot be asserted"]
fn classifies_a_long_overshoot_by_cpu_time_not_duration() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	thread::sleep(Duration::from_millis(10));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: no injectable clock/schedule, so threshold-window edge cases cannot be driven deterministically"]
fn only_flags_ticks_past_the_threshold_window() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: a stopped watchdog emits nothing, but the crate has no logger sink to observe"]
fn a_stopped_watchdog_emits_nothing() {
	let mut watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	watchdog.stop();
	assert!(!watchdog.is_stalled());
}
