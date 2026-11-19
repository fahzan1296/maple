//! Port of `l/test/start-listener.test.ts`.
//!
//! Deviations: `Tui`'s fields are private to the `tui` module, so the listener
//! registry is process-wide and `StartListeningTerminal` bridges the virtual
//! terminal's start/stop to it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;

use maple_tui::start_listener::StartListeningTerminal;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::tui::Tui;

static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

fn lock_registry() -> std::sync::MutexGuard<'static, ()> {
	REGISTRY_LOCK
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tui() -> Tui {
	Tui::new(Box::new(StartListeningTerminal::new(Box::new(
		VirtualTerminal::new(80, 24),
	))))
}

#[test]
fn fires_registered_hooks_on_initial_start_and_restart() {
	let _guard = lock_registry();
	let mut tui = tui();
	let starts = Rc::new(Cell::new(0));
	let id = {
		let starts = Rc::clone(&starts);
		tui.add_start_listener(move || starts.set(starts.get() + 1))
	};
	tui.start().unwrap();
	assert_eq!(starts.get(), 1);
	tui.stop().unwrap();
	tui.start().unwrap();
	assert_eq!(starts.get(), 2);
	tui.stop().unwrap();
	assert!(tui.remove_start_listener(id));
	assert!(!tui.remove_start_listener(id));
}

#[test]
fn runs_a_hook_added_to_an_already_started_tui_at_once() {
	let _guard = lock_registry();
	let mut tui = tui();
	let starts = Rc::new(Cell::new(0));
	tui.start().unwrap();
	let id = {
		let starts = Rc::clone(&starts);
		tui.add_start_listener(move || starts.set(starts.get() + 1))
	};
	assert_eq!(starts.get(), 1);
	tui.stop().unwrap();
	tui.start().unwrap();
	assert_eq!(starts.get(), 2);
	tui.stop().unwrap();
	assert!(tui.remove_start_listener(id));
}

#[test]
fn holds_a_hook_added_to_a_stopped_tui_until_next_start() {
	let _guard = lock_registry();
	let mut tui = tui();
	let starts = Rc::new(Cell::new(0));
	tui.start().unwrap();
	tui.stop().unwrap();
	let id = {
		let starts = Rc::clone(&starts);
		tui.add_start_listener(move || starts.set(starts.get() + 1))
	};
	assert_eq!(starts.get(), 0);
	tui.start().unwrap();
	assert_eq!(starts.get(), 1);
	tui.stop().unwrap();
	assert!(tui.remove_start_listener(id));
}
