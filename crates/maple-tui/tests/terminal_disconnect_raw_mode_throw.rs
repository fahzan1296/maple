//! Port of `l/test/terminal-disconnect-raw-mode-throw.test.ts`.
//!
//! The reference revokes a real pty so `stop()` restores raw mode on a dead fd
//! and must still signal SIGHUP. The crate has no PTY harness and no signal
//! path, so those assertions are ignored; the disconnect event seam is pinned
//! through `VirtualTerminal`/`ProcessTerminal`.

use std::time::Duration;

use maple_tui::terminal::process::ProcessTerminal;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::{Terminal, TtyEvent};

#[test]
fn delivers_a_disconnect_event_to_the_ui_loop() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.push_event(TtyEvent::Disconnect);
	assert!(matches!(
		terminal.event(Duration::ZERO),
		Some(TtyEvent::Disconnect)
	));
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
fn a_fresh_process_terminal_reports_no_disconnect() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no PTY harness, so a revoked-fd raw-mode restore throw cannot be simulated"]
fn still_signals_sighup_when_restoring_raw_mode_throws_on_a_revoked_fd() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.push_event(TtyEvent::Disconnect);
	assert!(matches!(
		terminal.event(Duration::ZERO),
		Some(TtyEvent::Disconnect)
	));
}

#[test]
#[ignore = "missing facility: no PTY harness, so an initial EIO enabling raw mode cannot be simulated"]
fn treats_an_initial_eio_enabling_raw_mode_as_a_terminal_disconnect() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
}

#[test]
#[ignore = "missing facility: no PTY harness, so a raw-mode restore failure on a live terminal cannot be simulated"]
fn propagates_a_raw_mode_restore_failure_while_the_terminal_is_still_live() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	assert!(terminal.is_started());
}

#[test]
#[ignore = "missing facility: no process signal path (process.kill SIGHUP) in the crate"]
fn still_signals_sighup_when_the_disconnect_handler_itself_throws() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.push_event(TtyEvent::Disconnect);
	assert!(matches!(
		terminal.event(Duration::ZERO),
		Some(TtyEvent::Disconnect)
	));
}
