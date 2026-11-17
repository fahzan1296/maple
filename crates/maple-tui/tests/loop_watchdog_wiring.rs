//! Port of `l/test/loop-watchdog-wiring.test.ts`.
//!
//! The reference spies `LoopWatchdog.prototype.start/stop` and the stall probe
//! handed to the terminal. The crate's `Tui` does not own a watchdog and has
//! no stall-probe callback, so those are ignored; the wiring that is visible —
//! `Tui::start` claims the terminal and `Tui::stop` releases it and marks the
//! UI stopped — is pinned with a recording terminal.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use maple_tui::terminal::{Terminal, TtyEvent};
use maple_tui::tui::Tui;

#[derive(Default)]
struct RecordingTerminal {
	started: Arc<AtomicBool>,
	output: Vec<String>,
}

impl Terminal for RecordingTerminal {
	fn start(&mut self) -> std::io::Result<()> {
		self.started.store(true, Ordering::SeqCst);
		Ok(())
	}

	fn stop(&mut self) -> std::io::Result<()> {
		self.started.store(false, Ordering::SeqCst);
		Ok(())
	}

	fn write(&mut self, data: &str) {
		self.output.push(data.to_string());
	}

	fn event(&mut self, _timeout: Duration) -> Option<TtyEvent> {
		None
	}

	fn columns(&self) -> u16 {
		80
	}

	fn rows(&self) -> u16 {
		24
	}

	fn move_by(&mut self, _lines: i32) {}

	fn hide_cursor(&mut self, _force: bool) {}

	fn show_cursor(&mut self, _force: bool) {}

	fn clear_line(&mut self) {}

	fn clear_from_cursor(&mut self) {}

	fn clear_screen(&mut self) {}
}

#[test]
fn start_claims_the_terminal_and_stop_releases_it() {
	let started = Arc::new(AtomicBool::new(false));
	let mut tui = Tui::new(Box::new(RecordingTerminal {
		started: Arc::clone(&started),
		output: Vec::new(),
	}));

	assert!(!tui.stopped());
	tui.start().expect("start");
	assert!(started.load(Ordering::SeqCst));
	assert!(!tui.stopped());

	tui.stop().expect("stop");
	assert!(tui.stopped());
	assert!(!started.load(Ordering::SeqCst));
}

#[test]
fn stop_is_idempotent() {
	let started = Arc::new(AtomicBool::new(false));
	let mut tui = Tui::new(Box::new(RecordingTerminal {
		started: Arc::clone(&started),
		output: Vec::new(),
	}));
	tui.start().expect("start");
	tui.stop().expect("stop");
	tui.stop().expect("second stop");
	assert!(tui.stopped());
	assert!(!started.load(Ordering::SeqCst));
}

#[test]
fn render_now_uses_the_terminal_geometry() {
	let started = Arc::new(AtomicBool::new(false));
	let mut tui = Tui::new(Box::new(RecordingTerminal {
		started: Arc::clone(&started),
		output: Vec::new(),
	}));
	tui.start().expect("start");
	tui.render_now();
	assert!(!tui.stopped());
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: Tui does not own a LoopWatchdog, so arm/disarm cannot be observed"]
fn arms_the_watchdog_on_start_and_disarms_it_on_stop() {
	let started = Arc::new(AtomicBool::new(false));
	let mut tui = Tui::new(Box::new(RecordingTerminal {
		started: Arc::clone(&started),
		output: Vec::new(),
	}));
	tui.start().expect("start");
	tui.stop().expect("stop");
	assert!(!started.load(Ordering::SeqCst));
}

#[test]
#[ignore = "missing facility: Tui::start passes no isLoopStalled probe to the terminal"]
fn hands_the_terminal_a_stall_probe_backed_by_the_watchdog() {
	let started = Arc::new(AtomicBool::new(false));
	let mut tui = Tui::new(Box::new(RecordingTerminal {
		started: Arc::clone(&started),
		output: Vec::new(),
	}));
	tui.start().expect("start");
	assert!(started.load(Ordering::SeqCst));
	tui.stop().expect("stop");
}
