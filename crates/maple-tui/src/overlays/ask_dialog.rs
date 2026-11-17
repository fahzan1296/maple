//! Ask-dialog data model and pure helpers.
//!
//! Reference: `l/src/overlays/ask-dialog.ts`.
//!
//! Deviations:
//! - Interactive `AskDialogComponent` (rendering, tabs/scroll/input routing,
//!   markdown preview, native picker/description, image attachments, prompt
//!   editor integration, key/mouse handling) is not ported. This module ports
//!   exported types/interfaces/constants and pure normalization/formatting/
//!   state helpers.
//! - `ImageContent` from `@oh-my-pi/pi-ai` and markdown/theme/render primitives
//!   are abstracted as opaque/string data.
//! - Rendering helpers return plain strings/spans-like data where possible;
//!   ANSI/markdown layout parity requires host renderers.

use std::collections::{HashMap, HashSet};

/// One selectable answer in an ask dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionAskDialogOption {
	pub label: String,
	pub description: Option<String>,
	pub recommended: Option<bool>,
}

/// Question and answer choices displayed by the ask dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionAskDialogQuestion {
	pub id: String,
	pub header: Option<String>,
	pub question: String,
	pub options: Vec<ExtensionAskDialogOption>,
	pub multi: Option<bool>,
	/// Optional recommended answer index/label hint in TS; kept flexible.
	pub recommended: Option<String>,
}

/// Prompt text and attached images (opaque bytes/base64 refs in host).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskDialogPromptValue {
	pub text: String,
	pub images: Vec<String>,
}

/// Draft left without submitting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskDialogPromptDraft {
	pub draft: AskDialogPromptValue,
}

/// Submitted answer to one dialog question.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionAskDialogResultItem {
	pub id: String,
	pub selected: Vec<String>,
	pub custom: Option<String>,
	pub note: Option<String>,
	pub note_option: Option<String>,
}

/// Answers submitted from the dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionAskDialogSubmitResult {
	pub kind: String, // "submit"
	pub answers: Vec<ExtensionAskDialogResultItem>,
}

/// Request to discuss instead of answering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionAskDialogChatResult {
	pub kind: String, // "chat"
}

/// Cancellation result: why the dialog closed and the partial answers gathered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionAskDialogCancelResult {
	pub kind: String, // "cancel"
	pub reason: String,
	pub partial: ExtensionAskDialogSubmitResult,
}

/// Completed dialog action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionAskDialogResult {
	Submit(ExtensionAskDialogSubmitResult),
	Cancel(ExtensionAskDialogCancelResult),
	Chat(ExtensionAskDialogChatResult),
}

/// What a custom-answer/note prompt asks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskDialogPrompt {
	pub title: String,
	pub question: String,
}


/// Input guard routing typing to composer draft while dialog active.
pub trait AskDialogInputGuard {
	fn has_draft(&self) -> bool;
	fn handle_input(&self, key_data: &str);
}

pub const OTHER_OPTION: &str = "Other (type your own)";
pub const SUBMIT_OPTION: &str = "Submit";
pub const RECOMMENDED_SUFFIX: &str = " (Recommended)";
pub const OTHER_LABEL: &str = "Other…";
pub const REVIEW_TAB: &str = "Review";
pub const GUEST_ACTION_LABELS: &[&str] = &["Chat about this", "Next →"];

pub const DIALOG_HEIGHT_RATIO: f32 = 0.7;
pub const MIN_DIALOG_ROWS: usize = 12;
pub const MIN_BODY_ROWS: usize = 5;
pub const MAX_HEADER_CHIP_WIDTH: usize = 16;
pub const MAX_HEADER_ROWS: usize = 4;
pub const MAX_DESC_ROWS: usize = 2;

/// Per-question UI state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionState {
	pub selected_options: HashSet<usize>,
	pub custom_input: Option<String>,
	pub note_text: Option<String>,
	pub note_row_key: Option<String>,
	pub cursor: usize,
}

/// Question row kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestionRowKind {
	Option,
	Other,
}

/// A rendered/selectable question row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionRow {
	pub kind: QuestionRowKind,
	pub option_index: Option<usize>,
	pub key: String,
}

#[must_use]
pub fn clamp_usize(value: usize, min: usize, max: usize) -> usize {
	value.max(min).min(max)
}

/// Sanitize carriage returns (host may have richer behavior).
#[must_use]
pub fn sanitize_carriage_returns(text: &str) -> String {
	text.replace('\r', "")
}

/// Replace tabs with spaces (simple).
#[must_use]
pub fn replace_tabs(text: &str) -> String {
	text.replace('\t', "    ")
}

/// Full tab label for a question.
#[must_use]
pub fn question_tab_name(question: &ExtensionAskDialogQuestion, index: usize) -> String {
	let header = question.header.as_deref().map(str::trim).filter(|s| !s.is_empty());
	let fallback = if !question.id.trim().is_empty() {
		sanitize_carriage_returns(&question.id)
	} else {
		format!("Q{}", index + 1)
	};
	replace_tabs(header.unwrap_or(&fallback))
}

/// Clipped tab label.
#[must_use]
pub fn question_tab_label(question: &ExtensionAskDialogQuestion, index: usize) -> String {
	truncate_chars(&question_tab_name(question, index), MAX_HEADER_CHIP_WIDTH)
}

fn truncate_chars(s: &str, max: usize) -> String {
	let chars: Vec<char> = s.chars().collect();
	if chars.len() <= max {
		s.to_string()
	} else if max == 0 {
		String::new()
	} else {
		let take = max.saturating_sub(1);
		let mut out: String = chars.into_iter().take(take).collect();
		out.push('…');
		out
	}
}

/// Wrap question title to width by simple char wrapping, capped by max rows.
#[must_use]
pub fn render_question_title(question: &ExtensionAskDialogQuestion, width: usize, max_rows: usize) -> Vec<String> {
	let text = sanitize_carriage_returns(&question.question);
	simple_wrap(&text, width.max(1), max_rows.max(1))
}

fn simple_wrap(text: &str, width: usize, max_rows: usize) -> Vec<String> {
	let mut lines = Vec::new();
	for para in text.split('\n') {
		if lines.len() >= max_rows {
			break;
		}
		if para.is_empty() {
			lines.push(String::new());
			continue;
		}
		let chars: Vec<char> = para.chars().collect();
		let mut i = 0;
		while i < chars.len() && lines.len() < max_rows {
			let end = (i + width).min(chars.len());
			lines.push(chars[i..end].iter().collect());
			i = end;
		}
	}
	if lines.is_empty() {
		lines.push(String::new());
	}
	if lines.len() > max_rows {
		lines.truncate(max_rows);
	}
	lines
}

/// Normalize inline input for custom answers.
#[must_use]
pub fn normalized_inline_input(input: &str) -> String {
	replace_tabs(input)
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.trim()
		.to_string()
}

/// Display option labels: sanitized and suffixed for recommended (uniqueness handled by host in TS;
/// here we preserve base transform and recommendation badge text).
#[must_use]
pub fn display_option_labels(question: &ExtensionAskDialogQuestion) -> Vec<String> {
	question
		.options
		.iter()
		.map(|o| {
			let mut label = sanitize_carriage_returns(&replace_tabs(&o.label));
			if o.recommended.unwrap_or(false) {
				label.push_str(RECOMMENDED_SUFFIX);
			}
			label
		})
		.collect()
}

/// Whether a question state has an answer.
#[must_use]
pub fn is_answered(state: &QuestionState) -> bool {
	!state.selected_options.is_empty() || state.custom_input.as_ref().is_some_and(|s| !s.is_empty())
}

pub fn clear_note(state: &mut QuestionState) {
	state.note_text = None;
	state.note_row_key = None;
}

pub fn clear_note_if_row(state: &mut QuestionState, row_key: &str) {
	if state.note_row_key.as_deref() == Some(row_key) {
		clear_note(state);
	}
}

pub fn clear_note_unless_row(state: &mut QuestionState, row_key: &str) {
	if state.note_row_key.as_deref().is_some_and(|k| k != row_key) {
		clear_note(state);
	}
}

/// Normalize untrusted dialog questions into a render-safe shape.
#[must_use]
pub fn normalize_dialog_questions(questions: &[ExtensionAskDialogQuestion]) -> Vec<ExtensionAskDialogQuestion> {
	questions
		.iter()
		.enumerate()
		.map(|(i, q)| {
			let id = if q.id.trim().is_empty() { format!("q{}", i + 1) } else { sanitize_carriage_returns(&q.id) };
			let options = q
				.options
				.iter()
				.map(|o| ExtensionAskDialogOption {
					label: replace_tabs(&sanitize_carriage_returns(&o.label)),
					description: o.description.as_ref().map(|d| sanitize_carriage_returns(d)),
					recommended: o.recommended,
				})
				.collect();
			ExtensionAskDialogQuestion {
				id,
				header: q.header.as_ref().map(|h| sanitize_carriage_returns(h)),
				question: sanitize_carriage_returns(&q.question),
				options,
				multi: q.multi,
				recommended: q.recommended.clone(),
			}
		})
		.collect()
}

/// Build submit result items from question states.
#[must_use]
pub fn build_submit_result(
	questions: &[ExtensionAskDialogQuestion],
	states: &[QuestionState],
) -> ExtensionAskDialogSubmitResult {
	let answers = questions
		.iter()
		.zip(states.iter())
		.map(|(q, st)| {
			let labels = display_option_labels(q);
			let mut selected = Vec::new();
			let mut idxs: Vec<_> = st.selected_options.iter().copied().collect();
			idxs.sort_unstable();
			for i in idxs {
				if let Some(l) = labels.get(i) {
					selected.push(l.clone());
				}
			}
			let note_row_matches = st
				.note_row_key
				.as_deref()
				.and_then(|k| k.strip_prefix("opt:"))
				.and_then(|n| n.parse::<usize>().ok())
				.map(|i| st.selected_options.contains(&i))
				.unwrap_or(st.note_row_key.as_deref() == Some("other") && st.custom_input.is_some());
			let note = if note_row_matches { st.note_text.clone().filter(|s| !s.trim().is_empty()) } else { None };
			ExtensionAskDialogResultItem {
				id: q.id.clone(),
				selected,
				custom: st.custom_input.clone().filter(|s| !s.is_empty()),
				note: note.clone(),
				note_option: st.note_row_key.as_deref().and_then(|k| k.strip_prefix("opt:")).and_then(|n| n.parse::<usize>().ok()).and_then(|i| labels.get(i).cloned()),
			}
		})
		.collect();
	ExtensionAskDialogSubmitResult { kind: "submit".to_string(), answers }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn question_labels_and_normalization() {
		let q = ExtensionAskDialogQuestion {
			id: "".into(),
			header: Some("  Head\t".into()),
			question: "Hi".into(),
			options: vec![ExtensionAskDialogOption { label: "A\r".into(), recommended: Some(true), ..Default::default() }],
			..Default::default()
		};
		assert_eq!(question_tab_name(&q, 0), "Head    ");
		let nq = normalize_dialog_questions(&[q])[0].clone();
		assert_eq!(nq.id, "q1");
		assert_eq!(display_option_labels(&nq)[0], format!("A{RECOMMENDED_SUFFIX}"));
	}

	#[test]
	fn inline_input_and_answered_state() {
		assert_eq!(normalized_inline_input(" a \t b\n"), "a b");
		let mut st = QuestionState::default();
		assert!(!is_answered(&st));
		st.custom_input = Some("x".into());
		assert!(is_answered(&st));
	}

	#[test]
	fn wrap_and_note_helpers() {
		let q = ExtensionAskDialogQuestion { question: "abcd".into(), ..Default::default() };
		let lines = render_question_title(&q, 2, 4);
		assert_eq!(lines, vec!["ab", "cd"]);
		let mut st = QuestionState { note_text: Some("n".into()), note_row_key: Some("opt:1".into()), ..Default::default() };
		clear_note_if_row(&mut st, "opt:1");
		assert!(st.note_text.is_none());
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Callbacks for ask-dialog result/cancel.
pub trait AskDialogCallbacks {
	fn on_result(&self, result: ExtensionAskDialogResult);
	fn request_render(&self) {}
}

/// Tabbed extension ask dialog component.
///
/// Reference: `AskDialogComponent` in `l/src/overlays/ask-dialog.ts`.
///
/// Deviations: rich option pills/editor widgets, mouse routing, timers, and
/// full keyboard choreography are simplified to state-driven tab/question/option
/// navigation with submit/cancel semantics.
pub struct AskDialogComponent<CB: AskDialogCallbacks> {
	pub questions: Vec<ExtensionAskDialogQuestion>,
	pub states: Vec<QuestionState>,
	pub tab_index: usize,
	pub option_index: usize,
	pub cancelled: bool,
	pub submitted: bool,
	callbacks: CB,
	theme: OverlayTheme,
}

impl<CB: AskDialogCallbacks> std::fmt::Debug for AskDialogComponent<CB> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AskDialogComponent")
			.field("tab_index", &self.tab_index)
			.field("option_index", &self.option_index)
			.field("submitted", &self.submitted)
			.field("cancelled", &self.cancelled)
			.finish_non_exhaustive()
	}
}

impl<CB: AskDialogCallbacks> AskDialogComponent<CB> {
	#[must_use]
	pub fn new(questions: Vec<ExtensionAskDialogQuestion>, callbacks: CB) -> Self {
		let questions = normalize_dialog_questions(&questions);
		let states = questions.iter().map(|_| QuestionState::default()).collect();
		Self {
			questions,
			states,
			tab_index: 0,
			option_index: 0,
			cancelled: false,
			submitted: false,
			callbacks,
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	fn current_question(&self) -> Option<&ExtensionAskDialogQuestion> {
		self.questions.get(self.tab_index)
	}

	fn option_count(&self) -> usize {
		self.current_question().map(|q| q.options.len() + 1).unwrap_or(0) // + custom/submit row
	}

	fn toggle_or_select(&mut self) {
		let Some(q) = self.current_question().cloned() else { return };
		let Some(st) = self.states.get_mut(self.tab_index) else { return };
		let opt_count = q.options.len();
		if self.option_index < opt_count {
			if q.multi == Some(true) {
				if !st.selected_options.remove(&self.option_index) {
					st.selected_options.insert(self.option_index);
				}
			} else {
				st.selected_options.clear();
				st.selected_options.insert(self.option_index);
				st.custom_input = None;
			}
		}
	}

	pub fn submit(&mut self) {
		if self.submitted || self.cancelled {
			return;
		}
		self.submitted = true;
		let result = build_submit_result(&self.questions, &self.states);
		self.callbacks.on_result(ExtensionAskDialogResult::Submit(result));
	}

	pub fn cancel(&mut self) {
		if self.submitted || self.cancelled {
			return;
		}
		self.cancelled = true;
		self.callbacks.on_result(ExtensionAskDialogResult::Cancel(ExtensionAskDialogCancelResult {
			kind: "cancel".to_string(),
			reason: "user".to_string(),
			partial: ExtensionAskDialogSubmitResult { kind: "submit".to_string(), answers: vec![] },
		}));
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let title = self
			.current_question()
			.map(|q| question_tab_name(q, self.tab_index))
			.unwrap_or_else(|| "Ask".to_string());
		let mut out = vec![top_border(width, &title, &self.theme)];
		if let Some(q) = self.current_question() {
			out.push(row(&q.question, width, &self.theme));
			out.push(divider(width, &self.theme));
			let labels = display_option_labels(q);
			for (i, label) in labels.iter().enumerate() {
				let cursor = if i == self.option_index { "›" } else { " " };
				let checked = self.states.get(self.tab_index).map(|s| s.selected_options.contains(&i)).unwrap_or(false);
				let mark = if q.multi == Some(true) { if checked { "[x]" } else { "[ ]" } } else if checked { "(*)" } else { "( )" };
				out.push(row(&format!("{cursor} {mark} {label}"), width, &self.theme));
			}
			let custom_idx = labels.len();
			let cursor = if self.option_index == custom_idx { "›" } else { " " };
			let custom = self.states.get(self.tab_index).and_then(|s| s.custom_input.as_deref()).unwrap_or("");
			out.push(row(&format!("{cursor} Other: {custom}"), width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		out.push(row(&format!("Tab {}/{} · enter select/submit · esc cancel", self.tab_index + 1, self.questions.len().max(1)), width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<CB: AskDialogCallbacks> Component for AskDialogComponent<CB> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.submitted || self.cancelled {
			return;
		}
		match key {
			Key::Esc => self.cancel(),
			Key::Left => {
				if self.tab_index > 0 {
					self.tab_index -= 1;
					self.option_index = 0;
				}
			}
			Key::Right => {
				if self.tab_index + 1 < self.questions.len() {
					self.tab_index += 1;
					self.option_index = 0;
				}
			}
			Key::Tab => {
				if self.tab_index + 1 < self.questions.len() {
					self.tab_index += 1;
					self.option_index = 0;
				}
			}
			Key::BackTab => {
				if self.tab_index > 0 {
					self.tab_index -= 1;
					self.option_index = 0;
				}
			}
			Key::Up => self.option_index = self.option_index.saturating_sub(1),
			Key::Down => {
				let max = self.option_count().saturating_sub(1);
				self.option_index = (self.option_index + 1).min(max);
			}
			Key::Char(' ') => self.toggle_or_select(),
			Key::Enter => {
				// last tab/row submit convenience
				self.toggle_or_select();
				if self.tab_index + 1 >= self.questions.len() {
					self.submit();
				} else {
					self.tab_index += 1;
					self.option_index = 0;
				}
			}
			Key::Char('s') if key.is_ctrl() => self.submit(),
			_ => {}
		}
		self.callbacks.request_render();
	}
}

impl<CB: AskDialogCallbacks> NativeDescribe for AskDialogComponent<CB> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("tabIndex", serde_json::Value::from(self.tab_index as i64));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;
	use std::cell::RefCell;

	struct Cb { got: RefCell<bool> }
	impl AskDialogCallbacks for Cb {
		fn on_result(&self, _result: ExtensionAskDialogResult) { *self.got.borrow_mut() = true; }
		fn request_render(&self) {}
	}

	#[test]
	fn ask_dialog_component_renders_navigates_and_submits() {
		let qs = vec![ExtensionAskDialogQuestion {
			id: "q".into(),
			question: "Pick".into(),
			options: vec![ExtensionAskDialogOption { label: "A".into(), ..Default::default() }],
			..Default::default()
		}];
		let mut d = AskDialogComponent::new(qs, Cb { got: RefCell::new(false) });
		let lines = d.render(60);
		assert!(lines.iter().any(|l| l.contains("Pick")));
		d.handle_input(&Key::Down); // option 0 already, move down
		d.handle_input(&Key::Enter); // select+advance/submit
		assert!(*d.callbacks.got.borrow() || d.submitted);
		let node = d.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
