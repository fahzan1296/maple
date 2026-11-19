//! Reference: `l/src/setup/startup-splash.ts`.
//!
//! Standalone startup animation: the setup splash on its own fullscreen page.
//! Any of enter/return/space/escape (or a native `skip` action) ends it.
//!
//! Deviations from the TypeScript original:
//! - `performance.now()` becomes [`SetupUiHost::now_ms`]; the `setInterval` /
//!   `setTimeout` timers become [`StartupSplashComponent::pump`], driven by
//!   [`run_startup_splash`].
//! - `Promise.withResolvers` becomes an explicit `done` flag.
//! - `ctx` is `Rc<dyn SetupHost>` (not `SetupUiHost`) because the splash renders
//!   through a `Theme`; the reference reads the theme module singleton.
//! - `OverlayFocusOwner` has no crate equivalent; the runner restores focus with
//!   `set_focus(None)` after the overlay hides.
//! - The `now` option is `Rc<dyn Fn() -> f64>` (a test seam); `None` uses the
//!   host clock.

use std::cell::RefCell;
use std::rc::Rc;

use crate::component::Component;
use crate::keys::Key;
use crate::native::node::{DescribeContext, NativeNode, NativeScreen};
use crate::setup::scenes::splash::{
	describe_setup_splash, render_setup_splash, SETUP_SPLASH_MS, SETUP_TICK_MS,
};
use crate::setup::scenes::types::{
	Overlay, OverlayOptions, SetupHost, SetupInput, SetupNativeEvent, SETUP_SURFACE_ROLE,
};

/// Timing controls for the standalone startup animation.
pub struct RunStartupSplashOptions {
	pub duration_ms: Option<f64>,
	pub tick_ms: Option<f64>,
	/// Clock override; `None` uses [`SetupUiHost::now_ms`].
	pub now: Option<Rc<dyn Fn() -> f64>>,
}

impl Default for RunStartupSplashOptions {
	fn default() -> Self {
		RunStartupSplashOptions {
			duration_ms: None,
			tick_ms: None,
			now: None,
		}
	}
}

impl RunStartupSplashOptions {
	pub fn duration_ms(&self) -> f64 {
		self.duration_ms.unwrap_or(SETUP_SPLASH_MS)
	}

	pub fn tick_ms(&self) -> f64 {
		self.tick_ms.unwrap_or(SETUP_TICK_MS)
	}
}

/// Whether `key` ends the startup splash.
pub fn skip_key(key: &Key) -> bool {
	matches!(key, Key::Enter | Key::Esc | Key::Char(' '))
}

/// The standalone startup splash page.
pub struct StartupSplashComponent {
	ctx: Rc<dyn SetupHost>,
	duration_ms: f64,
	now: Option<Rc<dyn Fn() -> f64>>,
	phase_started_at: f64,
	done: bool,
	disposed: bool,
}

impl StartupSplashComponent {
	pub fn new(ctx: Rc<dyn SetupHost>, options: RunStartupSplashOptions) -> Self {
		StartupSplashComponent {
			ctx,
			duration_ms: options.duration_ms(),
			now: options.now,
			phase_started_at: 0.0,
			done: false,
			disposed: false,
		}
	}

	fn clock(&self) -> f64 {
		match &self.now {
			Some(now) => now(),
			None => self.ctx.now_ms(),
		}
	}

	/// Start the animation.
	pub fn run(&mut self) {
		self.phase_started_at = self.clock();
		self.ctx.request_render();
	}

	/// Advance the animation. Returns `true` once it completed or was disposed.
	pub fn pump(&mut self, now_ms: f64) -> bool {
		if self.disposed {
			return true;
		}
		if now_ms - self.phase_started_at >= self.duration_ms {
			self.complete();
		}
		self.done || self.disposed
	}

	pub fn is_done(&self) -> bool {
		self.done || self.disposed
	}

	pub fn handle_input(&mut self, key: &Key) {
		if skip_key(key) {
			self.complete();
		}
	}

	pub fn handle_native_event(&mut self, event: &SetupNativeEvent) {
		if let SetupNativeEvent::Action { act, .. } = event {
			if act == "skip" {
				self.complete();
			}
		}
	}

	fn complete(&mut self) {
		if self.disposed {
			return;
		}
		self.done = true;
	}

	fn render(&mut self, width: u16) -> Vec<String> {
		let elapsed = self
			.duration_ms
			.min(self.clock() - self.phase_started_at)
			.max(0.0);
		let ctx = self.ctx.clone();
		let theme = ctx.theme();
		render_setup_splash(theme, width.max(1) as usize, ctx.rows().max(1), elapsed)
	}

	/// The setup page's splash, on a screen surface of the setup page's role.
	pub fn describe_screen(&mut self) -> NativeScreen {
		NativeScreen {
			role: SETUP_SURFACE_ROLE.to_string(),
			main: vec![describe_setup_splash()],
			dock: Vec::new(),
			layer: Vec::new(),
		}
	}

	pub fn describe(&mut self, _cx: &DescribeContext) -> NativeNode {
		describe_setup_splash()
	}
}

impl Component for StartupSplashComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.handle_input(key);
	}

	fn dispose(&mut self) {
		self.disposed = true;
	}
}

/// Forwards the overlay's component calls to the shared splash instance.
#[derive(Clone)]
pub struct StartupSplashHandle(pub Rc<RefCell<StartupSplashComponent>>);

impl Component for StartupSplashHandle {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.0.borrow_mut().render(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.0.borrow_mut().handle_input(key);
	}

	fn dispose(&mut self) {
		self.0.borrow_mut().dispose();
	}
}

/// Show the startup animation and restore the previous overlay focus afterward.
pub fn run_startup_splash(ctx: Rc<dyn SetupHost>, options: RunStartupSplashOptions) {
	let tick_ms = options.tick_ms().max(1.0) as u64;
	let handle = Rc::new(RefCell::new(StartupSplashComponent::new(
		ctx.clone(),
		options,
	)));
	handle.borrow_mut().run();
	let overlay = ctx.show_overlay(
		Box::new(StartupSplashHandle(handle.clone())),
		OverlayOptions::fullscreen(),
	);
	while !handle.borrow_mut().pump(ctx.now_ms()) {
		while let Some(input) = ctx.poll_input() {
			match input {
				SetupInput::Key(key) => handle.borrow_mut().handle_input(&key),
				SetupInput::Native(event) => handle.borrow_mut().handle_native_event(&event),
				SetupInput::Sgr(_) => {}
			}
		}
		ctx.request_render();
		std::thread::sleep(std::time::Duration::from_millis(tick_ms));
	}
	drop(overlay);
	handle.borrow_mut().dispose();
	ctx.set_focus(None);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn skip_keys_end_the_splash() {
		let cases = [
			(Key::Enter, true),
			(Key::Esc, true),
			(Key::Char(' '), true),
			(Key::Char('x'), false),
			(Key::Up, false),
			(Key::Ctrl('c'), false),
		];
		for (key, expected) in cases {
			assert_eq!(skip_key(&key), expected, "{key:?}");
		}
	}

	#[test]
	fn default_timing_matches_the_splash_constants() {
		let options = RunStartupSplashOptions::default();
		assert_eq!(options.duration_ms(), SETUP_SPLASH_MS);
		assert_eq!(options.tick_ms(), SETUP_TICK_MS);
	}

	#[test]
	fn explicit_timing_overrides_defaults() {
		let options = RunStartupSplashOptions {
			duration_ms: Some(1.0),
			tick_ms: Some(2.0),
			now: None,
		};
		assert_eq!(options.duration_ms(), 1.0);
		assert_eq!(options.tick_ms(), 2.0);
	}
}
