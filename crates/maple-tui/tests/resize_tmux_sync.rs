//! Port of `l/test/resize-tmux-sync.test.ts`.
//!
//! The reference drives tmux synchronized-output holds, large replay padding
//! and alternate-screen fallback. The crate's `multiplexers` module only reports
//! `honors_synchronized_output`; there is no replay, hold or fallback path, so
//! all cases stay ignored; the bodies exercise the headless render seam such a
//! port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no drag-throttled rebuild or notice in the crate"]
fn keeps_the_screen_untouched_through_the_drag_then_rebuilds_once_without_flashing_a_notice() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no throttled-frame settlement in the crate"]
fn still_paints_the_settled_rebuild_when_a_throttled_frame_is_pending_as_the_resize_settles() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no synchronized-update notice in the crate"]
fn publishes_the_notice_as_its_own_synchronized_update_right_before_a_large_replay() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no replay padding or cell diffing in the crate"]
fn replays_compact_padding_with_identical_cells_colors_links_and_scrollback() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no tmux hold renewal in the crate"]
fn renews_tmuxs_hold_across_a_large_utf8_replay_and_releases_only_after_the_editor_cursor() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no queued-rebuild width selection in the crate"]
fn uses_the_latest_width_when_a_queued_rebuild_is_blocked_by_older_output() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no height-only short settle in the crate"]
fn keeps_height_only_grows_on_the_short_settle_without_replaying_history() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no stale-frame history recovery in the crate"]
fn recovers_history_overwritten_by_a_stale_frame_before_a_grow_notification() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no height-burst rebuild coalescing in the crate"]
fn rebuilds_once_after_a_height_burst_containing_a_shrink_even_when_it_ends_taller() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no alternate-screen fallback in the crate"]
fn retains_the_alternate_screen_fallback_when_tmux_reports_no_synchronized_output() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no synchronized-output opt-out in the crate"]
fn honors_a_synchronized_output_opt_out_even_when_tmux_reports_support() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no overlay-during-settle conversation restore in the crate"]
fn restores_the_rebuilt_conversation_after_an_overlay_opens_during_settling() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no pending-rebuild cancellation on stop in the crate"]
fn cancels_a_pending_rebuild_on_stop_without_later_writes_or_buffer_switches() {
	render_headless();
}
