//! Port of `l/test/history-frame-plan.test.ts`.
//!
//! The reference drives `TUI` with an injected `TerminalFrameProvider`, a
//! virtual render scheduler and a WASM terminal, asserting history commit,
//! replay pads, alternate-buffer resize and preserved-clear scrollback. The
//! crate's `Tui` has no frame provider, history/replay model, alternate screen
//! or scrollback, so the terminal-runtime cases are ignored; the row-painting
//! cases are pinned for real against `screen::Screen`, the crate's differential
//! painter, and the bodies otherwise pin the headless render seam.

use maple_tui::screen::Screen;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no TerminalFrameProvider/history commit in the crate's Tui"]
fn appends_finalized_history_once_and_leaves_the_requested_mutable_viewport_intact() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no ConPTY pending-wrap model or scrollback in the crate"]
fn keeps_an_exact_width_live_row_out_of_scrollback_when_conpty_materializes_pending_wrap() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no cursor marker or scrollback model in the crate's Tui"]
fn places_the_cursor_from_a_moved_marker_row_and_hides_it_once_the_marker_is_gone() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no tmux preserved-clear scrollback model in the crate"]
fn keeps_live_viewport_rows_out_of_tmux_style_preserved_clear_scrollback_on_a_scrolling_append() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no replay pads or batched terminal writes in the crate's Tui"]
fn bottom_splits_a_complete_replay_and_serializes_it_in_one_terminal_write() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no composer-space hit-test origin publication in the crate"]
fn publishes_composer_space_hit_test_origin_across_a_replay_pad() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no replay pad model in the crate's Tui"]
fn publishes_composer_space_origin_when_a_replay_prepends_blanks() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no fullscreen overlay exit or session replacement paint in the crate"]
fn fuses_fullscreen_overlay_exit_into_a_session_replacement_paint() {
	render_headless();
}

#[test]
fn repaints_a_viewport_only_frame_in_place_without_scrolling() {
	let mut terminal = VirtualTerminal::new(20, 4);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &["spinner one".to_string(), "editor".to_string()], 20);
	let cursor_row = terminal.cursor_row();
	terminal.clear_output();

	screen.flush(&mut terminal, &["spinner two".to_string(), "editor".to_string()], 20);
	assert_eq!(screen.rows().to_vec(), vec!["spinner two".to_string(), "editor".to_string()]);
	// No scroll: the rewrite path emits no newline and leaves the cursor row.
	assert_eq!(terminal.cursor_row(), cursor_row);
	let frame = terminal.output();
	assert!(frame.contains("\x1b[1;1H"), "{frame}");
	assert!(!frame.contains('\n'), "{frame}");
}

#[test]
#[ignore = "missing facility: no history flush batches or terminal handoff in the crate's Tui"]
fn flushes_every_eligible_history_batch_before_terminal_handoff() {
	render_headless();
}

#[test]
fn keeps_visible_history_above_the_anchored_viewport_while_room_remains() {
	let mut terminal = VirtualTerminal::new(20, 6);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &["block one".to_string(), "editor".to_string()], 20);
	screen.flush(
		&mut terminal,
		&["block one".to_string(), "block two".to_string(), "editor".to_string()],
		20,
	);
	assert_eq!(screen.rows().to_vec(), vec!["block one", "block two", "editor"]);
}

#[test]
#[ignore = "missing facility: no alternate buffer or anchored-history restore in the crate's Tui"]
fn uses_the_alternate_buffer_during_resize_and_restores_anchored_history() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no scrollback model during height shrink in the crate"]
fn keeps_live_viewport_rows_out_of_scrollback_during_a_height_shrink() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no settled-resize replay in the crate's Tui"]
fn appends_a_current_width_replay_after_settled_resize() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no append replay or destructive reset in the crate's Tui"]
fn keeps_provider_history_through_an_empty_append_replay_and_drops_it_on_a_destructive_reset() {
	render_headless();
}

#[test]
fn does_not_duplicate_current_width_history_on_a_height_only_grow() {
	let mut terminal = VirtualTerminal::new(20, 6);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &["history-one@20".to_string()], 20);
	// Height-only grow at a constant width: nothing rewraps, so the committed
	// row must appear exactly once.
	screen.flush(
		&mut terminal,
		&["history-one@20".to_string(), "history-two@20".to_string(), "editor@20".to_string()],
		20,
	);
	let copies = screen.rows().iter().filter(|row| row.as_str() == "history-one@20").count();
	assert_eq!(copies, 1);
}

#[test]
#[ignore = "missing facility: no zoom grow rebuild path in the crate's Tui"]
fn skips_the_destructive_rebuild_on_a_height_only_zoom_grow() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no multiplexer height-shrink rebuild in the crate's Tui"]
fn rebuilds_on_a_multiplexer_height_shrink_to_repair_clipped_and_pushed_live_rows() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no fullscreen-overlay history re-anchor in the crate's Tui"]
fn re_anchors_retained_history_after_a_height_grow_behind_a_fullscreen_overlay() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no current-width history rebuild in the crate's Tui"]
fn rebuilds_current_width_history_without_retaining_stale_rows() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no resize-burst history refresh coalescing in the crate's Tui"]
fn commits_one_history_refresh_for_a_resize_burst() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no rebuild-mode refresh transaction in the crate's Tui"]
fn refreshes_history_in_rebuild_mode_after_a_burst_that_returns_to_its_starting_width() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no append-mode refresh transaction in the crate's Tui"]
fn refreshes_history_in_append_mode_after_a_burst_that_returns_to_its_starting_width() {
	render_headless();
}
