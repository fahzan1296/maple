//! Reference: `l/src/overlays/hook-input.ts`.
//!
//! One-line text prompt an extension (hook) asks the user: a title (with the
//! countdown while a timeout runs), a text field, and Cancel/Submit actions.
//!
//! Deviations:
//! - `OverlayPanel` and the `Text`/`Spacer`/`KeyLegend` composition are
//!   replaced by [`crate::components::Form`] plus a native description; the
//!   host composes and renders it.
//! - The countdown is [`crate::chrome::countdown_timer::CountdownTimer`], which
//!   the reference also uses; the first clock tick comes from the host.
//! - Key bindings for the legend/action keycaps are injected
//!   ([`HookInputKeys`]) rather than read from the keybinding manager.

use crate::chrome::countdown_timer::CountdownTimer;
use crate::component::Component;
use crate::components::form::{EmptyInput, Form, FormField, FormFieldKind, FormTheme};
use crate::keys::Key;
use crate::native::describe::{col, node, span};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{action_bar, action_button, ActionButtonOptions, ActionButtonTone};

/// Role prefix of the sheet's parts (`omp.hook-input.head`, `.title`, `.timer`,
/// `.detail`, `.field`).
pub const PART: &str = "omp.hook-input";

/// Role of the overlay root.
pub const ROLE: &str = "omp.overlay.hook-input";

/// Options for a hook prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookInputOptions {
	/// Milliseconds until the prompt cancels itself; `None`/`0` disables.
	pub timeout_ms: Option<u64>,
	/// Mask the typed value.
	pub secret: bool,
}

/// Keycap hints for the legend/actions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookInputKeys {
	/// Key id that submits (`tui.input.submit`).
	pub submit: Option<String>,
	/// Key id that cancels (`app.interrupt`, usually `escape`).
	pub cancel: Option<String>,
}

/// The one-line prompt.
pub struct HookInputComponent {
	form: Form,
	base_title: String,
	placeholder: Option<String>,
	on_submit: Box<dyn FnMut(&str)>,
	on_cancel: Box<dyn FnMut()>,
	on_timeout: Option<Box<dyn FnMut()>>,
	countdown: Option<CountdownTimer>,
	timeout_ms: Option<u64>,
	/// Last clock reading the host supplied; used by the [`Component`] path.
	now_ms: u64,
}

impl HookInputComponent {
	pub fn new(
		title: impl Into<String>,
		placeholder: Option<&str>,
		on_submit: impl FnMut(&str) + 'static,
		on_cancel: impl FnMut() + 'static,
		options: HookInputOptions,
	) -> Self {
		let base_title = title.into();
		let placeholder = placeholder
			.map(str::trim)
			.filter(|text| !text.is_empty())
			.map(str::to_string);

		let mut field = FormField::new(
			"value",
			"",
			if options.secret {
				FormFieldKind::secret("")
			} else {
				FormFieldKind::text("")
			},
		);
		field.empty = EmptyInput::Submit;

		HookInputComponent {
			form: Form::new(vec![field], FormTheme::default()),
			base_title,
			placeholder,
			on_submit: Box::new(on_submit),
			on_cancel: Box::new(on_cancel),
			on_timeout: None,
			countdown: None,
			timeout_ms: options.timeout_ms,
			now_ms: 0,
		}
	}

	/// Start the countdown. The reference builds it in the constructor from the
	/// injected `TUI` clock; here the host supplies the first tick.
	pub fn start_countdown(&mut self, now_ms: u64) {
		self.now_ms = now_ms;
		if let Some(timeout) = self.timeout_ms.filter(|timeout| *timeout > 0) {
			self.countdown = Some(CountdownTimer::new(timeout, now_ms));
		}
	}

	/// Advance the host clock without touching the countdown.
	pub fn set_now_ms(&mut self, now_ms: u64) {
		self.now_ms = now_ms;
	}


	pub fn set_on_timeout(&mut self, callback: impl FnMut() + 'static) {
		self.on_timeout = Some(Box::new(callback));
	}

	#[must_use]
	pub fn base_title(&self) -> &str {
		&self.base_title
	}

	#[must_use]
	pub fn placeholder(&self) -> Option<&str> {
		self.placeholder.as_deref()
	}

	#[must_use]
	pub fn value(&self) -> String {
		self.form
			.field("value")
			.map(FormField::value)
			.unwrap_or_default()
	}

	#[must_use]
	pub fn has_error(&self) -> bool {
		self.form
			.field("value")
			.and_then(|field| field.error.as_ref())
			.is_some()
	}

	/// Title including the countdown suffix while a timeout runs.
	#[must_use]
	pub fn title(&self, _now_ms: u64) -> String {
		match &self.countdown {
			Some(countdown) => format!("{} ({}s)", self.base_title, countdown.remaining_seconds()),
			None => self.base_title.clone(),
		}
	}

	/// Head text used when the host draws a title bar.
	#[must_use]
	pub fn native_head(&self, now_ms: u64) -> NodeText {
		NodeText::Plain(self.title(now_ms))
	}

	/// Whether a timeout is running.
	#[must_use]
	pub fn has_countdown(&self) -> bool {
		self.countdown.is_some()
	}

	/// Submit the current value (same as pressing Enter).
	pub fn submit(&mut self) {
		let value = self.value();
		(self.on_submit)(&value);
	}

	pub fn cancel(&mut self) {
		(self.on_cancel)();
	}

	/// Handle one key: any interaction restarts the countdown, then the form
	/// edits; Enter submits and Escape cancels.
	pub fn handle_key(&mut self, key: &Key, now_ms: u64) -> bool {
		if let Some(countdown) = self.countdown.as_mut() {
			countdown.reset(now_ms);
		}
		if crate::keys::is_enter(key) {
			self.submit();
			return true;
		}
		if crate::keys::is_escape(key) {
			self.cancel();
			return true;
		}
		self.form.handle_input(key);
		true
	}

	/// Route non-bracketed paste transports into the field; pasting counts as
	/// interaction, so the countdown resets like any keystroke.
	pub fn paste_text(&mut self, text: &str, now_ms: u64) {
		if let Some(countdown) = self.countdown.as_mut() {
			countdown.reset(now_ms);
		}
		self.form.handle_paste(text);
	}

	/// Advance the countdown; on expiry run the timeout callback then cancel.
	pub fn poll_countdown(&mut self, now_ms: u64) -> bool {
		let expired = self
			.countdown
			.as_mut()
			.is_some_and(|countdown| countdown.expire_if_due(now_ms));
		if expired {
			if let Some(callback) = self.on_timeout.as_mut() {
				callback();
			}
			self.cancel();
		}
		expired
	}

	/// Native action (`submit` / `cancel`) from the action bar.
	pub fn handle_native_action(&mut self, act: &str, now_ms: u64) -> bool {
		match act {
			"submit" => {
				if let Some(countdown) = self.countdown.as_mut() {
					countdown.reset(now_ms);
				}
				self.submit();
				true
			}
			"cancel" => {
				self.cancel();
				true
			}
			_ => false,
		}
	}

	/// Terminal-only key legend under the field.
	#[must_use]
	pub fn key_legend(keys: &HookInputKeys) -> String {
		let submit = keys.submit.as_deref().unwrap_or("enter");
		let cancel = keys.cancel.as_deref().unwrap_or("escape");
		format!("{submit} submit  {cancel} cancel")
	}

	/// Native sheet: title (countdown at its end), detail lines, the field, and
	/// Cancel · Submit.
	#[must_use]
	pub fn describe(&self, now_ms: u64, keys: &HookInputKeys) -> NativeNode {
		let raw_title = self.base_title.trim();
		let mut lines = raw_title.split('\n');
		let title_line = lines.next().unwrap_or("").trim().to_string();
		let detail = lines.collect::<Vec<_>>().join("\n").trim().to_string();

		let mut title_row = vec![node(
			NodeKind::Text,
			{
				let mut props = Props::new();
				props.set_spans(vec![span(title_line, None)]);
				props.set_str("role", format!("{PART}.title"));
				props.set_str("wrap", "word");
				props.set_i64("grow", 1);
				props
			},
			Vec::new(),
			Some("title"),
		)];
		if let Some(countdown) = &self.countdown {
			let mut props = Props::new();
			props.set_str("role", format!("{PART}.timer"));
			props.set_str("title", "Cancels when the time runs out; typing restarts it");
			title_row.push(node(
				NodeKind::Row,
				props,
				vec![countdown.describe(now_ms)],
				Some("timer"),
			));
		}
		let mut head = vec![node(
			NodeKind::Row,
			{
				let mut props = Props::new();
				props.set_str("gap", "md");
				props.set_str("align", "baseline");
				props
			},
			title_row,
			Some("line"),
		)];
		if !detail.is_empty() {
			let mut props = Props::new();
			props.set_spans(vec![span(detail, None)]);
			props.set_str("role", format!("{PART}.detail"));
			props.set_str("wrap", "word");
			head.push(node(NodeKind::Text, props, Vec::new(), Some("detail")));
		}

		let mut head_props = Props::new();
		head_props.set_str("role", format!("{PART}.head"));
		head_props.set_str("gap", "xs");
		let head_col = col(head, head_props);

		let mut field_props = Props::new();
		field_props.set_str("role", format!("{PART}.field"));
		if self.has_error() {
			field_props.set_str("tone", "error");
		}
		let field_row = node(
			NodeKind::Col,
			field_props,
			vec![self.form_native()],
			Some("field"),
		);

		let actions = action_bar(
			&[
				None,
				Some(action_button(
					"Cancel",
					"cancel",
					ActionButtonOptions {
						keys: keys.cancel.clone(),
						..ActionButtonOptions::default()
					},
				)),
				Some(action_button(
					"Submit",
					"submit",
					ActionButtonOptions {
						keys: keys.submit.clone(),
						tone: Some(ActionButtonTone::Accent),
						..ActionButtonOptions::default()
					},
				)),
			],
			"actions",
		);

		let mut root_props = Props::new();
		root_props.set_role(ROLE);
		col(vec![head_col, field_row, actions], root_props)
	}

	/// The field as a native node: its value and placeholder.
	fn form_native(&self) -> NativeNode {
		let mut props = Props::new();
		props.set_str("value", self.value());
		if let Some(placeholder) = &self.placeholder {
			props.set_str("placeholder", placeholder);
		}
		if self.has_error() {
			props.set_str("tone", "error");
		}
		node(NodeKind::Col, props, Vec::new(), Some("form"))
	}
}

impl Component for HookInputComponent {
	/// Terminal sheet: title (countdown at its end), detail lines, the field,
	/// a hairline, and the key legend.
	fn render(&mut self, width: u16) -> Vec<String> {
		let mut lines = vec![self.title(self.now_ms)];
		let mut title_lines = self.base_title.split('\n');
		title_lines.next();
		for detail in title_lines {
			if !detail.trim().is_empty() {
				lines.push(detail.trim().to_string());
			}
		}
		lines.push(String::new());
		lines.extend(self.form.render(width));
		if self.has_error() {
			if let Some(error) = self
				.form
				.field("value")
				.and_then(|field| field.error.as_deref())
			{
				lines.push(error.to_string());
			}
		}
		lines.push(String::new());
		lines.push(Self::key_legend(&HookInputKeys::default()));
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		self.handle_key(key, self.now_ms);
	}

	fn handle_paste(&mut self, text: &str) {
		self.paste_text(text, self.now_ms);
	}

	fn dispose(&mut self) {
		if let Some(countdown) = self.countdown.as_mut() {
			countdown.dispose();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	#[test]
	fn trims_the_placeholder_and_exposes_the_title() {
		let prompt = HookInputComponent::new(
			"Deploy target",
			Some("   "),
			|_| {},
			|| {},
			HookInputOptions::default(),
		);
		assert_eq!(prompt.base_title(), "Deploy target");
		assert_eq!(prompt.placeholder(), None);
		assert_eq!(prompt.title(0), "Deploy target");
		assert!(!prompt.has_countdown());
	}

	#[test]
	fn enter_submits_the_typed_value() {
		let submitted: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = submitted.clone();
		let mut prompt = HookInputComponent::new(
			"Name",
			Some("service name"),
			move |value| sink.borrow_mut().push(value.to_string()),
			|| {},
			HookInputOptions::default(),
		);
		for ch in "api".chars() {
			prompt.handle_key(&Key::Char(ch), 0);
		}
		assert_eq!(prompt.value(), "api");
		assert!(prompt.handle_key(&Key::Enter, 0));
		assert_eq!(&*submitted.borrow(), &["api".to_string()]);
	}

	#[test]
	fn escape_cancels() {
		let cancelled = Rc::new(RefCell::new(0));
		let sink = cancelled.clone();
		let mut prompt = HookInputComponent::new("Name", None, |_| {}, move || {
			*sink.borrow_mut() += 1;
		});
		prompt.handle_key(&Key::Esc, 0);
		prompt.handle_native_action("cancel", 0);
		assert_eq!(*cancelled.borrow(), 2);
	}

	#[test]
	fn countdown_title_and_timeout_cancel() {
		let cancelled = Rc::new(RefCell::new(0));
		let timed_out = Rc::new(RefCell::new(0));
		let cancel_sink = cancelled.clone();
		let timeout_sink = timed_out.clone();
		let mut prompt = HookInputComponent::new(
			"Token",
			None,
			|_| {},
			move || *cancel_sink.borrow_mut() += 1,
			HookInputOptions {
				timeout_ms: Some(5_000),
				secret: true,
			},
		);
		prompt.set_on_timeout(move || *timeout_sink.borrow_mut() += 1);
		prompt.start_countdown(1_000);
		assert!(prompt.has_countdown());
		assert_eq!(prompt.title(1_000), "Token (5s)");
		assert!(!prompt.poll_countdown(5_999));
		assert!(prompt.poll_countdown(6_000));
		assert_eq!(*timed_out.borrow(), 1);
		assert_eq!(*cancelled.borrow(), 1);
	}

	#[test]
	fn typing_restarts_the_countdown() {
		let mut prompt = HookInputComponent::new(
			"Token",
			None,
			|_| {},
			|| {},
			HookInputOptions {
				timeout_ms: Some(4_000),
				secret: false,
			},
		);
		prompt.start_countdown(0);
		prompt.handle_key(&Key::Char('x'), 3_000);
		assert_eq!(prompt.title(3_000), "Token (4s)");
		assert!(!prompt.poll_countdown(6_000));
		assert!(prompt.poll_countdown(7_000));
	}
	#[test]
	fn paste_appends_and_counts_as_interaction() {
		let mut prompt = HookInputComponent::new(
			"Token",
			None,
			|_| {},
			|| {},
			HookInputOptions {
				timeout_ms: Some(2_000),
				secret: false,
			},
		);
		prompt.start_countdown(0);
		prompt.paste_text("ab", 1_500);
		prompt.paste_text("cd", 1_600);
		assert_eq!(prompt.value(), "abcd");
		assert!(!prompt.poll_countdown(3_000));
	}

	#[test]
	fn describe_carries_parts_and_actions() {
		let prompt = HookInputComponent::new(
			"Title\nMore detail",
			Some("type here"),
			|_| {},
			|| {},
			HookInputOptions::default(),
		);
		let described = prompt.describe(
			0,
			&HookInputKeys {
				submit: Some("enter".to_string()),
				cancel: Some("escape".to_string()),
			},
		);
		assert_eq!(described.props.get_str("role"), Some(ROLE));
		assert_eq!(described.children.len(), 3);
		let head = &described.children[0];
		assert_eq!(head.props.get_str("role"), Some("omp.hook-input.head"));
		assert_eq!(head.children.len(), 2);

		let field = &described.children[1];
		assert_eq!(field.props.get_str("role"), Some("omp.hook-input.field"));
		assert_eq!(
			field.children[0].props.get_str("placeholder"),
			Some("type here")
		);

		let actions = &described.children[2];
		let acts: Vec<&str> = actions
			.children
			.iter()
			.filter_map(|child| {
				child
					.prop("actions")
					.and_then(|actions| actions.get("click"))
					.and_then(|click| click.as_str())
			})
			.collect();
		assert_eq!(acts, vec!["cancel", "submit"]);
	}

	#[test]
	fn key_legend_falls_back_to_defaults() {
		assert_eq!(
			HookInputComponent::key_legend(&HookInputKeys::default()),
			"enter submit  escape cancel"
		);
		assert_eq!(
			HookInputComponent::key_legend(&HookInputKeys {
				submit: Some("ctrl+enter".to_string()),
				cancel: Some("ctrl+c".to_string()),
			}),
			"ctrl+enter submit  ctrl+c cancel"
		);
	}

	#[test]
	fn component_render_stacks_title_field_and_legend() {
		let submitted = Rc::new(RefCell::new(None::<String>));
		let sink = submitted.clone();
		let mut prompt = HookInputComponent::new(
			"Deploy target\nextra detail",
			Some("host"),
			move |value| *sink.borrow_mut() = Some(value.to_string()),
			|| {},
			HookInputOptions::default(),
		);
		let lines = prompt.render(40);
		assert_eq!(lines[0], "Deploy target");
		assert_eq!(lines[1], "extra detail");
		assert!(lines.iter().any(|line| line.contains("host")));
		assert_eq!(
			lines.last().map(String::as_str),
			Some("enter submit  escape cancel")
		);
		prompt.handle_paste("prod");
		prompt.handle_input(&Key::Enter);
		assert_eq!(submitted.borrow().as_deref(), Some("prod"));
	}
}
