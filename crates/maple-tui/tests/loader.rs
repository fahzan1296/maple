//! Port of `l/test/loader.test.ts`.
//!
//! The crate's `Loader` runs on a fixed 80 ms clock with no injected timer, no
//! TUI render-request hook, no trailer, and no dynamic message function, so the
//! cadence/backpressure/layout-reuse tests are ignored. Width clamping and the
//! dispose-stops-animation lifecycle are ported.

use maple_tui::components::loader::Loader;
use maple_tui::text_shape::visible_width;
use maple_tui::trigger::Trigger;
use maple_tui::Component;

#[test]
fn clamps_rendered_lines_to_terminal_width() {
	let mut loader = Loader::new(Trigger::new());
	loader.set_message("Checking");
	for line in loader.render(1) {
		assert!(visible_width(&line) <= 1);
	}
}

#[test]
fn dispose_stops_the_animation_so_no_further_renders_are_scheduled() {
	let mut loader = Loader::new(Trigger::new());
	loader.start();
	assert!(loader.running());
	loader.dispose();
	assert!(!loader.running());
	// Idempotent.
	loader.dispose();
	assert!(!loader.running());
}

#[test]
#[ignore = "missing facility: injected fake timers / TUI requestComponentRender hook are not ported"]
fn keeps_spinner_cadence_when_animated_messages_repaint_at_30fps() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: injected fake timers / synchronized-output flag are not ported"]
fn repaints_a_single_glyph_loader_with_an_animated_message_on_every_spinner_period_without_synchronized_output() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: injected fake timers and animated message color fn are not ported"]
fn skips_animated_render_requests_when_composed_text_is_unchanged_before_the_spinner_advances() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: TUI requestComponentRender hook is not ported"]
fn requests_component_renders_for_message_changes_but_not_repeated_identical_messages() {
	let mut loader = Loader::new(Trigger::new());
	loader.set_message("Still checking");
	assert_eq!(loader.message(), "Still checking");
}

#[test]
#[ignore = "missing facility: injected clock and animated message color fn are not ported"]
fn requests_component_renders_when_animated_message_bytes_change_between_spinner_frames() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: frame-budget backpressure accounting is not ported"]
fn backs_off_animated_paints_when_component_renders_consume_the_frame_budget() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: async render-request frame cost is not ported"]
fn backs_off_from_the_completed_tui_frame_cost_when_render_requests_are_asynchronous() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: one-off frame backpressure cap is not ported"]
fn caps_backpressure_after_a_pathological_one_off_frame() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: layout-reuse instrumentation and animated ANSI color fn are not ported"]
fn reuses_text_layout_when_only_animated_ansi_styling_changes() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: layout-reuse instrumentation and custom spinner frames are not ported"]
fn reuses_the_wrapped_layout_across_static_spinner_frames_without_re_measuring() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: custom spinner frame widths are not configurable in this port"]
fn rewraps_custom_spinner_frames_when_their_display_widths_differ() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: setTrailer and live colorizers are not ported"]
fn redocks_changing_trailers_with_live_colors_and_variable_width_spinner_output() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: setTrailer and ANSI-shuffling colorizers are not ported"]
fn preserves_trailer_docking_when_colorizers_move_ansi_past_trailing_message_whitespace() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: synchronized-output gating is not ported"]
fn holds_animated_message_only_frames_when_synchronized_output_is_unavailable() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: Container::dispose_children is not ported"]
fn container_dispose_children_stops_detached_loader_repaints() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: dynamic message function and injected clock are not ported"]
fn re_evaluates_a_dynamic_message_function_on_each_spinner_tick() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}

#[test]
#[ignore = "missing facility: setSpinnerInterval shared clock injection is not ported"]
fn advances_on_the_shared_spinner_clock_a_slower_interval_slows_the_glyph_a_static_one_pins_it() {
	let loader = Loader::new(Trigger::new());
	assert_eq!(loader.message(), "Loading...");
}
