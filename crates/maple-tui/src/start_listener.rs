//! Reference: `l/src/tui.ts` (`addStartListener`).
//!
//! Deviations:
//! - `Tui`'s fields are private to the `tui` module, so the listener set lives
//!   in a process-wide registry and [`StartListeningTerminal`] bridges
//!   `Terminal::start`/`stop` to it. Moving the storage into `Tui` is a
//!   `tui.rs` change; the public API is already the reference's.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use crate::terminal::{Terminal, TtyEvent};
use crate::tui::Tui;

/// Opaque handle removing a start listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartListenerId(u64);

thread_local! {
	static LISTENERS: RefCell<Vec<(StartListenerId, Box<dyn FnMut()>)>> = RefCell::new(Vec::new());
	static NEXT_ID: Cell<u64> = Cell::new(0);
	static STARTED: Cell<bool> = Cell::new(false);
}

/// Register a hook that runs on every start. A hook added while the TUI is
/// already running runs immediately, matching the reference's prepaint case.
pub fn add_start_listener<F>(listener: F) -> StartListenerId
where
	F: FnMut() + 'static,
{
	let id = NEXT_ID.with(|next| {
		let id = next.get();
		next.set(id + 1);
		StartListenerId(id)
	});
	LISTENERS.with(|listeners| listeners.borrow_mut().push((id, Box::new(listener))));
	if is_started() {
		run_listener(id);
	}
	id
}

/// Remove a listener; returns whether it was registered.
pub fn remove_start_listener(id: StartListenerId) -> bool {
	LISTENERS.with(|listeners| {
		let mut listeners = listeners.borrow_mut();
		let before = listeners.len();
		listeners.retain(|(listener_id, _)| *listener_id != id);
		before != listeners.len()
	})
}

/// Whether the TUI is currently started.
pub fn is_started() -> bool {
	STARTED.with(|started| started.get())
}

/// Mark the TUI started and run every registered hook.
pub fn notify_started() {
	STARTED.with(|started| started.set(true));
	let ids: Vec<StartListenerId> = LISTENERS
		.with(|listeners| listeners.borrow().iter().map(|(id, _)| *id).collect());
	for id in ids {
		run_listener(id);
	}
}

/// Mark the TUI stopped.
pub fn notify_stopped() {
	STARTED.with(|started| started.set(false));
}

fn run_listener(id: StartListenerId) {
	LISTENERS.with(|listeners| {
		let mut listeners = listeners.borrow_mut();
		if let Some((_, listener)) = listeners
			.iter_mut()
			.find(|(listener_id, _)| *listener_id == id)
		{
			listener();
		}
	});
}

impl Tui {
	/// Register a hook that re-enables terminal modes on every `start`.
	///
	/// The returned id removes it via [`Tui::remove_start_listener`].
	pub fn add_start_listener<F>(&mut self, listener: F) -> StartListenerId
	where
		F: FnMut() + 'static,
	{
		add_start_listener(listener)
	}

	/// Remove a hook registered by [`Tui::add_start_listener`].
	pub fn remove_start_listener(&mut self, id: StartListenerId) -> bool {
		remove_start_listener(id)
	}
}

/// Terminal backend that reports `start`/`stop` to the start-listener
/// registry, so a [`Tui`] built on it fires hooks without the `tui` module
/// owning the registry.
pub struct StartListeningTerminal {
	inner: Box<dyn Terminal>,
}

impl StartListeningTerminal {
	pub fn new(inner: Box<dyn Terminal>) -> Self {
		StartListeningTerminal { inner }
	}

	pub fn into_inner(self) -> Box<dyn Terminal> {
		self.inner
	}
}

impl Terminal for StartListeningTerminal {
	fn start(&mut self) -> std::io::Result<()> {
		self.inner.start()?;
		notify_started();
		Ok(())
	}

	fn stop(&mut self) -> std::io::Result<()> {
		self.inner.stop()?;
		notify_stopped();
		Ok(())
	}

	fn write(&mut self, data: &str) {
		self.inner.write(data);
	}

	fn poll_event(&mut self) -> Option<TtyEvent> {
		self.inner.poll_event()
	}

	fn event(&mut self, timeout: Duration) -> Option<TtyEvent> {
		self.inner.event(timeout)
	}

	fn columns(&self) -> u16 {
		self.inner.columns()
	}

	fn rows(&self) -> u16 {
		self.inner.rows()
	}

	fn move_by(&mut self, lines: i32) {
		self.inner.move_by(lines);
	}

	fn hide_cursor(&mut self, force: bool) {
		self.inner.hide_cursor(force);
	}

	fn show_cursor(&mut self, force: bool) {
		self.inner.show_cursor(force);
	}

	fn clear_line(&mut self) {
		self.inner.clear_line();
	}

	fn clear_from_cursor(&mut self) {
		self.inner.clear_from_cursor();
	}

	fn clear_screen(&mut self) {
		self.inner.clear_screen();
	}

	fn append_scrollback(&mut self, rows: &[String]) {
		self.inner.append_scrollback(rows);
	}

	fn set_viewport(&mut self, rows: &[String]) {
		self.inner.set_viewport(rows);
	}

	fn set_hardware_cursor(&mut self, position: Option<(u16, u16)>) {
		self.inner.set_hardware_cursor(position);
	}

	fn clear_scrollback(&mut self) {
		self.inner.clear_scrollback();
	}
}
