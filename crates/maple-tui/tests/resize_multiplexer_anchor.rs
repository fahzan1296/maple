//! Port of `l/test/resize-multiplexer-anchor.test.ts`.
//!
//! The reference drives tmux's CPR probe transaction: stash/restore of input,
//! late-reply retirement, retry windows and the conservative pull bound. The
//! crate's `multiplexers` module only classifies host capabilities; there is no
//! CPR probe, input stash or resize transaction, so all cases stay ignored; the
//! bodies exercise the headless render seam such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no SIGWINCH erase or CPR transaction in the crate"]
fn skips_the_sigwinch_side_erase_so_a_racing_re_layout_cannot_blank_popped_scrollback() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no parked-cursor anchor model in the crate"]
fn anchors_a_settled_single_step_shrink_from_the_parked_cursor_not_content_depth() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no coalesced resize-burst accounting in the crate"]
fn treats_a_coalesced_multi_step_shrink_burst_cumulatively_from_pre_burst_state() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no settled-CPR outranking in the crate"]
fn lets_the_settled_cpr_outrank_the_clip_model_on_a_shrink() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no tmux grid-height wait in the crate"]
fn waits_for_the_new_height_when_tmux_answers_from_a_grid_taller_than_the_applications_cached_size() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no tmux width-reflow CPR math in the crate"]
fn accounts_for_tmux_width_reflow_in_the_cpr_offset_math() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no input stash or probe tags in the crate"]
fn preserves_modified_f3_input_while_a_probe_tag_is_outstanding() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no coalesced-burst direction reversal handling in the crate"]
fn falls_back_to_the_settled_cpr_when_a_coalesced_burst_reverses_direction() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no CPR-less grow anchor in the crate"]
fn anchors_a_doubly_cpr_less_grow_at_the_conservative_pull_bound() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no partial-regrowth accounting in the crate"]
fn accounts_for_partial_regrowth_in_the_cpr_less_pull_bound() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no late CPR reply resolution in the crate"]
fn resolves_a_cpr_less_grow_from_the_original_probes_late_reply() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no probe restart or reply retirement in the crate"]
fn discards_a_pre_restart_reply_instead_of_trusting_it() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no probe retry window in the crate"]
fn resolves_from_probe_bs_reply_arriving_during_the_retry_window() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no probe retry window in the crate"]
fn rejects_a_pre_restart_reply_arriving_during_the_retry_window() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no dropped-transaction recovery in the crate"]
fn recovers_eager_cpr_resolution_after_a_fully_dropped_transaction() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no retry-reply eager resolution in the crate"]
fn resolves_eagerly_from_the_retry_reply_when_the_original_was_dropped() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no live column tag tracking in the crate"]
fn does_not_reuse_a_live_column_tag_when_the_pane_narrows() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no cursor-parked probe offset in the crate"]
fn zeroes_the_probe_offset_after_the_erase_parks_the_cursor_on_the_viewport_top() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no mid-probe restart stash in the crate"]
fn keeps_the_stash_when_a_mid_probe_sigwinch_restarts_the_transaction() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no canceled-probe reply discard in the crate"]
fn discards_the_canceled_probes_delayed_reply_after_a_mid_probe_restart() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no replacement-probe reply resolution in the crate"]
fn resolves_the_replacement_probes_reply_when_the_canceled_reply_was_dropped() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no empty-viewport snapshotting in the crate"]
fn snapshots_a_legitimately_empty_viewport_on_a_fresh_transaction() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no pre-restart reply discard on grow in the crate"]
fn discards_a_pre_restart_reply_and_retries_on_a_grow() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no clamped CPR-less pull bound in the crate"]
fn anchors_a_cpr_less_grow_at_the_pull_bound_under_the_clamp() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no retired-transaction reply filtering in the crate"]
fn discards_a_retired_transactions_late_replies_exactly() {
	render_headless();
}
