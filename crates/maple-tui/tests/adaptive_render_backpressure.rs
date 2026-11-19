//! Port of `l/test/adaptive-render-backpressure.test.ts`.
//!
//! Deviations: the crate has no live TUI/timer loop, so the reference's
//! `DeferredRenderScheduler` is mirrored by a scripted clock: each step takes
//! `next_delay`, advances the clock by it (the scheduled timer), then records
//! the frame span. The input-grace term is not part of the pure pacer.

use maple_tui::render_backpressure::{
	INPUT_FRAME_INTERVAL_MS, MAX_ADAPTIVE_RENDER_MS, RenderPacer, adaptive_floor_ms,
};

const MIN_RENDER_INTERVAL_MS: f64 = INPUT_FRAME_INTERVAL_MS;

struct ScriptedClock {
	now_ms: f64,
	next_cost_ms: Option<f64>,
	pacer: RenderPacer,
}

impl ScriptedClock {
	fn new() -> Self {
		ScriptedClock {
			now_ms: 0.0,
			next_cost_ms: None,
			pacer: RenderPacer::new(),
		}
	}

	fn schedule_cost(&mut self, cost_ms: f64) {
		self.next_cost_ms = Some(cost_ms);
	}

	/// Fire the next scheduled render and return its delay.
	fn step(&mut self) -> f64 {
		let delay = self.pacer.next_delay(self.now_ms, MIN_RENDER_INTERVAL_MS);
		assert!(delay >= 0.0, "delay must never be negative");
		self.now_ms += delay;
		let cost = self.next_cost_ms.take().unwrap_or(0.0);
		let start = self.now_ms;
		self.now_ms += cost;
		self.pacer.record_frame(start, self.now_ms);
		delay
	}
}

#[test]
fn keeps_the_plain_min_interval_cadence_when_frames_are_cheap() {
	let mut clock = ScriptedClock::new();
	for _ in 0..3 {
		clock.schedule_cost(1.0);
		let delay = clock.step();
		assert!(
			delay <= MIN_RENDER_INTERVAL_MS + 1e-9,
			"cheap frames hug the cadence floor, got {delay}"
		);
	}
}

#[test]
fn inflates_the_next_delay_to_the_previous_frames_cost_after_a_slow_frame() {
	let mut clock = ScriptedClock::new();
	clock.schedule_cost(100.0);
	clock.step();

	clock.schedule_cost(1.0);
	let delay = clock.step();
	assert!(
		delay >= 100.0 - 1e-9,
		"slow frame must inflate the follow-up delay, got {delay}"
	);
}

#[test]
fn caps_the_adaptive_delay_so_a_pathological_frame_does_not_stall_the_ui() {
	assert_eq!(adaptive_floor_ms(5_000.0), MAX_ADAPTIVE_RENDER_MS);
	let mut clock = ScriptedClock::new();
	clock.schedule_cost(5_000.0);
	clock.step();

	clock.schedule_cost(1.0);
	let delay = clock.step();
	assert!(
		delay <= MAX_ADAPTIVE_RENDER_MS,
		"pathological frame must be capped, got {delay}"
	);
}
