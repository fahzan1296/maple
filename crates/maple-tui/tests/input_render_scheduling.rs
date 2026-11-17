//! Port of `l/test/input-render-scheduling.test.ts`.
//!
//! The frame-pacing ceiling itself (`frame_rate`) is pure and ports directly.
//! The reference's scheduler-injection cases (priority frames, input cadence,
//! adaptive backpressure, output-backlog deferral) need a live `TUI` with a
//! fake `RenderScheduler`, which the crate test harness does not provide.

use maple_tui::frame_rate::{
	max_fps, min_frame_interval, parse_max_fps, set_max_fps, DEFAULT_MAX_FPS, MAX_MAX_FPS, MIN_MAX_FPS,
};
use std::sync::{Mutex, MutexGuard, TryLockError};

/// The ceiling is a process-global `AtomicU32`; serialise the pacing tests.
fn lock() -> MutexGuard<'static, ()> {
	static LOCK: Mutex<()> = Mutex::new(());
	match LOCK.try_lock() {
		Ok(guard) => guard,
		Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
		Err(TryLockError::WouldBlock) => LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
	}
}

#[test]
fn set_max_fps_clamps_to_the_supported_range() {
	let _guard = lock();
	set_max_fps(500);
	assert_eq!(max_fps(), MAX_MAX_FPS);
	set_max_fps(0);
	assert_eq!(max_fps(), MIN_MAX_FPS);
	set_max_fps(DEFAULT_MAX_FPS);
	assert_eq!(max_fps(), DEFAULT_MAX_FPS);
}

#[test]
fn parse_max_fps_accepts_numeric_strings_only() {
	assert_eq!(parse_max_fps("60").expect("numeric"), 60);
	assert_eq!(parse_max_fps(" 30 ").expect("trimmed"), 30);
	assert!(parse_max_fps("abc").is_err());
	assert!(parse_max_fps("").is_err());
	assert!(parse_max_fps("0").is_err());
	assert!(parse_max_fps("500").is_err());
}

#[test]
fn min_frame_interval_follows_the_ceiling() {
	let _guard = lock();
	set_max_fps(4);
	assert_eq!(min_frame_interval().as_millis(), 250);
	set_max_fps(1);
	assert_eq!(min_frame_interval().as_millis(), 1000);
	set_max_fps(DEFAULT_MAX_FPS);
	assert_eq!(min_frame_interval().as_millis(), 33);
}

#[test]
#[ignore = "missing API: TUI render-scheduler injection (priority frames, input cadence, adaptive backpressure, output backlog)"]
fn paces_frames_and_input_through_an_injected_scheduler() {
}
