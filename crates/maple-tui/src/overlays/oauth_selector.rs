//! Reference: `l/src/overlays/oauth-selector.ts`.
//!
//! OAuth provider selector for `/login` and `/logout`: a searchable provider
//! list with credential state and provenance, a scroll window centred on the
//! selection, and a native picker/card describing the same list.
//!
//! Deviations:
//! - The reference reads the provider catalogue from `getOAuthProviders()` and
//!   credentials from `pi-ai`; the crate has no such port, so the providers and
//!   a [`OAuthSelectorAuthSource`] are injected.
//! - Credential validation is synchronous (`validate_all(fn(&str) -> bool)`)
//!   instead of a promise per provider; the spinner is advanced by the host
//!   through [`OAuthSelectorComponent::tick_spinner`].
//! - Rows are painted by this component over [`MenuSelection`] (filtering,
//!   selection movement and the scroll window come from its ported state);
//!   there is no `ScrollView`/`TruncatedText` pair.
//! - Mouse routing (`routeMouse`, hover bands) has no crate counterpart; a
//!   native `select`/`activate` event stands in for the click.
//! - The process-global theme and keybindings are injected.

use std::collections::BTreeMap;

use crate::chrome::keybinding_hints::{editor_key, editor_keys};
use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::components::Input;
use crate::components::menu_selection::{MenuOption, MenuSelection, MenuTheme};
use crate::components::scroll_viewport::centered_viewport_range;
use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{ActionButtonOptions, ActionButtonTone, action_bar, action_button, overlay_card};
use crate::theme::Theme;

/// Visible provider rows.
pub const OAUTH_SELECTOR_MAX_VISIBLE: usize = 10;

/// Rendered lines before the provider rows (the top border).
pub const LIST_ROW_OFFSET: usize = 1;

/// Role of the native card.
pub const OAUTH_SELECTOR_ROLE: &str = "omp.overlay.oauth";

/// Whether the selector signs in or removes credentials.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OAuthSelectorMode {
	/// Pick a provider to sign in.
	#[default]
	Login,
	/// Pick a provider whose credentials to remove.
	Logout,
}

impl OAuthSelectorMode {
	/// Panel title.
	pub fn title(self) -> &'static str {
		match self {
			OAuthSelectorMode::Login => "Select provider to login",
			OAuthSelectorMode::Logout => "Select provider to logout",
		}
	}

	/// Sheet heading.
	pub fn sheet_title(self) -> &'static str {
		match self {
			OAuthSelectorMode::Login => "Sign in",
			OAuthSelectorMode::Logout => "Sign out",
		}
	}

	/// Sheet subtitle.
	pub fn sheet_subtitle(self) -> &'static str {
		match self {
			OAuthSelectorMode::Login => "Pick a provider",
			OAuthSelectorMode::Logout => "Remove stored credentials",
		}
	}

	/// The confirming action's label.
	pub fn confirm_label(self) -> &'static str {
		match self {
			OAuthSelectorMode::Login => "Sign in",
			OAuthSelectorMode::Logout => "Sign out",
		}
	}
}

/// Where a provider's credential came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialOriginKind {
	/// The `--api-key` runtime flag.
	Runtime,
	/// `config.toml`.
	Config,
	/// An OAuth login.
	OAuth,
	/// An API-key entry.
	ApiKey,
	/// An environment variable.
	Env,
}

impl CredentialOriginKind {
	/// Compact human-readable tag for the origin.
	pub fn label(self) -> &'static str {
		match self {
			CredentialOriginKind::Runtime => "--api-key",
			CredentialOriginKind::Config => "config",
			CredentialOriginKind::OAuth => "login",
			CredentialOriginKind::ApiKey => "api key",
			CredentialOriginKind::Env => "env",
		}
	}
}

/// A credential's origin, with the variable name when it came from the
/// environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialOrigin {
	/// Where the credential lives.
	pub kind: CredentialOriginKind,
	/// The environment variable, for [`CredentialOriginKind::Env`].
	pub env_var: Option<String>,
}

impl CredentialOrigin {
	/// A credential from an environment variable.
	pub fn env(env_var: impl Into<String>) -> Self {
		CredentialOrigin { kind: CredentialOriginKind::Env, env_var: Some(env_var.into()) }
	}

	/// A credential from anywhere else.
	pub fn new(kind: CredentialOriginKind) -> Self {
		CredentialOrigin { kind, env_var: None }
	}

	/// The provenance text (`env: TOKEN`, `login`, …).
	pub fn detail(&self) -> String {
		match (&self.kind, &self.env_var) {
			(CredentialOriginKind::Env, Some(env_var)) => format!("env: {env_var}"),
			(kind, _) => kind.label().to_string(),
		}
	}
}

/// Credential presence and provenance needed by the provider picker.
pub trait OAuthSelectorAuthSource {
	/// Whether credentials are stored for `provider_id`.
	fn has_credential(&self, provider_id: &str) -> bool;

	/// Where the provider's credential comes from, if it has one.
	fn source(&self, provider_id: &str) -> Option<CredentialOrigin>;
}

/// One login provider as the selector lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthProviderInfo {
	/// Provider id.
	pub id: String,
	/// Display name.
	pub name: String,
	/// Whether the provider can run in this environment.
	pub available: bool,
	/// Provider id its credentials are stored under, when it differs.
	pub store_credentials_as: Option<String>,
}

impl OAuthProviderInfo {
	/// An available provider.
	pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
		OAuthProviderInfo {
			id: id.into(),
			name: name.into(),
			available: true,
			store_credentials_as: None,
		}
	}

	/// Mark the provider unavailable in this environment.
	pub fn unavailable(mut self) -> Self {
		self.available = false;
		self
	}
}

/// Sheet size the host asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SheetSize {
	/// Medium.
	#[default]
	Md,
	/// Large.
	Lg,
}

impl SheetSize {
	/// The size id as the wire spells it.
	pub fn as_str(self) -> &'static str {
		match self {
			SheetSize::Md => "md",
			SheetSize::Lg => "lg",
		}
	}
}

/// The Esc action a host offers in place of Close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaveAction {
	/// Action id sent by the button.
	pub id: String,
	/// Button label.
	pub label: String,
}

/// How a host other than `/login` presents the native provider sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthSelectorSheet {
	/// Sheet heading.
	pub title: String,
	/// Sheet subtitle.
	pub subtitle: String,
	/// Sheet role.
	pub role: String,
	/// Sheet size.
	pub size: SheetSize,
	/// Status line docked above the action bar.
	pub status: Option<String>,
	/// The action that leaves, cancelling like Esc.
	pub leave: Option<LeaveAction>,
}

/// Credential validation state of one provider row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthState {
	/// Validation is running.
	Checking,
	/// The credential works.
	Valid,
	/// The credential is rejected.
	Invalid,
}

impl AuthState {
	/// The state id as the wire spells it.
	pub fn as_str(self) -> &'static str {
		match self {
			AuthState::Checking => "checking",
			AuthState::Valid => "valid",
			AuthState::Invalid => "invalid",
		}
	}
}

/// Construction options of [`OAuthSelectorComponent`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OAuthSelectorOptions {
	/// Provider ids hidden from the login list.
	pub disabled_providers: Vec<String>,
	/// A host-specific native sheet presentation.
	pub sheet: Option<OAuthSelectorSheet>,
}

/// OAuth provider selector.
pub struct OAuthSelectorComponent {
	mode: OAuthSelectorMode,
	providers: Vec<OAuthProviderInfo>,
	auth: Box<dyn OAuthSelectorAuthSource>,
	on_select: Option<Box<dyn FnMut(String)>>,
	on_cancel: Option<Box<dyn Fn()>>,
	search: Input,
	menu: MenuSelection,
	max_visible: usize,
	scroll_start: usize,
	visible_count: usize,
	auth_state: BTreeMap<String, AuthState>,
	spinner_frame: usize,
	status_message: Option<String>,
	sheet: Option<OAuthSelectorSheet>,
	theme: Theme,
	bindings: KeybindingsManager,
	panel_theme: OverlayTheme,
}

impl OAuthSelectorComponent {
	/// A selector over `providers`, answering `on_select`/`on_cancel`.
	pub fn new(
		theme: Theme,
		bindings: KeybindingsManager,
		mode: OAuthSelectorMode,
		providers: Vec<OAuthProviderInfo>,
		auth: Box<dyn OAuthSelectorAuthSource>,
		on_select: impl FnMut(String) + 'static,
		on_cancel: impl Fn() + 'static,
		options: OAuthSelectorOptions,
	) -> Self {
		let mut selector = OAuthSelectorComponent {
			mode,
			providers: Vec::new(),
			auth,
			on_select: Some(Box::new(on_select)),
			on_cancel: Some(Box::new(on_cancel)),
			search: Input::new(),
			menu: MenuSelection::new(Vec::new(), OAUTH_SELECTOR_MAX_VISIBLE, MenuTheme::default()),
			max_visible: OAUTH_SELECTOR_MAX_VISIBLE,
			scroll_start: 0,
			visible_count: 0,
			auth_state: BTreeMap::new(),
			spinner_frame: 0,
			status_message: None,
			sheet: options.sheet,
			theme,
			bindings,
			panel_theme: OverlayTheme::default(),
		};
		selector.load_providers(providers, &options.disabled_providers);
		selector.update_window();
		selector
	}

	/// The sign-in/out mode.
	pub fn mode(&self) -> OAuthSelectorMode {
		self.mode
	}

	/// The panel title.
	pub fn title(&self) -> &'static str {
		self.mode.title()
	}

	/// The provider menu (items, query, selection).
	pub fn menu(&self) -> &MenuSelection {
		&self.menu
	}

	/// The provider menu, for hosts that drive it directly.
	pub fn menu_mut(&mut self) -> &mut MenuSelection {
		&mut self.menu
	}

	/// The search field's current text.
	pub fn search_value(&self) -> &str {
		self.search.value()
	}

	/// Credential validation state of `provider_id`.
	pub fn auth_state(&self, provider_id: &str) -> Option<AuthState> {
		self.auth_state.get(provider_id).copied()
	}

	/// The transient warning under the list (an unavailable pick, a failed save).
	pub fn status_message(&self) -> Option<&str> {
		self.status_message.as_deref()
	}

	/// The host-specific sheet, when one was supplied.
	pub fn sheet(&self) -> Option<&OAuthSelectorSheet> {
		self.sheet.as_ref()
	}

	/// Replace the sheet presentation.
	pub fn set_sheet(&mut self, sheet: OAuthSelectorSheet) {
		self.sheet = Some(sheet);
	}

	/// The visible row budget.
	pub fn max_visible(&self) -> usize {
		self.max_visible
	}

	/// The first visible menu index.
	pub fn scroll_start(&self) -> usize {
		self.scroll_start
	}

	/// Advance the checking spinner one frame.
	pub fn tick_spinner(&mut self) {
		let frames = self.theme.spinner_frames().len();
		if frames > 0 {
			self.spinner_frame = (self.spinner_frame + 1) % frames;
		}
	}

	/// Drop all validation state and stop the spinner.
	pub fn stop_validation(&mut self) {
		self.auth_state.clear();
		self.spinner_frame = 0;
	}

	/// Re-read credential state, keeping the query and selection.
	pub fn refresh_auth(&mut self, auth: Box<dyn OAuthSelectorAuthSource>) {
		self.auth = auth;
		self.update_window();
	}

	/// Validate every provider that has credentials, marking rows checking and
	/// then valid/invalid. Returns how many providers were checked.
	pub fn validate_all(&mut self, mut validate: impl FnMut(&str) -> bool) -> usize {
		let ids: Vec<String> = self.menu.options().iter().map(|option| option.id.clone()).collect();
		let mut pending = Vec::new();
		for id in ids {
			if !self.has_selectable_auth(&id) {
				self.auth_state.remove(&id);
				continue;
			}
			self.auth_state.insert(id.clone(), AuthState::Checking);
			pending.push(id);
		}
		for id in &pending {
			let valid = validate(id);
			self.auth_state.insert(id.clone(), if valid { AuthState::Valid } else { AuthState::Invalid });
		}
		pending.len()
	}

	/// Fit the selector into `lines` rows, shrinking the visible window.
	pub fn set_max_height(&mut self, lines: usize) {
		let strict = lines.saturating_sub(LIST_ROW_OFFSET + 2);
		let relaxed = lines.saturating_sub(LIST_ROW_OFFSET + 1);
		let rows = OAUTH_SELECTOR_MAX_VISIBLE.min(strict.max(1).max(relaxed.min(3)));
		if rows == self.max_visible {
			return;
		}
		self.max_visible = rows;
		self.menu.set_max_visible(rows);
		self.update_window();
	}

	/// Whether the provider holds the credential this mode acts on.
	fn has_selectable_auth(&self, provider_id: &str) -> bool {
		match self.mode {
			OAuthSelectorMode::Logout => self.auth.has_credential(provider_id),
			OAuthSelectorMode::Login => self.auth.source(provider_id).is_some(),
		}
	}

	/// Load the provider catalogue into the menu.
	fn load_providers(&mut self, providers: Vec<OAuthProviderInfo>, disabled: &[String]) {
		let filtered: Vec<OAuthProviderInfo> = match self.mode {
			// Logout stays unfiltered by disabled ids: a now-disabled provider
			// may still hold stored credentials worth removing.
			OAuthSelectorMode::Logout => providers
				.into_iter()
				.filter(|provider| self.auth.has_credential(&provider.id))
				.collect(),
			OAuthSelectorMode::Login => providers
				.into_iter()
				.filter(|provider| {
					!disabled.contains(&provider.id)
						&& !provider
							.store_credentials_as
							.as_deref()
							.is_some_and(|alias| disabled.iter().any(|id| id == alias))
				})
				.collect(),
		};
		let options: Vec<MenuOption> = filtered
			.iter()
			.map(|provider| {
				let mut option = MenuOption::new(provider.id.clone(), provider.name.clone())
					.with_description(self.search_text(provider));
				if !provider.available {
					option = option.disabled();
				}
				option
			})
			.collect();
		self.providers = filtered;
		self.menu.set_menu(options);
	}

	/// Search text of a provider row: name, id, credential provenance.
	fn search_text(&self, provider: &OAuthProviderInfo) -> String {
		let mut text = format!("{} {}", provider.name, provider.id);
		if let Some(origin) = self.auth.source(&provider.id) {
			text.push_str(&format!(" logged in authenticated {}", origin.kind.label()));
			if let Some(env_var) = &origin.env_var {
				text.push(' ');
				text.push_str(env_var);
			}
		}
		if !provider.available {
			text.push_str(" unavailable");
		}
		text
	}

	/// The provider row for `provider_id`.
	pub fn provider(&self, provider_id: &str) -> Option<&OAuthProviderInfo> {
		self.providers.iter().find(|provider| provider.id == provider_id)
	}

	/// Apply the search field's value to the filter.
	fn sync_search_query(&mut self) {
		self.menu.set_query(self.search.value());
		self.status_message = None;
		self.update_window();
	}

	/// Whether typing filters: the catalogue overflows the window, or a query
	/// is already present.
	fn search_enabled(&self) -> bool {
		!self.menu.options().is_empty() && self.menu.options().len() > self.max_visible
	}

	/// Whether the search status line renders.
	fn should_render_search_status(&self) -> bool {
		self.search_enabled() || !self.menu.query().is_empty()
	}

	/// Recompute the centred scroll window.
	fn update_window(&mut self) {
		let total = self.menu.visible_options().len();
		let selected = self.menu.selected_index().max(0) as usize;
		let range = centered_viewport_range(selected, total, self.max_visible);
		self.scroll_start = range.start;
		self.visible_count = range.end.saturating_sub(range.start);
	}

	/// The credential status indicator of a row (`checking`, `invalid`,
	/// `logged in`, or empty).
	pub fn status_indicator(&self, provider_id: &str) -> String {
		let source = self.source_suffix(provider_id);
		match self.auth_state.get(provider_id) {
			Some(AuthState::Checking) => {
				let frames = self.theme.spinner_frames();
				let spinner = if frames.is_empty() {
					self.theme.status().pending.clone()
				} else {
					frames[self.spinner_frame % frames.len()].clone()
				};
				self.theme
					.fg("warning", &format!(" {spinner} checking"))
					.unwrap_or_else(|_| format!(" {spinner} checking"))
					+ &source
			}
			Some(AuthState::Invalid) => {
				let error = &self.theme.status().error;
				let invalid = self
					.theme
					.fg("error", &format!(" {error} invalid"))
					.unwrap_or_else(|_| format!(" {error} invalid"));
				invalid + &source
			}
			Some(AuthState::Valid) => self.logged_in(source),
			None if self.has_selectable_auth(provider_id) => self.logged_in(source),
			None => String::new(),
		}
	}

	fn logged_in(&self, source: String) -> String {
		let enabled = &self.theme.status().enabled;
		let label = self
			.theme
			.fg("success", &format!(" {enabled} logged in"))
			.unwrap_or_else(|_| format!(" {enabled} logged in"));
		label + &source
	}

	/// Muted provenance suffix (` (env: TOKEN)`, ` (login)`, …).
	fn source_suffix(&self, provider_id: &str) -> String {
		match self.auth.source(provider_id) {
			Some(origin) => {
				let detail = format!(" ({})", origin.detail());
				self.theme.fg("muted", &detail).unwrap_or(detail)
			}
			None => String::new(),
		}
	}

	/// Styled rows for the visible provider window.
	pub fn provider_rows(&self) -> Vec<String> {
		let items = self.menu.visible_options();
		let mut rows = Vec::with_capacity(self.visible_count);
		for index in self.scroll_start..self.scroll_start + self.visible_count {
			let Some(option) = items.get(index) else { continue };
			let selected = index as isize == self.menu.selected_index();
			let available = self.provider(&option.id).is_some_and(|provider| provider.available);
			let marker = if selected {
				let cursor = self.theme.nav().cursor.clone();
				self.theme.fg("accent", &format!("{cursor} ")).unwrap_or_else(|_| format!("{cursor} "))
			} else {
				"  ".to_string()
			};
			let label = if selected {
				self.theme.fg("accent", &option.label).unwrap_or_else(|_| option.label.clone())
			} else if available {
				format!("  {}", option.label)
			} else {
				self.theme
					.fg("dim", &format!("  {}", option.label))
					.unwrap_or_else(|_| format!("  {}", option.label))
			};
			rows.push(marker + &label + &self.status_indicator(&option.id));
		}
		rows
	}

	/// The empty-list message.
	pub fn empty_text(&self) -> &'static str {
		if self.menu.options().is_empty() {
			match self.mode {
				OAuthSelectorMode::Login => "No OAuth providers available",
				OAuthSelectorMode::Logout => "No stored provider credentials to log out",
			}
		} else {
			"No matching providers"
		}
	}

	/// The "Type to search"/"Search: …" status line.
	fn search_status(&mut self) -> String {
		if self.menu.query().trim().is_empty() {
			return self.theme.fg("muted", "Type to search").unwrap_or_else(|_| "Type to search".to_string());
		}
		let rendered = self.search.render(40).into_iter().next().unwrap_or_default();
		format!("{}{}", self.theme.fg("muted", "Search: ").unwrap_or_default(), rendered)
	}

	/// Confirm the highlighted provider.
	pub fn confirm_selection(&mut self) {
		let Some(selected) = self.menu.selected().cloned() else {
			return;
		};
		let available = self.provider(&selected.id).is_some_and(|provider| provider.available);
		if !available {
			self.status_message = Some("Provider unavailable in this environment.".to_string());
			return;
		}
		self.status_message = None;
		self.stop_validation();
		if let Some(mut on_select) = self.on_select.take() {
			on_select(selected.id);
			self.on_select = Some(on_select);
		}
	}

	/// Dismiss the selector.
	pub fn cancel(&mut self) {
		self.stop_validation();
		if let Some(on_cancel) = self.on_cancel.take() {
			on_cancel();
			self.on_cancel = Some(on_cancel);
		}
	}

	/// Wheel notch: move the selection one row, clamped.
	pub fn handle_wheel(&mut self, delta: isize) {
		if self.menu.visible_options().is_empty() {
			return;
		}
		if self.menu.move_selection(delta, false) {
			self.status_message = None;
			self.update_window();
		}
	}

	/// Route a native action or row selection.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		match event {
			NativeUiEvent::Action { act, .. } => {
				let leave = self.sheet.as_ref().and_then(|sheet| sheet.leave.as_ref()).map(|leave| leave.id.clone());
				if act == "confirm" {
					self.confirm_selection();
				} else if act == "clear" {
					self.search.set_value("");
					self.sync_search_query();
				} else if act == "close" || act == "cancel" || leave.as_deref() == Some(act.as_str()) {
					self.cancel();
				}
			}
			NativeUiEvent::Select { item, .. } | NativeUiEvent::Activate { item, .. } => {
				let Some(index) = self.menu.visible_options().iter().position(|option| &option.id == item) else {
					return;
				};
				self.menu.set_selected_index(index as isize);
				self.status_message = None;
				self.update_window();
				if matches!(event, NativeUiEvent::Activate { .. }) {
					self.confirm_selection();
				}
			}
			_ => {}
		}
	}

	/// The provider rows as native list items carrying their credential state.
	fn describe_items(&self) -> Vec<NativeNode> {
		self.menu
			.visible_options()
			.iter()
			.map(|option| {
				let provider = self.provider(&option.id);
				let mut props = Props::new();
				props.set_str("label", option.label.clone());
				if let Some(origin) = self.auth.source(&option.id) {
					props.insert(
						"detail",
						serde_json::Value::Array(vec![serde_json::Value::String(origin.detail())]),
					);
				}
				if let Some(state) = self.auth_state.get(&option.id) {
					props.set_str("value", state.as_str());
				} else if self.has_selectable_auth(&option.id) {
					props.set_str("value", "logged in");
				}
				if provider.is_some_and(|provider| !provider.available) {
					props.set_str("tone", "muted");
					props.set_str("disabled", "Provider unavailable in this environment");
				}
				node(NodeKind::Item, props, Vec::new(), Some(option.id.as_str()))
			})
			.collect()
	}

	/// The native card: search field, provider list, status and actions.
	pub fn describe(&self, cx: &DescribeContext) -> NativeNode {
		if cx.supports(NodeKind::Picker) {
			return self.describe_picker();
		}
		let mut children: Vec<NativeNode> = Vec::new();
		if self.should_render_search_status() {
			let mut props = Props::new();
			props.set_text(self.search.value().to_string());
			props.set_str("placeholder", "Type to search");
			children.push(node(NodeKind::Input, props, Vec::new(), Some("search")));
		}
		let mut list_props = Props::new();
		if let Some(selected) = self.menu.selected_key() {
			list_props.set_str("selected", selected);
		}
		if !self.menu.query().trim().is_empty() {
			list_props.set_str("filter", self.menu.query().trim().to_string());
		}
		list_props.insert(
			"empty",
			serde_json::Value::Array(vec![serde_json::Value::String(self.empty_text().to_string())]),
		);
		children.push(node(NodeKind::List, list_props, self.describe_items(), Some("list")));
		if let Some(message) = self.status_message.as_deref() {
			children.push(text(
				NodeText::Spans(vec![span(message, Some("warning"))]),
				Props::new(),
			));
		}
		let actions = self.describe_actions();
		children.push(actions);
		overlay_card(
			OAUTH_SELECTOR_ROLE,
			Some(NodeText::Plain(self.title().to_string())),
			children,
			None,
		)
	}

	fn describe_picker(&self) -> NativeNode {
		let login = self.mode == OAuthSelectorMode::Login;
		let sheet = self.sheet.as_ref();
		let title = sheet.map(|sheet| sheet.title.clone()).unwrap_or_else(|| self.mode.sheet_title().to_string());
		let subtitle = sheet
			.map(|sheet| sheet.subtitle.clone())
			.unwrap_or_else(|| self.mode.sheet_subtitle().to_string());
		let mut props = Props::new();
		props.set_str("title", title);
		props.set_head(NodeText::Plain(subtitle));
		props.set_str("icon", "key-round");
		props.set_str("noun", "providers");
		props.set_str("size", sheet.map(|sheet| sheet.size.as_str()).unwrap_or("md"));
		props.set_str("layout", "cards");
		props.set_str("preview", "none");
		props.set_str("placeholder", "Search providers…");
		props.set_i64("total", self.menu.options().len() as i64);
		props.set_text(self.empty_text().to_string());
		if let Some(selected) = self.menu.selected_key() {
			props.set_str("selected", selected);
		}
		if let Some(message) = self.status_message.as_deref() {
			props.set_str("message", message.to_string());
		}
		if let Some(role) = sheet {
			props.set_role(role.role.clone());
		}
		let items = self.describe_items();
		let actions = self.describe_actions();
		node(NodeKind::Picker, props, vec![node(NodeKind::List, Props::new(), items, Some("list")), actions], Some("providers"))
	}

	/// The action bar: confirm, then clear/leave/close.
	fn describe_actions(&self) -> NativeNode {
		let login = self.mode == OAuthSelectorMode::Login;
		let confirm = action_button(
			self.mode.confirm_label(),
			"confirm",
			ActionButtonOptions {
				keys: Some("enter".to_string()),
				tone: Some(if login { ActionButtonTone::Accent } else { ActionButtonTone::Error }),
				..Default::default()
			},
		);
		let leave = match self.sheet.as_ref().and_then(|sheet| sheet.leave.as_ref()) {
			Some(leave) => action_button(
				&leave.label,
				&leave.id,
				ActionButtonOptions { keys: Some("escape".to_string()), ..Default::default() },
			),
			None => action_button(
				"Close",
				"close",
				ActionButtonOptions {
					keys: Some(editor_key(&self.bindings, &self.theme, "tui.select.cancel")),
					..Default::default()
				},
			),
		};
		let clear = if !self.menu.query().is_empty() {
			Some(action_button(
				"Clear search",
				"clear",
				ActionButtonOptions { keys: Some("backspace".to_string()), ..Default::default() },
			))
		} else {
			None
		};
		action_bar(&[Some(confirm), clear, Some(leave)], OAUTH_SELECTOR_ROLE)
	}
}

impl Component for OAuthSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let title = self.title();
		let mut lines = vec![top_border(width, title, &self.panel_theme)];
		for provider_row in self.provider_rows() {
			lines.push(row(&provider_row, width, &self.panel_theme));
		}
		if self.menu.visible_options().is_empty() {
			let empty = self.theme.fg("muted", self.empty_text()).unwrap_or_else(|_| self.empty_text().to_string());
			lines.push(row(&empty, width, &self.panel_theme));
		}
		if self.should_render_search_status() {
			let status = self.search_status();
			lines.push(row(&status, width, &self.panel_theme));
		}
		if let Some(message) = self.status_message.clone() {
			lines.push(row("", width, &self.panel_theme));
			let message = self.theme.fg("warning", &message).unwrap_or(message);
			lines.push(row(&message, width, &self.panel_theme));
		}
		lines.push(bottom_border(width, &self.panel_theme));
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		match key {
			Key::Esc | Key::Ctrl('c') => self.cancel(),
			Key::Up => {
				if self.menu.move_selection(-1, true) {
					self.status_message = None;
					self.update_window();
				}
			}
			Key::Down => {
				if self.menu.move_selection(1, true) {
					self.status_message = None;
					self.update_window();
				}
			}
			Key::PageUp => {
				if self.menu.move_selection(-(self.max_visible as isize), false) {
					self.status_message = None;
					self.update_window();
				}
			}
			Key::PageDown => {
				if self.menu.move_selection(self.max_visible as isize, false) {
					self.status_message = None;
					self.update_window();
				}
			}
			Key::Enter => self.confirm_selection(),
			_ => {
				if !self.search_enabled() {
					return;
				}
				let before = self.search.value().to_string();
				if before.is_empty() && matches!(key, Key::Backspace) {
					return;
				}
				if let Some(ch) = key.as_char() {
					if before.is_empty() && ch.is_whitespace() {
						return;
					}
				}
				self.search.handle_input(key);
				if self.search.value() != before {
					self.sync_search_query();
				}
			}
		}
	}

	fn handle_paste(&mut self, text: &str) {
		if !self.search_enabled() {
			return;
		}
		self.search.handle_paste(text);
		self.sync_search_query();
	}

	fn invalidate(&mut self) {
		self.panel_theme = OverlayTheme::default();
	}
}

impl OAuthSelectorComponent {
	/// The footer hint of the search field: navigation plus the select key.
	pub fn footer_hint(&self) -> String {
		let formatter = KeyHintFormatter::new(&self.theme);
		let nav = editor_keys(&self.bindings, &self.theme, &["tui.select.up", "tui.select.down"]);
		let enter = formatter.format_key_hint("enter");
		let close = format!("{} close", editor_key(&self.bindings, &self.theme, "tui.select.cancel"));
		format!("{nav} providers · {enter} {} · type to search · {close}", self.mode.confirm_label())
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

	struct FakeAuth {
		logged_in: Vec<(&'static str, CredentialOrigin)>,
	}

	impl OAuthSelectorAuthSource for FakeAuth {
		fn has_credential(&self, provider_id: &str) -> bool {
			self.logged_in.iter().any(|(id, _)| *id == provider_id)
		}

		fn source(&self, provider_id: &str) -> Option<CredentialOrigin> {
			self.logged_in.iter().find(|(id, _)| *id == provider_id).map(|(_, origin)| origin.clone())
		}
	}

	fn auth() -> FakeAuth {
		FakeAuth {
			logged_in: vec![
				("anthropic", CredentialOrigin::new(CredentialOriginKind::OAuth)),
				("github", CredentialOrigin::env("GITHUB_TOKEN")),
			],
		}
	}

	fn providers() -> Vec<OAuthProviderInfo> {
		vec![
			OAuthProviderInfo::new("anthropic", "Anthropic"),
			OAuthProviderInfo::new("github", "GitHub"),
			OAuthProviderInfo::new("openai-codex", "OpenAI Codex"),
			OAuthProviderInfo::new("copilot", "Copilot").unavailable(),
		]
	}

	fn selector(
		mode: OAuthSelectorMode,
		options: OAuthSelectorOptions,
	) -> (OAuthSelectorComponent, Rc<RefCell<Vec<String>>>) {
		let selected = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&selected);
		let component = OAuthSelectorComponent::new(
			theme(),
			KeybindingsManager::new(),
			mode,
			providers(),
			Box::new(auth()),
			move |id| sink.borrow_mut().push(id),
			|| {},
			options,
		);
		(component, selected)
	}

	#[test]
	fn origin_details_name_the_environment_variable() {
		assert_eq!(CredentialOrigin::env("GH_TOKEN").detail(), "env: GH_TOKEN");
		assert_eq!(CredentialOrigin::new(CredentialOriginKind::OAuth).detail(), "login");
		assert_eq!(CredentialOrigin::new(CredentialOriginKind::Runtime).detail(), "--api-key");
	}

	#[test]
	fn login_lists_every_provider_and_logout_lists_only_stored_ones() {
		let (login, _) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		assert_eq!(login.menu().options().len(), 4);
		let (logout, _) = selector(OAuthSelectorMode::Logout, OAuthSelectorOptions::default());
		let ids: Vec<&str> = logout.menu().options().iter().map(|option| option.id.as_str()).collect();
		assert_eq!(ids, vec!["anthropic", "github"]);
	}

	#[test]
	fn disabled_ids_and_their_aliases_are_hidden_from_login() {
		let mut providers = providers();
		providers.push(OAuthProviderInfo {
			store_credentials_as: Some("openai-codex".to_string()),
			..OAuthProviderInfo::new("openai-codex-device", "Codex device")
		});
		let selected = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&selected);
		let component = OAuthSelectorComponent::new(
			theme(),
			KeybindingsManager::new(),
			OAuthSelectorMode::Login,
			providers,
			Box::new(auth()),
			move |id| sink.borrow_mut().push(id),
			|| {},
			OAuthSelectorOptions {
				disabled_providers: vec!["openai-codex".to_string()],
				sheet: None,
			},
		);
		let ids: Vec<&str> = component.menu().options().iter().map(|option| option.id.as_str()).collect();
		assert_eq!(ids, vec!["anthropic", "github", "copilot"]);
	}

	#[test]
	fn typing_filters_the_provider_list() {
		let (mut selector, _) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		for ch in "git".chars() {
			selector.handle_input(&Key::Char(ch));
		}
		let ids: Vec<&str> = selector.menu().visible_options().iter().map(|option| option.id.as_str()).collect();
		assert_eq!(ids, vec!["github"]);
		selector.handle_input(&Key::Backspace);
		assert_eq!(selector.menu().visible_options().len(), 4);
	}

	#[test]
	fn enter_selects_the_highlighted_provider() {
		let (mut selector, selected) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		selector.handle_input(&Key::Down);
		selector.handle_input(&Key::Enter);
		assert_eq!(selected.borrow().as_slice(), ["github"]);
	}

	#[test]
	fn unavailable_providers_report_a_status_instead_of_selecting() {
		let (mut selector, selected) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		selector.menu_mut().set_selected_key("copilot");
		selector.handle_input(&Key::Enter);
		assert_eq!(selector.status_message(), Some("Provider unavailable in this environment."));
		assert!(selected.borrow().is_empty());
	}

	#[test]
	fn validation_marks_rows_valid_or_invalid() {
		let (mut selector, _) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		let checked = selector.validate_all(|id| id == "anthropic");
		assert_eq!(checked, 2);
		assert_eq!(selector.auth_state("anthropic"), Some(AuthState::Valid));
		assert_eq!(selector.auth_state("github"), Some(AuthState::Invalid));
		assert_eq!(selector.auth_state("copilot"), None);
		let rows = selector.provider_rows();
		assert!(rows.iter().any(|row| row.contains("logged in")));
		assert!(rows.iter().any(|row| row.contains("invalid")));
	}

	#[test]
	fn escape_cancels_and_drops_validation() {
		let cancelled = Rc::new(RefCell::new(0));
		let sink = Rc::clone(&cancelled);
		let mut component = OAuthSelectorComponent::new(
			theme(),
			KeybindingsManager::new(),
			OAuthSelectorMode::Login,
			providers(),
			Box::new(auth()),
			|_| {},
			move || *sink.borrow_mut() += 1,
			OAuthSelectorOptions::default(),
		);
		component.validate_all(|_| true);
		assert!(component.auth_state("anthropic").is_some());
		component.handle_input(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 1);
		assert!(component.auth_state("anthropic").is_none());
	}

	#[test]
	fn max_height_shrinks_the_window_but_keeps_the_selection_visible() {
		let (mut selector, _) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		selector.set_max_height(4);
		assert_eq!(selector.max_visible(), 2);
		selector.menu_mut().set_selected_index(3);
		selector.update_window();
		assert!(selector.scroll_start() + selector.visible_count > 3);
	}

	#[test]
	fn render_frames_the_rows_and_search_status() {
		let (mut selector, _) = selector(OAuthSelectorMode::Login, OAuthSelectorOptions::default());
		let lines = selector.render(50);
		assert!(lines[0].contains("Select provider to login"));
		assert!(lines.iter().any(|line| line.contains("Anthropic")));
		assert!(!lines.iter().any(|line| line.contains("Type to search")));
		let mut narrow = selector_set_narrow();
		let lines = narrow.render(50);
		assert!(lines.iter().any(|line| line.contains("Type to search")));
	}

	fn selector_set_narrow() -> OAuthSelectorComponent {
		let mut providers = providers();
		for index in 0..12 {
			providers.push(OAuthProviderInfo::new(format!("extra-{index}"), format!("Extra {index}")));
		}
		OAuthSelectorComponent::new(
			theme(),
			KeybindingsManager::new(),
			OAuthSelectorMode::Login,
			providers,
			Box::new(auth()),
			|_| {},
			|| {},
			OAuthSelectorOptions::default(),
		)
	}
}
