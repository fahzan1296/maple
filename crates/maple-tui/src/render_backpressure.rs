//! Adaptive render backpressure (pure scheduler math).
//!
//! Reference: `l/src/tui.ts`.
//!
//! Ports the delay math of `TUI.#scheduleRender`: a cheap frame keeps the plain
//! cadence floor, a slow frame inflates the follow-up delay toward the frame
//! cost, and the inflation is capped at [`MAX_ADAPTIVE_RENDER_MS`] so one
//! pathological frame cannot lock the UI (#4145). The input-grace term and the
//! terminal output-backlog retry are host concerns and are not modeled here.

/// Inflation ceiling for a follow-up delay (reference `MAX_ADAPTIVE_RENDER_MS`).
pub const MAX_ADAPTIVE_RENDER_MS: f64 = 200.0;

/// Input-frame cadence floor (reference `INPUT_FRAME_INTERVAL_MS`).
pub const INPUT_FRAME_INTERVAL_MS: f64 = 1000.0 / 30.0;

/// The adaptive floor: twice the last frame cost, capped at
/// [`MAX_ADAPTIVE_RENDER_MS`].
pub fn adaptive_floor_ms(frame_cost_ms: f64) -> f64 {
	(frame_cost_ms * 2.0).min(MAX_ADAPTIVE_RENDER_MS)
}

/// Last-frame bookkeeping for [`RenderPacer`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderPacer {
	last_render_at: Option<f64>,
	last_frame_cost_ms: f64,
}

impl RenderPacer {
	/// A pacer with no recorded frame yet.
	pub fn new() -> Self {
		Self::default()
	}

	/// Record a painted frame's wall-clock span on the scheduler clock (ms).
	pub fn record_frame(&mut self, start_ms: f64, end_ms: f64) {
		self.last_render_at = Some(start_ms);
		self.last_frame_cost_ms = (end_ms - start_ms).max(0.0);
	}

	/// Delay before the next frame may paint, given the caller's minimum
	/// cadence interval. `max(cadenceDelay, adaptiveDelay)` from the reference.
	pub fn next_delay(&self, now_ms: f64, min_interval_ms: f64) -> f64 {
		let elapsed = match self.last_render_at {
			Some(at) => (now_ms - at).max(0.0),
			None => 0.0,
		};
		let cadence = (min_interval_ms - elapsed).max(0.0);
		let adaptive = (adaptive_floor_ms(self.last_frame_cost_ms) - elapsed).max(0.0);
		cadence.max(adaptive)
	}
}
