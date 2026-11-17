//! Reference: `l/src/overlays/mcp-add-wizard.ts`.
//!
//! Interactive multi-step wizard for adding an MCP server: name, transport,
//! connection details, a connection probe, auth (OAuth or API key), scope, and
//! a review before saving.
//!
//! Deviations:
//! - The reference awaits host promises and delays the next step with
//!   `setTimeout`; this port is synchronous. Host calls
//!   ([`McpAddWizardHost`]) return at once, and the connection/OAuth status
//!   steps do not auto-advance: the status is stored on the next step's lead
//!   and the wizard moves straight to `scope`. The status text still reaches
//!   the sheet through [`McpAddWizard::connection_note`].
//! - `AbortController`/`AbortSignal` have no crate equivalent. A host that
//!   wants cancellation returns `Err` with `cancelled: true`, which lands on
//!   the same `oauth-error` step as the reference's neutral cancellation.
//! - `WizardStep`/`TextFormField`/`SelectList` rendering is replaced by one
//!   step model shared by [`Component::render`] and [`NativeDescribe`].
//! - `getMCPConfigPath`/`getProjectDir` live in the host: the labels are
//!   passed in through [`WizardPaths`].
//! - Config path shortening (`shortenPath`) is the host's job too.

use std::collections::BTreeMap;

use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::components::form::{EmptyInput, Form, FormField, FormFieldKind, FormTheme};
use crate::keys::{
	is_arrow_down, is_arrow_up, is_enter, is_escape, Key,
};
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{NativeDescribe, item, node, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span};
use crate::native::overlay::{
	ActionButtonOptions, ActionButtonTone, action_bar, action_button, overlay_card,
};
use crate::text_shape::wrap_text_with_ansi;
use crate::theme::Theme;

/// Semantic role of the wizard sheet.
pub const MCP_ADD_ROLE: &str = "omp.overlay.mcpAdd";

/// Max display width for sanitized error/URL text.
pub const MAX_DISPLAY_WIDTH: usize = 120;

/// Where a server connects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transport {
	#[default]
	Stdio,
	Http,
	Sse,
}

impl Transport {
	/// List order.
	pub const ALL: [Transport; 3] = [Transport::Stdio, Transport::Http, Transport::Sse];

	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			Transport::Stdio => "stdio",
			Transport::Http => "http",
			Transport::Sse => "sse",
		}
	}

	#[must_use]
	pub fn icon(self) -> &'static str {
		match self {
			Transport::Stdio => "terminal",
			Transport::Http => "globe",
			Transport::Sse => "broadcast",
		}
	}

	#[must_use]
	pub fn label(self) -> &'static str {
		match self {
			Transport::Stdio => "Local command",
			Transport::Http => "Remote server",
			Transport::Sse => "Server-sent events",
		}
	}

	#[must_use]
	pub fn description(self) -> &'static str {
		match self {
			Transport::Stdio => "omp starts it on this machine",
			Transport::Http => "Streamable HTTP endpoint",
			Transport::Sse => "Older HTTP servers",
		}
	}
}

/// How the server authenticates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AuthMethod {
	#[default]
	None,
	Oauth,
	Manual,
}

/// Where a manual key goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthLocation {
	Env,
	Header,
}

/// Where the server is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
	User,
	Project,
}

impl Scope {
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			Scope::User => "user",
			Scope::Project => "project",
		}
	}

	#[must_use]
	pub fn label(self) -> &'static str {
		match self {
			Scope::User => "Every project",
			Scope::Project => "This project",
		}
	}
}

/// Auth fields produced by the wizard.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpOAuthAuth {
	pub credential_id: String,
	pub token_url: Option<String>,
	pub client_id: Option<String>,
	pub client_secret: Option<String>,
	pub resource: Option<String>,
}

/// Server configuration fields produced for testing and persistence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpServerConfig {
	/// `stdio`, `http` or `sse`.
	pub kind: Transport,
	pub command: String,
	pub args: Vec<String>,
	pub url: String,
	pub env: BTreeMap<String, String>,
	pub headers: BTreeMap<String, String>,
	pub timeout: Option<u64>,
	pub auth: Option<McpOAuthAuth>,
}

impl McpServerConfig {
	/// The wire shape the reference writes into `mcp.json`.
	#[must_use]
	pub fn to_json(&self) -> serde_json::Value {
		let mut out = serde_json::Map::new();
		out.insert("type".to_string(), serde_json::json!(self.kind.as_str()));
		if self.kind == Transport::Stdio {
			out.insert("command".to_string(), serde_json::json!(self.command));
			if !self.args.is_empty() {
				out.insert("args".to_string(), serde_json::json!(self.args));
			}
			if !self.env.is_empty() {
				out.insert("env".to_string(), serde_json::json!(self.env));
			}
		} else {
			out.insert("url".to_string(), serde_json::json!(self.url));
			if !self.headers.is_empty() {
				out.insert("headers".to_string(), serde_json::json!(self.headers));
			}
		}
		if let Some(timeout) = self.timeout {
			out.insert("timeout".to_string(), serde_json::json!(timeout));
		}
		if let Some(auth) = &self.auth {
			let mut entry = serde_json::Map::new();
			entry.insert("type".to_string(), serde_json::json!("oauth"));
			entry.insert("credentialId".to_string(), serde_json::json!(auth.credential_id));
			for (key, value) in [
				("tokenUrl", &auth.token_url),
				("clientId", &auth.client_id),
				("clientSecret", &auth.client_secret),
				("resource", &auth.resource),
			] {
				if let Some(value) = value {
					entry.insert(key.to_string(), serde_json::json!(value));
				}
			}
			out.insert("auth".to_string(), serde_json::Value::Object(entry));
		}
		serde_json::Value::Object(out)
	}
}

/// OAuth endpoints the host discovered for a server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpOAuthEndpoints {
	pub authorization_url: String,
	pub token_url: String,
	pub issuer_url: Option<String>,
	pub client_id: Option<String>,
	pub registration_url: Option<String>,
	pub scopes: Option<String>,
	pub resource: Option<String>,
}

/// What kind of auth an error asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthType {
	Oauth,
	ApiKey,
	Unknown,
}

/// The host's reading of a connection error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthDetection {
	pub requires_auth: bool,
	pub auth_type: Option<AuthType>,
	pub oauth: Option<McpOAuthEndpoints>,
	pub auth_server_url: Option<String>,
	pub resource_metadata_url: Option<String>,
	pub scopes: Option<String>,
}

/// An error a host call reported.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WizardError {
	pub message: String,
	pub cancelled: bool,
}

impl WizardError {
	pub fn new(message: impl Into<String>) -> Self {
		WizardError {
			message: message.into(),
			cancelled: false,
		}
	}

	pub fn cancelled(message: impl Into<String>) -> Self {
		WizardError {
			message: message.into(),
			cancelled: true,
		}
	}
}

/// Options of the host's OAuth callback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OAuthFlowOptions {
	pub server_url: Option<String>,
	pub registration_url: Option<String>,
	pub issuer_url: Option<String>,
	pub resource: Option<String>,
	pub strip_same_origin_resource: bool,
}

/// The host's OAuth result. `client_id` is set when dynamic client
/// registration happened; refresh material stays in the stored credential.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OAuthResult {
	pub credential_id: String,
	pub client_id: Option<String>,
	pub resource: Option<String>,
}

/// Host-owned validation, discovery and connection capabilities.
pub trait McpAddWizardHost {
	/// Why `name` is not a valid server name, if it is not.
	fn validate_server_name(&self, name: &str) -> Option<String>;
	/// What an error says about the server's auth needs.
	fn analyze_auth_error(&self, error: &WizardError, server_url: Option<&str>) -> AuthDetection;
	/// Discover OAuth endpoints for a remote server.
	fn discover_oauth_endpoints(
		&mut self,
		server_url: &str,
		auth_server_url: Option<&str>,
		resource_metadata_url: Option<&str>,
		protected_scopes: Option<&str>,
	) -> Option<McpOAuthEndpoints>;
	/// Scopes advertised in a protected-resource metadata document.
	fn fetch_resource_metadata_scopes(&mut self, resource_metadata_url: &str) -> Option<String>;
	/// Probe a config. The transport layer owns the 5-second test timeout.
	fn test_connection(&mut self, config: &McpServerConfig) -> Result<(), WizardError>;
	/// Run the browser OAuth flow.
	fn start_oauth(
		&mut self,
		auth_url: &str,
		token_url: &str,
		client_id: &str,
		client_secret: &str,
		scopes: &str,
		options: &OAuthFlowOptions,
	) -> Result<OAuthResult, WizardError>;
}

/// Config file labels the sheet shows for each scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WizardPaths {
	pub user: String,
	pub project: String,
}

impl WizardPaths {
	#[must_use]
	pub fn label(&self, scope: Scope) -> &str {
		match scope {
			Scope::User => &self.user,
			Scope::Project => &self.project,
		}
	}
}

/// One step of the wizard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McpWizardStep {
	#[default]
	Name,
	Transport,
	Command,
	Args,
	Url,
	AuthMethod,
	OAuthError,
	OAuthAuthUrl,
	OAuthTokenUrl,
	OAuthClientId,
	OAuthClientSecret,
	OAuthScopes,
	ApiKey,
	AuthLocation,
	EnvVarName,
	HeaderName,
	Scope,
	Confirm,
}

impl McpWizardStep {
	/// The phase shown in the progress strip.
	#[must_use]
	pub fn phase(self) -> usize {
		match self {
			McpWizardStep::Name => 0,
			McpWizardStep::Transport => 1,
			McpWizardStep::Command
			| McpWizardStep::Args
			| McpWizardStep::Url => 2,
			McpWizardStep::AuthMethod
			| McpWizardStep::OAuthError
			| McpWizardStep::OAuthAuthUrl
			| McpWizardStep::OAuthTokenUrl
			| McpWizardStep::OAuthClientId
			| McpWizardStep::OAuthClientSecret
			| McpWizardStep::OAuthScopes
			| McpWizardStep::ApiKey
			| McpWizardStep::AuthLocation
			| McpWizardStep::EnvVarName
			| McpWizardStep::HeaderName => 3,
			McpWizardStep::Scope => 4,
			McpWizardStep::Confirm => 5,
		}
	}

	#[must_use]
	pub fn is_input(self) -> bool {
		matches!(
			self,
			McpWizardStep::Name
				| McpWizardStep::Command
				| McpWizardStep::Args
				| McpWizardStep::Url
				| McpWizardStep::OAuthAuthUrl
				| McpWizardStep::OAuthTokenUrl
				| McpWizardStep::OAuthClientId
				| McpWizardStep::OAuthClientSecret
				| McpWizardStep::OAuthScopes
				| McpWizardStep::ApiKey
				| McpWizardStep::EnvVarName
				| McpWizardStep::HeaderName
		)
	}

	#[must_use]
	pub fn is_choice(self) -> bool {
		matches!(
			self,
			McpWizardStep::Transport
				| McpWizardStep::AuthMethod
				| McpWizardStep::OAuthError
				| McpWizardStep::AuthLocation
				| McpWizardStep::Scope
		)
	}
}

/// The phase names, in strip order.
pub const PHASES: [&str; 6] = ["Name", "Transport", "Connection", "Auth", "Scope", "Review"];

/// The wizard's editable state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WizardState {
	pub name: String,
	pub transport: Option<Transport>,
	pub command: String,
	pub args: String,
	pub url: String,
	pub auth_method: AuthMethod,
	pub oauth_auth_url: String,
	pub oauth_token_url: String,
	pub oauth_registration_url: String,
	pub oauth_issuer_url: String,
	pub oauth_client_id: String,
	pub oauth_client_secret: String,
	pub oauth_scopes: String,
	pub oauth_resource: String,
	pub oauth_resource_is_fallback: bool,
	pub oauth_credential_id: Option<String>,
	pub api_key: String,
	pub auth_location: Option<AuthLocation>,
	pub env_var_name: String,
	pub header_name: String,
	pub scope: Option<Scope>,
}

impl WizardState {
	fn new() -> Self {
		WizardState {
			env_var_name: "API_KEY".to_string(),
			header_name: "Authorization".to_string(),
			..WizardState::default()
		}
	}
}

/// A completed wizard. `config` is what `/mcp` persists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpAddWizardResult {
	pub name: String,
	pub config: McpServerConfig,
	pub scope: Scope,
}

/// Why the last OAuth attempt ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OAuthFailure {
	pub cancelled: bool,
	pub message: String,
	/// Fitted for the terminal sheet.
	pub ansi_message: String,
	pub tip: Option<String>,
}

/// A connection attempt's outcome, kept for the sheet's status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionNote {
	pub ok: bool,
	pub message: String,
}

/// A choice row of a choice step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WizardChoice {
	pub label: String,
	pub description: Option<String>,
	/// Detail shown instead of the description in the native list.
	pub detail: Option<String>,
	pub icon: Option<String>,
	/// Short machine tag shown after the value (`stdio`).
	pub tag: Option<String>,
}

impl WizardChoice {
	#[must_use]
	pub fn new(label: impl Into<String>) -> Self {
		WizardChoice {
			label: label.into(),
			description: None,
			detail: None,
			icon: None,
			tag: None,
		}
	}

	#[must_use]
	pub fn described(mut self, description: impl Into<String>) -> Self {
		self.description = Some(description.into());
		self
	}

	#[must_use]
	pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
		self.icon = Some(icon.into());
		self
	}

	#[must_use]
	pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
		self.tag = Some(tag.into());
		self
	}

	#[must_use]
	pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
		self.detail = Some(detail.into());
		self
	}

	fn list_label(&self) -> String {
		match &self.tag {
			Some(tag) => format!("{} ({tag})", self.label),
			None => self.label.clone(),
		}
	}
}

/// A review row: the label and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewEntry {
	pub key: String,
	pub value: String,
}

/// The wizard.
pub struct McpAddWizard {
	host: Box<dyn McpAddWizardHost>,
	paths: WizardPaths,
	step: McpWizardStep,
	state: WizardState,
	selected_index: usize,
	validation_error: Option<String>,
	oauth_failure: Option<OAuthFailure>,
	connection_note: Option<ConnectionNote>,
	form: Form,
	field_props: FieldSpec,
	choices: Vec<WizardChoice>,
	review: Vec<ReviewEntry>,
	on_complete: Box<dyn FnMut(McpAddWizardResult)>,
	on_cancel: Box<dyn FnMut()>,
	on_render: Box<dyn FnMut()>,
	theme: Theme,
	chrome: OverlayTheme,
}

/// The live text field's presentation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FieldSpec {
	secret: bool,
	placeholder: Option<String>,
	note: Option<String>,
	error: Option<String>,
	optional: bool,
}

impl std::fmt::Debug for McpAddWizard {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("McpAddWizard")
			.field("step", &self.step)
			.field("selected_index", &self.selected_index)
			.finish()
	}
}

impl McpAddWizard {
	/// Build the wizard. `initial_name` skips the name step when it is not
	/// blank, as the reference does.
	pub fn new(
		host: Box<dyn McpAddWizardHost>,
		paths: WizardPaths,
		on_complete: impl FnMut(McpAddWizardResult) + 'static,
		on_cancel: impl FnMut() + 'static,
		on_render: impl FnMut() + 'static,
		initial_name: Option<&str>,
	) -> Self {
		let mut wizard = McpAddWizard {
			host,
			paths,
			step: McpWizardStep::Name,
			state: WizardState::new(),
			selected_index: 0,
			validation_error: None,
			oauth_failure: None,
			connection_note: None,
			form: Form::new(Vec::new(), FormTheme::default()),
			field_props: FieldSpec::default(),
			choices: Vec::new(),
			review: Vec::new(),
			on_complete: Box::new(on_complete),
			on_cancel: Box::new(on_cancel),
			on_render: Box::new(on_render),
			theme: crate::theme::default_theme(),
			chrome: OverlayTheme::default(),
		};
		if let Some(name) = initial_name.map(str::trim).filter(|name| !name.is_empty()) {
			wizard.state.name = name.to_string();
			wizard.step = McpWizardStep::Transport;
		}
		wizard.mount_step();
		wizard
	}

	/// The current step.
	#[must_use]
	pub fn step(&self) -> McpWizardStep {
		self.step
	}

	/// The editable state.
	#[must_use]
	pub fn state(&self) -> &WizardState {
		&self.state
	}

	/// The semantic role reported to the native host.
	#[must_use]
	pub fn native_role(&self) -> &str {
		MCP_ADD_ROLE
	}

	/// The last connection attempt's outcome, if it ran.
	#[must_use]
	pub fn connection_note(&self) -> Option<&ConnectionNote> {
		self.connection_note.as_ref()
	}

	/// Why the last OAuth attempt ended, if one did.
	#[must_use]
	pub fn oauth_failure(&self) -> Option<&OAuthFailure> {
		self.oauth_failure.as_ref()
	}

	/// The choices of the current choice step.
	#[must_use]
	pub fn choices(&self) -> &[WizardChoice] {
		&self.choices
	}

	/// The review rows of the confirm step.
	#[must_use]
	pub fn review(&self) -> &[ReviewEntry] {
		&self.review
	}

	/// The highlighted choice.
	#[must_use]
	pub fn selected_index(&self) -> usize {
		self.selected_index
	}

	fn request_render(&mut self) {
		(self.on_render)();
	}

	fn cancel(&mut self) {
		(self.on_cancel)();
		self.request_render();
	}

	fn complete(&mut self) {
		let Some(scope) = self.state.scope else {
			return;
		};
		let result = McpAddWizardResult {
			name: self.state.name.clone(),
			config: self.build_config(),
			scope,
		};
		(self.on_complete)(result);
		self.request_render();
	}

	/// Mount the current step's field / choices / summary.
	fn mount_step(&mut self) {
		self.validation_error = None;
		self.field_props = FieldSpec::default();
		self.choices.clear();
		self.review.clear();
		let step = self.step;
		if step.is_choice() {
			self.choices = self.choice_options();
			self.selected_index = self.selected_index.min(self.choices.len().saturating_sub(1));
			return;
		}
		if step == McpWizardStep::Confirm {
			self.review = self.review_entries();
			self.form.set_fields(Vec::new());
			return;
		}
		if !step.is_input() {
			self.form.set_fields(Vec::new());
			return;
		}
		let (initial, secret, placeholder, note, optional) = self.input_spec(step);
		self.field_props = FieldSpec {
			secret,
			placeholder: placeholder.clone().or(note.clone()),
			note,
			error: None,
			optional,
		};
		let mut field = FormField::new(
			"value",
			"",
			FormFieldKind::Text {
				value: initial,
				secret,
				prompt: placeholder.map(|hint| format!("{hint}: ")),
			},
		);
		field.empty = if optional {
			EmptyInput::Submit
		} else {
			EmptyInput::Reject
		};
		self.form.set_fields(vec![field]);
	}

	fn input_spec(
		&self,
		step: McpWizardStep,
	) -> (String, bool, Option<String>, Option<String>, bool) {
		let state = &self.state;
		match step {
			McpWizardStep::Name => (
				state.name.clone(),
				false,
				Some("github".to_string()),
				Some("Letters, numbers, dash, underscore, dot and colon".to_string()),
				false,
			),
			McpWizardStep::Command => (
				state.command.clone(),
				false,
				Some("npx".to_string()),
				None,
				false,
			),
			McpWizardStep::Args => (
				state.args.clone(),
				false,
				Some("-y @modelcontextprotocol/server-github".to_string()),
				None,
				true,
			),
			McpWizardStep::Url => (
				state.url.clone(),
				false,
				Some("https://mcp.example.com/mcp".to_string()),
				Some("Starts with http:// or https://".to_string()),
				false,
			),
			McpWizardStep::OAuthAuthUrl => (
				state.oauth_auth_url.clone(),
				false,
				Some("https://auth.example.com/oauth/authorize".to_string()),
				None,
				false,
			),
			McpWizardStep::OAuthTokenUrl => (
				state.oauth_token_url.clone(),
				false,
				Some("https://auth.example.com/oauth/token".to_string()),
				None,
				false,
			),
			McpWizardStep::OAuthClientId => (
				state.oauth_client_id.clone(),
				false,
				None,
				None,
				false,
			),
			McpWizardStep::OAuthClientSecret => (
				state.oauth_client_secret.clone(),
				true,
				None,
				None,
				true,
			),
			McpWizardStep::OAuthScopes => (
				state.oauth_scopes.clone(),
				false,
				Some("read write".to_string()),
				None,
				true,
			),
			McpWizardStep::ApiKey => (
				state.api_key.clone(),
				false,
				Some("!op read op://dev/github/token".to_string()),
				None,
				false,
			),
			McpWizardStep::EnvVarName => (
				state.env_var_name.clone(),
				false,
				Some("API_KEY".to_string()),
				None,
				false,
			),
			McpWizardStep::HeaderName => (
				state.header_name.clone(),
				false,
				Some("Authorization".to_string()),
				None,
				false,
			),
			_ => (String::new(), false, None, None, false),
		}
	}

	fn choice_options(&self) -> Vec<WizardChoice> {
		let state = &self.state;
		match self.step {
			McpWizardStep::Transport => Transport::ALL
				.iter()
				.map(|transport| {
					WizardChoice::new(transport.label())
						.described(transport.description())
						.with_icon(transport.icon())
						.with_tag(transport.as_str())
				})
				.collect(),
			McpWizardStep::AuthMethod => vec![
				WizardChoice::new("Sign in with OAuth")
					.described("Opens your browser")
					.with_icon("log-in"),
				WizardChoice::new("API key or token")
					.described("Paste it, or run a command that prints it")
					.with_icon("key"),
			],
			McpWizardStep::OAuthError => vec![
				WizardChoice::new("Try again")
					.described("Opens the browser again")
					.with_icon("repeat"),
				WizardChoice::new("Edit OAuth settings")
					.described("Endpoints, client and scopes")
					.with_icon("sliders"),
			],
			McpWizardStep::AuthLocation => vec![
				WizardChoice::new("Environment variable")
					.described("Set when the server starts")
					.with_icon("terminal"),
				WizardChoice::new("HTTP header")
					.described("Sent with every request")
					.with_icon("globe"),
			],
			McpWizardStep::Scope => vec![
				WizardChoice::new(Scope::User.label())
					.described(self.paths.label(Scope::User))
					.with_icon("home")
					.with_detail(self.paths.label(Scope::User)),
				WizardChoice::new(Scope::Project.label())
					.described(self.paths.label(Scope::Project))
					.with_icon("folder")
					.with_detail(self.paths.label(Scope::Project)),
			],
			_ => {
				let _ = state;
				Vec::new()
			}
		}
	}

	fn review_entries(&self) -> Vec<ReviewEntry> {
		let state = &self.state;
		let transport = state.transport.unwrap_or(Transport::Stdio);
		let scope = state.scope.unwrap_or(Scope::User);
		let mut entries = vec![
			ReviewEntry {
				key: "Name".to_string(),
				value: state.name.clone(),
			},
			ReviewEntry {
				key: "Transport".to_string(),
				value: format!("{}  {}", transport.label(), transport.as_str()),
			},
		];
		if transport == Transport::Stdio {
			entries.push(ReviewEntry {
				key: "Command".to_string(),
				value: state.command.clone(),
			});
			if !state.args.is_empty() {
				entries.push(ReviewEntry {
					key: "Arguments".to_string(),
					value: state.args.clone(),
				});
			}
		} else {
			entries.push(ReviewEntry {
				key: "URL".to_string(),
				value: state.url.clone(),
			});
		}
		entries.push(ReviewEntry {
			key: "Auth".to_string(),
			value: match state.auth_method {
				AuthMethod::Oauth => "OAuth, signed in".to_string(),
				AuthMethod::Manual => match state.auth_location {
					Some(AuthLocation::Env) => format!("API key in {}", state.env_var_name),
					_ => format!("API key in header {}", state.header_name),
				},
				AuthMethod::None => "None".to_string(),
			},
		});
		entries.push(ReviewEntry {
			key: "Saved in".to_string(),
			value: format!("{}  {}", scope.label(), self.paths.label(scope)),
		});
		entries
	}

	/// The step's title, as the sheet heads it.
	#[must_use]
	pub fn title(&self) -> &'static str {
		match self.step {
			McpWizardStep::Name => "Name the server",
			McpWizardStep::Transport => "How does omp reach it?",
			McpWizardStep::Command => "Command",
			McpWizardStep::Args => "Arguments",
			McpWizardStep::Url => "Server URL",
			McpWizardStep::AuthMethod => "How does it authenticate?",
			McpWizardStep::OAuthError => {
				if self.oauth_failure.as_ref().is_some_and(|failure| failure.cancelled) {
					"Sign-in cancelled"
				} else {
					"Sign-in failed"
				}
			}
			McpWizardStep::OAuthAuthUrl => "Authorization URL",
			McpWizardStep::OAuthTokenUrl => "Token URL",
			McpWizardStep::OAuthClientId => "Client ID",
			McpWizardStep::OAuthClientSecret => "Client secret",
			McpWizardStep::OAuthScopes => "Scopes",
			McpWizardStep::ApiKey => "API key",
			McpWizardStep::AuthLocation => "Where does the key go?",
			McpWizardStep::EnvVarName => "Environment variable",
			McpWizardStep::HeaderName => "Header name",
			McpWizardStep::Scope => "Where should it live?",
			McpWizardStep::Confirm => "Review and save",
		}
	}

	/// The lead under the title.
	#[must_use]
	pub fn lead(&self) -> String {
		let name = &self.state.name;
		match self.step {
			McpWizardStep::Name => {
				"A unique name: it labels the server in /mcp and prefixes its tools."
					.to_string()
			}
			McpWizardStep::Transport => format!("Pick how {name} talks to omp."),
			McpWizardStep::Command => "The program omp starts for this server.".to_string(),
			McpWizardStep::Args => {
				format!("Passed to {}, separated by spaces. omp tries the connection next.", self.state.command)
			}
			McpWizardStep::Url => {
				format!("Where {name} listens. omp tries the connection next.")
			}
			McpWizardStep::AuthMethod => {
				format!("{name} needs credentials before omp can use it.")
			}
			McpWizardStep::OAuthError => {
				if self.oauth_failure.as_ref().is_some_and(|failure| failure.cancelled) {
					"The browser sign-in stopped before it finished.".to_string()
				} else {
					"The provider didn't finish signing you in.".to_string()
				}
			}
			McpWizardStep::OAuthAuthUrl => "The provider's OAuth authorize endpoint.".to_string(),
			McpWizardStep::OAuthTokenUrl => {
				"The endpoint that exchanges the authorization code for a token.".to_string()
			}
			McpWizardStep::OAuthClientId => {
				"The ID of the OAuth app registered with the provider.".to_string()
			}
			McpWizardStep::OAuthClientSecret => "Leave it empty for PKCE-only flows.".to_string(),
			McpWizardStep::OAuthScopes => {
				"What omp may access, separated by spaces. Sign-in starts next.".to_string()
			}
			McpWizardStep::ApiKey => {
				"Paste the key or token. Start with ! to run a command that prints it, e.g. a password manager."
					.to_string()
			}
			McpWizardStep::AuthLocation => "How the server expects to receive it.".to_string(),
			McpWizardStep::EnvVarName => "The variable the server reads its key from.".to_string(),
			McpWizardStep::HeaderName => "The HTTP header that carries the key.".to_string(),
			McpWizardStep::Scope => {
				"Saved for you everywhere, or only in this project's config.".to_string()
			}
			McpWizardStep::Confirm => {
				"omp connects to the server as soon as it is saved.".to_string()
			}
		}
	}

	fn footer(&self) -> String {
		if self.step.is_input() {
			return if self.step == McpWizardStep::Name {
				"[enter to continue, esc to cancel]".to_string()
			} else {
				"[enter to continue, esc to go back]".to_string()
			};
		}
		match self.step {
			McpWizardStep::Confirm => "[enter to save, esc to go back]".to_string(),
			_ => "[up/down to navigate, enter to select, esc to go back]".to_string(),
		}
	}

	// --- value flow -------------------------------------------------------

	fn handle_key(&mut self, key: &Key) -> bool {
		if is_escape(key) {
			self.back_or_cancel();
			return true;
		}
		if self.step.is_input() {
			if is_enter(key) {
				self.save_input_and_proceed();
				return true;
			}
			self.form.handle_input(key);
			self.request_render();
			return true;
		}
		if self.step.is_choice() {
			if is_arrow_up(key) {
				self.move_selection(-1);
			} else if is_arrow_down(key) {
				self.move_selection(1);
			} else if is_enter(key) {
				self.select_current_option();
			}
			return true;
		}
		if self.step == McpWizardStep::Confirm && is_enter(key) {
			self.complete();
			return true;
		}
		false
	}

	fn back_or_cancel(&mut self) {
		if self.step == McpWizardStep::Name {
			self.cancel();
		} else {
			self.go_back();
		}
	}

	fn saved_input(&self) -> String {
		self.form
			.field("value")
			.map(|field| field.value().trim().to_string())
			.unwrap_or_default()
	}

	fn save_input_and_proceed(&mut self) {
		if !self.step.is_input() {
			return;
		}
		let value = self.saved_input();
		match self.step {
			McpWizardStep::Name => {
				self.state.name = value;
				if let Some(error) = self.host.validate_server_name(&self.state.name) {
					self.validation_error = Some(error.clone());
					self.field_props.error = Some(error.clone());
					if let Some(field) = self.form.field_mut("value") {
						field.set_error(Some(error));
					}
					self.request_render();
					return;
				}
				self.step = McpWizardStep::Transport;
				self.selected_index = 0;
			}
			McpWizardStep::Command => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.command = value;
				self.step = McpWizardStep::Args;
			}
			McpWizardStep::Args => {
				self.state.args = value;
				self.test_connection_and_detect_auth();
				return;
			}
			McpWizardStep::Url => {
				self.state.url = value;
				let Some(error) = validate_url(&self.state.url) else {
					self.test_connection_and_detect_auth();
					return;
				};
				self.validation_error = Some(error.clone());
				self.field_props.error = Some(error.clone());
				if let Some(field) = self.form.field_mut("value") {
					field.set_error(Some(error));
				}
				self.request_render();
				return;
			}
			McpWizardStep::OAuthAuthUrl => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.oauth_auth_url = value;
				self.step = McpWizardStep::OAuthTokenUrl;
			}
			McpWizardStep::OAuthTokenUrl => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.oauth_token_url = value;
				self.step = McpWizardStep::OAuthClientId;
			}
			McpWizardStep::OAuthClientId => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.oauth_client_id = value;
				self.step = McpWizardStep::OAuthClientSecret;
			}
			McpWizardStep::OAuthClientSecret => {
				self.state.oauth_client_secret = value;
				self.step = McpWizardStep::OAuthScopes;
			}
			McpWizardStep::OAuthScopes => {
				self.state.oauth_scopes = value;
				self.launch_oauth_flow();
				return;
			}
			McpWizardStep::ApiKey => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.api_key = value;
				if self.state.transport == Some(Transport::Stdio) {
					self.step = McpWizardStep::EnvVarName;
				} else {
					self.step = McpWizardStep::AuthLocation;
					self.selected_index = 0;
				}
			}
			McpWizardStep::EnvVarName => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.env_var_name = value;
				self.state.auth_location = Some(AuthLocation::Env);
				self.step = McpWizardStep::Scope;
				self.selected_index = 0;
			}
			McpWizardStep::HeaderName => {
				if value.is_empty() {
					self.field_props.error = Some(" ".to_string());
					self.request_render();
					return;
				}
				self.state.header_name = value;
				self.state.auth_location = Some(AuthLocation::Header);
				self.step = McpWizardStep::Scope;
				self.selected_index = 0;
			}
			_ => return,
		}
		self.mount_step();
		self.request_render();
	}

	fn select_current_option(&mut self) {
		match self.step {
			McpWizardStep::Transport => {
				self.state.transport = Transport::ALL
					.get(self.selected_index)
					.copied()
					.or(Some(Transport::Stdio));
				self.step = if self.state.transport == Some(Transport::Stdio) {
					McpWizardStep::Command
				} else {
					McpWizardStep::Url
				};
			}
			McpWizardStep::AuthMethod => {
				self.state.auth_method = if self.selected_index == 0 {
					AuthMethod::Oauth
				} else {
					AuthMethod::Manual
				};
				self.step = if self.state.auth_method == AuthMethod::Oauth {
					McpWizardStep::OAuthAuthUrl
				} else {
					McpWizardStep::ApiKey
				};
			}
			McpWizardStep::OAuthError => {
				if self.selected_index == 0 {
					self.launch_oauth_flow();
					return;
				}
				self.step = McpWizardStep::OAuthAuthUrl;
			}
			McpWizardStep::AuthLocation => {
				self.state.auth_location = Some(if self.selected_index == 0 {
					AuthLocation::Env
				} else {
					AuthLocation::Header
				});
				self.step = if self.state.auth_location == Some(AuthLocation::Env) {
					McpWizardStep::EnvVarName
				} else {
					McpWizardStep::HeaderName
				};
			}
			McpWizardStep::Scope => {
				self.state.scope = Some(if self.selected_index == 0 {
					Scope::User
				} else {
					Scope::Project
				});
				self.step = McpWizardStep::Confirm;
				self.selected_index = 0;
			}
			McpWizardStep::Confirm => {
				self.complete();
				return;
			}
			_ => return,
		}
		self.mount_step();
		self.request_render();
	}

	fn move_selection(&mut self, delta: isize) {
		let max = self.choices.len().saturating_sub(1);
		let next = (self.selected_index as isize + delta).rem_euclid(max as isize + 1);
		self.selected_index = next as usize;
		self.request_render();
	}

	fn go_back(&mut self) {
		self.validation_error = None;
		let transport = self.state.transport;
		let connection_step = if transport == Some(Transport::Stdio) {
			McpWizardStep::Args
		} else {
			McpWizardStep::Url
		};
		self.step = match self.step {
			McpWizardStep::Transport => McpWizardStep::Name,
			McpWizardStep::Command | McpWizardStep::Url => {
				self.selected_index = match transport {
					Some(Transport::Stdio) => 0,
					Some(Transport::Http) => 1,
					_ => 2,
				};
				McpWizardStep::Transport
			}
			McpWizardStep::Args => McpWizardStep::Command,
			McpWizardStep::AuthMethod | McpWizardStep::OAuthAuthUrl | McpWizardStep::ApiKey => {
				connection_step
			}
			McpWizardStep::AuthLocation => McpWizardStep::ApiKey,
			McpWizardStep::EnvVarName | McpWizardStep::HeaderName => {
				if transport == Some(Transport::Stdio) {
					McpWizardStep::ApiKey
				} else {
					self.selected_index =
						if self.state.auth_location == Some(AuthLocation::Env) { 0 } else { 1 };
					McpWizardStep::AuthLocation
				}
			}
			McpWizardStep::OAuthTokenUrl => McpWizardStep::OAuthAuthUrl,
			McpWizardStep::OAuthClientId => McpWizardStep::OAuthTokenUrl,
			McpWizardStep::OAuthClientSecret => McpWizardStep::OAuthClientId,
			McpWizardStep::OAuthScopes => McpWizardStep::OAuthClientSecret,
			McpWizardStep::Scope => {
				if self.state.auth_method == AuthMethod::Oauth {
					McpWizardStep::OAuthScopes
				} else if self.state.auth_location == Some(AuthLocation::Env) {
					McpWizardStep::EnvVarName
				} else {
					McpWizardStep::HeaderName
				}
			}
			McpWizardStep::OAuthError => McpWizardStep::OAuthAuthUrl,
			McpWizardStep::Confirm => {
				self.selected_index = if self.state.scope == Some(Scope::User) { 0 } else { 1 };
				McpWizardStep::Scope
			}
			McpWizardStep::Name => McpWizardStep::Name,
		};
		self.mount_step();
		self.request_render();
	}

	/// The config with no auth, as the probe sends it.
	#[must_use]
	pub fn probe_config(&self) -> McpServerConfig {
		self.build_server_config_with_auth(false)
	}

	/// The config saved at the end.
	#[must_use]
	pub fn build_config(&self) -> McpServerConfig {
		let transport = self.state.transport.unwrap_or(Transport::Stdio);
		if transport == Transport::Stdio {
			let mut config = McpServerConfig {
				kind: Transport::Stdio,
				command: self.state.command.clone(),
				args: self
					.state
					.args
					.split_whitespace()
					.map(str::to_string)
					.collect(),
				..McpServerConfig::default()
			};
			if self.state.auth_method == AuthMethod::Oauth {
				config.auth = self.oauth_auth();
			}
			if self.state.auth_method == AuthMethod::Manual && !self.state.api_key.is_empty() {
				let key = if self.state.env_var_name.is_empty() {
					"API_KEY".to_string()
				} else {
					self.state.env_var_name.clone()
				};
				config.env.insert(key, self.state.api_key.clone());
			}
			config
		} else {
			let mut config = McpServerConfig {
				kind: transport,
				url: self.state.url.clone(),
				..McpServerConfig::default()
			};
			if self.state.auth_method == AuthMethod::Oauth {
				config.auth = self.oauth_auth();
			}
			if self.state.auth_method == AuthMethod::Manual && !self.state.api_key.is_empty() {
				let header = if self.state.header_name.is_empty() {
					"Authorization".to_string()
				} else {
					self.state.header_name.clone()
				};
				config.headers.insert(header, self.state.api_key.clone());
			}
			config
		}
	}

	fn oauth_auth(&self) -> Option<McpOAuthAuth> {
		let credential_id = self.state.oauth_credential_id.clone()?;
		Some(McpOAuthAuth {
			credential_id,
			token_url: non_empty(&self.state.oauth_token_url),
			client_id: non_empty(&self.state.oauth_client_id),
			client_secret: non_empty(&self.state.oauth_client_secret),
			resource: non_empty(&self.state.oauth_resource),
		})
	}

	fn build_server_config_with_auth(&self, include_auth: bool) -> McpServerConfig {
		let mut config = self.build_config();
		config.timeout = Some(5000);
		if !include_auth {
			config.auth = None;
			config.env.clear();
			config.headers.clear();
		}
		config
	}

	/// Probe the server; an auth error continues into the auth phase, anything
	/// else keeps the note and moves on to the scope step.
	fn test_connection_and_detect_auth(&mut self) {
		let config = self.probe_config();
		let target = if config.kind == Transport::Stdio {
			let mut text = config.command.clone();
			for arg in &config.args {
				text.push(' ');
				text.push_str(arg);
			}
			text
		} else {
			config.url.clone()
		};
		let error = self.host.test_connection(&config);
		let Err(error) = error else {
			self.state.auth_method = AuthMethod::None;
			self.connection_note = Some(ConnectionNote {
				ok: true,
				message: "✓ No authentication required".to_string(),
			});
			self.step = McpWizardStep::Scope;
			self.selected_index = 0;
			self.mount_step();
			self.request_render();
			return;
		};
		let detection = self
			.host
			.analyze_auth_error(&error, non_empty(&self.state.url).as_deref());
		if !detection.requires_auth {
			self.connection_note = Some(ConnectionNote {
				ok: false,
				message: format!("{} · Adding server anyway…", sanitize(&error.message)),
			});
			self.step = McpWizardStep::Scope;
			self.selected_index = 0;
			self.mount_step();
			self.request_render();
			return;
		}
		let mut oauth = if detection.auth_type == Some(AuthType::Oauth) {
			detection.oauth.clone()
		} else {
			None
		};
		if oauth.is_none() && self.state.transport != Some(Transport::Stdio) && !self.state.url.is_empty() {
			oauth = self.host.discover_oauth_endpoints(
				&self.state.url,
				detection.auth_server_url.as_deref(),
				detection.resource_metadata_url.as_deref(),
				detection.scopes.as_deref(),
			);
		}
		if let Some(endpoints) = &oauth {
			if endpoints.scopes.is_none() {
				if let Some(url) = detection.resource_metadata_url.as_deref() {
					if let Some(scopes) = self.host.fetch_resource_metadata_scopes(url) {
						let mut endpoints = endpoints.clone();
						endpoints.scopes = Some(scopes);
						oauth = Some(endpoints);
					}
				}
			}
		}
		if let Some(endpoints) = oauth {
			self.state.oauth_auth_url = endpoints.authorization_url;
			self.state.oauth_token_url = endpoints.token_url;
			self.state.oauth_registration_url = endpoints.registration_url.unwrap_or_default();
			self.state.oauth_issuer_url = endpoints.issuer_url.unwrap_or_default();
			self.state.oauth_client_id = endpoints.client_id.unwrap_or_default();
			self.state.oauth_scopes = endpoints.scopes.unwrap_or_default();
			let remote = self.state.transport != Some(Transport::Stdio);
			self.state.oauth_resource = endpoints
				.resource
				.unwrap_or_else(|| if remote { self.state.url.clone() } else { String::new() });
			self.state.oauth_resource_is_fallback = remote && !self.state.oauth_resource.is_empty();
			self.state.auth_method = AuthMethod::Oauth;
			self.launch_oauth_flow();
			return;
		}
		let _ = target;
		self.step = McpWizardStep::ApiKey;
		self.mount_step();
		self.request_render();
	}

	fn launch_oauth_flow(&mut self) {
		if self.state.oauth_auth_url.is_empty() || self.state.oauth_token_url.is_empty() {
			self.oauth_failure = Some(OAuthFailure {
				cancelled: false,
				message: "Authorization and Token URLs are required.".to_string(),
				ansi_message: "Authorization and Token URLs are required.".to_string(),
				tip: None,
			});
			self.step = McpWizardStep::OAuthError;
			self.selected_index = 0;
			self.mount_step();
			self.request_render();
			return;
		}
		let resource_is_fallback = self.state.oauth_resource_is_fallback
			|| (self.state.oauth_resource.is_empty()
				&& self.state.transport != Some(Transport::Stdio));
		self.state.oauth_resource_is_fallback = resource_is_fallback;
		let resource = if self.state.oauth_resource.is_empty() {
			if self.state.transport == Some(Transport::Stdio) {
				String::new()
			} else {
				self.state.url.clone()
			}
		} else {
			self.state.oauth_resource.clone()
		};
		let options = OAuthFlowOptions {
			server_url: non_empty(&self.state.url),
			registration_url: non_empty(&self.state.oauth_registration_url),
			issuer_url: non_empty(&self.state.oauth_issuer_url),
			resource: non_empty(&resource),
			strip_same_origin_resource: resource_is_fallback,
		};
		let auth_url = self.state.oauth_auth_url.clone();
		let token_url = self.state.oauth_token_url.clone();
		let client_id = self.state.oauth_client_id.clone();
		let client_secret = self.state.oauth_client_secret.clone();
		let scopes = self.state.oauth_scopes.clone();
		match self.host.start_oauth(
			&auth_url,
			&token_url,
			&client_id,
			&client_secret,
			&scopes,
			&options,
		) {
			Ok(result) => {
				self.state.oauth_credential_id = Some(result.credential_id);
				if let Some(client) = result.client_id {
					self.state.oauth_client_id = client;
				}
				if let Some(resource) = result.resource {
					self.state.oauth_resource = resource;
				}
				let health = self
					.host
					.test_connection(&self.build_server_config_with_auth(true));
				self.connection_note = Some(match health {
					Ok(()) => ConnectionNote {
						ok: true,
						message: "✓ Health check passed".to_string(),
					},
					Err(error) => ConnectionNote {
						ok: false,
						message: format!(
							"⚠ Health check failed (will still save config): {}",
							sanitize(&error.message)
						),
					},
				});
				self.step = McpWizardStep::Scope;
				self.selected_index = 0;
				self.mount_step();
			}
			Err(error) => {
				let tip = if error.cancelled {
					Some("Try again reopens the browser.".to_string())
				} else if error.message.contains("timeout") || error.message.contains("timed out") {
					Some("Complete authorization faster next time.".to_string())
				} else if error.message.contains("Invalid OAuth URLs") {
					Some("Check that the OAuth URLs are correct.".to_string())
				} else if error.message.contains("ECONNREFUSED") {
					Some("Verify the OAuth server is accessible.".to_string())
				} else {
					None
				};
				self.oauth_failure = Some(OAuthFailure {
					cancelled: error.cancelled,
					message: error.message.clone(),
					ansi_message: sanitize(&error.message),
					tip,
				});
				self.step = McpWizardStep::OAuthError;
				self.selected_index = 0;
				self.mount_step();
			}
		}
		self.request_render();
	}

	/// Route enhanced clipboard text into the live field.
	pub fn paste_text(&mut self, text: &str) {
		if self.step.is_input() {
			self.form.handle_paste(text);
			self.request_render();
		}
	}

	/// Footer buttons drive the same paths as their keys.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		match event {
			NativeUiEvent::Action { act, .. } => match act.as_str() {
				"cancel" => self.cancel(),
				"back" => self.back_or_cancel(),
				"next" => {
					if self.step.is_input() {
						self.save_input_and_proceed();
					} else if self.step.is_choice() {
						self.select_current_option();
					} else if self.step == McpWizardStep::Confirm {
						self.complete();
					}
				}
				_ => {}
			},
			NativeUiEvent::Select { key, item }
			| NativeUiEvent::Activate { key, item } => {
				let expected = format!("choices-{}", step_key(self.step));
				if key.last().map(String::as_str) != Some(expected.as_str()) {
					return;
				}
				if !self.step.is_choice() {
					return;
				}
				if let Ok(index) = item.parse::<usize>() {
					if index < self.choices.len() {
						self.selected_index = index;
						self.select_current_option();
					}
				}
			}
			_ => {}
		}
	}
}


fn non_empty(value: &str) -> Option<String> {
	if value.is_empty() {
		None
	} else {
		Some(value.to_string())
	}
}

fn step_key(step: McpWizardStep) -> &'static str {
	match step {
		McpWizardStep::Name => "name",
		McpWizardStep::Transport => "transport",
		McpWizardStep::Command => "command",
		McpWizardStep::Args => "args",
		McpWizardStep::Url => "url",
		McpWizardStep::AuthMethod => "auth-method",
		McpWizardStep::OAuthError => "oauth-error",
		McpWizardStep::OAuthAuthUrl => "oauth-auth-url",
		McpWizardStep::OAuthTokenUrl => "oauth-token-url",
		McpWizardStep::OAuthClientId => "oauth-client-id",
		McpWizardStep::OAuthClientSecret => "oauth-client-secret",
		McpWizardStep::OAuthScopes => "oauth-scopes",
		McpWizardStep::ApiKey => "apikey",
		McpWizardStep::AuthLocation => "auth-location",
		McpWizardStep::EnvVarName => "env-var-name",
		McpWizardStep::HeaderName => "header-name",
		McpWizardStep::Scope => "scope",
		McpWizardStep::Confirm => "confirm",
	}
}

/// Replace tabs and truncate to [`MAX_DISPLAY_WIDTH`].
#[must_use]
pub fn sanitize(text: &str) -> String {
	let expanded = text.replace('\t', "  ");
	if crate::text_shape::visible_width(&expanded) <= MAX_DISPLAY_WIDTH {
		return expanded;
	}
	crate::text_shape::truncate_to_width(
		&expanded,
		MAX_DISPLAY_WIDTH,
		crate::text_shape::Ellipsis::Unicode,
	)
}

/// Why `url` is not usable, if it is not.
#[must_use]
pub fn validate_url(url: &str) -> Option<String> {
	if url.is_empty() {
		return Some("URL is required".to_string());
	}
	let Some((scheme, rest)) = url.split_once("://") else {
		return Some("Invalid URL format (must start with http:// or https://)".to_string());
	};
	if !matches!(scheme, "http" | "https") {
		return Some("URL must use http:// or https:// scheme".to_string());
	}
	if rest.is_empty() {
		return Some("Invalid URL format (must start with http:// or https://)".to_string());
	}
	None
}

impl Component for McpAddWizard {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1);
		let inner = (width as usize).saturating_sub(4).max(1);
		let dim = |value: &str| self.theme.fg("dim", value).unwrap_or_else(|_| value.to_string());
		let accent = |value: &str| self.theme.fg("accent", value).unwrap_or_else(|_| value.to_string());
		let mut rows = vec![
			top_border(width, "Add MCP Server", &self.chrome),
			row("", width, &self.chrome),
			row(
				&dim(&format!("{}/{}", self.step.phase() + 1, PHASES.len())),
				width,
				&self.chrome,
			),
			row(&accent(self.title()), width, &self.chrome),
		];
		for line in wrap_text_with_ansi(&self.lead(), inner) {
			rows.push(row(&line, width, &self.chrome));
		}
		if let Some(note) = &self.connection_note {
			let tone = if note.ok { "success" } else { "warning" };
			let line = self.theme.fg(tone, &note.message).unwrap_or_else(|_| note.message.clone());
			rows.push(row(&line, width, &self.chrome));
		}
		if self.step == McpWizardStep::OAuthError {
			if let Some(failure) = self.oauth_failure.clone().filter(|failure| !failure.cancelled) {
				let line = self
					.theme
					.fg("error", &failure.ansi_message)
					.unwrap_or_else(|_| failure.ansi_message.clone());
				for line in wrap_text_with_ansi(&line, inner) {
					rows.push(row(&line, width, &self.chrome));
				}
			}
			if let Some(tip) = self.oauth_failure.as_ref().and_then(|failure| failure.tip.clone()) {
				rows.push(row(&dim(&format!("Tip: {tip}")), width, &self.chrome));
			}
		}
		if self.step == McpWizardStep::Confirm {
			for entry in &self.review {
				rows.push(row(&format!("{}: {}", entry.key, entry.value), width, &self.chrome));
			}
			rows.push(row("", width, &self.chrome));
			rows.push(row("Save this configuration?", width, &self.chrome));
		}
		if self.step.is_input() {
			if let Some(error) = self.form.field("value").and_then(|field| field.error.clone()) {
				let line = self
					.theme
					.fg("error", &sanitize(&error))
					.unwrap_or_else(|_| error.clone());
				rows.push(row(&format!("✗ {line}"), width, &self.chrome));
			}
			if let Some(note) = self.field_props.note.clone() {
				rows.push(row(&dim(&format!("[{note}]")), width, &self.chrome));
			}
			for line in self.form.render(inner as u16) {
				rows.push(row(&line, width, &self.chrome));
			}
		}
		if self.step.is_choice() {
			for (index, choice) in self.choices.iter().enumerate() {
				let selected = index == self.selected_index;
				let mut line = if selected {
					format!("{} {}", if selected { ">" } else { " " }, choice.list_label())
				} else {
					format!("  {}", choice.list_label())
				};
				if let Some(description) = &choice.description {
					if !selected {
						line.push_str(&dim(&format!("\n    {description}")));
					}
				}
				rows.push(row(&line, width, &self.chrome));
			}
		}
		rows.push(row("", width, &self.chrome));
		rows.push(row(&dim(&self.footer()), width, &self.chrome));
		rows.push(bottom_border(width, &self.chrome));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		self.handle_key(key);
	}

	fn handle_paste(&mut self, text: &str) {
		self.paste_text(text);
	}
}

impl NativeDescribe for McpAddWizard {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let phase = self.step.phase();
		let mut progress_props = Props::new();
		progress_props.set_role("omp.mcpAdd.progress");
		progress_props.set_str("gap", "xs");
		let auth_skipped = phase > 3 && self.state.auth_method == AuthMethod::None;
		let segments: Vec<NativeNode> = PHASES
			.iter()
			.enumerate()
			.map(|(index, label)| {
				let mut props = Props::new();
				props.set_role("omp.mcpAdd.phase");
				let tone = if index == phase {
					"accent"
				} else if index > phase {
					"muted"
				} else if index == 3 && auth_skipped {
					"neutral"
				} else {
					"success"
				};
				props.set_str("tone", tone);
				if index == 3 && auth_skipped {
					props.set_str("title", "Auth · not needed");
				}
				node(
					NodeKind::Col,
					props,
					vec![text(NodeText::plain(*label), Props::new())],
					Some(label),
				)
			})
			.collect();

		let mut head_children = vec![text(
			NodeText::plain(self.title()),
			{
				let mut props = Props::new();
				props.set_role("omp.mcpAdd.title");
				props.set_str("wrap", "word");
				props
			},
		)];
		if self.field_props.optional {
			let mut tag = Props::new();
			tag.set_role("omp.mcpAdd.tag");
			head_children.push(text(NodeText::plain("Optional"), tag));
		}
		let mut head_props = Props::new();
		head_props.set_str("gap", "sm");
		head_props.set_str("align", "center");
		let mut head_col = Props::new();
		head_col.set_role("omp.mcpAdd.head");
		head_col.set_str("gap", "xs");
		let head = node(
			NodeKind::Col,
			head_col,
			vec![
				node(NodeKind::Row, head_props, head_children, None),
				text(
					NodeText::plain(self.lead()),
					{
						let mut props = Props::new();
						props.set_role("omp.mcpAdd.lead");
						props.set_str("wrap", "word");
						props
					},
				),
			],
			Some("head"),
		);

		let mut children = vec![node(NodeKind::Row, progress_props, segments, Some("progress")), head];
		if self.step.is_input() {
			let mut field_props = Props::new();
			field_props.set_role("omp.mcpAdd.field");
			field_props.set_bool("secret", self.field_props.secret);
			field_props.set_str(
				"value",
				self.form.field("value").map(FormField::value).unwrap_or_default(),
			);
			if let Some(placeholder) = &self.field_props.placeholder {
				field_props.set_str("placeholder", placeholder.as_str());
			}
			if let Some(error) = &self.field_props.error {
				field_props.set_str("tone", "error");
				field_props.set_str("error", error.as_str());
			}
			children.push(node(NodeKind::Input, field_props, Vec::new(), Some("field")));
			if let Some(note) = &self.field_props.note {
				children.push(text(
					NodeText::plain(note.clone()),
					{
						let mut props = Props::new();
						props.set_role("omp.mcpAdd.note");
						props.set_str("wrap", "word");
						props
					},
				));
			}
		}
		if self.step.is_choice() {
			let items: Vec<NativeNode> = self
				.choices
				.iter()
				.enumerate()
				.map(|(index, choice)| {
					let mut props = Props::new();
					props.set_str("label", choice.label.as_str());
					if let Some(detail) = choice.detail.as_ref().or(choice.description.as_ref()) {
						props.set_str("detail", detail.as_str());
					}
					if let Some(icon) = &choice.icon {
						props.set_str("icon", icon.as_str());
					}
					if let Some(tag) = &choice.tag {
						props.set_spans(vec![Span::styled(tag.clone(), "mono")]);
					}
					item(index.to_string(), props)
				})
				.collect();
			let mut list_props = Props::new();
			list_props.set_str("selected", self.selected_index.to_string());
			list_props.set_role("omp.mcpAdd.choices");
			children.push(node(
				NodeKind::List,
				list_props,
				items,
				Some(&format!("choices-{}", step_key(self.step))),
			));
		}
		if self.step == McpWizardStep::Confirm {
			let items: Vec<serde_json::Value> = self
				.review
				.iter()
				.map(|entry| serde_json::json!({ "k": entry.key, "v": entry.value }))
				.collect();
			let mut props = Props::new();
			props.set_role("omp.mcpAdd.summary");
			props.insert("items", serde_json::Value::Array(items));
			children.push(node(NodeKind::Kv, props, Vec::new(), Some("summary")));
		}

		let cancel_key = if self.step == McpWizardStep::Name { "esc" } else { "esc" };
		let cancel = action_button(
			"Cancel",
			"cancel",
			ActionButtonOptions {
				keys: Some(cancel_key.to_string()),
				..ActionButtonOptions::default()
			},
		);
		let mut buttons = vec![Some(cancel), None];
		if self.step != McpWizardStep::Name {
			buttons.push(Some(action_button(
				"Back",
				"back",
				ActionButtonOptions {
					keys: Some("esc".to_string()),
					..ActionButtonOptions::default()
				},
			)));
		}
		if self.step.is_input() || self.step.is_choice() || self.step == McpWizardStep::Confirm {
			let label = if self.step == McpWizardStep::Confirm {
				"Save server"
			} else {
				"Continue"
			};
			buttons.push(Some(action_button(
				label,
				"next",
				ActionButtonOptions {
					keys: Some("enter".to_string()),
					tone: Some(ActionButtonTone::Accent),
					..ActionButtonOptions::default()
				},
			)));
		}
		children.push(action_bar(&buttons, "actions"));

		let mut body_props = Props::new();
		body_props.set_str("gap", "lg");
		let mut sheet_props = Props::new();
		sheet_props.set_str("anchor", "center");
		sheet_props.set_str("size", "md");
		sheet_props.set_bool("modal", true);
		overlay_card(
			MCP_ADD_ROLE,
			Some(NodeText::plain("Add MCP server")),
			vec![node(NodeKind::Col, body_props, children, Some("sheet"))],
			Some(sheet_props),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct FakeHost {
		names: Vec<String>,
		test: Option<Result<(), WizardError>>,
		/// Returned for every probe after the first (health checks).
		after: Option<Result<(), WizardError>>,
		probes: usize,
		detection: AuthDetection,
		oauth: Option<McpOAuthEndpoints>,
		oauth_result: Option<Result<OAuthResult, WizardError>>,
		discovered: usize,
	}

	impl Default for FakeHost {
		fn default() -> Self {
			FakeHost {
				names: Vec::new(),
				test: Some(Ok(())),
				after: None,
				probes: 0,
				detection: AuthDetection::default(),
				oauth: None,
				oauth_result: None,
				discovered: 0,
			}
		}
	}

	impl McpAddWizardHost for FakeHost {
		fn validate_server_name(&self, name: &str) -> Option<String> {
			if name.contains(' ') {
				Some("No spaces".to_string())
			} else {
				None
			}
		}

		fn analyze_auth_error(&self, _error: &WizardError, _server_url: Option<&str>) -> AuthDetection {
			self.detection.clone()
		}

		fn discover_oauth_endpoints(
			&mut self,
			_server_url: &str,
			_auth_server_url: Option<&str>,
			_resource_metadata_url: Option<&str>,
			_protected_scopes: Option<&str>,
		) -> Option<McpOAuthEndpoints> {
			self.discovered += 1;
			self.oauth.clone()
		}

		fn fetch_resource_metadata_scopes(&mut self, _url: &str) -> Option<String> {
			None
		}

		fn test_connection(&mut self, _config: &McpServerConfig) -> Result<(), WizardError> {
			self.probes += 1;
			if self.probes > 1 {
				if let Some(after) = &self.after {
					return after.clone();
				}
			}
			self.test.clone().unwrap_or(Ok(()))
		}

		fn start_oauth(
			&mut self,
			_auth_url: &str,
			_token_url: &str,
			_client_id: &str,
			_client_secret: &str,
			_scopes: &str,
			_options: &OAuthFlowOptions,
		) -> Result<OAuthResult, WizardError> {
			self.oauth_result
				.clone()
				.unwrap_or_else(|| Ok(OAuthResult {
					credential_id: "cred-1".to_string(),
					client_id: None,
					resource: None,
				}))
		}
	}

	fn wizard(host: FakeHost) -> (McpAddWizard, std::rc::Rc<std::cell::RefCell<Vec<McpAddWizardResult>>>, std::rc::Rc<std::cell::RefCell<usize>>) {
		let completed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let cancelled = std::rc::Rc::new(std::cell::RefCell::new(0));
		let sink = completed.clone();
		let cancel_sink = cancelled.clone();
		let wizard = McpAddWizard::new(
			Box::new(host),
			WizardPaths {
				user: "~/.omp/mcp.json".to_string(),
				project: ".omp/mcp.json".to_string(),
			},
			move |result| sink.borrow_mut().push(result),
			move || *cancel_sink.borrow_mut() += 1,
			|| {},
			None,
		);
		(wizard, completed, cancelled)
	}

	fn type_text(wizard: &mut McpAddWizard, text: &str) {
		wizard.paste_text(text);
	}

	#[test]
	fn stdio_flow_builds_a_command_config() {
		let (mut wizard, completed, _cancelled) = wizard(FakeHost::default());
		type_text(&mut wizard, "github");
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::Transport);
		wizard.handle_input(&Key::Enter); // stdio
		assert_eq!(wizard.step(), McpWizardStep::Command);
		type_text(&mut wizard, "npx");
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::Args);
		type_text(&mut wizard, "-y server-github");
		wizard.handle_input(&Key::Enter);
		// Probe succeeds: scope.
		assert_eq!(wizard.step(), McpWizardStep::Scope);
		assert!(wizard.connection_note().is_some_and(|note| note.ok));
		wizard.handle_input(&Key::Enter); // user scope
		assert_eq!(wizard.step(), McpWizardStep::Confirm);
		assert_eq!(
			wizard.review()[0],
			ReviewEntry { key: "Name".to_string(), value: "github".to_string() }
		);
		wizard.handle_input(&Key::Enter);
		let results = completed.borrow();
		assert_eq!(results.len(), 1);
		let result = &results[0];
		assert_eq!(result.name, "github");
		assert_eq!(result.scope, Scope::User);
		assert_eq!(result.config.kind, Transport::Stdio);
		assert_eq!(result.config.command, "npx");
		assert_eq!(result.config.args, vec!["-y", "server-github"]);
	}

	#[test]
	fn an_invalid_name_stays_on_the_name_step() {
		let (mut wizard, _completed, _cancelled) = wizard(FakeHost::default());
		type_text(&mut wizard, "has space");
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::Name);
		assert!(wizard.validation_error.is_some());
	}

	#[test]
	fn a_remote_url_must_be_http() {
		let (mut wizard, _completed, _cancelled) = wizard(FakeHost::default());
		wizard.state.name = "x".to_string();
		wizard.step = McpWizardStep::Url;
		wizard.mount_step();
		type_text(&mut wizard, "ftp://example.test/mcp");
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::Url);
		assert_eq!(
			wizard.validation_error.as_deref(),
			Some("URL must use http:// or https:// scheme")
		);
	}

	#[test]
	fn a_manual_api_key_lands_in_env_for_stdio() {
		let (mut wizard, completed, _cancelled) = wizard(FakeHost::default());
		wizard.state.name = "x".to_string();
		wizard.state.transport = Some(Transport::Stdio);
		wizard.state.auth_method = AuthMethod::Manual;
		wizard.state.api_key = "sk-1".to_string();
		wizard.step = McpWizardStep::EnvVarName;
		wizard.mount_step();
		type_text(&mut wizard, "GITHUB_TOKEN");
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::Scope);
		wizard.handle_input(&Key::Enter);
		wizard.handle_input(&Key::Enter);
		let result = completed.borrow()[0].clone();
		assert_eq!(
			result.config.env.get("GITHUB_TOKEN").map(String::as_str),
			Some("sk-1")
		);
	}

	#[test]
	fn a_manual_api_key_lands_in_headers_for_http() {
		let (mut wizard, completed, _cancelled) = wizard(FakeHost::default());
		wizard.state.name = "x".to_string();
		wizard.state.transport = Some(Transport::Http);
		wizard.state.url = "https://example.test/mcp".to_string();
		wizard.state.auth_method = AuthMethod::Manual;
		wizard.state.api_key = "sk-2".to_string();
		wizard.step = McpWizardStep::HeaderName;
		wizard.mount_step();
		type_text(&mut wizard, "X-Api-Key");
		wizard.handle_input(&Key::Enter);
		wizard.handle_input(&Key::Enter);
		wizard.handle_input(&Key::Enter);
		let result = completed.borrow()[0].clone();
		assert_eq!(
			result.config.headers.get("X-Api-Key").map(String::as_str),
			Some("sk-2")
		);
	}

	#[test]
	fn an_auth_error_detects_oauth_and_signs_in() {
		let mut host = FakeHost::default();
		host.test = Some(Err(WizardError::new("401 unauthorized")));
		host.after = Some(Ok(()));
		host.detection = AuthDetection {
			requires_auth: true,
			auth_type: Some(AuthType::Oauth),
			oauth: Some(McpOAuthEndpoints {
				authorization_url: "https://auth.test/authorize".to_string(),
				token_url: "https://auth.test/token".to_string(),
				scopes: Some("read".to_string()),
				..McpOAuthEndpoints::default()
			}),
			..AuthDetection::default()
		};
		host.oauth_result = Some(Ok(OAuthResult {
			credential_id: "cred-9".to_string(),
			client_id: Some("client-9".to_string()),
			resource: None,
		}));
		let (mut wizard, completed, _cancelled) = wizard(host);
		wizard.state.name = "remote".to_string();
		wizard.state.transport = Some(Transport::Http);
		wizard.state.url = "https://example.test/mcp".to_string();
		wizard.step = McpWizardStep::Url;
		wizard.mount_step();
		type_text(&mut wizard, "https://example.test/mcp");
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::Scope);
		assert_eq!(wizard.state.auth_method, AuthMethod::Oauth);
		assert_eq!(wizard.state.oauth_credential_id.as_deref(), Some("cred-9"));
		assert_eq!(wizard.state.oauth_client_id, "client-9");
		assert!(wizard.connection_note().is_some_and(|note| note.ok));
		wizard.handle_input(&Key::Enter);
		wizard.handle_input(&Key::Enter);
		let config = completed.borrow()[0].config.clone();
		assert_eq!(
			config.auth.as_ref().map(|auth| auth.credential_id.as_str()),
			Some("cred-9")
		);
		assert_eq!(config.to_json()["auth"]["clientId"], "client-9");
	}

	#[test]
	fn an_oauth_failure_lands_on_the_retry_or_edit_step() {
		let mut host = FakeHost::default();
		host.oauth_result = Some(Err(WizardError::new("timeout waiting for approval")));
		let (mut wizard, _completed, _cancelled) = wizard(host);
		wizard.state.name = "remote".to_string();
		wizard.state.transport = Some(Transport::Http);
		wizard.state.url = "https://example.test/mcp".to_string();
		wizard.state.oauth_auth_url = "https://auth.test/authorize".to_string();
		wizard.state.oauth_token_url = "https://auth.test/token".to_string();
		wizard.step = McpWizardStep::OAuthScopes;
		wizard.mount_step();
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::OAuthError);
		let failure = wizard.oauth_failure().expect("failure");
		assert_eq!(failure.tip.as_deref(), Some("Complete authorization faster next time."));
		assert_eq!(wizard.choices().len(), 2);
		// Edit settings goes back to the authorize URL step.
		wizard.handle_input(&Key::ArrowDown);
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::OAuthAuthUrl);
	}

	#[test]
	fn cancelled_oauth_reads_as_a_neutral_heading() {
		let mut host = FakeHost::default();
		host.oauth_result = Some(Err(WizardError::cancelled("MCP OAuth flow cancelled by user")));
		let (mut wizard, _completed, _cancelled) = wizard(host);
		wizard.state.oauth_auth_url = "https://auth.test/authorize".to_string();
		wizard.state.oauth_token_url = "https://auth.test/token".to_string();
		wizard.step = McpWizardStep::OAuthScopes;
		wizard.mount_step();
		wizard.handle_input(&Key::Enter);
		assert_eq!(wizard.step(), McpWizardStep::OAuthError);
		assert_eq!(wizard.title(), "Sign-in cancelled");
		assert_eq!(
			wizard.oauth_failure().and_then(|failure| failure.tip.clone()).as_deref(),
			Some("Try again reopens the browser.")
		);
	}

	#[test]
	fn going_back_walks_the_same_path() {
		let (mut wizard, _completed, _cancelled) = wizard(FakeHost::default());
		wizard.state.name = "github".to_string();
		wizard.state.transport = Some(Transport::Http);
		wizard.step = McpWizardStep::Url;
		wizard.mount_step();
		wizard.handle_input(&Key::Esc);
		assert_eq!(wizard.step(), McpWizardStep::Transport);
		assert_eq!(wizard.selected_index(), 1);
		wizard.handle_input(&Key::Esc);
		assert_eq!(wizard.step(), McpWizardStep::Name);
		wizard.handle_input(&Key::Esc);
		assert_eq!(*_cancelled.borrow(), 1);
	}

	#[test]
	fn initial_name_skips_the_name_step() {
		let completed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = completed.clone();
		let wizard = McpAddWizard::new(
			Box::new(FakeHost::default()),
			WizardPaths::default(),
			move |result| sink.borrow_mut().push(result),
			|| {},
			|| {},
			Some("  linear "),
		);
		assert_eq!(wizard.step(), McpWizardStep::Transport);
		assert_eq!(wizard.state().name, "linear");
	}

	#[test]
	fn native_sheet_carries_the_progress_and_lists() {
		let (wizard, _completed, _cancelled) = wizard(FakeHost::default());
		let described = wizard.describe_native(&DescribeContext::new(80));
		let mut kinds = Vec::new();
		let mut stack = vec![&described];
		while let Some(node) = stack.pop() {
			kinds.push(node.kind);
			stack.extend(node.children.iter());
		}
		assert!(kinds.contains(&NodeKind::Overlay));
		assert!(kinds.contains(&NodeKind::Input));
	}

	#[test]
	fn validation_helpers_match_the_reference_messages() {
		assert_eq!(validate_url(""), Some("URL is required".to_string()));
		assert_eq!(
			validate_url("example.test"),
			Some("Invalid URL format (must start with http:// or https://)".to_string())
		);
		assert_eq!(
			validate_url("ftp://example.test"),
			Some("URL must use http:// or https:// scheme".to_string())
		);
		assert_eq!(validate_url("https://example.test/mcp"), None);
		assert_eq!(sanitize("a\tb"), "a  b");
	}
}
