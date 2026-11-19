//! Reference: `l/test/program-status-probe.test.ts`.
//!
//! The reference drives a real `ProcessTerminal` against a mocked TTY. This
//! port drives the crate's raw-stdin seam: `ProgramStatusProbe::begin` returns
//! the query action, `ProcessTerminal::feed_raw_stdin` reassembles bytes
//! through `StdinBuffer` and feeds the probe, and `stop` invalidates support.
//! No PTY, no process-stream mocking.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::terminal::process::ProcessTerminal;
use maple_tui::terminal::program_status::{ProgramStatusAction, PROGRAM_STATUS_QUERY};
use maple_tui::terminal_capabilities::{TERMINAL_MULTIPLEXER_ENV_KEYS, TestEnv};

const DA1_REPLY: &str = "\x1b[?1;2c";
const REPLY: &str = "\x1b]7501;?:states=idle,working,done,blocked,error\x1b\\";

fn subscribe(terminal: &mut ProcessTerminal) -> (Rc<RefCell<Vec<bool>>>, usize) {
	let log = Rc::new(RefCell::new(Vec::new()));
	let sink = Rc::clone(&log);
	let id = terminal.on_program_status_support(Box::new(move |supported| {
		sink.borrow_mut().push(supported);
	}));
	(log, id)
}

fn query() -> Vec<ProgramStatusAction> {
	vec![ProgramStatusAction::Write(PROGRAM_STATUS_QUERY.to_string())]
}

#[test]
fn confirms_any_direct_terminal_that_answers_even_without_rex_identity() {
	let mut terminal = ProcessTerminal::new();
	let (results, _id) = subscribe(&mut terminal);
	assert_eq!(
		terminal.program_status_probe_mut().begin(&TestEnv::new()),
		query()
	);

	assert!(terminal.feed_raw_stdin(&REPLY[..20]).is_empty());
	assert!(terminal.feed_raw_stdin(&REPLY[20..]).is_empty());
	assert_eq!(results.borrow().as_slice(), &[true]);

	let (late, late_id) = subscribe(&mut terminal);
	assert_eq!(late.borrow().as_slice(), &[true]);
	for _ in 0..15 {
		assert!(terminal.feed_raw_stdin(DA1_REPLY).is_empty());
	}
	assert_eq!(results.borrow().as_slice(), &[true]);
	terminal.unsubscribe_program_status_support(late_id);
	assert_eq!(late.borrow().as_slice(), &[true]);
}

#[test]
fn invalidates_support_on_stop_and_reprobes_the_resumed_terminal() {
	let mut terminal = ProcessTerminal::new();
	let (results, _id) = subscribe(&mut terminal);
	assert_eq!(
		terminal.program_status_probe_mut().begin(&TestEnv::new()),
		query()
	);
	terminal.feed_raw_stdin(REPLY);
	assert_eq!(results.borrow().as_slice(), &[true]);

	assert_eq!(terminal.program_status_probe_mut().stop(), vec![false]);
	assert_eq!(results.borrow().as_slice(), &[true, false]);

	assert_eq!(
		terminal.program_status_probe_mut().begin(&TestEnv::new()),
		query()
	);
	terminal.feed_raw_stdin(REPLY);
	assert_eq!(results.borrow().as_slice(), &[true, false, true]);

	let (late, late_id) = subscribe(&mut terminal);
	assert_eq!(late.borrow().as_slice(), &[true]);
	terminal.unsubscribe_program_status_support(late_id);
	assert_eq!(late.borrow().as_slice(), &[true]);
}

#[test]
fn accepts_the_protocols_implicit_state_set_when_the_reply_omits_states() {
	let mut terminal = ProcessTerminal::new();
	let (results, _id) = subscribe(&mut terminal);
	terminal.program_status_probe_mut().begin(&TestEnv::new());
	terminal.feed_raw_stdin("\x1b]7501;?\x1b\\");
	assert_eq!(results.borrow().as_slice(), &[true]);
}

#[test]
fn leaves_an_unsupported_terminal_unconfirmed_and_swallows_its_late_reply() {
	let mut terminal = ProcessTerminal::new();
	let (results, _id) = subscribe(&mut terminal);
	terminal.program_status_probe_mut().begin(&TestEnv::new());
	for _ in 0..15 {
		terminal.feed_raw_stdin(DA1_REPLY);
	}
	assert_eq!(results.borrow().as_slice(), &[false]);
	terminal.feed_raw_stdin(REPLY);
	assert_eq!(results.borrow().as_slice(), &[false]);
}

#[test]
fn rejects_a_reply_without_both_working_and_blocked_states() {
	for reply in [
		"\x1b]7501;?:states=idle,working,done\x07",
		"\x1b]7501;?: states = idle , working , done \x07",
	] {
		let mut terminal = ProcessTerminal::new();
		let (results, _id) = subscribe(&mut terminal);
		terminal.program_status_probe_mut().begin(&TestEnv::new());
		terminal.feed_raw_stdin(reply);
		assert_eq!(results.borrow().as_slice(), &[false], "{reply}");
	}
}

#[test]
fn does_not_query_behind_a_multiplexer_that_cannot_route_the_reply() {
	let mut env = TestEnv::new();
	env.insert("TMUX", "/tmp/tmux/default,1,0");
	let mut terminal = ProcessTerminal::new();
	let (results, _id) = subscribe(&mut terminal);
	assert!(terminal.program_status_probe_mut().begin(&env).is_empty());
	assert!(results.borrow().is_empty());
	assert!(TERMINAL_MULTIPLEXER_ENV_KEYS.contains(&"TMUX"));
	assert!(TERMINAL_MULTIPLEXER_ENV_KEYS.contains(&"TERM"));
}
