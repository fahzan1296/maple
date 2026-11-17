//! Reference: `l/src/overlays/login-dialog.ts`.
//!
//! The OAuth login sheet: the sign-in link, the provider's instructions (or
//! the device code split out of them), the questions asked along the flow,
//! its progress, and Cancel / Continue / Open sign-in page.
//!
//! Deviations:
//! - The reference returns promises for prompts and aborts them with an
//!   `AbortController`; the crate is synchronous, so each question returns a
//!   [`std::sync::mpsc::Receiver`] and cancelling drops the sender (the
//!   receiver reports `RecvError`).
//! - `TextFormField` is the crate's [`Form`] over one text field; the field's
//!   `prompt`/error chrome comes from [`FormField::kind`] and
//!   [`FormField::set_error`].
//! - The provider registry (`getOAuthProviders`) has no crate equivalent: the
//!   host passes the display name to [`LoginDialogComponent::new`].
//! - `DEVICE_CODE_INSTRUCTIONS` is matched by hand (last token, preceded by
//!   the word `code`) because the crate does not run provider regexes here;
//!   the accepted shape is the same for every device flow the app ships.
//! - The native cache is not kept: `describe_native` rebuilds the tree, which
//!   is cheap and keeps the state single-sourced.

use std::sync::mpsc::{Receiver, Sender};

use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::components::form::{EmptyInput, Form, FormField, FormFieldKind, FormTheme};
use crate::keys::{is_enter, is_escape, Key};
use crate::native::describe::{NativeDescribe, node, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span};
use crate::native::overlay::{
	ActionButtonOptions, ActionButtonTone, action_bar, action_button, overlay_card,
};
use crate::text_shape::wrap_text_with_ansi;
use crate::theme::Theme;

/// Semantic role of the login sheet.
pub const LOGIN_ROLE: &str = "omp.overlay.login";

/// Shown on the field when the flow asks for the manual code again: the last
/// paste had no usable code.
pub const RETRY_MESSAGE: &str =
	"Couldn't read a code from that. Paste the authorization code or the full redirect URL.";

/// Shown while the browser half of the flow runs and the provider reports
/// nothing.
pub const BROWSER_WAIT: &str = "Waiting for you to finish in the browser…";

/// The field id of the live answer.
const ANSWER_FIELD: &str = "answer";

/// The sign-in link from `showAuth`: the provider's instructions, or the
/// device code split out of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginAuth {
	/// Full authorization URL.
	pub url: String,
	/// Local-only shortcut (`launchUrl`), when the host offers one.
	pub launch_url: Option<String>,
	/// The provider's instructions, as plain text.
	pub instructions: Option<String>,
	/// The device code the instructions carry, when they do.
	pub device_code: Option<String>,
}

/// One prompt of the flow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginPrompt {
	/// The question.
	pub message: String,
	/// An example answer.
	pub placeholder: Option<String>,
	/// Whether the answer is masked.
	pub secret: bool,
}

/// One step after the sign-in link: an answer (live or given) or a progress
/// message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginStep {
	/// A question; `answer` is set once submitted.
	Input { message: String, answer: Option<String> },
	/// A progress message.
	Progress { message: String },
}

/// Sheet copy: plain text, a trailing ASCII ellipsis as `…`.
#[must_use]
pub fn sheet_text(message: &str) -> String {
	let trimmed = message.trim();
	trimmed.strip_suffix("...").map_or_else(
		|| trimmed.to_string(),
		|head| format!("{head}…"),
	)
}

/// `github.com/login/device` for a tooltip; the input when it isn't a URL.
#[must_use]
pub fn url_place(url: &str) -> String {
	let Some((_, rest)) = url.split_once("://") else {
		return url.to_string();
	};
	let rest = rest.split(['#', '?']).next().unwrap_or(rest);
	let mut parts = rest.splitn(2, '/');
	let host = parts.next().unwrap_or_default();
	let path = parts.next().unwrap_or_default();
	if host.is_empty() {
		return url.to_string();
	}
	if path.is_empty() {
		host.to_string()
	} else {
		format!("{host}/{path}")
	}
}

/// The device code inside provider instructions, when they carry one.
#[must_use]
pub fn device_code(instructions: &str) -> Option<String> {
	let trimmed = instructions.trim();
	let split = trimmed.rfind(char::is_whitespace)?;
	let code = trimmed[split..].trim();
	if code.len() < 4 || !code.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-') {
		return None;
	}
	let lead = trimmed[..split].trim_end().to_ascii_lowercase();
	if lead.contains("code") {
		Some(code.to_string())
	} else {
		None
	}
}

/// A prompt's label without its trailing `, then press Enter:` (the sheet's
/// Continue button says it).
#[must_use]
pub fn prompt_label(message: &str) -> String {
	let mut label = message.trim().to_string();
	for suffix in [", then press", "then press"] {
		if let Some(at) = label.to_ascii_lowercase().rfind(suffix) {
			label.truncate(at);
			label = label.trim().trim_end_matches(',').trim().to_string();
		}
	}
	label.trim_end_matches(':').trim().to_string()
}

/// Progress that reports a problem rather than a finished stage.
fn progress_problem(message: &str) -> bool {
	let lower = message.to_ascii_lowercase();
	["fail", "error", "unavailable", "in use", "couldn't", "could not", "unable"]
		.iter()
		.any(|needle| lower.contains(needle))
}

/// The login sheet.
pub struct LoginDialogComponent {
	title: String,
	role: String,
	provider_name: String,
	auth: Option<LoginAuth>,
	steps: Vec<LoginStep>,
	form: Form,
	pending: Option<Sender<String>>,
	request_render: Box<dyn FnMut()>,
	on_complete: Box<dyn FnMut(bool, Option<String>)>,
	open_url: Box<dyn FnMut(&str)>,
	focused: bool,
	use_terminal_cursor: bool,
	theme: OverlayTheme,
	colors: Theme,
}

impl std::fmt::Debug for LoginDialogComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LoginDialogComponent")
			.field("title", &self.title)
			.field("role", &self.role)
			.field("focused", &self.focused)
			.finish()
	}
}

impl LoginDialogComponent {
	/// Build the sheet for `provider_name`.
	pub fn new(
		provider_name: &str,
		request_render: impl FnMut() + 'static,
		on_complete: impl FnMut(bool, Option<String>) + 'static,
		open_url: impl FnMut(&str) + 'static,
	) -> Self {
		let title = format!("Login to {provider_name}");
		LoginDialogComponent {
			title: title.clone(),
			role: LOGIN_ROLE.to_string(),
			provider_name: provider_name.to_string(),
			auth: None,
			steps: Vec::new(),
			form: Form::new(Vec::new(), FormTheme::default()),
			pending: None,
			request_render: Box::new(request_render),
			on_complete: Box::new(on_complete),
			open_url: Box::new(open_url),
			focused: false,
			use_terminal_cursor: false,
			theme: OverlayTheme::default(),
			colors: crate::theme::default_theme(),
		}
	}

	/// The collapsed title shown in the top border.
	#[must_use]
	pub fn title(&self) -> &str {
		&self.title
	}

	/// The semantic role reported to the native host.
	#[must_use]
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The provider the sheet signs in to.
	#[must_use]
	pub fn provider_name(&self) -> &str {
		&self.provider_name
	}

	/// The sign-in link shown, if any.
	#[must_use]
	pub fn auth(&self) -> Option<&LoginAuth> {
		self.auth.as_ref()
	}

	/// The steps after the link, in arrival order.
	#[must_use]
	pub fn steps(&self) -> &[LoginStep] {
		&self.steps
	}

	/// Whether focus reaches the live field.
	#[must_use]
	pub fn focused(&self) -> bool {
		self.focused
	}

	pub fn set_focused(&mut self, focused: bool) {
		self.focused = focused;
	}

	pub fn set_use_terminal_cursor(&mut self, use_terminal_cursor: bool) {
		self.use_terminal_cursor = use_terminal_cursor;
	}

	/// Whether the terminal's own cursor should draw the caret.
	#[must_use]
	pub fn uses_terminal_cursor(&self) -> bool {
		self.use_terminal_cursor
	}

	/// Whether the sheet waits for an answer.
	#[must_use]
	pub fn active_field(&self) -> bool {
		self.pending.is_some() && !self.form.fields().is_empty()
	}

	fn request_render(&mut self) {
		(self.request_render)();
	}

	/// The instructions from `showAuth` as the sheet shows them.
	fn lead(&self, auth: &LoginAuth) -> String {
		if auth.device_code.is_some() {
			return "Enter this code on the sign-in page to connect your account.".to_string();
		}
		if let Some(instructions) = &auth.instructions {
			return sheet_text(instructions);
		}
		"Continue in your browser. If it didn't open, open or copy the link below.".to_string()
	}

	/// Called by the OAuth `onAuth` callback: shows the link, the device code
	/// and the instructions, then opens the browser.
	pub fn show_auth(
		&mut self,
		url: &str,
		instructions: Option<&str>,
		launch_url: Option<&str>,
	) {
		let plain = instructions.map(sheet_text);
		self.auth = Some(LoginAuth {
			url: url.to_string(),
			launch_url: launch_url.filter(|launch| *launch != url).map(str::to_string),
			device_code: plain.as_deref().and_then(device_code),
			instructions: plain,
		});
		self.steps.clear();
		(self.open_url)(url);
		self.request_render();
	}

	/// A fresh field for the next answer.
	fn replace_input(&mut self, secret: bool, placeholder: Option<&str>) {
		let mut field = FormField::new(ANSWER_FIELD, "", FormFieldKind::Text {
			value: String::new(),
			secret,
			prompt: placeholder.map(|hint| format!("{hint}: ")),
		});
		field.empty = EmptyInput::Submit;
		self.form.set_fields(vec![field]);
	}

	/// Record the live field's value as an answer and clear the field.
	fn settle_field(&mut self, answer: Option<String>) {
		if let Some(LoginStep::Input { answer: slot, .. }) = self
			.steps
			.iter_mut()
			.rev()
			.find(|step| matches!(step, LoginStep::Input { answer: None, .. }))
		{
			*slot = Some(answer.unwrap_or_else(|| "********".to_string()));
		}
		self.form.set_fields(Vec::new());
	}

	/// Show input for manual code/URL entry (for callback-server providers).
	/// `retry` keeps the prompt mounted and explains why it asks again.
	pub fn show_manual_input(&mut self, prompt: &str, retry: bool) -> Receiver<String> {
		let (sender, receiver) = std::sync::mpsc::channel();
		if retry && self.active_field() {
			if let Some(field) = self.form.field_mut(ANSWER_FIELD) {
				field.set_error(Some(RETRY_MESSAGE.to_string()));
			}
		} else {
			self.settle_field(None);
			let reuse = matches!(
				self.steps.last(),
				Some(LoginStep::Input { answer: None, .. })
			);
			if reuse {
				if let Some(LoginStep::Input { message, .. }) = self.steps.last_mut() {
					*message = prompt.to_string();
				}
			} else {
				self.steps.push(LoginStep::Input {
					message: prompt.to_string(),
					answer: None,
				});
			}
			self.replace_input(false, None);
		}
		self.pending = Some(sender);
		self.request_render();
		receiver
	}

	/// Show a prompt and wait for its answer. Earlier answers stay visible;
	/// secrets are masked.
	pub fn show_prompt(&mut self, prompt: LoginPrompt) -> Receiver<String> {
		let previous = self
			.form
			.field(ANSWER_FIELD)
			.map(FormField::value)
			.unwrap_or_default();
		let previous_secret = matches!(
			self.form.field(ANSWER_FIELD).map(|field| &field.kind),
			Some(FormFieldKind::Text { secret: true, .. })
		);
		if !previous.is_empty() {
			self.settle_field(Some(if previous_secret {
				"********".to_string()
			} else {
				previous
			}));
		}
		self.steps.push(LoginStep::Input {
			message: prompt.message.clone(),
			answer: None,
		});
		let (sender, receiver) = std::sync::mpsc::channel();
		self.replace_input(prompt.secret, prompt.placeholder.as_deref());
		self.pending = Some(sender);
		self.request_render();
		receiver
	}

	/// Show a waiting message (for polling flows like GitHub Copilot).
	pub fn show_waiting(&mut self, message: &str) {
		self.steps.push(LoginStep::Progress {
			message: sheet_text(message),
		});
		self.request_render();
	}

	/// Called by the `onProgress` callback.
	pub fn show_progress(&mut self, message: &str) {
		self.steps.push(LoginStep::Progress {
			message: sheet_text(message),
		});
		self.request_render();
	}

	/// Cancel the flow: the waiting receiver errors and `on_complete` hears
	/// about it.
	pub fn cancel(&mut self) {
		self.pending = None;
		self.form.set_fields(Vec::new());
		(self.on_complete)(false, Some("Login cancelled".to_string()));
		self.request_render();
	}

	/// Submit the live field's value as the answer.
	pub fn submit(&mut self) -> bool {
		let Some(field) = self.form.field(ANSWER_FIELD) else {
			return false;
		};
		let value = field.value();
		if value.trim().is_empty() {
			return false;
		}
		let Some(sender) = self.pending.take() else {
			return false;
		};
		self.settle_field(Some(value.clone()));
		let _ = sender.send(value);
		self.request_render();
		true
	}

	/// Route non-bracketed paste transports into the live login input.
	pub fn paste_text(&mut self, text: &str) {
		self.form.handle_paste(text);
		self.request_render();
	}

	/// The answered steps, for hosts that redraw the sheet themselves.
	#[must_use]
	pub fn answers(&self) -> Vec<(String, String)> {
		self.steps
			.iter()
			.filter_map(|step| match step {
				LoginStep::Input {
					message,
					answer: Some(answer),
				} => Some((message.clone(), answer.clone())),
				_ => None,
			})
			.collect()
	}

	/// Cancel and Continue run what Esc and Enter run.
	pub fn handle_native_event(&mut self, event: &crate::native::backend::NativeUiEvent) {
		use crate::native::backend::NativeUiEvent;
		match event {
			NativeUiEvent::Action { act, .. } => match act.as_str() {
				"cancel" => self.cancel(),
				"submit" => {
					self.submit();
				}
				_ => {}
			},
			_ => {}
		}
	}

	fn hint_line(&self) -> String {
		if self.active_field() {
			"(esc to cancel, enter to submit)".to_string()
		} else {
			"(esc to cancel)".to_string()
		}
	}
}

impl Component for LoginDialogComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1);
		let inner = (width as usize).saturating_sub(4).max(1);
		let dim = |value: &str| self.colors.fg("dim", value).unwrap_or_else(|_| value.to_string());
		let mut rows = vec![
			top_border(width, &self.title, &self.theme),
			row("", width, &self.theme),
		];
		if let Some(auth) = &self.auth {
			let lead = self.lead(auth);
			for line in wrap_text_with_ansi(&lead, inner) {
				rows.push(row(&line, width, &self.theme));
			}
			if let Some(code) = &auth.device_code {
				let accented = self
					.colors
					.fg("accent", &self.colors.bold(code))
					.unwrap_or_else(|_| code.clone());
				rows.push(row(&accented, width, &self.theme));
			}
			let url = self
				.colors
				.fg("accent", &auth.url)
				.unwrap_or_else(|_| auth.url.clone());
			for line in wrap_text_with_ansi(&url, inner) {
				rows.push(row(&line, width, &self.theme));
			}
			rows.push(row(&dim("ctrl+click to open"), width, &self.theme));
			if let Some(launch) = &auth.launch_url {
				rows.push(row(
					&dim(&format!("Local shortcut (this machine only): {launch}")),
					width,
					&self.theme,
				));
			}
			if let Some(instructions) = &auth.instructions {
				rows.push(row("", width, &self.theme));
				let warning = self
					.colors
					.fg("warning", instructions)
					.unwrap_or_else(|_| instructions.clone());
				for line in wrap_text_with_ansi(&warning, inner) {
					rows.push(row(&line, width, &self.theme));
				}
			}
		}
		let mut live_input = false;
		for step in &self.steps {
			match step {
				LoginStep::Input {
					message,
					answer: Some(answer),
				} => {
					rows.push(row(
						&dim(&format!("{} {answer}", prompt_label(message))),
						width,
						&self.theme,
					));
				}
				LoginStep::Input { message, answer: None } => {
					rows.push(row("", width, &self.theme));
					rows.push(row(&dim(&prompt_label(message)), width, &self.theme));
					live_input = true;
				}
				LoginStep::Progress { message } => {
					rows.push(row(&dim(message), width, &self.theme));
				}
			}
		}
		if live_input {
			for line in self.form.render(inner as u16) {
				rows.push(row(&line, width, &self.theme));
			}
		}
		rows.push(row("", width, &self.theme));
		rows.push(row(&dim(&self.hint_line()), width, &self.theme));
		rows.push(bottom_border(width, &self.theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		if is_escape(key) {
			self.cancel();
			return;
		}
		if is_enter(key) {
			self.submit();
			return;
		}
		self.form.handle_input(key);
	}

	fn handle_paste(&mut self, text: &str) {
		self.paste_text(text);
	}
}

impl NativeDescribe for LoginDialogComponent {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let mut body: Vec<NativeNode> = Vec::new();
		if let Some(auth) = &self.auth {
			let mut lead_props = Props::new();
			lead_props.set_role("omp.login.lead");
			lead_props.set_str("wrap", "word");
			body.push(text(NodeText::plain(self.lead(auth)), lead_props));
			if let Some(code) = &auth.device_code {
				let mut code_props = Props::new();
				code_props.set_role("omp.login.code");
				code_props.set_str("align", "center");
				code_props.set_str("href", code.as_str());
				code_props.set_str("title", "Copy code");
				body.push(node(
					NodeKind::Row,
					code_props,
					vec![text(
						NodeText::Spans(vec![Span::styled(code.clone(), "mono")]),
						Props::new(),
					)],
					Some("code"),
				));
			}
			let mut link_props = Props::new();
			link_props.set_role("omp.login.url");
			link_props.set_str("truncate", "middle");
			link_props.set_i64("lines", 1);
			link_props.set_str("title", auth.url.as_str());
			body.push(node(
				NodeKind::Row,
				{
					let mut props = Props::new();
					props.set_role("omp.login.link");
					props.set_str("gap", "sm");
					props.set_str("align", "center");
					props
				},
				vec![
					text(
						NodeText::Spans(vec![Span {
							href: Some(auth.url.clone()),
							..Span::styled(auth.url.clone(), "mono")
						}]),
						link_props,
					),
					action_button(
						"Copy",
						"copy",
						ActionButtonOptions {
							key: Some("copyLink".to_string()),
							href: Some(auth.url.clone()),
							title: Some("Copy the sign-in link".to_string()),
							..ActionButtonOptions::default()
						},
					),
				],
				Some("link"),
			));
			if let Some(launch) = &auth.launch_url {
				let mut props = Props::new();
				props.set_role("omp.login.shortcut");
				props.set_str("truncate", "middle");
				props.set_i64("lines", 1);
				body.push(text(
					NodeText::Spans(vec![
						Span::styled("On this machine  ".to_string(), "dim"),
						Span {
							href: Some(launch.clone()),
							..Span::styled(launch.clone(), "mono")
						},
					]),
					props,
				));
			}
		}
		let mut status: Vec<NativeNode> = Vec::new();
		let live = self
			.steps
			.iter()
			.rposition(|step| matches!(step, LoginStep::Progress { .. }));
		for (index, step) in self.steps.iter().enumerate() {
			match step {
				LoginStep::Progress { message } => {
					let mut props = Props::new();
					props.set_role(if Some(index) == live {
						"omp.login.waiting"
					} else {
						"omp.login.done"
					});
					props.set_str("gap", "sm");
					props.set_str("align", "center");
					let mut children = Vec::new();
					if Some(index) == live {
						let mut spinner = Props::new();
						spinner.set_str("style", "orbit");
						spinner.set_str("label", message.as_str());
						spinner.set_str("tone", "muted");
						children.push(node(NodeKind::Spinner, spinner, Vec::new(), None));
					} else {
						let mut icon = Props::new();
						icon.set_str("name", if progress_problem(message) { "warn" } else { "check" });
						children.push(node(NodeKind::Icon, icon, Vec::new(), None));
						children.push(text(
							NodeText::plain(sheet_text(message).trim_end_matches('…').to_string()),
							Props::new(),
						));
						if progress_problem(message) {
							props.set_str("tone", "warning");
						}
					}
					status.push(node(
						NodeKind::Row,
						props,
						children,
						Some(&format!("p{index}")),
					));
				}
				LoginStep::Input {
					message,
					answer: Some(answer),
				} => {
					let mut label_props = Props::new();
					label_props.set_str("truncate", "end");
					label_props.set_i64("lines", 1);
					let mut value_props = Props::new();
					value_props.set_str("truncate", "end");
					value_props.set_i64("lines", 1);
					let mut row_props = Props::new();
					row_props.set_role("omp.login.answer");
					row_props.set_str("gap", "sm");
					row_props.set_str("align", "baseline");
					body.push(node(
						NodeKind::Row,
						row_props,
						vec![
							text(NodeText::plain(prompt_label(message)), label_props),
							text(
								NodeText::plain(if answer.is_empty() {
									"—".to_string()
								} else {
									answer.clone()
								}),
								value_props,
							),
						],
						Some(&format!("a{index}")),
					));
				}
				LoginStep::Input { message, answer: None } => {
					let mut col_props = Props::new();
					col_props.set_role("omp.login.prompt");
					col_props.set_str("gap", "sm");
					let mut field_props = Props::new();
					field_props.set_role("omp.login.field");
					field_props.set_bool("secret", self.form.field(ANSWER_FIELD).is_some_and(
						|field| matches!(&field.kind, FormFieldKind::Text { secret: true, .. }),
					));
					field_props.set_str("value", self.form.field(ANSWER_FIELD).map(FormField::value).unwrap_or_default());
					body.push(node(
						NodeKind::Col,
						col_props,
						vec![
							text(NodeText::plain(prompt_label(message)), Props::new()),
							node(NodeKind::Input, field_props, Vec::new(), Some("field")),
						],
						Some(&format!("i{index}")),
					));
				}
			}
		}
		if self.auth.is_some() && live.is_none() && !self.active_field() {
			let mut props = Props::new();
			props.set_role("omp.login.waiting");
			props.set_str("gap", "sm");
			props.set_str("align", "center");
			let mut spinner = Props::new();
			spinner.set_str("style", "orbit");
			spinner.set_str("label", BROWSER_WAIT);
			spinner.set_str("tone", "muted");
			status.push(node(
				NodeKind::Row,
				props,
				vec![node(NodeKind::Spinner, spinner, Vec::new(), None)],
				Some("browser"),
			));
		}
		if !status.is_empty() {
			let mut props = Props::new();
			props.set_role("omp.login.status");
			props.set_str("gap", "xs");
			body.push(node(NodeKind::Col, props, status, Some("status")));
		}

		let cancel = action_button(
			"Cancel",
			"cancel",
			ActionButtonOptions {
				keys: Some("esc".to_string()),
				..ActionButtonOptions::default()
			},
		);
		let open = self.auth.as_ref().map(|auth| {
			action_button(
				"Open sign-in page",
				"open",
				ActionButtonOptions {
					key: Some("openLink".to_string()),
					href: Some(auth.url.clone()),
					title: Some(format!("Open {} in your browser", url_place(&auth.url))),
					icon: Some("open".to_string()),
					tone: self.active_field().then_some(ActionButtonTone::Accent),
					..ActionButtonOptions::default()
				},
			)
		});
		let buttons: Vec<Option<NativeNode>> = if self.active_field() {
			let submit = action_button(
				"Continue",
				"submit",
				ActionButtonOptions {
					tone: Some(ActionButtonTone::Accent),
					keys: Some("enter".to_string()),
					..ActionButtonOptions::default()
				},
			);
			match open {
				Some(open) => vec![Some(open), None, Some(cancel), Some(submit)],
				None => vec![None, Some(cancel), Some(submit)],
			}
		} else {
			match open {
				Some(open) => vec![None, Some(cancel), Some(open)],
				None => vec![None, Some(cancel)],
			}
		};

		let mut sheet_props = Props::new();
		sheet_props.set_role("omp.login.sheet");
		sheet_props.set_str("anchor", "center");
		sheet_props.set_str("size", "md");
		sheet_props.set_bool("modal", true);
		let mut body_props = Props::new();
		body_props.set_role("omp.login.body");
		body_props.set_str("gap", "md");
		overlay_card(
			&self.role,
			Some(NodeText::plain(format!("Sign in to {}", self.provider_name))),
			vec![
				node(NodeKind::Col, body_props, body, Some("body")),
				action_bar(&buttons, "actions"),
			],
			Some(sheet_props),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn dialog() -> (
		LoginDialogComponent,
		std::rc::Rc<std::cell::RefCell<Vec<(bool, Option<String>)>>>,
		std::rc::Rc<std::cell::RefCell<Vec<String>>>,
	) {
		let completed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = completed.clone();
		let opener = opened.clone();
		let component = LoginDialogComponent::new(
			"anthropic",
			|| {},
			move |ok, message| sink.borrow_mut().push((ok, message)),
			move |url| opener.borrow_mut().push(url.to_string()),
		);
		(component, completed, opened)
	}

	#[test]
	fn show_auth_splits_a_device_code_and_opens_the_browser() {
		let (mut component, _completed, opened) = dialog();
		component.show_auth(
			"https://github.com/login/device",
			Some("Enter code: ABCD-1234"),
			Some("http://127.0.0.1:7777/launch"),
		);
		let auth = component.auth().expect("auth shown");
		assert_eq!(auth.device_code.as_deref(), Some("ABCD-1234"));
		assert_eq!(auth.instructions.as_deref(), Some("Enter code: ABCD-1234"));
		assert_eq!(auth.launch_url.as_deref(), Some("http://127.0.0.1:7777/launch"));
		assert_eq!(&*opened.borrow(), &["https://github.com/login/device".to_string()]);
		assert_eq!(component.title(), "Login to anthropic");
	}

	#[test]
	fn launch_url_equal_to_the_url_is_not_offered() {
		let (mut component, _completed, _opened) = dialog();
		component.show_auth("https://example.test/auth", None, Some("https://example.test/auth"));
		assert_eq!(component.auth().expect("auth").launch_url, None);
	}

	#[test]
	fn manual_input_submits_its_answer() {
		let (mut component, _completed, _opened) = dialog();
		component.show_auth("https://example.test/auth", None, None);
		let receiver = component.show_manual_input("Paste the authorization code", false);
		assert!(component.active_field());
		component.paste_text("abc-123");
		component.handle_input(&Key::Enter);
		assert_eq!(receiver.recv().expect("answer"), "abc-123");
		assert_eq!(
			component.answers(),
			vec![("Paste the authorization code".to_string(), "abc-123".to_string())]
		);
	}

	#[test]
	fn a_retry_keeps_the_prompt_and_explains_itself() {
		let (mut component, _completed, _opened) = dialog();
		let _first = component.show_manual_input("Paste the code", false);
		let _second = component.show_manual_input("Paste the code", true);
		assert_eq!(component.steps().len(), 1, "one prompt, not two");
		let form_rows = component.form.render(40).join("\n");
		assert!(form_rows.contains("Couldn't read a code"), "{form_rows}");
	}

	#[test]
	fn a_secret_answer_is_masked_in_the_log() {
		let (mut component, _completed, _opened) = dialog();
		let _receiver = component.show_prompt(LoginPrompt {
			message: "API key".to_string(),
			placeholder: None,
			secret: true,
		});
		component.paste_text("sk-secret");
		component.handle_input(&Key::Enter);
		assert_eq!(
			component.answers(),
			vec![("API key".to_string(), "********".to_string())]
		);
	}

	#[test]
	fn cancel_drops_the_waiting_receiver_and_completes() {
		let (mut component, completed, _opened) = dialog();
		let receiver = component.show_prompt(LoginPrompt {
			message: "Continue".to_string(),
			placeholder: None,
			secret: false,
		});
		component.cancel();
		assert!(receiver.recv().is_err());
		assert_eq!(
			&*completed.borrow(),
			&[(false, Some("Login cancelled".to_string()))]
		);
	}

	#[test]
	fn progress_keeps_every_stage_and_the_last_is_live() {
		let (mut component, _completed, _opened) = dialog();
		component.show_auth("https://example.test/auth", None, None);
		component.show_progress("Opening browser");
		component.show_progress("Waiting for approval...");
		assert_eq!(component.steps().len(), 2);
		let described = component.describe_native(&DescribeContext::new(80));
		let roles = describe_roles(&described);
		assert!(roles.iter().any(|role| role == "omp.login.waiting"));
		assert!(roles.iter().any(|role| role == "omp.login.done"));
	}

	fn describe_roles(root: &NativeNode) -> Vec<String> {
		let mut roles = Vec::new();
		let mut stack = vec![root];
		while let Some(node) = stack.pop() {
			if let Some(role) = node.props.get_str("role") {
				roles.push(role.to_string());
			}
			stack.extend(node.children.iter());
		}
		roles
	}

	#[test]
	fn render_shows_the_link_instructions_and_hint() {
		let (mut component, _completed, _opened) = dialog();
		component.show_auth(
			"https://example.test/auth",
			Some("Approve the sign-in..."),
			Some("http://127.0.0.1:7777/launch"),
		);
		let frame = component.render(70).join("\n");
		assert!(frame.contains("Login to anthropic"));
		assert!(frame.contains("https://example.test/auth"));
		assert!(frame.contains("Approve the sign-in…"));
		assert!(frame.contains("Local shortcut (this machine only)"));
		assert!(frame.contains("(esc to cancel)"));
	}

	#[test]
	fn helpers_match_the_reference_shapes() {
		assert_eq!(sheet_text("Waiting...  "), "Waiting…");
		assert_eq!(sheet_text("done"), "done");
		assert_eq!(url_place("https://github.com/login/device"), "github.com/login/device");
		assert_eq!(url_place("https://github.com/"), "github.com");
		assert_eq!(url_place("not a url"), "not a url");
		assert_eq!(device_code("Open the page and enter code: ABCD-1234"), Some("ABCD-1234".to_string()));
		assert_eq!(device_code("Follow the provider's steps"), None);
		assert_eq!(prompt_label("Paste the code, then press Enter:"), "Paste the code");
		assert_eq!(prompt_label("API key"), "API key");
	}
}
