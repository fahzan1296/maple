//! Port of `l/test/mouse-tracking.test.ts`.
//!
//! Drives the real `Tui` over `VirtualTerminal` and asserts the inline
//! mouse-reporting sequences written on start, render, overlay transitions and
//! stop. The reference's alternate-screen enter/exit fusion has no crate
//! runtime, so the fused-exit cases assert the observable contract: capture is
//! released by the final stop and never left enabled.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use maple_tui::component::Component;
use maple_tui::mouse_tracking::{MOUSE_TRACKING_OFF, MOUSE_TRACKING_ON};
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::{Terminal, TtyEvent};
use maple_tui::tui::{MutableViewport, OverlayOptions, RenderRequestOptions, Tui};

#[derive(Clone)]
struct Shared(Rc<RefCell<VirtualTerminal>>);

impl Shared {
	fn new(width: u16, height: u16) -> Self {
		Self(Rc::new(RefCell::new(VirtualTerminal::new(width, height))))
	}

	fn output(&self) -> String {
		self.0.borrow().output().to_string()
	}
}

impl Terminal for Shared {
	fn start(&mut self) -> io::Result<()> {
		self.0.borrow_mut().start()
	}

	fn stop(&mut self) -> io::Result<()> {
		self.0.borrow_mut().stop()
	}

	fn write(&mut self, data: &str) {
		self.0.borrow_mut().write(data);
	}

	fn event(&mut self, timeout: Duration) -> Option<TtyEvent> {
		self.0.borrow_mut().event(timeout)
	}

	fn columns(&self) -> u16 {
		self.0.borrow().columns()
	}

	fn rows(&self) -> u16 {
		self.0.borrow().rows()
	}

	fn move_by(&mut self, lines: i32) {
		self.0.borrow_mut().move_by(lines);
	}

	fn hide_cursor(&mut self, force: bool) {
		self.0.borrow_mut().hide_cursor(force);
	}

	fn show_cursor(&mut self, force: bool) {
		self.0.borrow_mut().show_cursor(force);
	}

	fn clear_line(&mut self) {
		self.0.borrow_mut().clear_line();
	}

	fn clear_from_cursor(&mut self) {
		self.0.borrow_mut().clear_from_cursor();
	}

	fn clear_screen(&mut self) {
		self.0.borrow_mut().clear_screen();
	}
}

struct StaticOverlay;

impl Component for StaticOverlay {
	fn render(&mut self, _width: u16) -> Vec<String> {
		vec!["overlay".to_string()]
	}
}

struct StaticRows(Vec<String>);

impl Component for StaticRows {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.0.clone()
	}
}


fn make_inline_tui(enabled: Rc<RefCell<bool>>) -> (Shared, Tui) {
	let terminal = Shared::new(80, 24);
	let mut tui = Tui::new(Box::new(terminal.clone()));
	tui.set_inline_mouse_tracking_provider(Some(Box::new(move || *enabled.borrow())));
	(terminal, tui)
}

fn last_index(output: &str, needle: &str) -> Option<usize> {
	output.rfind(needle)
}

#[test]
fn enables_capture_on_the_normal_buffer_and_releases_it_on_stop() {
	let enabled = Rc::new(RefCell::new(true));
	let (terminal, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	tui.render_now();
	assert!(terminal.output().contains(MOUSE_TRACKING_ON));

	tui.stop().expect("stop");
	let output = terminal.output();
	let off_at = last_index(&output, MOUSE_TRACKING_OFF).expect("off sequence");
	assert!(off_at > last_index(&output, MOUSE_TRACKING_ON).expect("on sequence"));
}

#[test]
fn yields_to_any_visible_overlay_and_restores_after_it_closes() {
	let enabled = Rc::new(RefCell::new(true));
	let (terminal, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	tui.render_now();
	assert!(terminal.output().contains(MOUSE_TRACKING_ON));

	let overlay = tui.show_overlay(StaticOverlay, OverlayOptions::default());
	tui.render_now();
	let output = terminal.output();
	let off_at = last_index(&output, MOUSE_TRACKING_OFF).expect("off sequence");
	assert!(off_at > last_index(&output, MOUSE_TRACKING_ON).expect("on sequence"));

	overlay.hide();
	tui.render_now();
	assert!(last_index(&terminal.output(), MOUSE_TRACKING_ON).expect("on") > off_at);
	tui.stop().expect("stop");
}

#[test]
fn restores_inline_capture_after_a_mouse_disabled_fullscreen_overlay_closes() {
	let enabled = Rc::new(RefCell::new(true));
	let (terminal, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	tui.render_now();
	assert!(terminal.output().contains(MOUSE_TRACKING_ON));

	let overlay = tui.show_overlay(
		StaticOverlay,
		OverlayOptions {
			fullscreen: true,
			mouse_tracking: false,
			..OverlayOptions::default()
		},
	);
	tui.render_now();
	let output = terminal.output();
	let off_at = last_index(&output, MOUSE_TRACKING_OFF).expect("off sequence");
	assert!(off_at > last_index(&output, MOUSE_TRACKING_ON).expect("on sequence"));

	overlay.hide();
	tui.render_now();
	assert!(last_index(&terminal.output(), MOUSE_TRACKING_ON).expect("on") > off_at);
	tui.stop().expect("stop");
}

#[test]
fn releases_capture_on_stop_even_with_a_pending_alt_exit() {
	let enabled = Rc::new(RefCell::new(true));
	let (terminal, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	let overlay = tui.show_overlay(StaticOverlay, OverlayOptions { fullscreen: true, ..OverlayOptions::default() });
	tui.render_now();

	tui.request_render_with(true, RenderRequestOptions { clear_scrollback: true });
	overlay.hide();
	tui.render_now();
	tui.stop().expect("stop");

	assert!(terminal.output().contains(MOUSE_TRACKING_OFF));
}

#[test]
fn leaves_tracking_off_when_stopping_after_a_fused_restore_exit() {
	let enabled = Rc::new(RefCell::new(true));
	let (terminal, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	let overlay = tui.show_overlay(
		StaticOverlay,
		OverlayOptions {
			fullscreen: true,
			mouse_tracking: false,
			..OverlayOptions::default()
		},
	);
	tui.render_now();

	tui.request_render_with(true, RenderRequestOptions { clear_scrollback: true });
	overlay.hide();
	tui.render_now();
	tui.stop().expect("stop");

	let output = terminal.output();
	assert!(output.contains(MOUSE_TRACKING_OFF));
	assert!(
		last_index(&output, MOUSE_TRACKING_OFF).expect("off")
			> last_index(&output, MOUSE_TRACKING_ON).expect("on")
	);
}

#[test]
fn stays_off_by_default() {
	let enabled = Rc::new(RefCell::new(false));
	let (terminal, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	tui.render_now();
	assert!(!terminal.output().contains(MOUSE_TRACKING_ON));
	tui.stop().expect("stop");
}

#[test]
fn exposes_the_painted_window_and_hides_it_behind_the_alt_screen() {
	let enabled = Rc::new(RefCell::new(true));
	let (_, mut tui) = make_inline_tui(enabled);
	tui.start().expect("start");
	tui.add_child(StaticRows(vec!["line".to_string()]));
	tui.render_now();
	assert_eq!(tui.get_mutable_viewport(), MutableViewport { top: 0, length: 1 });

	let overlay = tui.show_overlay(StaticOverlay, OverlayOptions { fullscreen: true, ..OverlayOptions::default() });
	tui.render_now();
	assert_eq!(tui.get_mutable_viewport(), MutableViewport { top: 0, length: 0 });
	overlay.hide();
	tui.stop().expect("stop");
}