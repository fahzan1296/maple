//! Reference: `l/src/overlays/plan-save-overlay.ts`.
//!
//! Collects a destination path before saving a plan and starting a new
//! session.
//!
//! Deviations:
//! - The reference reads the process-global theme and keybindings; the crate has
//!   no global UI state, so [`PlanSaveOverlay::new`] takes a [`Theme`] and a
//!   [`KeybindingsManager`], and paints the panel chrome itself with
//!   [`crate::chrome::overlay_box`] helpers instead of mounting an
//!   `OverlayPanel`/`PanelRows` pair (the same four chrome rows, two body rows).
//! - `Input.onEscape` does not exist in the crate, so Escape is handled here and
//!   answers the `done` callback directly.
//! - The crate's `Input` has no native description and is not cloneable, so
//!   `describe` emits the field's current line as a plain `text` node.

use std::cell::RefCell;

use crate::chrome::keybinding_hints::editor_key;
use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::components::Input;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{CURSOR_MARKER, col, keyed, span, text};
use crate::native::node::{DescribeContext, NativeNode, Props};
use crate::native::overlay::{ActionButtonOptions, ActionButtonTone, action_bar, action_button};
use crate::text_shape::{Ellipsis, truncate_to_width, visible_width};
use crate::theme::Theme;

/// A confirmed destination chosen from [`PlanSaveOverlay`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanSaveOverlayResult {
	/// Where the plan is written.
	pub path: String,
}

/// Semantic role of the native sheet.
pub const PLAN_SAVE_ROLE: &str = "omp.overlay.planSave";

/// Title of the panel and its native head.
pub const PLAN_SAVE_TITLE: &str = "Save and quit";

/// Collects a destination path before saving a plan and starting a new session.
pub struct PlanSaveOverlay {
	input: Input,
	suggested_path: String,
	done: RefCell<Option<Box<dyn FnMut(Option<PlanSaveOverlayResult>)>>>,
	focused: bool,
	theme: Theme,
	bindings: KeybindingsManager,
	panel_theme: OverlayTheme,
}

impl PlanSaveOverlay {
	/// An overlay proposing `suggested_path`, answering `done` on submit
	/// (`Some`) or cancel (`None`).
	pub fn new(
		theme: Theme,
		bindings: KeybindingsManager,
		suggested_path: impl Into<String>,
		done: impl FnMut(Option<PlanSaveOverlayResult>) + 'static,
	) -> Self {
		PlanSaveOverlay {
			input: Input::new(),
			suggested_path: suggested_path.into(),
			done: RefCell::new(Some(Box::new(done))),
			focused: false,
			theme,
			bindings,
			panel_theme: OverlayTheme::default(),
		}
	}

	/// Whether the path field has focus (drawn as a caret).
	pub fn focused(&self) -> bool {
		self.focused
	}

	/// Set focus.
	pub fn set_focused(&mut self, focused: bool) {
		self.focused = focused;
	}

	/// Replaces the dimmed path accepted when the operator submits an empty
	/// input.
	pub fn set_suggested_path(&mut self, path: impl Into<String>) {
		self.suggested_path = path.into();
	}

	/// The path typed so far.
	pub fn value(&self) -> &str {
		self.input.value()
	}

	/// The path a submit would use: the typed value, else the suggestion.
	pub fn submitted_path(&self) -> String {
		let typed = self.input.value().trim();
		if typed.is_empty() {
			self.suggested_path.clone()
		} else {
			typed.to_string()
		}
	}

	/// Answer the `done` callback with a result (`Some`) or a cancel (`None`).
	pub fn finish(&self, result: Option<PlanSaveOverlayResult>) {
		if let Some(mut done) = self.done.borrow_mut().take() {
			done(result);
			// The reference keeps its callback installed for a second submit;
			// put it back so a host that renders once more behaves the same.
			*self.done.borrow_mut() = Some(done);
		}
	}

	/// Route a pointer/native action: `save` submits, `cancel` cancels.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		let NativeUiEvent::Action { act, .. } = event else {
			return;
		};
		match act.as_str() {
			"save" => self.finish(Some(PlanSaveOverlayResult { path: self.submitted_path() })),
			"cancel" => self.finish(None),
			_ => {}
		}
	}

	/// A small glass sheet titled "Save and quit".
	pub fn describe(&self, _cx: &DescribeContext) -> NativeNode {
		let mut children = vec![text(self.input.value().to_string(), Props::new())];
		if self.input.value().is_empty() {
			children.push(keyed(
				text(
					vec![
						span("Empty saves to ", Some("dim")),
						span(self.suggested_path.as_str(), Some("path")),
					],
					Props::new(),
				),
				"default",
			));
		}
		let cancel_key = self.bindings.get_keys("tui.select.cancel").first().cloned();
		let submit_key = self.bindings.get_keys("tui.input.submit").first().cloned();
		children.push(action_bar(
			&[
				None,
				Some(action_button(
					"Cancel",
					"cancel",
					ActionButtonOptions { keys: cancel_key, ..Default::default() },
				)),
				Some(action_button(
					"Save and quit",
					"save",
					ActionButtonOptions {
						keys: submit_key,
						tone: Some(ActionButtonTone::Accent),
						..Default::default()
					},
				)),
			],
			PLAN_SAVE_ROLE,
		));
		col(children, Props::new())
	}

	/// The hint row: submit and cancel keys, resolved from the bindings.
	fn hint(&self) -> String {
		let submit = editor_key(&self.bindings, &self.theme, "tui.input.submit");
		let cancel = editor_key(&self.bindings, &self.theme, "tui.select.cancel");
		format!("{submit} save and quit · {cancel} cancel")
	}

	/// The path row: the typed value, else the dimmed suggestion behind a caret.
	fn render_input(&mut self, width: usize) -> String {
		if !self.input.value().is_empty() {
			return self.input.render(width.max(1) as u16).first().cloned().unwrap_or_default();
		}
		let prompt = self.theme.fg("dim", "Path: ").unwrap_or_else(|_| "Path: ".to_string());
		let available = width.saturating_sub(visible_width(&prompt));
		if available == 0 {
			return prompt;
		}
		let marker = if self.focused { CURSOR_MARKER } else { "" };
		let suggested = truncate_to_width(&self.suggested_path, available.saturating_sub(1), Ellipsis::Omit);
		let suggested = self.theme.fg("dim", &suggested).unwrap_or(suggested);
		format!("{prompt}{marker}\x1b[7m \x1b[27m{suggested}")
	}
}

impl Component for PlanSaveOverlay {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = usize::from(width).saturating_sub(4);
		let input_row = self.render_input(inner);
		let hint = self.hint();
		let hint = self.theme.fg("dim", &hint).unwrap_or(hint);
		vec![
			top_border(width, PLAN_SAVE_TITLE, &self.panel_theme),
			row(&input_row, width, &self.panel_theme),
			row(&hint, width, &self.panel_theme),
			bottom_border(width, &self.panel_theme),
		]
	}

	fn handle_input(&mut self, key: &Key) {
		if matches!(key, Key::Esc) {
			self.finish(None);
			return;
		}
		if matches!(key, Key::Enter) {
			self.finish(Some(PlanSaveOverlayResult { path: self.submitted_path() }));
			return;
		}
		self.input.handle_input(key);
	}

	fn handle_paste(&mut self, text: &str) {
		self.input.handle_paste(text);
	}

	fn invalidate(&mut self) {
		self.input.invalidate();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{CreateThemeOptions, create_theme};
	use std::cell::RefCell as StdRefCell;
	use std::rc::Rc;

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin dark theme"),
			CreateThemeOptions::default(),
		)
		.expect("valid theme")
	}

	fn overlay(
		suggested: &str,
	) -> (PlanSaveOverlay, Rc<StdRefCell<Option<Option<PlanSaveOverlayResult>>>>) {
		let answered = Rc::new(StdRefCell::new(None));
		let sink = Rc::clone(&answered);
		let overlay = PlanSaveOverlay::new(
			theme(),
			KeybindingsManager::new(),
			suggested,
			move |result| *sink.borrow_mut() = Some(result),
		);
		(overlay, answered)
	}

	#[test]
	fn empty_submit_saves_to_the_suggested_path() {
		let (mut overlay, answered) = overlay("plans/next.md");
		overlay.handle_input(&Key::Enter);
		assert_eq!(
			answered.borrow().clone(),
			Some(Some(PlanSaveOverlayResult { path: "plans/next.md".to_string() }))
		);
	}

	#[test]
	fn typed_path_overrides_the_suggestion() {
		let (mut overlay, answered) = overlay("plans/next.md");
		overlay.handle_paste("notes/draft.md");
		assert_eq!(overlay.submitted_path(), "notes/draft.md");
		overlay.handle_input(&Key::Enter);
		assert_eq!(
			answered.borrow().clone(),
			Some(Some(PlanSaveOverlayResult { path: "notes/draft.md".to_string() }))
		);
	}

	#[test]
	fn escape_cancels_without_a_result() {
		let (mut overlay, answered) = overlay("plans/next.md");
		overlay.handle_input(&Key::Esc);
		assert_eq!(answered.borrow().clone(), Some(None));
	}

	#[test]
	fn suggested_path_is_replaceable_and_drives_the_hint_row() {
		let (mut overlay, _answered) = overlay("plans/next.md");
		overlay.set_suggested_path("plans/final.md");
		assert_eq!(overlay.submitted_path(), "plans/final.md");
		let rows = overlay.render(40);
		assert_eq!(rows.len(), 4, "top border, path, hint, bottom border");
		assert!(rows[1].contains("Path:"));
		assert!(rows[2].contains("save and quit"));
	}

	#[test]
	fn native_save_action_submits_and_cancel_action_cancels() {
		let (mut overlay, answered) = overlay("plans/next.md");
		overlay.handle_paste("typed.md");
		overlay.handle_native_event(&NativeUiEvent::Action {
			key: vec!["save".to_string()],
			act: "save".to_string(),
			value: None,
			mods: Vec::new(),
		});
		assert_eq!(
			answered.borrow().clone(),
			Some(Some(PlanSaveOverlayResult { path: "typed.md".to_string() }))
		);
		overlay.handle_native_event(&NativeUiEvent::Action {
			key: vec!["cancel".to_string()],
			act: "cancel".to_string(),
			value: None,
			mods: Vec::new(),
		});
		assert_eq!(answered.borrow().clone(), Some(None));
	}

	#[test]
	fn describe_carries_the_default_hint_only_while_the_field_is_empty() {
		let (mut overlay, _answered) = overlay("plans/next.md");
		let cx = DescribeContext::new(60);
		let node = overlay.describe(&cx);
		assert!(
			node.children.iter().any(|child| child.key.as_deref() == Some("default")),
			"an empty field advertises the default"
		);
		overlay.handle_paste("typed.md");
		let node = overlay.describe(&cx);
		assert!(node.children.iter().all(|child| child.key.as_deref() != Some("default")));
	}
}
