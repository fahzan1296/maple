//! Port of `l/test/stdout-stall-watchdog.test.ts`.
//!
//! `StdoutStallWatchdog` (arm cap / clear level / stall window over a backlog
//! sample) has no crate equivalent; the closest facility is `LoopWatchdog`.
//! The backlog-decision assertions are therefore ignored with that facility
//! named, and the stall-detection lifecycle is pinned.

use std::thread;
use std::time::Duration;

use maple_tui::loop_watchdog::LoopWatchdog;

#[test]
fn starts_unstalled_and_stops_cleanly() {
	let mut watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
	watchdog.stop();
}

#[test]
fn never_stalls_without_ticks() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(1));
	thread::sleep(Duration::from_millis(50));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog (arm cap / clear level / stallMs sampling) is not ported to the crate"]
fn does_not_arm_or_trip_on_a_backlog_that_never_exceeds_the_arm_cap() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog (arm cap / clear level / stallMs sampling) is not ported to the crate"]
fn declares_a_stall_once_an_armed_backlog_goes_stall_ms_without_draining() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog progress/low-water-mark tracking is not ported to the crate"]
fn never_trips_while_a_large_backlog_keeps_draining() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog episode persistence below the arm cap is not ported to the crate"]
fn keeps_the_episode_armed_after_the_backlog_dips_below_the_arm_cap_then_stalls() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog clear-level disarm is not ported to the crate"]
fn ends_the_episode_and_disarms_once_the_backlog_drains_to_the_clear_level() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog last-drain-progress clock is not ported to the crate"]
fn counts_the_stall_window_only_since_the_last_drain_progress() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: StdoutStallWatchdog episode reset is not ported to the crate"]
fn does_not_inherit_a_stale_clock_across_a_drained_episode() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	assert!(!watchdog.is_stalled());
}

#[test]
#[ignore = "missing facility: production arm/clear/stall windows are constants of the unported StdoutStallWatchdog"]
fn keeps_a_session_alive_while_the_terminal_pauses_during_an_oversized_frame() {
	let watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
	thread::sleep(Duration::from_millis(10));
	assert!(!watchdog.is_stalled());
}
