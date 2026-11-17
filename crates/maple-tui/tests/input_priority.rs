//! Port of `l/test/input-priority.test.ts`.
//!
//! Every case drives a live `TUI` with fake timers and a custom
//! `RenderScheduler`: queued double-Ctrl+C before a slow repaint, navigation
//! without an interrupt grace, editor space-hold routing, global Ctrl+O
//! fallback, listener refusal order, PTT raw visibility and BTW paste routing.
//! The crate has no TUI input-priority loop in its test harness, so all seven
//! stay ignored.

#[test]
#[ignore = "missing API: TUI input priority loop with fake timers (double-interrupt window)"]
fn handles_a_queued_second_ctrl_c_before_a_slow_repaint() {
}

#[test]
#[ignore = "missing API: TUI input priority loop with fake timers (navigation without interrupt grace)"]
fn renders_ordinary_navigation_without_an_interrupt_grace_delay() {
}

#[test]
#[ignore = "missing API: TUI input priority loop + CustomEditor space-hold routing"]
fn routes_an_enabled_main_editor_ctrl_o_hold_first() {
}

#[test]
#[ignore = "missing API: TUI input priority loop (global Ctrl+O action fallback)"]
fn preserves_the_global_ctrl_o_action_when_ptt_is_disabled() {
}

#[test]
#[ignore = "missing API: TUI input listener ordering (foreground listener first refusal)"]
fn gives_a_foreground_listener_first_refusal_over_a_preexisting_shortcut() {
}

#[test]
#[ignore = "missing API: TUI raw input listeners (printable PTT keys stay visible)"]
fn keeps_printable_ptt_keys_visible_to_raw_input_listeners() {
}

#[test]
#[ignore = "missing API: TUI input listener ordering (BTW Ctrl+V into its follow-up Input)"]
fn routes_btw_ctrl_v_into_its_active_follow_up_input() {
}
