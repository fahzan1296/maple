//! Port of `l/test/focus-menu-regression.test.ts`.
//!
//! The reference drives a live `TUI` through focus changes and asserts the
//! viewport/cursor after a tall menu closes on an ED3-risk terminal. The crate
//! test harness has no TUI event loop or virtual terminal, so the case stays
//! ignored.

#[test]
#[ignore = "missing API: TUI event loop / virtual terminal (focus teardown repaint and commit-index resync)"]
fn repaints_stale_menu_and_working_rows_on_ed3_risk_terminals() {
}
