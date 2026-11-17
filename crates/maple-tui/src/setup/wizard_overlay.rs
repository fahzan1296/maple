//! Reference: `l/src/setup/wizard-overlay.ts`.
//!
//! Fullscreen onboarding presentation: splash, cross-dissolve into the first
//! scene, scene pages under a stepper header, and the outro.
//!
//! Deviations from the TypeScript original (JS-only shapes, made explicit):
//! - `SetupWizardComponent` is a concrete struct; the class's `#private` fields
//!   are private Rust fields.
//! - `performance.now()` becomes [`SetupUiHost::now_ms`]. The `setInterval` /
//!   `setTimeout` timers become [`SetupWizardComponent::pump`], driven by
//!   `wizard::run_setup_wizard`; `pump` returns `true` once the wizard is done.
//! - `Promise.withResolvers` becomes an explicit `done` flag.
//! - `SetupSceneHost` is a trait object. The wizard implements it as
//!   [`WizardSceneHost`] and shares a [`WizardControl`] cell so scene callbacks
//!   can request a finish without borrowing the wizard.
//! - `OverlayFocusOwner` has no crate equivalent: the wizard records whether a
//!   scene took focus and restores focus to itself via `set_focus(None)`.
//! - `Memo` caches are dropped; descriptions are rebuilt per call.
//! - `isNativeRendering()` is `native::state::native_state().is_native_rendering()`.
//! - `routeSgrMouseInput` is `scenes::types::route_sgr_mouse_input`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::chrome::keybinding_hints::{editor_key, editor_keys};
use crate::component::Component;
use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::describe::{col, keyed, node, row, span, text};
use crate::native::node::{
	DescribeContext, NativeNode, NativeScreen, NodeKind, NodeText, Props, Tone,
};
use crate::native::overlay::{action_button, ActionButtonOptions};
use crate::native::state::native_state;
use crate::setup::scenes::outro::{render_setup_outro, describe_setup_outro, SETUP_OUTRO_MS};
use crate::setup::scenes::splash::{
	center_line, describe_setup_splash, fg, gradient_logo, render_setup_splash, APP_NAME, PI_LOGO,
	SETUP_SPLASH_MS,
};
use crate::setup::scenes::types::{
	route_sgr_mouse_input, Overlay, OverlayOptions, SetupHost, SetupNativeEvent, SetupScene,
	SetupSceneController, SetupSceneHost, SetupSceneResult, SETUP_SURFACE_ROLE,
};
use crate::text_shape::pad_to_width;
use crate::theme::theme_class::Theme;

/// Left/right gutter around the scene body.
pub const SCENE_MARGIN_X: usize = 4;
/// Narrowest scene body the wizard will render.
pub const MIN_CONTENT_WIDTH: usize = 20;
/// Cross-dissolve duration from the splash into the first scene.
pub const SCENE_TRANSITION_MS: f64 = 420.0;

/// Whether `scene` is asked under the current renderer: a TSP surface draws
/// `ansi_only` settings itself.
pub fn setup_scene_applies(scene: &SetupScene) -> bool {
	!scene.ansi_only || !native_state().is_native_rendering()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardPhase {
	Splash,
	Transition,
	Scene,
	Outro,
	Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepState {
	Done,
	Current,
	Next,
}

impl StepState {
	fn tone(self) -> Tone {
		match self {
			StepState::Done => Tone::Success,
			StepState::Current => Tone::Accent,
			StepState::Next => Tone::Muted,
		}
	}
}

/// Scene callbacks the wizard shares with its [`WizardSceneHost`].
#[derive(Default)]
struct WizardControl {
	finish_requested: bool,
	focus_taken: bool,
}

/// Fullscreen onboarding presentation with scene focus and mouse routing.
pub struct SetupWizardComponent {
	ctx: Rc<dyn SetupHost>,
	scenes: Vec<SetupScene>,
	phase: WizardPhase,
	phase_started_at: f64,
	scene_index: usize,
	active_scene: Option<Box<dyn SetupSceneController>>,
	/// `None` = not looked up yet, `Some(None)` = looked up and absent.
	outro_model: Option<Option<String>>,
	done: bool,
	disposed: bool,
	/// Screen row where the active scene's body began in the last rendered frame.
	body_row_start: i32,
	control: Rc<RefCell<WizardControl>>,
}

impl SetupWizardComponent {
	pub fn new(ctx: Rc<dyn SetupHost>, scenes: Vec<SetupScene>) -> Self {
		SetupWizardComponent {
			ctx,
			scenes,
			phase: WizardPhase::Splash,
			phase_started_at: 0.0,
			scene_index: 0,
			active_scene: None,
			outro_model: None,
			done: false,
			disposed: false,
			body_row_start: 0,
			control: Rc::new(RefCell::new(WizardControl::default())),
		}
	}

	/// Start the wizard. When nothing applies it completes immediately.
	pub fn run(&mut self) {
		if self.scenes.is_empty() || !self.scenes.iter().any(setup_scene_applies) {
			self.complete();
			return;
		}
		self.phase = WizardPhase::Splash;
		self.phase_started_at = self.ctx.now_ms();
		self.ctx.request_render();
	}

	/// Advance timed phases. Returns `true` once the wizard is finished or was
	/// disposed.
	pub fn pump(&mut self, now_ms: f64) -> bool {
		if self.disposed || self.done {
			return true;
		}
		let elapsed = now_ms - self.phase_started_at;
		match self.phase {
			WizardPhase::Splash if elapsed >= SETUP_SPLASH_MS => self.begin_scene(),
			WizardPhase::Transition if elapsed >= SCENE_TRANSITION_MS => {
				self.phase = WizardPhase::Scene;
				self.phase_started_at = now_ms;
				self.ctx.request_render();
			}
			WizardPhase::Outro if elapsed >= SETUP_OUTRO_MS => self.complete(),
			_ => {}
		}
		if self.control.borrow().finish_requested {
			self.control.borrow_mut().finish_requested = false;
			self.finish_scene();
		}
		self.done || self.disposed
	}

	pub fn is_done(&self) -> bool {
		self.done || self.disposed
	}

	/// Whether a scene currently owns the overlay's focus target.
	pub fn focus_taken(&self) -> bool {
		self.control.borrow().focus_taken
	}

	pub fn phase(&self) -> WizardPhase {
		self.phase
	}

	pub fn dispose(&mut self) {
		if self.disposed {
			return;
		}
		self.disposed = true;
		self.unmount_active_scene();
	}

	fn invalidate(&mut self) {
		if let Some(scene) = self.active_scene.as_mut() {
			scene.invalidate();
		}
		self.ctx.invalidate();
	}

	pub fn handle_input(&mut self, key: &Key) {
		if self.done || self.disposed {
			return;
		}
		if matches!(key, Key::Ctrl('c')) {
			self.begin_outro();
			return;
		}
		match self.phase {
			WizardPhase::Splash => {
				if matches!(key, Key::Enter | Key::Esc | Key::Char(' ')) {
					self.begin_scene();
				}
			}
			WizardPhase::Outro => {
				if matches!(key, Key::Enter | Key::Esc | Key::Char(' ')) {
					self.complete();
				}
			}
			WizardPhase::Scene | WizardPhase::Transition => {
				if let Some(scene) = self.active_scene.as_mut() {
					scene.handle_input(key);
				}
			}
			WizardPhase::Done => {}
		}
	}

	/// Route one raw SGR report. The frame paints from screen row 0, so report
	/// coordinates index directly into the last rendered lines.
	pub fn handle_sgr(&mut self, data: &str) {
		if self.disposed {
			return;
		}
		let mut routed = None;
		route_sgr_mouse_input(data, &mut |event| routed = Some(*event));
		let Some(event) = routed else {
			return;
		};
		if self.phase == WizardPhase::Splash || self.phase == WizardPhase::Outro {
			if !event.left_click {
				return;
			}
			if self.phase == WizardPhase::Splash {
				self.begin_scene();
			} else {
				self.complete();
			}
			return;
		}
		let Some(scene) = self.active_scene.as_mut() else {
			return;
		};
		if scene.routes_mouse() {
			let line = event.row - self.body_row_start;
			let column = event.col - SCENE_MARGIN_X as i32;
			scene.route_mouse(&event, line, column);
			return;
		}
		if let Some(wheel) = event.wheel {
			scene.handle_input(&if wheel < 0 { Key::Up } else { Key::Down });
		}
	}

	pub fn handle_native_event(&mut self, event: &SetupNativeEvent) {
		let SetupNativeEvent::Action { act, .. } = event else {
			return;
		};
		match act.as_str() {
			"skip" if self.phase == WizardPhase::Splash => self.begin_scene(),
			"continue" if self.phase == WizardPhase::Outro => self.complete(),
			"exit" if self.phase == WizardPhase::Scene || self.phase == WizardPhase::Transition => {
				self.begin_outro();
			}
			_ => {}
		}
	}

	/// The steps the stepper and the ANSI counter show: every scene that applies
	/// to the current renderer, plus the active one, and the active position.
	fn steps(&self) -> (Vec<&'static str>, usize) {
		let mut steps = Vec::new();
		let mut current = 0;
		for (index, scene) in self.scenes.iter().enumerate() {
			if index == self.scene_index {
				current = steps.len();
			}
			if index == self.scene_index || setup_scene_applies(scene) {
				steps.push(scene.step);
			}
		}
		(steps, current)
	}

	fn current_scene(&self) -> Option<SetupScene> {
		self.scenes.get(self.scene_index).copied()
	}

	fn mount_scene_controller(&mut self, target_phase: WizardPhase) {
		if self.disposed {
			return;
		}
		self.unmount_active_scene();
		while self.scene_index < self.scenes.len()
			&& !setup_scene_applies(&self.scenes[self.scene_index])
		{
			self.scene_index += 1;
		}
		if self.scene_index >= self.scenes.len() {
			self.begin_outro();
			return;
		}
		let scene = self.scenes[self.scene_index];
		let host = WizardSceneHost {
			ctx: self.ctx.clone(),
			control: self.control.clone(),
		};
		self.active_scene = Some((scene.mount)(Box::new(host)));
		self.phase = target_phase;
		self.phase_started_at = self.ctx.now_ms();
		self.control.borrow_mut().focus_taken = false;
		self.ctx.set_focus(None);
		if let Some(active) = self.active_scene.as_mut() {
			active.on_mount();
		}
		self.ctx.request_render();
	}

	/// Enter the first scene through a dissolve from the splash (a row effect,
	/// so native cuts straight in).
	fn begin_scene(&mut self) {
		let target = if native_state().is_native_rendering() {
			WizardPhase::Scene
		} else {
			WizardPhase::Transition
		};
		self.mount_scene_controller(target);
	}

	fn mount_current_scene(&mut self) {
		self.mount_scene_controller(WizardPhase::Scene);
	}

	fn finish_scene(&mut self) {
		if self.phase != WizardPhase::Scene && self.phase != WizardPhase::Transition {
			return;
		}
		self.unmount_active_scene();
		self.scene_index += 1;
		self.mount_current_scene();
	}

	fn unmount_active_scene(&mut self) {
		self.control.borrow_mut().focus_taken = false;
		if let Some(mut scene) = self.active_scene.take() {
			scene.on_unmount();
			scene.dispose();
		}
	}

	fn begin_outro(&mut self) {
		if self.phase == WizardPhase::Done {
			return;
		}
		self.unmount_active_scene();
		self.phase = WizardPhase::Outro;
		self.outro_model = None;
		self.phase_started_at = self.ctx.now_ms();
		self.ctx.set_focus(None);
		self.ctx.request_render();
	}

	fn complete(&mut self) {
		if self.phase == WizardPhase::Done {
			return;
		}
		self.phase = WizardPhase::Done;
		self.done = true;
		self.unmount_active_scene();
		self.ctx.set_focus(None);
		self.ctx.request_render();
	}

	/// The default model's display name for the outro, looked up once per outro.
	fn outro_model_name(&mut self) -> Option<&str> {
		if self.outro_model.is_none() {
			let name = self
				.ctx
				.get_models()
				.current
				.as_ref()
				.map(|model| model.name.clone());
			self.outro_model = Some(name);
		}
		self.outro_model.as_ref().and_then(|model| model.as_deref())
	}

	fn render(&mut self, width: u16) -> Vec<String> {
		let safe_width = width.max(1) as usize;
		let height = self.ctx.rows().max(1);
		let now = self.ctx.now_ms();
		let ctx = self.ctx.clone();
		let theme = ctx.theme();
		let lines = match self.phase {
			WizardPhase::Splash => {
				render_setup_splash(theme, safe_width, height, now - self.phase_started_at)
			}
			WizardPhase::Transition => {
				let elapsed = now - self.phase_started_at;
				let progress = (elapsed / SCENE_TRANSITION_MS).min(1.0);
				let splash = render_setup_splash(theme, safe_width, height, SETUP_SPLASH_MS + elapsed);
				let scene = self.render_scene(width, height);
				dissolve_frames(&splash, &scene, progress, height)
			}
			WizardPhase::Outro => render_setup_outro(
				theme,
				safe_width,
				height,
				now - self.phase_started_at,
				self.outro_model.clone().flatten(),
			),
			WizardPhase::Scene => self.render_scene(width, height),
			WizardPhase::Done => Vec::new(),
		};
		fit_to_screen(&lines, safe_width, height)
	}

	fn render_scene(&mut self, width: u16, height: usize) -> Vec<String> {
		let width = width.max(1) as usize;
		let ctx = self.ctx.clone();
		let theme = ctx.theme();
		let scene = self.current_scene();
		let title = self
			.active_scene
			.as_ref()
			.map(|active| active.title().to_string())
			.or_else(|| scene.map(|scene| scene.title.to_string()))
			.unwrap_or_else(|| "Setup".to_string());
		let subtitle = self
			.active_scene
			.as_ref()
			.and_then(|active| active.subtitle().map(str::to_string));
		let (steps, current) = self.steps();
		let logo = gradient_logo(&PI_LOGO, 0.0, None);
		let mut header: Vec<String> = vec![String::new()];
		header.extend(logo.iter().map(|line| center_line(line, width)));
		header.push(center_line(&theme.bold(APP_NAME), width));
		header.push(center_line(
			&fg(
				theme,
				"muted",
				&format!("Setup step {} of {}", current + 1, steps.len()),
			),
			width,
		));
		header.push(String::new());
		header.push(indent_line(&theme.bold(&title), width, SCENE_MARGIN_X));
		if let Some(subtitle) = &subtitle {
			header.push(indent_line(&fg(theme, "muted", subtitle), width, SCENE_MARGIN_X));
		}
		header.push(String::new());
		self.body_row_start = header.len() as i32;

		let footer = vec![
			String::new(),
			center_line(&fg(theme, "dim", &scene_footer_hint(theme)), width),
		];
		let max_body_lines = height.saturating_sub(header.len() + footer.len());
		let content_width = width.saturating_sub(SCENE_MARGIN_X * 2).max(MIN_CONTENT_WIDTH);
		let body = match self.active_scene.as_mut() {
			Some(active) => active.render_body(content_width as u16, Some(max_body_lines)),
			None => Vec::new(),
		};
		let mut lines = header;
		lines.extend(
			body.into_iter()
				.take(max_body_lines)
				.map(|line| indent_line(&line, width, SCENE_MARGIN_X)),
		);
		while lines.len() + footer.len() < height {
			lines.push(String::new());
		}
		lines.extend(footer);
		lines
	}

	/// The wizard is a page of its own: a screen surface styled by
	/// [`SETUP_SURFACE_ROLE`].
	pub fn describe_screen(&mut self) -> NativeScreen {
		let cx = DescribeContext::default();
		NativeScreen {
			role: SETUP_SURFACE_ROLE.to_string(),
			main: vec![self.describe(&cx)],
			dock: Vec::new(),
			layer: Vec::new(),
		}
	}

	/// Fullscreen description: splash, the active scene under the setup header,
	/// or the outro. A transition describes as the scene.
	pub fn describe(&mut self, cx: &DescribeContext) -> NativeNode {
		let content = match self.phase {
			WizardPhase::Splash => describe_setup_splash(),
			WizardPhase::Transition | WizardPhase::Scene => self.describe_scene(cx),
			WizardPhase::Outro => describe_setup_outro(self.outro_model_name()),
			WizardPhase::Done => col(Vec::new(), Props::new()),
		};
		let key = match self.phase {
			WizardPhase::Transition => "scene".to_string(),
			other => format!("{:?}", other).to_lowercase(),
		};
		let mut node = content;
		node.key = Some(key);
		let mut props = Props::new();
		props.insert("grow", 1.into());
		props.set_role("omp.setup");
		col(vec![node], props)
	}

	fn describe_head(&self) -> NativeNode {
		let (steps, current) = self.steps();
		let mut step_nodes: Vec<NativeNode> = Vec::new();
		for (position, label) in steps.iter().enumerate() {
			if position > 0 {
				let mut props = Props::new();
				props.set_role("omp.setup.step.line");
				props.set_tone(if position <= current {
					Tone::Success
				} else {
					Tone::Muted
				});
				let key = format!("line:{}", position);
				step_nodes.push(node(NodeKind::Rule, props, Vec::new(), Some(&key)));
			}
			let state = if position < current {
				StepState::Done
			} else if position == current {
				StepState::Current
			} else {
				StepState::Next
			};
			step_nodes.push(step_node(label, position, state));
		}

		let mut mark = Props::new();
		mark.set_str("builtin", "omp");
		mark.set_str("alt", APP_NAME);
		mark.set_i64("w", 22);
		mark.set_role("omp.setup.mark");
		let mut name = Props::new();
		name.set_str("wrap", "none");
		name.set_role("omp.setup.brand.name");
		let mut label = Props::new();
		label.set_str("wrap", "none");
		label.set_role("omp.setup.brand.label");
		let mut brand_props = Props::new();
		brand_props.set_str("align", "center");
		brand_props.set_role("omp.setup.brand");
		let brand = keyed(
			row(
				vec![
					node(NodeKind::Image, mark, Vec::new(), Some("logo")),
					keyed(text(NodeText::Plain(APP_NAME.to_string()), name), "name"),
					keyed(text(NodeText::Plain("Setup".to_string()), label), "label"),
				],
				brand_props,
			),
			"brand",
		);
		let mut steps_props = Props::new();
		steps_props.set_str("align", "center");
		steps_props.set_role("omp.setup.steps");
		let mut head_props = Props::new();
		head_props.set_str("align", "center");
		head_props.set_role("omp.setup.head");
		let exit = action_button(
			"Exit setup",
			"exit",
			ActionButtonOptions {
				keys: Some("ctrl+c".to_string()),
				..ActionButtonOptions::default()
			},
		);
		keyed(
			row(
				vec![
					brand,
					keyed(row(step_nodes, steps_props), "steps"),
					exit,
				],
				head_props,
			),
			"head",
		)
	}

	/// The scene page: the setup header over the body. A terminal that draws
	/// `picker` sheets gets the active scene alone; otherwise the scene title
	/// and subtitle head the scene's inline content.
	fn describe_scene(&mut self, cx: &DescribeContext) -> NativeNode {
		let pickers = cx.supports(NodeKind::List);
		let scene = self.current_scene();
		let title = self
			.active_scene
			.as_ref()
			.map(|active| active.title().to_string())
			.or_else(|| scene.map(|scene| scene.title.to_string()))
			.unwrap_or_else(|| "Setup".to_string());
		let subtitle = self
			.active_scene
			.as_ref()
			.and_then(|active| active.subtitle().map(str::to_string));
		let head = self.describe_head();
		let mut body: Vec<NativeNode> = Vec::new();
		if !pickers {
			let mut title_props = Props::new();
			title_props.set_role("omp.setup.title");
			body.push(keyed(text(NodeText::Plain(title), title_props), "title"));
			if let Some(subtitle) = subtitle {
				let mut subtitle_props = Props::new();
				subtitle_props.set_role("omp.setup.subtitle");
				body.push(keyed(
					text(NodeText::Plain(subtitle), subtitle_props),
					"subtitle",
				));
			}
		}
		if let Some(active) = self.active_scene.as_mut() {
			body.push(active.describe_scene(cx));
		}
		let mut body_props = Props::new();
		body_props.insert("grow", 1.into());
		body_props.set_role("omp.setup.body");
		let key = format!("body:{}", scene.map(|scene| scene.id).unwrap_or("_"));
		let body_node = node(NodeKind::Col, body_props, body, Some(&key));
		let mut scene_props = Props::new();
		scene_props.insert("grow", 1.into());
		scene_props.set_role("omp.setup.scene");
		col(vec![head, body_node], scene_props)
	}
}

impl Component for SetupWizardComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.handle_input(key);
	}

	fn invalidate(&mut self) {
		self.invalidate();
	}

	fn dispose(&mut self) {
		self.dispose();
	}
}

/// Forwards the overlay's component calls to the shared wizard instance.
#[derive(Clone)]
pub struct WizardHandle(pub Rc<RefCell<SetupWizardComponent>>);

impl Component for WizardHandle {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.0.borrow_mut().render(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.0.borrow_mut().handle_input(key);
	}

	fn invalidate(&mut self) {
		self.0.borrow_mut().invalidate();
	}

	fn dispose(&mut self) {
		self.0.borrow_mut().dispose();
	}
}

/// The wizard's `SetupSceneHost`: scene callbacks land in the shared control
/// cell, which the wizard drains on its next [`pump`](SetupWizardComponent::pump).
struct WizardSceneHost {
	ctx: Rc<dyn SetupHost>,
	control: Rc<RefCell<WizardControl>>,
}

impl SetupSceneHost for WizardSceneHost {
	fn ctx(&self) -> &dyn SetupHost {
		self.ctx.as_ref()
	}

	fn request_render(&self) {
		self.ctx.request_render();
	}

	fn finish(&self, _result: SetupSceneResult) {
		self.control.borrow_mut().finish_requested = true;
	}

	fn set_focus(&self, component: Option<&dyn Component>) {
		self.control.borrow_mut().focus_taken = component.is_some();
		self.ctx.set_focus(component);
	}

	fn restore_focus(&self) {
		self.control.borrow_mut().focus_taken = false;
		self.ctx.set_focus(None);
	}
}

/// Owns the fullscreen setup overlay until its scenes and outro finish. Dropping
/// the session disposes the wizard and hides the overlay.
pub struct SetupWizardSession {
	handle: Rc<RefCell<SetupWizardComponent>>,
	overlay: Option<Box<dyn Overlay>>,
}

impl SetupWizardSession {
	/// Show the wizard and start its splash. `ctx` is shared with the host so
	/// the caller can persist completion after the session drops.
	pub fn new(ctx: Rc<dyn SetupHost>, scenes: Vec<SetupScene>) -> Self {
		let handle = Rc::new(RefCell::new(SetupWizardComponent::new(
			ctx.clone(),
			scenes,
		)));
		handle.borrow_mut().run();
		let overlay = ctx.show_overlay(
			Box::new(WizardHandle(handle.clone())),
			OverlayOptions::fullscreen(),
		);
		SetupWizardSession {
			handle,
			overlay: Some(overlay),
		}
	}

	pub fn pump(&mut self, now_ms: f64) -> bool {
		self.handle.borrow_mut().pump(now_ms)
	}

	pub fn is_done(&self) -> bool {
		self.handle.borrow().is_done()
	}

	pub fn render(&mut self, width: u16) -> Vec<String> {
		self.handle.borrow_mut().render(width)
	}

	pub fn handle_input(&mut self, key: &Key) {
		self.handle.borrow_mut().handle_input(key);
	}

	pub fn handle_sgr(&mut self, data: &str) {
		self.handle.borrow_mut().handle_sgr(data);
	}

	pub fn handle_native_event(&mut self, event: &SetupNativeEvent) {
		self.handle.borrow_mut().handle_native_event(event);
	}

	pub fn describe_screen(&mut self) -> NativeScreen {
		self.handle.borrow_mut().describe_screen()
	}
}

impl Drop for SetupWizardSession {
	fn drop(&mut self) {
		self.handle.borrow_mut().dispose();
		if let Some(overlay) = self.overlay.take() {
			overlay.hide();
		}
	}
}

/// One stepper entry: a disc (check once done, else the step number) and the
/// label.
fn step_node(label: &str, position: usize, state: StepState) -> NativeNode {
	let disc = if state == StepState::Done {
		let mut props = Props::new();
		props.set_str("name", "check");
		props.set_role("omp.setup.step.disc");
		node(NodeKind::Icon, props, Vec::new(), None)
	} else {
		let mut props = Props::new();
		props.set_role("omp.setup.step.disc");
		text(NodeText::Plain((position + 1).to_string()), props)
	};
	let mut label_props = Props::new();
	label_props.set_str("wrap", "none");
	label_props.set_role("omp.setup.step.label");
	let mut row_props = Props::new();
	row_props.set_str("align", "center");
	row_props.set_str("gap", "xs");
	row_props.set_role("omp.setup.step");
	row_props.set_tone(state.tone());
	let key = format!("step:{}", position);
	node(
		NodeKind::Row,
		row_props,
		vec![disc, text(NodeText::Plain(label.to_string()), label_props)],
		Some(&key),
	)
}

fn scene_footer_hint(theme: &Theme) -> String {
	let bindings = KeybindingsManager::new();
	format!(
		"{} select · {} confirm · {} skip · {} exit setup",
		editor_keys(&bindings, theme, &["tui.select.up", "tui.select.down"]),
		editor_key(&bindings, theme, "tui.select.confirm"),
		editor_key(&bindings, theme, "tui.select.cancel"),
		KeyHintFormatter::new(theme).format_key_hint("ctrl+c"),
	)
}

fn indent_line(line: &str, width: usize, indent: usize) -> String {
	if width == 0 {
		return String::new();
	}
	let prefix = " ".repeat(indent.min(width.saturating_sub(1)));
	pad_to_width(&format!("{}{}", prefix, line), width)
}

/// Stable per-row jitter in `[0,1)` for the dissolve reveal order.
fn row_noise(y: usize) -> f64 {
	let mut h = (y as u32) ^ 0x9e37_79b9;
	h = h.wrapping_mul(265_443_5761);
	let mixed = (h ^ (h >> 15)) & 0xffff_ffff;
	mixed as f64 / 4_294_967_296.0
}

/// Top-biased cross-dissolve between two equal-height frames. Each row flips
/// from `from` to `to` once it crosses a per-row threshold — top rows reveal
/// first, with a little jitter for an organic edge.
fn dissolve_frames(from: &[String], to: &[String], progress: f64, height: usize) -> Vec<String> {
	let eased = progress * progress * (3.0 - 2.0 * progress);
	let denom = height.saturating_sub(1).max(1) as f64;
	(0..height)
		.map(|y| {
			let threshold = 0.78 * (y as f64 / denom) + 0.22 * row_noise(y);
			let line = if eased >= threshold { to.get(y) } else { from.get(y) };
			line.cloned().unwrap_or_default()
		})
		.collect()
}

fn fit_to_screen(lines: &[String], width: usize, height: usize) -> Vec<String> {
	let mut fitted: Vec<String> = lines
		.iter()
		.take(height)
		.map(|line| {
			if width > 0 {
				pad_to_width(line, width)
			} else {
				String::new()
			}
		})
		.collect();
	while fitted.len() < height {
		fitted.push(" ".repeat(width));
	}
	fitted
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn scene_applies_skips_ansi_only_under_native() {
		let mut scene = SetupScene {
			id: "glyph",
			title: "Glyphs",
			step: "Glyphs",
			min_version: 1,
			ansi_only: true,
			should_run: None,
			mount: |_| unreachable!(),
		};
		let native = native_state().is_native_rendering();
		let applies = setup_scene_applies(&scene);
		assert_eq!(applies, !scene.ansi_only || !native);
		scene.ansi_only = false;
		assert!(setup_scene_applies(&scene));
	}

	#[test]
	fn indent_never_exceeds_width() {
		let cases = [(20, 4, "hi"), (10, 40, "hi"), (0, 4, "hi")];
		for (width, indent, line) in cases {
			let out = indent_line(line, width, indent);
			assert!(out.chars().count() <= width.max(0));
		}
	}

	#[test]
	fn dissolve_reveals_top_rows_first() {
		let from: Vec<String> = (0..8).map(|y| format!("from{}", y)).collect();
		let to: Vec<String> = (0..8).map(|y| format!("to{}", y)).collect();
		let early = dissolve_frames(&from, &to, 0.0, 8);
		let late = dissolve_frames(&from, &to, 1.0, 8);
		assert_eq!(early, from);
		assert_eq!(late, to);
	}

	#[test]
	fn row_noise_is_stable_and_bounded() {
		for y in 0..32 {
			let first = row_noise(y);
			assert!((0.0..1.0).contains(&first));
			assert_eq!(first, row_noise(y));
		}
	}

	#[test]
	fn fit_to_screen_pads_and_truncates() {
		let lines = vec!["abc".to_string()];
		let fitted = fit_to_screen(&lines, 6, 3);
		assert_eq!(fitted.len(), 3);
		assert_eq!(fitted[0], "abc   ");
		assert_eq!(fitted[1], "      ");
	}

	#[test]
	fn step_node_marks_done_with_a_check() {
		let done = step_node("Sign in", 0, StepState::Done);
		let next = step_node("Model", 1, StepState::Next);
		let done_disc = &done.children[0];
		let next_disc = &next.children[0];
		assert_eq!(done_disc.kind, NodeKind::Icon);
		assert_eq!(done_disc.props.get_str("name"), Some("check"));
		assert_eq!(next_disc.kind, NodeKind::Text);
		assert_eq!(next_disc.props.get_str("text"), Some("2"));
	}
}
