//! Reference: `l/src/overlays/hook-editor.ts`.
//!
//! Interactive multiline dialog for hooks and the ask tool's "Other" answer:
//! prompt-style or hook-style key routing, draft-preserving cancel, image
//! attachment markers, ordered clipboard delivery, and the native sheet.
//!
//! Deviations:
//! - `ImageContent` from `@oh-my-pi/pi-ai` is opaque here ([`ImageContent`]);
//!   only the count and marker numbering are modelled.
//! - `TUI` is not held: the external-editor round trip is the host's job; the
//!   component only reports the action and exposes the current draft.
//! - Keybinding lookups (`app.message.followUp`, `app.editor.external`,
//!   `app.interrupt`) are injected as [`HookEditorKeymap`].
//! - `FormField` chrome is not composed; [`HookEditorComponent::describe`]
//!   returns the native sheet and the host renders it.
//! - `Editor` is [`crate::components::Editor`], whose buffer replaces the
//!   reference's expanded-text/atomic-token editor.

use crate::component::Component;
use crate::components::editor::Editor;
use crate::keys::{is_enter, is_escape, Key};
use crate::native::describe::{col, md, node, span};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{action_bar, action_button, ActionButtonOptions, ActionButtonTone};
use crate::prompt::composer_attachments::{compact_image_markers, format_vision_marker, ChipKind};
use crate::prompt::custom_editor::extract_image_paste_paths_from_text;
use crate::text_shape::{truncate_to_width, wrap_text_with_ansi, Ellipsis};

/// Root role of the native sheet.
pub const ROLE: &str = "omp.overlay.hook-editor";

/// Role prefix of its parts (`.head`, `.title`, `.detail`, `.eyebrow`,
/// `.field`/`.line`).
pub const PART: &str = "omp.hook-editor";

/// Most title rows the terminal prompt shows, so a long question cannot push
/// the input row off-screen.
pub const MAX_PROMPT_TITLE_ROWS: usize = 3;

/// Border (2) + padX (2) columns consumed by the panel chrome.
pub const PROMPT_TITLE_CHROME_COLUMNS: usize = 4;

/// Image payload supplied by the host (`ImageContent` in the reference).
pub type ImageContent = serde_json::Value;

/// `prefix` and `question` flattened onto one line, wrapped to the terminal's
/// width and cut to [`MAX_PROMPT_TITLE_ROWS`] rows (the last ends in `…` when
/// cut). `columns` is the terminal width (the reference reads
/// `process.stdout.columns`).
#[must_use]
pub fn bound_prompt_title(prefix: &str, question: &str, columns: usize) -> String {
	let width = columns.saturating_sub(PROMPT_TITLE_CHROME_COLUMNS).max(1);
	let flat = format!("{prefix}{question}").replace('\t', "    ");
	let flat: String = flat.split_whitespace().collect::<Vec<_>>().join(" ");
	let wrapped = wrap_text_with_ansi(&flat, width);
	if wrapped.len() <= MAX_PROMPT_TITLE_ROWS {
		return wrapped.join("\n");
	}
	let kept = &wrapped[..MAX_PROMPT_TITLE_ROWS - 1];
	let last = truncate_to_width(
		&wrapped[MAX_PROMPT_TITLE_ROWS - 1..].join(" "),
		width,
		Ellipsis::Unicode,
	);
	let mut rows: Vec<String> = kept.to_vec();
	rows.push(last);
	rows.join("\n")
}

/// Options of the hook editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HookEditorOptions {
	/// Use prompt-style keybindings with the legacy ask prompt chrome.
	pub prompt_style: bool,
	/// The question the text answers.
	pub question: Option<String>,
	/// Allow clipboard images to be attached to this prompt.
	pub accept_images: bool,
	/// Images already represented by markers in the prefilled text.
	pub images: Vec<ImageContent>,
	/// Max rows the inner editor may occupy.
	pub max_height: Option<usize>,
}

/// Follow-up / external-editor key chords the host resolved from keybindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookEditorKeymap {
	/// Submit chord (default `ctrl+q`, reference `ctrl+q`/`ctrl+enter`).
	pub follow_up: Vec<Key>,
	/// External-editor chord (default `ctrl+g`).
	pub external: Vec<Key>,
	/// Interrupt chord (default `escape`).
	pub interrupt: Vec<Key>,
}

impl Default for HookEditorKeymap {
	fn default() -> Self {
		HookEditorKeymap {
			follow_up: vec![Key::Ctrl('q')],
			external: vec![Key::Ctrl('g')],
			interrupt: vec![Key::Esc],
		}
	}
}

impl HookEditorKeymap {
	fn matches(keys: &[Key], key: &Key) -> bool {
		keys.iter().any(|candidate| candidate == key)
	}

	#[must_use]
	pub fn is_follow_up(&self, key: &Key) -> bool {
		Self::matches(&self.follow_up, key)
	}

	#[must_use]
	pub fn is_external(&self, key: &Key) -> bool {
		Self::matches(&self.external, key)
	}

	#[must_use]
	pub fn is_interrupt(&self, key: &Key) -> bool {
		Self::matches(&self.interrupt, key)
	}
}

/// One reserved clipboard delivery slot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PendingPaste {
	settled: bool,
	text: Option<String>,
}

/// Outcome of feeding text to the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteOutcome {
	/// Text was inserted into the draft.
	Inserted,
	/// The text was delivered to an image-path callback.
	ImagePaths(Vec<String>),
	/// An empty paste while images are accepted; the host must read the
	/// clipboard and complete the reserved slot.
	RequestImage,
}

/// The dialog.
pub struct HookEditorComponent {
	editor: Editor,
	title: String,
	question: Option<String>,
	prompt_style: bool,
	accept_images: bool,
	images: Vec<ImageContent>,
	on_submit: Box<dyn FnMut(&str, &[ImageContent])>,
	on_cancel: Box<dyn FnMut(&str, &[ImageContent])>,
	on_paste_image_path: Option<Box<dyn FnMut(&str)>>,
	pending_pastes: Vec<PendingPaste>,
	submit_queued: bool,
	disposed: bool,
	has_external_editor: bool,
	keys: HookEditorKeymap,
}

impl HookEditorComponent {
	pub fn new(
		title: impl Into<String>,
		prefill: Option<&str>,
		on_submit: impl FnMut(&str, &[ImageContent]) + 'static,
		on_cancel: impl FnMut(&str, &[ImageContent]) + 'static,
		options: HookEditorOptions,
	) -> Self {
		let title = title.into();
		let mut editor = Editor::new();
		if options.prompt_style {
			editor.disable_submit(true);
		}
		if let Some(height) = options.max_height {
			editor.set_max_height(Some(height));
		}
		if let Some(prefill) = prefill {
			editor.set_text(prefill);
		}
		let images = if options.accept_images {
			options.images.clone()
		} else {
			Vec::new()
		};
		HookEditorComponent {
			editor,
			title,
			question: options.question,
			prompt_style: options.prompt_style,
			accept_images: options.accept_images,
			images,
			on_submit: Box::new(on_submit),
			on_cancel: Box::new(on_cancel),
			on_paste_image_path: None,
			pending_pastes: Vec::new(),
			submit_queued: false,
			disposed: false,
			has_external_editor: false,
			keys: HookEditorKeymap::default(),
		}
	}

	pub fn set_keymap(&mut self, keys: HookEditorKeymap) {
		self.keys = keys;
	}

	/// Mark that the host installed an external-editor callback, so the sheet
	/// shows the "Open in editor" action.
	pub fn set_external_editor_available(&mut self, available: bool) {
		self.has_external_editor = available;
	}

	pub fn set_on_paste_image_path(&mut self, callback: impl FnMut(&str) + 'static) {
		self.on_paste_image_path = Some(Box::new(callback));
	}

	#[must_use]
	pub fn title(&self) -> &str {
		&self.title
	}

	#[must_use]
	pub fn question(&self) -> Option<&str> {
		self.question.as_deref()
	}

	#[must_use]
	pub fn text(&self) -> String {
		self.editor.get_text()
	}

	#[must_use]
	pub fn prompt_style(&self) -> bool {
		self.prompt_style
	}

	#[must_use]
	pub fn disposed(&self) -> bool {
		self.disposed
	}

	/// Whether this prompt opted into clipboard image attachments.
	#[must_use]
	pub fn accepts_images(&self) -> bool {
		self.accept_images && !self.disposed
	}

	#[must_use]
	pub fn images(&self) -> &[ImageContent] {
		&self.images
	}

	/// Attach an image; returns its `[Image #N, WxH]` marker for the caller to
	/// deliver.
	pub fn attach_image(
		&mut self,
		image: ImageContent,
		dims: Option<(u32, u32)>,
	) -> Option<String> {
		if !self.accepts_images() {
			return None;
		}
		self.images.push(image);
		Some(format_vision_marker(
			ChipKind::Image,
			self.images.len() as i64,
			dims,
		))
	}

	/// Expanded editor text, with attached images renumbered in text order.
	#[must_use]
	pub fn current_value(&self) -> (String, Option<Vec<ImageContent>>) {
		let text = self.text();
		if self.images.is_empty() {
			return (text, None);
		}
		match compact_image_markers(&text, self.images.len() as i64, true) {
			None => (text, Some(self.images.clone())),
			Some((compacted, keep)) => {
				let images = keep
					.iter()
					.filter_map(|index| self.images.get((*index - 1).max(0) as usize).cloned())
					.collect();
				(compacted, Some(images))
			}
		}
	}

	/// Replace the draft (Ctrl+R history search does this for the composer).
	pub fn set_text(&mut self, text: &str) {
		if self.disposed {
			return;
		}
		self.editor.set_text(text);
	}

	/// Replace the draft, as the external editor's result does.
	pub fn apply_external_result(&mut self, text: &str) {
		if !self.disposed {
			self.editor.set_text(text);
		}
	}

	/// Submit the current text. With `require_text`, an empty draft is ignored.
	pub fn submit(&mut self, require_text: bool) {
		if self.disposed {
			return;
		}
		if !self.pending_pastes.is_empty() {
			self.submit_queued = true;
			return;
		}
		let (text, images) = self.current_value();
		if require_text && text.trim().is_empty() {
			return;
		}
		self.dispose();
		(self.on_submit)(&text, images.as_deref().unwrap_or(&[]));
	}

	pub fn cancel(&mut self) {
		if self.disposed {
			return;
		}
		let (text, images) = self.current_value();
		self.dispose();
		(self.on_cancel)(&text, images.as_deref().unwrap_or(&[]));
	}

	/// Reserve ordered clipboard delivery. Complete it with [`Self::complete_paste`].
	pub fn begin_paste(&mut self) -> usize {
		if self.disposed {
			return usize::MAX;
		}
		self.pending_pastes.push(PendingPaste::default());
		self.pending_pastes.len() - 1
	}

	/// Complete a reserved slot. Nonempty text lands once; `None` releases the
	/// wait without submitting. Returns whether text was delivered.
	pub fn complete_paste(&mut self, token: usize, text: Option<&str>) -> bool {
		if self.disposed {
			return false;
		}
		let Some(slot) = self.pending_pastes.get_mut(token) else {
			return false;
		};
		if slot.settled {
			return false;
		}
		slot.settled = true;
		slot.text = text.map(str::to_string);
		if text.is_none() {
			self.submit_queued = false;
		}
		let mut delivered = 0;
		for paste in &self.pending_pastes {
			if !paste.settled {
				break;
			}
			if let Some(text) = &paste.text {
				self.editor.handle_paste(text);
			}
			delivered += 1;
		}
		if delivered > 0 {
			self.pending_pastes.drain(..delivered);
		}
		if self.pending_pastes.is_empty() && self.submit_queued {
			self.submit_queued = false;
			self.submit(true);
		}
		text.is_some()
	}

	/// Route non-bracketed paste transports into the editor, mirroring
	/// bracketed-paste semantics.
	pub fn paste_text(&mut self, text: &str) -> PasteOutcome {
		if self.disposed {
			return PasteOutcome::Inserted;
		}
		if !self.pending_pastes.is_empty() {
			let token = self.begin_paste();
			self.complete_paste(token, Some(text));
			return PasteOutcome::Inserted;
		}
		if self.accepts_images() && text.is_empty() {
			return PasteOutcome::RequestImage;
		}
		if self.accepts_images() {
			if let Some(paths) = extract_image_paste_paths_from_text(text) {
				if self.on_paste_image_path.is_some() {
					for path in paths.iter() {
						if let Some(callback) = self.on_paste_image_path.as_mut() {
							callback(path);
						}
					}
					return PasteOutcome::ImagePaths(paths);
				}
			}
		}
		self.editor.handle_paste(text);
		PasteOutcome::Inserted
	}

	/// Handle one key. Returns the action the host still has to perform.
	pub fn handle_key(&mut self, key: &Key) -> HookEditorAction {
		if self.disposed {
			return HookEditorAction::None;
		}
		if self.prompt_style {
			// Submit on the follow-up chord first so it wins over the editor's
			// own modified-Enter handling.
			if self.keys.is_follow_up(key) {
				self.submit(false);
				return HookEditorAction::None;
			}
			if self.keys.is_interrupt(key) || is_escape(key) {
				self.cancel();
				return HookEditorAction::None;
			}
			if self.keys.is_external(key) {
				return HookEditorAction::ExternalEditor;
			}
			if is_enter(key) {
				self.submit(false);
				return HookEditorAction::None;
			}
			self.editor.handle_input(key);
			return HookEditorAction::None;
		}

		if self.keys.is_follow_up(key) {
			self.submit(false);
			return HookEditorAction::None;
		}
		if is_enter(key) {
			// Plain Enter inserts a new line in hook style.
			self.editor.handle_paste("\n");
			return HookEditorAction::None;
		}
		if self.keys.is_interrupt(key) {
			self.cancel();
			return HookEditorAction::None;
		}
		if self.keys.is_external(key) {
			return HookEditorAction::ExternalEditor;
		}
		self.editor.handle_input(key);
		HookEditorAction::None
	}

	/// Native action (`submit` / `cancel` / `external`).
	pub fn handle_native_action(&mut self, act: &str) -> HookEditorAction {
		if self.disposed {
			return HookEditorAction::None;
		}
		match act {
			"submit" => {
				self.submit(false);
				HookEditorAction::None
			}
			"cancel" => {
				self.cancel();
				HookEditorAction::None
			}
			"external" => HookEditorAction::ExternalEditor,
			_ => HookEditorAction::None,
		}
	}

	/// Native sheet: title (or question under the prompt's name), the editor,
	/// and Open in editor · Cancel · Submit.
	#[must_use]
	pub fn describe(&self) -> NativeNode {
		let field_role = if self.prompt_style {
			format!("{PART}.line")
		} else {
			format!("{PART}.field")
		};
		let mut head_children = Vec::new();
		match &self.question {
			Some(question) => {
				head_children.push(node(
					NodeKind::Text,
					{
						let mut props = Props::new();
						props.set_spans(vec![span(self.title.clone(), None)]);
						props.set_str("role", format!("{PART}.eyebrow"));
						props.set_str("wrap", "none");
						props.set_str("truncate", "end");
						props
					},
					Vec::new(),
					Some("eyebrow"),
				));
				head_children.push(md(
					question.replace('\t', "    ").replace('\r', ""),
					{
						let mut props = Props::new();
						props.set_str("role", "omp.ask.question");
						props
					},
				));
			}
			None => {
				let mut lines = self.title.split('\n');
				let title_line = lines.next().unwrap_or("").to_string();
				let detail = lines.collect::<Vec<_>>().join("\n");
				head_children.push(node(
					NodeKind::Text,
					{
						let mut props = Props::new();
						props.set_spans(vec![span(title_line, None)]);
						props.set_str("role", format!("{PART}.title"));
						props.set_str("wrap", "word");
						props
					},
					Vec::new(),
					Some("title"),
				));
				if !detail.is_empty() {
					head_children.push(node(
						NodeKind::Text,
						{
							let mut props = Props::new();
							props.set_spans(vec![span(detail, None)]);
							props.set_str("wrap", "word");
							props.set_str("role", format!("{PART}.detail"));
							props
						},
						Vec::new(),
						Some("detail"),
					));
				}
			}
		}
		let mut head_props = Props::new();
		head_props.set_str("role", format!("{PART}.head"));
		head_props.set_str("gap", "xs");
		let head = col(head_children, head_props);

		let mut field_props = Props::new();
		field_props.set_str("role", field_role);
		field_props.set_str("value", self.text());
		field_props.set_str(
			"placeholder",
			if self.prompt_style {
				"Type your answer"
			} else {
				"Type here"
			},
		);
		let field = node(NodeKind::Col, field_props, Vec::new(), Some("editor"));

		let cancel_key = "escape";
		let submit_key = if self.prompt_style {
			"enter".to_string()
		} else {
			"ctrl+q".to_string()
		};
		let newline_key = if self.prompt_style {
			"shift+enter"
		} else {
			"enter"
		};
		let mut buttons: Vec<Option<NativeNode>> = Vec::new();
		if self.has_external_editor {
			buttons.push(Some(action_button(
				"Open in editor",
				"external",
				ActionButtonOptions {
					keys: Some("ctrl+g".to_string()),
					title: Some("Edit in your external editor  ^G".to_string()),
					..ActionButtonOptions::default()
				},
			)));
		}
		buttons.push(None);
		buttons.push(Some(action_button(
			"Cancel",
			"cancel",
			ActionButtonOptions {
				keys: Some(cancel_key.to_string()),
				title: Some("Cancel  esc".to_string()),
				..ActionButtonOptions::default()
			},
		)));
		buttons.push(Some(action_button(
			"Submit",
			"submit",
			ActionButtonOptions {
				keys: Some(submit_key.clone()),
				tone: Some(ActionButtonTone::Accent),
				title: Some(format!("Submit  {submit_key} · New line  {newline_key}")),
				..ActionButtonOptions::default()
			},
		)));

		let mut root_props = Props::new();
		root_props.set_role(ROLE);
		col(
			vec![head, field, action_bar(&buttons, "actions")],
			root_props,
		)
	}

	/// Head text used when the host draws a title bar.
	#[must_use]
	pub fn native_head(&self, columns: usize) -> NodeText {
		match &self.question {
			Some(question) => NodeText::Plain(bound_prompt_title(
				&format!("{}: ", self.title),
				question,
				columns,
			)),
			None => NodeText::Plain(self.title.clone()),
		}
	}

	/// Mark the dialog disposed; further input is ignored.
	pub fn dispose(&mut self) {
		if self.disposed {
			return;
		}
		self.disposed = true;
		self.submit_queued = false;
		self.pending_pastes.clear();
	}
}

/// What the host must do after a key or native action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEditorAction {
	None,
	/// Open the host's external editor with [`HookEditorComponent::text`] and
	/// feed the result back with `apply_external_result`.
	ExternalEditor,
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	fn editor(
		on_submit: impl FnMut(&str, &[ImageContent]) + 'static,
		on_cancel: impl FnMut(&str, &[ImageContent]) + 'static,
		options: HookEditorOptions,
	) -> HookEditorComponent {
		HookEditorComponent::new("Answer", Some("draft"), on_submit, on_cancel, options)
	}

	#[test]
	fn bound_prompt_title_wraps_and_cuts_to_three_rows() {
		let short = bound_prompt_title("Q: ", "why", 80);
		assert_eq!(short, "Q: why");

		let long = bound_prompt_title("Q: ", &"word ".repeat(200), 30);
		let rows: Vec<&str> = long.split('\n').collect();
		assert_eq!(rows.len(), MAX_PROMPT_TITLE_ROWS);
		assert!(rows[2].ends_with('…'));
		for row in rows {
			assert!(crate::text_shape::visible_width(row) <= 30 - PROMPT_TITLE_CHROME_COLUMNS);
		}
	}

	#[test]
	fn hook_style_enter_inserts_a_newline_and_follow_up_submits() {
		let submitted: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = submitted.clone();
		let mut dialog = editor(
			move |text, _| sink.borrow_mut().push(text.to_string()),
			|_, _| {},
			HookEditorOptions::default(),
		);
		dialog.handle_key(&Key::Enter);
		assert_eq!(dialog.text(), "draft\n");
		assert!(submitted.borrow().is_empty());
		assert_eq!(dialog.handle_key(&Key::Ctrl('q')), HookEditorAction::None);
		assert_eq!(&*submitted.borrow(), &["draft\n".to_string()]);
		assert!(dialog.disposed());
	}

	#[test]
	fn prompt_style_enter_submits_and_shift_enter_keeps_editing() {
		let submitted: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = submitted.clone();
		let mut dialog = editor(
			move |text, _| sink.borrow_mut().push(text.to_string()),
			|_, _| {},
			HookEditorOptions {
				prompt_style: true,
				..HookEditorOptions::default()
			},
		);
		dialog.handle_key(&Key::Enter);
		assert_eq!(&*submitted.borrow(), &["draft".to_string()]);
	}

	#[test]
	fn cancel_reports_the_draft() {
		let cancelled: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = cancelled.clone();
		let mut dialog = editor(|_, _| {}, move |draft, _| sink.borrow_mut().push(draft.to_string()));
		dialog.cancel();
		assert_eq!(&*cancelled.borrow(), &["draft".to_string()]);
		dialog.cancel();
		assert_eq!(cancelled.borrow().len(), 1);
	}

	#[test]
	fn external_editor_action_is_reported() {
		let mut dialog = editor(|_, _| {}, |_, _| {}, HookEditorOptions::default());
		assert_eq!(
			dialog.handle_key(&Key::Ctrl('g')),
			HookEditorAction::ExternalEditor
		);
		dialog.apply_external_result("from editor");
		assert_eq!(dialog.text(), "from editor");
		assert_eq!(
			dialog.handle_native_action("external"),
			HookEditorAction::ExternalEditor
		);
	}

	#[test]
	fn attach_image_only_when_accepted_and_marks_numbering() {
		let mut dialog = editor(
			|_, _| {},
			|_, _| {},
			HookEditorOptions {
				accept_images: true,
				..HookEditorOptions::default()
			},
		);
		assert_eq!(
			dialog.attach_image(serde_json::json!({"data": "a"}), Some((4, 2))),
			Some("[Image #1, 4x2]".to_string())
		);
		assert_eq!(
			dialog.attach_image(serde_json::json!({"data": "b"}), None),
			Some("[Image #2]".to_string())
		);

		let mut plain = editor(|_, _| {}, |_, _| {}, HookEditorOptions::default());
		assert_eq!(plain.attach_image(serde_json::json!({}), None), None);
	}

	#[test]
	fn current_value_compacts_images_in_text_order() {
		let mut dialog = editor(
			|_, _| {},
			|_, _| {},
			HookEditorOptions {
				accept_images: true,
				images: vec![
					serde_json::json!({"data": "first"}),
					serde_json::json!({"data": "second"}),
				],
				..HookEditorOptions::default()
			},
		);
		dialog.set_text("see [Image #2] then [Image #1]");
		let (text, images) = dialog.current_value();
		assert_eq!(text, "see [Image #1] then [Image #2]");
		let images = images.expect("images");
		assert_eq!(images[0]["data"], "second");
		assert_eq!(images[1]["data"], "first");
	}

	#[test]
	fn ordered_paste_slots_deliver_in_order_and_submit_once_complete() {
		let submitted: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = submitted.clone();
		let mut dialog = editor(
			move |text, _| sink.borrow_mut().push(text.to_string()),
			|_, _| {},
			HookEditorOptions::default(),
		);
		dialog.set_text("");
		let first = dialog.begin_paste();
		let second = dialog.begin_paste();
		dialog.submit(true);
		assert!(dialog.pending_pastes.len() == 2);
		assert!(dialog.submit_queued);
		assert!(dialog.complete_paste(second, Some("B")));
		assert_eq!(dialog.text(), "");
		assert!(submitted.borrow().is_empty());
		assert!(dialog.complete_paste(first, Some("A")));
		assert_eq!(dialog.text(), "AB");
		assert_eq!(&*submitted.borrow(), &["AB".to_string()]);
		assert!(dialog.submit_queued == false);
	}

	#[test]
	fn failed_paste_releases_the_wait_without_submitting() {
		let submitted = Rc::new(RefCell::new(0));
		let sink = submitted.clone();
		let mut dialog = editor(
			move |_, _| *sink.borrow_mut() += 1,
			|_, _| {},
			HookEditorOptions::default(),
		);
		dialog.set_text("words");
		let token = dialog.begin_paste();
		dialog.submit(false);
		assert!(!dialog.complete_paste(token, None));
		assert_eq!(*submitted.borrow(), 0);
		assert!(!dialog.disposed());
	}

	#[test]
	fn image_path_paste_routes_to_the_callback() {
		let paths: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = paths.clone();
		let mut dialog = editor(
			|_, _| {},
			|_, _| {},
			HookEditorOptions {
				accept_images: true,
				..HookEditorOptions::default()
			},
		);
		dialog.set_on_paste_image_path(move |path| sink.borrow_mut().push(path.to_string()));
		dialog.set_text("");
		let outcome = dialog.paste_text("/tmp/shot.png");
		match outcome {
			PasteOutcome::ImagePaths(paths) => assert_eq!(paths.len(), 1),
			other => panic!("expected image paths, got {other:?}"),
		}
		assert_eq!(paths.borrow().len(), 1);
		assert_eq!(dialog.text(), "");
	}

	#[test]
	fn empty_paste_with_images_requests_a_clipboard_read() {
		let mut dialog = editor(
			|_, _| {},
			|_, _| {},
			HookEditorOptions {
				accept_images: true,
				..HookEditorOptions::default()
			},
		);
		dialog.set_text("");
		assert_eq!(dialog.paste_text(""), PasteOutcome::RequestImage);
	}

	#[test]
	fn describe_uses_question_as_heading_when_present() {
		let dialog = editor(
			|_, _| {},
			|_, _| {},
			HookEditorOptions {
				question: Some("Which flavour?".to_string()),
				..HookEditorOptions::default()
			},
		);
		let described = dialog.describe();
		assert_eq!(described.props.get_str("role"), Some(ROLE));
		let head = &described.children[0];
		assert_eq!(head.children[0].props.get_str("role"), Some("omp.hook-editor.eyebrow"));
		assert_eq!(
			head.children[1].props.get_str("role"),
			Some("omp.ask.question")
		);
		assert_eq!(
			dialog.native_head(80),
			NodeText::Plain(bound_prompt_title("Answer: ", "Which flavour?", 80))
		);
	}
}
