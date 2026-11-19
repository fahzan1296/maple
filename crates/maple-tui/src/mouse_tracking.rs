//! Inline mouse-tracking state machine.
//!
//! Ported from the mouse-tracking section of `l/src/tui.ts` (the
//! `#mouseTracking` field, `#setMouseTracking`, and the inline provider).
//! The crate has no alternate-screen runtime, so the fullscreen state maps to
//! [`MouseTrackingState::Full`] and the alt-screen enter/exit fusion is
//! omitted: capture is released by [`MouseTracking::shutdown`] on stop.

/// Capture mode the terminal is currently in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseTrackingState {
	/// No reporting sequences are active.
	#[default]
	Off,
	/// Inline (normal buffer) reporting is on.
	Inline,
	/// Reporting is on for a fullscreen overlay.
	Full,
}

pub const MOUSE_TRACKING_ON: &str = "\x1b[?1000h\x1b[?1003h\x1b[?1006h";
pub const MOUSE_TRACKING_OFF: &str = "\x1b[?1006l\x1b[?1003l\x1b[?1000l";

/// Tracks which reporting sequences have been written and emits the bytes
/// needed to move between states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseTracking {
	state: MouseTrackingState,
}

impl MouseTracking {
	pub fn new() -> Self {
		Self {
			state: MouseTrackingState::Off,
		}
	}

	/// The current capture state.
	pub fn state(&self) -> MouseTrackingState {
		self.state
	}

	/// Move to `want`, returning the bytes to write (empty when unchanged or
	/// when both states are enabled variants).
	pub fn reconcile(&mut self, want: MouseTrackingState) -> &'static str {
		if want == self.state {
			return "";
		}
		let was_off = self.state == MouseTrackingState::Off;
		self.state = want;
		if want == MouseTrackingState::Off {
			MOUSE_TRACKING_OFF
		} else if was_off {
			MOUSE_TRACKING_ON
		} else {
			""
		}
	}

	/// Release capture on shutdown, returning the bytes to write.
	pub fn shutdown(&mut self) -> &'static str {
		if self.state == MouseTrackingState::Off {
			return "";
		}
		self.state = MouseTrackingState::Off;
		MOUSE_TRACKING_OFF
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn writes_on_only_from_off_and_off_only_when_enabled() {
		let mut tracking = MouseTracking::new();
		assert_eq!(tracking.reconcile(MouseTrackingState::Inline), MOUSE_TRACKING_ON);
		assert_eq!(tracking.reconcile(MouseTrackingState::Inline), "");
		assert_eq!(tracking.reconcile(MouseTrackingState::Full), "");
		assert_eq!(tracking.reconcile(MouseTrackingState::Off), MOUSE_TRACKING_OFF);
		assert_eq!(tracking.reconcile(MouseTrackingState::Off), "");
	}

	#[test]
	fn shutdown_releases_once() {
		let mut tracking = MouseTracking::new();
		tracking.reconcile(MouseTrackingState::Full);
		assert_eq!(tracking.shutdown(), MOUSE_TRACKING_OFF);
		assert_eq!(tracking.shutdown(), "");
		assert_eq!(tracking.state(), MouseTrackingState::Off);
	}
}