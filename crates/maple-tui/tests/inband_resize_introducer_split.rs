//! Port of `l/test/inband-resize-introducer-split.test.ts`.
//!
//! The reference feeds DEC 2048 in-band resize reports through the real
//! `ProcessTerminal` stdin buffer, split at every field boundary, and asserts
//! the geometry applies while no fragment reaches the input handler. The
//! crate's `ProcessTerminal` has no stdin buffering, no DEC 2048 parser and no
//! in-band geometry application, so every assertion is ignored; the bodies pin
//! the backend's event seam.

use std::time::Duration;

use maple_tui::terminal::process::ProcessTerminal;
use maple_tui::terminal::Terminal;

#[test]
#[ignore = "missing facility: no stdin read buffering or DEC 2048 in-band report parser in the crate"]
fn applies_and_swallows_a_report_whose_introducer_was_flushed_alone() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no split-report reassembly across stdin reads in the crate"]
fn applies_a_report_split_at_the_type_field() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no bundled introducer reassembly in the crate"]
fn applies_a_body_bundled_with_the_next_reports_introducer() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no unterminated-body hold in the crate"]
fn applies_a_body_that_arrives_without_its_terminator_and_swallows_the_delayed_one() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no held-body/keystroke boundary in the crate"]
fn applies_the_report_and_forwards_a_keystroke_that_ended_a_held_body() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no private-CSI reply interleaving in the crate"]
fn holds_the_introducer_across_a_pending_private_csi_reply() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no introducer grace window in the crate"]
fn releases_a_held_introducer_that_no_body_follows() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no held-introducer/CSI-key split handling in the crate"]
fn still_forwards_a_split_csi_key_after_a_held_introducer() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}
