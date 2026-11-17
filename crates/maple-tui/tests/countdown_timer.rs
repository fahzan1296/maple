//! Port of `l/test/countdown-timer.test.ts`.
//!
//! The reference uses fake timers; the crate's timer is deterministic over a
//! caller-supplied `now_ms`, so the same instants are replayed explicitly.

use maple_tui::chrome::countdown_timer::CountdownTimer;

#[test]
fn expires_using_precise_sub_second_timeout_instead_of_second_rounding() {
	let mut timer = CountdownTimer::new(250, 0);
	assert_eq!(timer.remaining_seconds(), 1);
	assert!(!timer.expire_if_due(249));

	assert!(timer.expire_if_due(250));
	assert!(!timer.expire_if_due(250));
}

#[test]
fn reset_restarts_the_precise_timeout_window() {
	let mut timer = CountdownTimer::new(300, 0);
	assert_eq!(timer.tick(200), None);
	timer.reset(200);
	assert!(!timer.expire_if_due(499));

	assert!(timer.expire_if_due(500));
}
