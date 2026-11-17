//! Reference: `l/src/overlays/move-overlay.ts`.
//!
//! `/move` overlay: a path field with live directory autocomplete. Tab accepts
//! the highlighted directory, Enter confirms the typed path (or the highlight
//! when the field is empty), Escape cancels.
//!
//! Deviations:
//! - The reference builds a `TextFormField` with a form theme; the crate has no
//!   form-field type, so the field is a [`crate::components::Input`] and the
//!   `Path: ` prompt plus the submit/cancel wiring live in this component.
//! - The process-global theme and keybindings are injected ([`Theme`],
//!   [`KeybindingsManager`]); hints are formatted through
//!   [`crate::chrome::keybinding_hints`].
//! - Input arrives as decoded [`Key`] events (the crate's component contract)
//!   instead of raw key data strings.
//! - `setUseTerminalCursor` has no crate counterpart (the renderer owns the
//!   caret) and is omitted.

use crate::chrome::keybinding_hints::{editor_key, editor_keys};
use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::components::Input;
use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{
	ActionButtonOptions, ActionButtonTone, action_bar, action_button, action_hint, overlay_card,
	status_hints_row,
};
use crate::theme::Theme;

/// Most directory suggestions the overlay offers.
pub const MOVE_MAX_RESULTS: usize = 15;

/// How many results the overlay asks its source for (the shown list plus slack).
pub const MOVE_SEARCH_LIMIT: usize = MOVE_MAX_RESULTS + 5;

/// Role of the native dialog.
pub const MOVE_OVERLAY_ROLE: &str = "omp.dialog.move";

/// A confirmed move destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveOverlayResult {
	/// The directory the session moved to.
	pub directory: String,
}

/// One autocomplete suggestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveDirectoryEntry {
	/// Full absolute path.
	pub value: String,
	/// Display label (basename plus a trailing slash).
	pub label: String,
}

impl MoveDirectoryEntry {
	/// A suggestion for `value`, labelled by its basename (`dir/`).
	pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
		MoveDirectoryEntry { value: value.into(), label: label.into() }
	}
}

/// Directory autocomplete supplied by the host.
pub trait MoveDirectorySource {
	/// Directories matching `prefix`, resolved against `cwd`, at most `max`.
	fn search(&self, prefix: &str, cwd: &str, max: usize) -> Vec<MoveDirectoryEntry>;
}

/// Path field with live directory autocomplete.
pub struct MoveOverlay {
	field: Input,
	focused: bool,
	selected_index: usize,
	results: Vec<MoveDirectoryEntry>,
	cwd: String,
	done: std::cell::RefCell<Option<Box<dyn FnMut(Option<MoveOverlayResult>)>>>,
	revision: u64,
	theme: Theme,
	bindings: KeybindingsManager,
	panel_theme: OverlayTheme,
}

impl MoveOverlay {
	/// An overlay resolving against `cwd`, answering `done` on confirm (`Some`)
	/// or cancel (`None`).
	pub fn new(
		theme: Theme,
		bindings: KeybindingsManager,
		cwd: impl Into<String>,
		source: &dyn MoveDirectorySource,
		done: impl FnMut(Option<MoveOverlayResult>) + 'static,
	) -> Self {
		let mut overlay = MoveOverlay {
			field: Input::new(),
			focused: false,
			selected_index: 0,
			results: Vec::new(),
			cwd: cwd.into(),
			done: std::cell::RefCell::new(Some(Box::new(done))),
			revision: 0,
			theme,
			bindings,
			panel_theme: OverlayTheme::default(),
		};
		overlay.update_results(source);
		overlay
	}

	/// Whether the path field has focus.
	pub fn focused(&self) -> bool {
		self.focused
	}

	/// Set focus.
	pub fn set_focused(&mut self, focused: bool) {
		self.focused = focused;
	}

	/// The typed path.
	pub fn value(&self) -> &str {
		self.field.value()
	}

	/// The current suggestions.
	pub fn results(&self) -> &[MoveDirectoryEntry] {
		&self.results
	}

	/// Index of the highlighted suggestion.
	pub fn selected_index(&self) -> usize {
		self.selected_index
	}

	/// The highlighted suggestion, if any.
	pub fn selected(&self) -> Option<&MoveDirectoryEntry> {
		self.results.get(self.selected_index)
	}

	/// A change counter bumped whenever the field, results or highlight move.
	pub fn revision(&self) -> u64 {
		self.revision
	}

	/// Route a pointer/native action or list selection. `source` refreshes the
	/// suggestions after an accepted path.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent, source: &dyn MoveDirectorySource) {
		match event {
			NativeUiEvent::Action { act, .. } => match act.as_str() {
				"accept" => self.accept(source),
				"confirm" => self.confirm(),
				"cancel" => self.finish(None),
				_ => {}
			},
			NativeUiEvent::Select { item, .. } | NativeUiEvent::Activate { item, .. } => {
				let Some(index) = self.results.iter().position(|result| &result.value == item) else {
					return;
				};
				if index != self.selected_index {
					self.selected_index = index;
					self.revision += 1;
				}
				if matches!(event, NativeUiEvent::Activate { .. }) {
					self.confirm();
				}
			}
			_ => {}
		}
	}

	/// Paste into the path field, refreshing the suggestions.
	pub fn paste_text(&mut self, text: &str, source: &dyn MoveDirectorySource) {
		self.field.handle_paste(text);
		self.selected_index = 0;
		self.update_results(source);
	}

	/// Answer the `done` callback: a destination (`Some`) or a cancel (`None`).
	pub fn finish(&self, result: Option<MoveOverlayResult>) {
		if let Some(mut done) = self.done.borrow_mut().take() {
			done(result);
			*self.done.borrow_mut() = Some(done);
		}
	}

	/// Fill the field with the highlighted suggestion.
	pub fn accept(&mut self, source: &dyn MoveDirectorySource) {
		let Some(selected) = self.results.get(self.selected_index) else {
			return;
		};
		let value = selected.value.clone();
		self.field.set_value(value);
		self.selected_index = 0;
		self.update_results(source);
	}

	/// Confirm the highlight, else the typed path; an empty field cancels.
	pub fn confirm(&self) {
		if let Some(selected) = self.results.get(self.selected_index) {
			self.finish(Some(MoveOverlayResult { directory: selected.value.clone() }));
			return;
		}
		let typed = self.field.value().trim();
		if typed.is_empty() {
			self.finish(None);
		} else {
			self.finish(Some(MoveOverlayResult { directory: typed.to_string() }));
		}
	}

	/// Re-run the host search for the current field value.
	pub fn update_results(&mut self, source: &dyn MoveDirectorySource) {
		self.results = source.search(self.field.value(), &self.cwd, MOVE_SEARCH_LIMIT);
		self.revision += 1;
		if self.selected_index >= self.results.len() {
			self.selected_index = self.results.len().saturating_sub(1);
		}
	}

	/// The hint row: filter, navigate, accept, confirm, cancel.
	fn hint(&self) -> String {
		let nav = editor_keys(&self.bindings, &self.theme, &["tui.select.up", "tui.select.down"]);
		let cancel = editor_key(&self.bindings, &self.theme, "tui.select.cancel");
		let formatter = KeyHintFormatter::new(&self.theme);
		let tab = formatter.format_key_hint("tab");
		let enter = formatter.format_key_hint("enter");
		format!("Type to filter · {nav} navigate · {tab} accept · {enter} confirm · {cancel} cancel")
	}

	/// The path row: the field, with a dim `Path: ` prompt while it is empty.
	fn field_row(&mut self, width: usize) -> String {
		if self.field.value().is_empty() {
			let prompt = self.theme.fg("dim", "Path: ").unwrap_or_else(|_| "Path: ".to_string());
			return prompt;
		}
		self.field.render(width.max(1) as u16).first().cloned().unwrap_or_default()
	}

	/// A native list of the suggestions with the highlight as its selection.
	pub fn describe_list(&self) -> NativeNode {
		let items: Vec<NativeNode> = self
			.results
			.iter()
			.take(MOVE_MAX_RESULTS)
			.map(|item| {
				let mut props = Props::new();
				props.set_str("label", item.label.clone());
				props.insert(
					"detail",
					serde_json::Value::Array(vec![serde_json::Value::String(item.value.clone())]),
				);
				node(NodeKind::Item, props, Vec::new(), Some(item.value.as_str()))
			})
			.collect();
		let mut props = Props::new();
		if let Some(selected) = self.selected() {
			props.set_str("selected", selected.value.clone());
		}
		if self.results.is_empty() && !self.field.value().is_empty() {
			props.insert(
				"empty",
				serde_json::Value::Array(vec![serde_json::Value::String("No matching directories".to_string())]),
			);
		}
		node(NodeKind::List, props, items, Some("results"))
	}

	/// The whole dialog: field, suggestions, hint strip and buttons.
	pub fn describe(&self, _cx: &DescribeContext) -> NativeNode {
		let mut field_props = Props::new();
		field_props.set_str("role", "omp.field");
		field_props.set_text(self.field.value().to_string());
		let field = node(NodeKind::Input, field_props, Vec::new(), Some("path"));
		let hints = status_hints_row(
			NodeText::Spans(vec![span("Type to filter", Some("dim"))]),
			&[action_hint(&self.bindings, &["tui.select.up", "tui.select.down"][..], "navigate")],
		);
		let cancel_key = self.bindings.get_keys("tui.select.cancel").first().cloned();
		let actions = action_bar(
			&[
				Some(action_button(
					"Accept",
					"accept",
					ActionButtonOptions { keys: Some("tab".to_string()), ..Default::default() },
				)),
				None,
				Some(action_button(
					"Cancel",
					"cancel",
					ActionButtonOptions {
						keys: Some(cancel_key.unwrap_or_else(|| "escape".to_string())),
						..Default::default()
					},
				)),
				Some(action_button(
					"Confirm",
					"confirm",
					ActionButtonOptions {
						keys: Some("enter".to_string()),
						tone: Some(ActionButtonTone::Accent),
						..Default::default()
					},
				)),
			],
			MOVE_OVERLAY_ROLE,
		);
		let list = self.describe_list();
		overlay_card(
			MOVE_OVERLAY_ROLE,
			Some(NodeText::Plain("Move to directory".to_string())),
			vec![field, list, hints, actions],
			None,
		)
	}
}

impl Component for MoveOverlay {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = usize::from(width).saturating_sub(4);
		let field = self.field_row(inner);
		let mut lines = vec![
			top_border(width, "Move to directory", &self.panel_theme),
			row(&field, width, &self.panel_theme),
			row("", width, &self.panel_theme),
		];
		if self.results.is_empty() && !self.field.value().is_empty() {
			let none = "No matching directories";
			let none = self.theme.fg("dim", none).unwrap_or_else(|_| none.to_string());
			lines.push(row(&none, width, &self.panel_theme));
		} else {
			for (index, item) in self.results.iter().take(MOVE_MAX_RESULTS).enumerate() {
				let selected = index == self.selected_index;
				let marker = if selected {
					self.theme.fg("accent", "▶ ").unwrap_or_else(|_| "▶ ".to_string())
				} else {
					"  ".to_string()
				};
				let tone = if selected { "accent" } else { "text" };
				let label = self.theme.fg(tone, &item.label).unwrap_or_else(|_| item.label.clone());
				lines.push(row(&format!("{marker}{label}"), width, &self.panel_theme));
			}
		}
		lines.push(row("", width, &self.panel_theme));
		let hint = self.hint();
		let hint = self.theme.fg("dim", &hint).unwrap_or(hint);
		lines.push(row(&hint, width, &self.panel_theme));
		lines.push(bottom_border(width, &self.panel_theme));
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		// Results only change through typed text; the host re-runs the search on
		// the next key by calling `update_results`, so a keystroke that yields no
		// new suggestions keeps the old list.
		match key {
			Key::Esc | Key::Ctrl('c') => self.finish(None),
			Key::Enter => self.confirm(),
			Key::Up => {
				if !self.results.is_empty() {
					let next = self.selected_index.saturating_sub(1);
					if next != self.selected_index {
						self.selected_index = next;
						self.revision += 1;
					}
				}
			}
			Key::Down => {
				if !self.results.is_empty() {
					let next = (self.selected_index + 1).min(self.results.len() - 1);
					if next != self.selected_index {
						self.selected_index = next;
						self.revision += 1;
					}
				}
			}
			_ => {
				self.field.handle_input(key);
			}
		}
	}

	fn handle_paste(&mut self, text: &str) {
		self.field.handle_paste(text);
		self.selected_index = 0;
		self.revision += 1;
	}

	fn invalidate(&mut self) {
		self.revision += 1;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{CreateThemeOptions, create_theme};
	use std::cell::RefCell;
	use std::rc::Rc;

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin dark theme"),
			CreateThemeOptions::default(),
		)
		.expect("valid theme")
	}

	/// A source whose suggestions are the directories under `/work`.
	struct FixedSource;

	impl MoveDirectorySource for FixedSource {
		fn search(&self, prefix: &str, _cwd: &str, max: usize) -> Vec<MoveDirectoryEntry> {
			["/work/alpha/", "/work/beta/", "/work/gamma/"]
				.into_iter()
				.filter(|path| path.contains(prefix))
				.take(max)
				.map(|path| {
					let label = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string();
					MoveDirectoryEntry::new(path, format!("{label}/"))
				})
				.collect()
		}
	}

	fn overlay(source: &FixedSource) -> (MoveOverlay, Rc<RefCell<Option<Option<MoveOverlayResult>>>>) {
		let answered = Rc::new(RefCell::new(None));
		let sink = Rc::clone(&answered);
		let overlay = MoveOverlay::new(
			theme(),
			KeybindingsManager::new(),
			"/work",
			source,
			move |result| *sink.borrow_mut() = Some(result),
		);
		(overlay, answered)
	}

	#[test]
	fn constructor_searches_once_for_the_empty_prefix() {
		let source = FixedSource;
		let (overlay, _answered) = overlay(&source);
		assert_eq!(overlay.results().len(), 3);
		assert_eq!(overlay.selected_index(), 0);
		assert_eq!(overlay.selected().map(|entry| entry.label.as_str()), Some("alpha/"));
	}

	#[test]
	fn tab_accepts_the_highlight_and_enter_confirms_it() {
		let source = FixedSource;
		let (mut overlay, answered) = overlay(&source);
		overlay.handle_input(&Key::Down);
		assert_eq!(overlay.selected_index(), 1);
		overlay.accept(&source);
		assert_eq!(overlay.value(), "/work/beta/");
		overlay.update_results(&source);
		overlay.handle_input(&Key::Enter);
		assert_eq!(
			answered.borrow().clone(),
			Some(Some(MoveOverlayResult { directory: "/work/beta/".to_string() }))
		);
	}

	#[test]
	fn arrow_keys_stay_inside_the_result_list() {
		let source = FixedSource;
		let (mut overlay, _answered) = overlay(&source);
		for _ in 0..5 {
			overlay.handle_input(&Key::Down);
		}
		assert_eq!(overlay.selected_index(), 2);
		for _ in 0..5 {
			overlay.handle_input(&Key::Up);
		}
		assert_eq!(overlay.selected_index(), 0);
	}

	#[test]
	fn escape_cancels_without_a_destination() {
		let source = FixedSource;
		let (mut overlay, answered) = overlay(&source);
		overlay.handle_input(&Key::Esc);
		assert_eq!(answered.borrow().clone(), Some(None));
	}

	#[test]
	fn filtering_the_field_repicks_the_highlight() {
		let source = FixedSource;
		let (mut overlay, _answered) = overlay(&source);
		overlay.handle_input(&Key::Down);
		overlay.paste_text("gam", &source);
		assert_eq!(overlay.results().len(), 1);
		assert_eq!(overlay.selected_index(), 0);
		assert_eq!(overlay.results()[0].label, "gamma/");
	}

	#[test]
	fn native_actions_accept_confirm_and_cancel() {
		let source = FixedSource;
		let (mut overlay, answered) = overlay(&source);
		overlay.handle_native_event(
			&NativeUiEvent::Select { key: vec!["results".to_string()], item: "/work/gamma/".to_string() },
			&source,
		);
		assert_eq!(overlay.selected_index(), 2);
		overlay.handle_native_event(
			&NativeUiEvent::Action { key: vec![], act: "accept".to_string(), value: None, mods: Vec::new() },
			&source,
		);
		assert_eq!(overlay.value(), "/work/gamma/");
		overlay.handle_native_event(
			&NativeUiEvent::Action { key: vec![], act: "cancel".to_string(), value: None, mods: Vec::new() },
			&source,
		);
		assert_eq!(answered.borrow().clone(), Some(None));
	}

	#[test]
	fn render_frames_the_field_results_and_hint() {
		let source = FixedSource;
		let (mut overlay, _answered) = overlay(&source);
		let lines = overlay.render(40);
		assert!(lines[0].contains("Move to directory"));
		assert!(lines.iter().any(|line| line.contains("alpha/")));
		assert!(lines.iter().any(|line| line.contains("Type to filter")));
		assert_eq!(lines.len(), 3 + 3 + 1 + 1 + 1);
	}

	#[test]
	fn empty_and_unmatched_input_shows_no_matches_and_cancels_on_enter() {
		let source = FixedSource;
		let (mut overlay, answered) = overlay(&source);
		overlay.paste_text("zzz", &source);
		assert!(overlay.results().is_empty());
		let lines = overlay.render(40);
		assert!(lines.iter().any(|line| line.contains("No matching directories")));
		overlay.handle_input(&Key::Enter);
		assert_eq!(
			answered.borrow().clone(),
			Some(Some(MoveOverlayResult { directory: "zzz".to_string() }))
		);
	}
}
