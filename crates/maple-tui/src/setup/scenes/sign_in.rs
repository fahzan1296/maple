//! Reference: `l/src/setup/scenes/sign-in.ts`.
//!
//! Onboarding scene for provider sign-in. It never auto-advances: the user may
//! sign in to several providers and then leave the step with Esc.
//!
//! Deviations from the TypeScript original:
//! - `overlays/login-dialog` and `overlays/oauth-selector` are not ported; compact
//!   local [`LoginDialog`] and [`OAuthSelector`] stand-ins live here.
//! - `AuthStorage::login` needs `&mut self`, but the host exposes `&dyn`; the
//!   scene calls [`SetupHost::login_provider`] instead.
//! - `PASTE_CODE_LOGIN_PROVIDERS` maps to [`AuthStorage::uses_manual_code`];
//!   `getOAuthProviders` to [`AuthStorage::providers`];
//!   `getAgentDbPath` to [`agent_db_path`].
//! - The login flow is synchronous; the `AbortController` lifetime is replaced by
//!   a disposed flag.
//! - `WizardStep`/`Memo` are omitted; intro, list, and status render directly.

use std::cell::RefCell;
use std::rc::Rc;

use crate::chrome::keybinding_hints::editor_key;
use crate::component::Component;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::describe::{col, item, list, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::setup::scenes::types::{
	agent_db_path, AuthStorage, LoginCallbacks, SetupOAuthProvider, SetupScene,
	SetupSceneController, SetupSceneHost, SetupSceneResult, SgrMouseEvent, StyledLine,
	SETUP_SHEET_ROLE,
};

const SCENE_TITLE: &str = "Sign in to your providers";
const LIST_LEAD: &str = "Pick a provider to sign in — you can connect more than one.";
const SHEET_LEAD: &str = "You can connect more than one.";

/// The last finished sign-in.
#[derive(Clone, Debug)]
enum SignInOutcome {
	Success { provider: String },
	Error { provider: String, message: String },
}

/// Compact stand-in for `overlays/oauth-selector`.
struct OAuthSelector {
	providers: Vec<SetupOAuthProvider>,
	selected: usize,
	disabled: Vec<String>,
	status: Option<String>,
}

pub enum SelectorOutcome {
	Selected(String),
	Cancelled,
}

impl OAuthSelector {
	fn new(auth: &dyn AuthStorage, disabled: &[String]) -> Self {
		OAuthSelector {
			providers: auth.providers(),
			selected: 0,
			disabled: disabled.to_vec(),
			status: None,
		}
	}

	fn refresh_auth(&mut self, auth: &dyn AuthStorage) {
		self.providers = auth.providers();
	}

	fn set_status(&mut self, status: Option<String>) {
		self.status = status;
	}

	fn handle_input(&mut self, key: &Key) -> Option<SelectorOutcome> {
		match key {
			Key::Up => {
				if self.selected > 0 {
					self.selected -= 1;
				}
				None
			}
			Key::Down => {
				if self.selected + 1 < self.providers.len() {
					self.selected += 1;
				}
				None
			}
			Key::Enter => self
				.providers
				.get(self.selected)
				.map(|provider| SelectorOutcome::Selected(provider.id.clone())),
			Key::Esc => Some(SelectorOutcome::Cancelled),
			_ => None,
		}
	}

	fn render(&self, width: u16) -> Vec<String> {
		let width = width.max(1) as usize;
		let mut lines = Vec::new();
		if let Some(status) = &self.status {
			lines.push(status.clone());
		}
		for (index, provider) in self.providers.iter().enumerate() {
			let marker = if index == self.selected { "❯" } else { " " };
			let credential = if self.disabled.iter().any(|id| *id == provider.id) {
				"  [disabled]"
			} else {
				""
			};
			let mut line = format!("{} {}{}", marker, provider.name, credential);
			if line.chars().count() > width {
				line = line.chars().take(width).collect();
			}
			lines.push(line);
		}
		lines
	}
}

/// Compact stand-in for `overlays/login-dialog`.
pub struct LoginDialog {
	state: LoginState,
	manual_buffer: String,
	aborted: bool,
}

enum LoginState {
	Idle,
	Auth { url: String, instructions: Option<String>, launch_url: Option<String> },
	Prompt(String),
	Progress(String),
	Manual { prompt: String },
}

impl LoginDialog {
	fn new() -> Self {
		LoginDialog {
			state: LoginState::Idle,
			manual_buffer: String::new(),
			aborted: false,
		}
	}

	fn show_auth(&mut self, url: &str, instructions: Option<&str>, launch_url: Option<&str>) {
		self.state = LoginState::Auth {
			url: url.to_string(),
			instructions: instructions.map(str::to_string),
			launch_url: launch_url.map(str::to_string),
		};
	}

	fn show_prompt(&mut self, prompt: &str) {
		self.state = LoginState::Prompt(prompt.to_string());
	}

	fn show_progress(&mut self, message: &str) {
		self.state = LoginState::Progress(message.to_string());
	}

	fn show_manual_input(&mut self, prompt: &str) -> Option<String> {
		self.state = LoginState::Manual {
			prompt: prompt.to_string(),
		};
		self.manual_buffer.clear();
		None
	}

	fn handle_input(&mut self, key: &Key) -> DialogOutcome {
		match key {
			Key::Esc => {
				self.aborted = true;
				DialogOutcome::Closed
			}
			Key::Enter => {
				if let LoginState::Manual { .. } = self.state {
					let code = std::mem::take(&mut self.manual_buffer);
					self.state = LoginState::Progress("Submitting code…".to_string());
					return DialogOutcome::ManualCode(code);
				}
				DialogOutcome::None
			}
			Key::Backspace => {
				if let LoginState::Manual { .. } = self.state {
					self.manual_buffer.pop();
				}
				DialogOutcome::None
			}
			Key::Char(character) => {
				if let LoginState::Manual { .. } = self.state {
					self.manual_buffer.push(*character);
				}
				DialogOutcome::None
			}
			_ => DialogOutcome::None,
		}
	}

	fn render(&self, width: u16) -> Vec<String> {
		let width = width.max(1) as usize;
		let mut lines = vec![format!("  Signing in…")];
		match &self.state {
			LoginState::Idle => {}
			LoginState::Auth { url, instructions, launch_url } => {
				if let Some(instructions) = instructions {
					lines.push(instructions.clone());
				}
				lines.push(url.clone());
				if let Some(launch_url) = launch_url {
					lines.push(launch_url.clone());
				}
			}
			LoginState::Prompt(prompt) => lines.push(prompt.clone()),
			LoginState::Progress(message) => lines.push(message.clone()),
			LoginState::Manual { prompt } => {
				lines.push(prompt.clone());
				lines.push(format!("  {}", self.manual_buffer));
			}
		}
		lines
			.into_iter()
			.map(|line| {
				if line.chars().count() > width {
					line.chars().take(width).collect()
				} else {
					line
				}
			})
			.collect()
	}
}

pub enum DialogOutcome {
	None,
	Closed,
	ManualCode(String),
}

impl Component for LoginDialog {
	fn render(&mut self, width: u16) -> Vec<String> {
		LoginDialog::render(self, width)
	}

	fn handle_input(&mut self, key: &Key) {
		LoginDialog::handle_input(self, key);
	}
}

struct DialogCallbacks {
	dialog: Rc<RefCell<LoginDialog>>,
}

impl LoginCallbacks for DialogCallbacks {
	fn show_auth(&mut self, url: &str, instructions: Option<&str>, launch_url: Option<&str>) {
		self.dialog.borrow_mut().show_auth(url, instructions, launch_url);
	}

	fn show_prompt(&mut self, prompt: &str) {
		self.dialog.borrow_mut().show_prompt(prompt);
	}

	fn show_progress(&mut self, message: &str) {
		self.dialog.borrow_mut().show_progress(message);
	}

	fn show_manual_input(&mut self, prompt: &str) -> Option<String> {
		self.dialog.borrow_mut().show_manual_input(prompt)
	}
}

/// Mount the provider sign-in scene.
pub fn mount_providers_scene(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	Box::new(SignInScene::new(host))
}

struct SignInScene {
	host: Rc<dyn SetupSceneHost>,
	selector: OAuthSelector,
	dialog: Option<Rc<RefCell<LoginDialog>>>,
	outcome: Option<SignInOutcome>,
	subtitle: String,
	signed_in: bool,
	disposed: bool,
}

impl SignInScene {
	fn new(host: Box<dyn SetupSceneHost>) -> Self {
		let host: Rc<dyn SetupSceneHost> = Rc::from(host);
		let ctx = host.ctx();
		let theme = ctx.theme();
		let subtitle = theme
			.fg(
				"muted",
				&format!(
					"Sign in to one or more providers. Press {} when you're done.",
					editor_key(&KeybindingsManager::new(), theme, "tui.select.cancel")
				),
			)
			.unwrap_or_else(|_| String::new());
		let disabled = ctx.disabled_providers().to_vec();
		let selector = OAuthSelector::new(ctx.auth_storage(), &disabled);
		SignInScene {
			host,
			selector,
			dialog: None,
			outcome: None,
			subtitle,
			signed_in: false,
			disposed: false,
		}
	}
	fn status_lines(&self) -> Vec<StyledLine> {
		let theme = self.host.ctx().theme();
		match &self.outcome {
			Some(SignInOutcome::Success { provider }) => vec![
				StyledLine::new(
					format!("{} Signed in to {}", theme.status().success, provider),
					"success",
				),
				StyledLine::new(
					format!("Credentials saved to {}", agent_db_path()),
					"dim",
				),
			],
			Some(SignInOutcome::Error { provider, message }) => vec![
				StyledLine::new(format!("Couldn't sign in to {}: {}", provider, message), "error"),
				StyledLine::new("Choose another provider or press Esc to continue.", "dim"),
			],
			None => Vec::new(),
		}
	}

	fn set_outcome(&mut self, outcome: Option<SignInOutcome>) {
		if matches!(outcome, Some(SignInOutcome::Success { .. })) {
			self.signed_in = true;
		}
		self.outcome = outcome;
		let status = match &self.outcome {
			Some(SignInOutcome::Success { provider }) => Some(format!(
				"Signed in to {}. Add another provider or continue.",
				provider
			)),
			Some(SignInOutcome::Error { message, .. }) => Some(format!("Couldn't sign in: {}", message)),
			None => None,
		};
		self.selector.set_status(status);
	}

	fn login(&mut self, provider_id: &str) {
		if self.dialog.is_some() || self.disposed {
			return;
		}
		let name = {
			let ctx = self.host.ctx();
			ctx.auth_storage()
				.providers()
				.into_iter()
				.find(|provider| provider.id == provider_id)
				.map(|provider| provider.name)
				.unwrap_or_else(|| provider_id.to_string())
		};
		let dialog = Rc::new(RefCell::new(LoginDialog::new()));
		self.dialog = Some(dialog.clone());
		self.set_outcome(None);
		self.host.set_focus(Some(&*dialog.borrow()));
		self.host.request_render();
		let callbacks = DialogCallbacks {
			dialog: dialog.clone(),
		};
		let mut callbacks = callbacks;
		let result = {
			let ctx = self.host.ctx();
			ctx.login_provider(provider_id, &mut callbacks)
		};
		if self.disposed {
			return;
		}
		match result {
			Ok(()) => {
				{
					let ctx = self.host.ctx();
					ctx.refresh_provider(provider_id);
					let auth = ctx.auth_storage();
					self.selector.refresh_auth(auth);
				}
				self.set_outcome(Some(SignInOutcome::Success { provider: name }));
			}
			Err(error) => {
				if dialog.borrow().aborted {
					return;
				}
				self.set_outcome(Some(SignInOutcome::Error {
					provider: name,
					message: error,
				}));
			}
		}
		self.close_dialog(&dialog);
	}

	fn close_dialog(&mut self, dialog: &Rc<RefCell<LoginDialog>>) {
		if let Some(current) = &self.dialog {
			if Rc::ptr_eq(current, dialog) {
				self.dialog = None;
			}
		}
		self.host.restore_focus();
		self.host.request_render();
	}
}

impl Component for SignInScene {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_body(width, None)
	}

	fn handle_input(&mut self, key: &Key) {
		if let Some(dialog) = self.dialog.clone() {
			let outcome = dialog.borrow_mut().handle_input(key);
			match outcome {
				DialogOutcome::Closed => self.close_dialog(&dialog),
				DialogOutcome::ManualCode(code) => {
					let _ = code;
				}
				DialogOutcome::None => {}
			}
			return;
		}
		match self.selector.handle_input(key) {
			Some(SelectorOutcome::Selected(provider)) => self.login(&provider),
			Some(SelectorOutcome::Cancelled) => {
				self.host
					.finish(if self.signed_in { SetupSceneResult::Done } else { SetupSceneResult::Skipped });
			}
			None => self.host.request_render(),
		}
	}

	fn invalidate(&mut self) {}

	fn dispose(&mut self) {
		self.disposed = true;
		self.dialog = None;
	}
}

impl SetupSceneController for SignInScene {
	fn title(&self) -> &str {
		SCENE_TITLE
	}

	fn subtitle(&self) -> Option<&str> {
		Some(&self.subtitle)
	}

	fn render_body(&mut self, width: u16, max_lines: Option<usize>) -> Vec<String> {
		let mut lines = Vec::new();
		if let Some(dialog) = &self.dialog {
			lines.extend(dialog.borrow_mut().render(width));
		} else {
			let theme = self.host.ctx().theme();
			lines.push(
				theme
					.fg("muted", LIST_LEAD)
					.unwrap_or_else(|_| LIST_LEAD.to_string()),
			);
			lines.push(String::new());
			lines.extend(self.selector.render(width));
			for status in self.status_lines() {
				lines.push(
					theme
						.fg(&status.color, &status.text)
						.unwrap_or_else(|_| status.text.clone()),
				);
			}
		}
		if let Some(max_lines) = max_lines {
			lines.truncate(max_lines);
		}
		lines
	}

	fn route_mouse(&mut self, event: &SgrMouseEvent, _line: i32, _col: i32) {
		if self.dialog.is_some() {
			return;
		}
		if let Some(wheel) = event.wheel {
			match wheel.cmp(&0) {
				std::cmp::Ordering::Less => {
					self.selector.handle_input(&Key::Up);
				}
				std::cmp::Ordering::Greater => {
					self.selector.handle_input(&Key::Down);
				}
				std::cmp::Ordering::Equal => {}
			}
			self.host.request_render();
		}
	}

	fn routes_mouse(&self) -> bool {
		true
	}

	fn describe_scene(&mut self, cx: &DescribeContext) -> NativeNode {
		if let Some(dialog) = &self.dialog {
			let lines = dialog.borrow_mut().render(cx.cols);
			let text_node = text(NodeText::Plain(lines.join("\n")), Props::new());
			let mut props = Props::new();
			props.set_role("omp.setup.sign-in.dialog");
			return col(vec![text_node], props);
		}
		let mut children: Vec<NativeNode> = vec![text(
			NodeText::Spans(vec![span(LIST_LEAD, Some("muted"))]),
			Props::new(),
		)];
		if cx.supports(NodeKind::List) {
			let items: Vec<NativeNode> = self
				.selector
				.providers
				.iter()
				.map(|provider| {
					let mut props = Props::new();
					props.set_str("title", provider.name.as_str());
					if self.selector.disabled.iter().any(|id| *id == provider.id) {
						props.set_str("disabled", "true");
					}
					item(provider.id.clone(), props)
				})
				.collect();
			let mut list_props = Props::new();
			list_props.set_role(SETUP_SHEET_ROLE);
			list_props.set_str("title", SCENE_TITLE);
			list_props.set_str("subtitle", SHEET_LEAD);
			list_props.set_str("leave", if self.signed_in { "Continue" } else { "Skip" });
			children.push(list(items, list_props, Some("providers")));
		} else {
			let items: Vec<NativeNode> = self
				.selector
				.providers
				.iter()
				.map(|provider| {
					let mut props = Props::new();
					props.set_str("title", provider.name.as_str());
					item(provider.id.clone(), props)
				})
				.collect();
			let mut list_props = Props::new();
			list_props.set_role("omp.setup.sign-in.list");
			children.push(list(items, list_props, Some("providers")));
		}
		for status in self.status_lines() {
			children.push(text(
				NodeText::Spans(vec![span(status.text, Some(status.color.as_str()))]),
				Props::new(),
			));
		}
		let mut col_props = Props::new();
		col_props.set_str("gap", "sm");
		col_props.set_role("omp.setup.sign-in");
		col(children, col_props)
	}
}

/// Onboarding scene for provider sign-in.
pub const PROVIDERS_SETUP_SCENE: SetupScene = SetupScene {
	id: "providers",
	title: SCENE_TITLE,
	step: "Sign in",
	min_version: 1,
	ansi_only: false,
	should_run: None,
	mount: mount_providers_scene,
};

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn scene_gate_and_geometry() {
		let scene = PROVIDERS_SETUP_SCENE;
		assert_eq!(scene.id, "providers");
		assert_eq!(scene.step, "Sign in");
		assert_eq!(scene.min_version, 1);
		assert!(!scene.ansi_only);
	}

	#[test]
	fn selector_moves_and_cancels() {
		struct Stub;
		impl AuthStorage for Stub {
			fn providers(&self) -> Vec<SetupOAuthProvider> {
				vec![
					SetupOAuthProvider {
						id: "a".to_string(),
						name: "A".to_string(),
						store_credentials_as: None,
					},
					SetupOAuthProvider {
						id: "b".to_string(),
						name: "B".to_string(),
						store_credentials_as: None,
					},
				]
			}
			fn has_credential(&self, _: &str) -> bool {
				false
			}
			fn uses_manual_code(&self, _: &str) -> bool {
				false
			}
			fn login(&mut self, _: &str, _: &mut dyn LoginCallbacks) -> Result<(), String> {
				Ok(())
			}
			fn refresh(&mut self, _: &str) {}
		}
		let mut selector = OAuthSelector::new(&Stub, &[]);
		selector.handle_input(&Key::Down);
		assert!(matches!(
			selector.handle_input(&Key::Enter),
			Some(SelectorOutcome::Selected(id)) if id == "b"
		));
		assert!(matches!(
			selector.handle_input(&Key::Esc),
			Some(SelectorOutcome::Cancelled)
		));
	}

	#[test]
	fn dialog_collects_manual_code() {
		let mut dialog = LoginDialog::new();
		dialog.show_manual_input("Paste the code:");
		dialog.handle_input(&Key::Char('a'));
		dialog.handle_input(&Key::Char('b'));
		assert!(matches!(
			dialog.handle_input(&Key::Enter),
			DialogOutcome::ManualCode(code) if code == "ab"
		));
	}
}
