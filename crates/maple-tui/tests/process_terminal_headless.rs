//! Port of `l/test/process-terminal-headless.test.ts`.
//!
//! The reference drives a real `ProcessTerminal` against a mocked TTY to prove
//! headless suppression, rmkx normalization, paste dispatch and progress
//! sequences. The crate has no headless gate on `ProcessTerminal` and no TTY
//! mock, so those are ignored; the detectable headless contract and the
//! terminal defaults are pinned through `is_terminal_headless`,
//! `ProcessTerminal::new` and `VirtualTerminal`.

use std::time::Duration;

use maple_tui::terminal::process::ProcessTerminal;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::terminal_capabilities::is_terminal_headless;

#[test]
fn reports_headless_when_stdout_is_not_an_interactive_terminal() {
	// libtest captures stdout, so the test process has no terminal attached.
	assert!(is_terminal_headless());
}

#[test]
fn process_terminal_constructs_without_a_tty_and_has_no_pending_events() {
	let mut terminal = ProcessTerminal::new();
	assert!(terminal.event(Duration::ZERO).is_none());
	assert!(terminal.columns() >= 1);
	assert!(terminal.rows() >= 1);
}

#[test]
fn virtual_terminal_start_writes_nothing_while_headless() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	assert!(terminal.is_started());
	assert_eq!(terminal.output(), "");
}

#[test]
fn virtual_terminal_stop_releases_the_claim() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	terminal.stop().expect("stop");
	assert!(!terminal.is_started());
}

#[test]
#[ignore = "missing facility: no headless gate on ProcessTerminal, so real-TTY write suppression cannot be observed"]
fn writes_nothing_to_a_real_tty_while_headless() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	assert_eq!(terminal.output(), "");
}

#[test]
#[ignore = "missing facility: rmkx/cursor-key normalization is emitted by the TS ProcessTerminal start/stop, not by the crate port"]
fn emits_rmkx_on_start_and_stop() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	terminal.stop().expect("stop");
	assert!(!terminal.is_started());
}

#[test]
#[ignore = "missing facility: bracketed-paste batching lives in the TS input pipeline; VirtualTerminal only replays queued events"]
fn dispatches_a_bracketed_paste_and_its_same_read_enter_as_one_input() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	assert!(terminal.is_started());
}

#[test]
#[ignore = "missing facility: Windows Terminal OSC 9;4 progress sequences are not emitted by the crate port"]
fn keeps_windows_terminal_progress_animation_continuous() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	assert!(terminal.is_started());
}

#[test]
#[ignore = "missing facility: Ghostty OSC 9;4 progress keep-alive is not emitted by the crate port"]
fn keeps_ghostty_progress_alive_while_work_remains_active() {
	let mut terminal = VirtualTerminal::new(80, 24);
	terminal.start().expect("start");
	assert!(terminal.is_started());
}
