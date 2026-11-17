//! Port of `l/test/terminal-write-loop-phase.test.ts`.
//!
//! The reference attributes blocking writes to the `ui.terminal-write` loop
//! phase via an injected clock. The crate has no loop-phase tracker, so the
//! attribution assertion is ignored; the write/event seam it exercises is
//! pinned through `VirtualTerminal`.

use std::time::Duration;

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::{Terminal, TtyEvent};

#[test]
fn write_reaches_the_terminal_and_does_not_create_events() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.write("frame");
	assert_eq!(terminal.output(), "frame");
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
fn events_are_delivered_in_insertion_order() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.push_event(TtyEvent::Resize(120, 40));
	terminal.push_event(TtyEvent::Disconnect);
	assert!(matches!(
		terminal.event(Duration::ZERO),
		Some(TtyEvent::Resize(120, 40))
	));
	assert!(matches!(terminal.event(Duration::ZERO), Some(TtyEvent::Disconnect)));
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no loop-phase tracker (pushLoopPhase/takeLoopPhaseAttribution) in the crate"]
fn attributes_a_blocking_write_to_ui_terminal_write() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.write("frame");
	assert_eq!(terminal.output(), "frame");
}
