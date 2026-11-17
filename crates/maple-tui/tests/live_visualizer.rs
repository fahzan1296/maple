//! Port of `l/test/live-visualizer.test.ts`.
//!
//! Deviations:
//! - The reference component's `render(width)` is the bordered panel; the crate
//!   splits that into `LiveVisualizerModel::render_panel` (bordered) and
//!   `render` (compact meter). This test drives `render_panel`.
//! - The reference `initTheme(false)` becomes `theme::default_theme()`.

use std::cell::Cell;
use std::rc::Rc;

use maple_tui::apps::live_visualizer::{LivePhase, LiveVisualizerModel, LiveVisualizerOptions};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::default_theme;

struct Options {
	stopped: Rc<Cell<u32>>,
	muted: Rc<Cell<u32>>,
}

impl LiveVisualizerOptions for Options {
	fn on_stop(&mut self) {
		self.stopped.set(self.stopped.get() + 1);
	}
	fn on_toggle_mute(&mut self) {
		self.muted.set(self.muted.get() + 1);
	}
}

fn visualizer() -> (LiveVisualizerModel<Options>, Rc<Cell<u32>>, Rc<Cell<u32>>) {
	let stopped = Rc::new(Cell::new(0));
	let muted = Rc::new(Cell::new(0));
	let model = LiveVisualizerModel::new(Options {
		stopped: stopped.clone(),
		muted: muted.clone(),
	});
	(model, stopped, muted)
}

#[test]
fn renders_across_the_entire_provided_width_even_when_wider_than_120_columns() {
	let (mut visualizer, _, _) = visualizer();
	let theme = default_theme();
	for target_width in [80, 140, 200] {
		let lines = visualizer.render_panel(&theme, target_width);
		assert!(!lines.is_empty());
		for line in &lines {
			assert_eq!(visible_width(line), target_width, "row not padded: {line:?}");
		}
	}
}

#[test]
fn panel_carries_the_phase_status_key_hints_and_streaming_transcript() {
	let (mut visualizer, _, _) = visualizer();
	let theme = default_theme();
	visualizer.set_phase(LivePhase::Listening);
	visualizer.set_input_level(0.8);
	visualizer.set_frame(3);
	visualizer.set_transcript("hello from the microphone");
	let lines = visualizer.render_panel(&theme, 80);
	assert!(lines[0].starts_with('┌'));
	assert!(lines[lines.len() - 1].contains("listening"));
	assert!(lines[lines.len() - 1].contains("mute"));
	assert!(lines.iter().any(|line| line.contains("hello from the microphone")));
}

#[test]
fn stop_and_mute_chords_reach_the_callbacks_without_rerendering() {
	let (mut visualizer, stopped, muted) = visualizer();
	visualizer.handle_input("space");
	assert_eq!(muted.get(), 1);
	visualizer.handle_input("escape");
	assert_eq!(stopped.get(), 1);
	visualizer.handle_input("ctrl+c");
	assert_eq!(stopped.get(), 2);
	assert_eq!(muted.get(), 1);
}

#[test]
fn compact_meter_view_stays_three_rows() {
	let (mut visualizer, _, _) = visualizer();
	let lines = visualizer.render(80);
	assert_eq!(lines.len(), 3);
	assert_eq!(visible_width(&lines[1]), 76);
}
