//! Port of `l/test/drain-input-da1-sentinel.test.ts`.
//!
//! The reference gates a DA1 (`CSI ? … c`) capability probe inside
//! `ProcessTerminal.drainInput()` and asserts when the sentinel is written and
//! awaited. The crate has no `ProcessTerminal` drain path and no DA1 sentinel
//! state machine, so all three cases stay ignored.

#[test]
#[ignore = "missing API: ProcessTerminal::drain_input DA1 sentinel gating (non-TSP terminals)"]
fn does_not_write_a_da1_sentinel_on_non_tsp_terminals() {
}

#[test]
#[ignore = "missing API: ProcessTerminal::drain_input DA1 sentinel write + await (active TSP)"]
fn writes_and_awaits_a_da1_sentinel_when_tsp_is_active() {
}

#[test]
#[ignore = "missing API: ProcessTerminal::drain_input DA1 sentinel while the TSP probe is pending"]
fn writes_a_da1_sentinel_when_the_tsp_probe_is_pending() {
}
