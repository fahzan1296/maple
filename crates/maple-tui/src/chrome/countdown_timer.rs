//! Reusable countdown timer for dialog components.
//!
//! Reference: `l/src/chrome/countdown-timer.ts`.
//!
//! Deviations:
//! - The reference drives ticks with `setInterval`/`setTimeout` against a `TUI`
//!   host and distinguishes native rendering via a global `isNativeRendering()`.
//!   The crate has no host timer/event-loop integration in this module; this
//!   port provides deterministic time/state management driven by explicit
//!   [`CountdownTimer::tick`] and [`CountdownTimer::expire_if_due`] calls over a
//!   caller-supplied clock (`now_ms`).
//! - Native description returns an `elapsed` node with the same `age`/`format`
//!   semantics (negative remaining time so a host can count down).

use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props};

/// Countdown timer state.
pub struct CountdownTimer {
	initial_ms: u64,
	deadline_ms: u64,
	remaining_seconds: u64,
	expired: bool,
}

impl CountdownTimer {
	/// Start a countdown of `timeout_ms` from `now_ms`.
	pub fn new(timeout_ms: u64, now_ms: u64) -> Self {
		let mut timer = CountdownTimer {
			initial_ms: timeout_ms,
			deadline_ms: now_ms.saturating_add(timeout_ms),
			remaining_seconds: 0,
			expired: false,
		};
		timer.remaining_seconds = timer.calculate_remaining_seconds(now_ms);
		timer
	}

	fn calculate_remaining_seconds(&self, now_ms: u64) -> u64 {
		let remaining_ms = self.deadline_ms.saturating_sub(now_ms);
		remaining_ms.div_ceil(1000)
	}

	/// Current remaining whole seconds (ceiling), clamped at 0.
	pub fn remaining_seconds(&self) -> u64 {
		self.remaining_seconds
	}

	/// Deadline in the same clock domain as `now_ms`.
	pub fn deadline_ms(&self) -> u64 {
		self.deadline_ms
	}

	/// Advance state to `now_ms`, returning `Some(seconds)` when the visible
	/// second count changed.
	pub fn tick(&mut self, now_ms: u64) -> Option<u64> {
		if self.expired {
			return None;
		}
		let next = self.calculate_remaining_seconds(now_ms);
		if next != self.remaining_seconds {
			self.remaining_seconds = next;
			Some(next)
		} else {
			None
		}
	}

	/// Mark expired and return `true` once when due at `now_ms`.
	pub fn expire_if_due(&mut self, now_ms: u64) -> bool {
		if self.expired {
			return false;
		}
		if now_ms >= self.deadline_ms {
			self.expired = true;
			self.remaining_seconds = 0;
			return true;
		}
		false
	}

	/// Whether the timer has expired.
	pub fn is_expired(&self) -> bool {
		self.expired
	}

	/// Reset the countdown to its initial value from `now_ms`.
	pub fn reset(&mut self, now_ms: u64) {
		self.deadline_ms = now_ms.saturating_add(self.initial_ms);
		self.expired = false;
		self.remaining_seconds = self.calculate_remaining_seconds(now_ms);
	}

	/// Dispose/cancel the timer (idempotent).
	pub fn dispose(&mut self) {
		self.expired = true;
	}

	/// Remaining time as a terminal-clocked `elapsed` node.
	///
	/// `age` is the negative remaining time (start lies in the future) so a host
	/// can count down to zero and hold at expiry.
	pub fn describe(&self, now_ms: u64) -> NativeNode {
		let remaining = self.deadline_ms as i64 - now_ms as i64;
		let age = remaining.min(0);
		let mut props = Props::new();
		props.set_i64("age", age);
		props.set_str("format", "short");
		node(NodeKind::Elapsed, props, Vec::new(), None)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn counts_down_and_expires() {
		let mut t = CountdownTimer::new(3000, 1000);
		assert_eq!(t.remaining_seconds(), 3);
		assert_eq!(t.tick(2000), Some(2));
		assert_eq!(t.tick(2500), None);
		assert!(!t.expire_if_due(3999));
		assert!(t.expire_if_due(4000));
		assert!(t.is_expired());
		assert!(!t.expire_if_due(5000));
	}

	#[test]
	fn reset_restarts_from_now() {
		let mut t = CountdownTimer::new(2000, 0);
		let _ = t.tick(1500);
		t.reset(5000);
		assert_eq!(t.remaining_seconds(), 2);
		assert_eq!(t.deadline_ms(), 7000);
		assert!(!t.is_expired());
	}

	#[test]
	fn describe_uses_negative_remaining_age() {
		let t = CountdownTimer::new(2000, 1000);
		let node = t.describe(1500);
		assert_eq!(node.kind, NodeKind::Elapsed);
		assert_eq!(node.prop("age").and_then(|v| v.as_i64()), Some(-1500));
		assert_eq!(node.prop("format").and_then(|v| v.as_str()), Some("short"));
		let expired = CountdownTimer::new(1000, 0);
		let node = expired.describe(5000);
		assert_eq!(node.prop("age").and_then(|v| v.as_i64()), Some(0));
	}

	#[test]
	fn dispose_marks_expired() {
		let mut t = CountdownTimer::new(5000, 0);
		t.dispose();
		assert!(t.is_expired());
		assert_eq!(t.tick(1000), None);
	}
}
