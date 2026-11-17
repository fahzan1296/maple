//! Reference: `l/src/overlays/model-picker.ts`.
//!
//! Compact session-model picker (alt+p / `/switch`): a bottom-anchored sheet
//! listing the session's models, a `@`-prefixed quick-role mode, and an
//! optional Task-subagent mode toggled by the picker shortcut.
//!
//! Deviations:
//! - The reference owns a `ModelBrowser` child that renders the search field,
//!   the list window and the two detail rows. The crate has no session-scope
//!   builder, so this component keeps the filtered list itself (query, cursor,
//!   window) and paints one row per model; provider/detail rows are omitted.
//! - The catalogue is read through [`ModelPickerRegistry`] synchronously
//!   ([`ModelPickerRegistry::refresh_if_stale`]) instead of a promise; the host
//!   repaints after a refresh.
//! - Role rows carry no `labelColor` (the crate's `ModelBrowserItem` has no
//!   colour slot) and the segment palette is not resolved.
//! - The process-global theme, keybindings and terminal size are injected
//!   ([`Theme`], [`KeybindingsManager`], [`ModelPickerComponent::set_max_visible`]).
//! - Input arrives as decoded [`Key`] events rather than raw key data strings,
//!   and task-mode keys match through [`key_id`].

use std::collections::HashMap;

use crate::chrome::keybinding_hints::{editor_key, editor_keys, interrupt_key};
use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::{KeybindingsManager, key_id};
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{card, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{NativeHint, action_hint, hints_row};
use crate::setup::scenes::types::{ModelBrowserItem, SetupModel};
use crate::theme::Theme;

/// Fixed chrome rows: top border, status row, footer, bottom border.
pub const CHROME_ROWS: usize = 4;
/// Rows the browser renders around its list window (search plus blank, blank
/// plus two detail rows).
pub const BROWSER_FRAME_ROWS: usize = 5;
/// Minimum rows for the list window on short terminals.
pub const MIN_VISIBLE: usize = 5;
/// Fraction of the terminal height the floating overlay occupies.
pub const HEIGHT_FRACTION: f64 = 0.4;

/// Status line of a session-only switch.
pub const STATUS_HINT: &str = "Session-only switch — role models stay unchanged";
/// Status line of a quick-role switch.
pub const QUICK_ROLE_STATUS_HINT: &str = "Quick role switch — applies its model and thinking for this session";
/// Status line of a Task-subagent switch.
pub const TASK_STATUS_HINT: &str = "Task subagent switch — spawned task agents use this model (session-only)";
/// Status line of a selection-only picker.
pub const SELECT_ONLY_STATUS_HINT: &str = "Selection only — does not change the session model";

/// Role of the native card.
pub const MODEL_PICKER_ROLE: &str = "omp.overlay.model-picker";

/// What the picker acts on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelPickerMode {
	/// Switch the session model.
	#[default]
	Session,
	/// Apply a quick role's model.
	Role,
	/// Switch the model Task subagents use.
	Task,
	/// Report the selection without changing anything.
	Selection,
}

impl ModelPickerMode {
	/// The status line for this mode.
	pub fn status_hint(self) -> &'static str {
		match self {
			ModelPickerMode::Role => QUICK_ROLE_STATUS_HINT,
			ModelPickerMode::Task => TASK_STATUS_HINT,
			ModelPickerMode::Selection => SELECT_ONLY_STATUS_HINT,
			ModelPickerMode::Session => STATUS_HINT,
		}
	}

	/// Sheet title for this mode.
	pub fn title(self) -> &'static str {
		match self {
			ModelPickerMode::Task => "Switch task model",
			ModelPickerMode::Selection => "Select model",
			_ => "Switch model",
		}
	}

	/// The noun the list is counted in.
	pub fn noun(self) -> &'static str {
		match self {
			ModelPickerMode::Role => "roles",
			_ => "models",
		}
	}

	/// What Enter does, for the hint strip and the picker action.
	pub fn confirm_label(self) -> &'static str {
		match self {
			ModelPickerMode::Role => "apply role model",
			ModelPickerMode::Task => "use for Task subagents",
			ModelPickerMode::Selection => "select model",
			ModelPickerMode::Session => "use for this session",
		}
	}
}

/// A configured role resolved to a concrete model.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRoleModel {
	/// Role id (`default`, `smol`, …).
	pub role: String,
	/// The model the role resolves to.
	pub model: SetupModel,
	/// The role's configured thinking level, when it has one.
	pub thinking_level: Option<String>,
	/// Whether the thinking level was set explicitly (not inherited).
	pub explicit_thinking_level: bool,
}

impl ResolvedRoleModel {
	/// The virtual `@role` selector the quick-role row carries.
	pub fn selector(&self) -> String {
		format!("@{}", self.role)
	}
}

/// Catalog access used by the session picker.
pub trait ModelPickerRegistry {
	/// Bring the catalog up to date; `Ok(true)` when the picker must re-read it.
	fn refresh_if_stale(&mut self) -> Result<bool, String>;

	/// Every selectable model, in catalogue order.
	fn items(&self) -> Vec<ModelBrowserItem>;

	/// The model's context window in tokens, when known.
	fn context_window(&self, _selector: &str) -> Option<u64> {
		None
	}
}

/// Host callbacks of [`ModelPickerComponent`].
#[derive(Default)]
pub struct ModelPickerCallbacks {
	/// A model was chosen for a session-only switch; the flag reports whether
	/// the transcript already exceeds the model's context window.
	pub on_pick: Option<Box<dyn FnMut(&SetupModel, &str, bool)>>,
	/// A quick role was chosen.
	pub on_pick_role: Option<Box<dyn FnMut(&ResolvedRoleModel)>>,
	/// A model was chosen for Task subagents.
	pub on_pick_task: Option<Box<dyn FnMut(&SetupModel, &str)>>,
	/// The picker was dismissed.
	pub on_cancel: Option<Box<dyn Fn()>>,
}

/// Construction options of [`ModelPickerComponent`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelPickerOptions {
	/// Session token count; models with smaller windows compact first on pick.
	pub current_context_tokens: Option<u64>,
	/// `provider/id` of the session's active model; highlighted and preselected.
	pub current_selector: Option<String>,
	/// Resolved role models in ctrl+p cycle order.
	pub quick_roles: Vec<ResolvedRoleModel>,
	/// Complete ctrl+p order, including unavailable roles.
	pub quick_role_order: Vec<String>,
	/// Active quick role, highlighted when the search begins with `@`.
	pub current_quick_role: Option<String>,
	/// Key ids that toggle task mode while the picker is open.
	pub task_mode_keys: Vec<String>,
	/// `provider/id` highlighted in task mode.
	pub task_selector: Option<String>,
	/// Present selection-only chrome.
	pub select_only: bool,
}

/// The picker's filtered list for `query`, in catalogue order.
pub fn filter_items<'a>(items: &'a [ModelBrowserItem], query: &str) -> Vec<&'a ModelBrowserItem> {
	let query = query.trim().to_ascii_lowercase();
	if query.is_empty() {
		return items.iter().collect();
	}
	items
		.iter()
		.filter(|item| item.selector.to_ascii_lowercase().contains(&query))
		.collect()
}

/// Compact session-model picker.
pub struct ModelPickerComponent {
	items: Vec<ModelBrowserItem>,
	quick_role_items: Vec<ModelBrowserItem>,
	quick_roles: HashMap<String, ResolvedRoleModel>,
	config_error: Option<String>,
	current_selector: Option<String>,
	current_quick_role_selector: Option<String>,
	task_selector: Option<String>,
	role_mode: bool,
	task_mode: bool,
	task_mode_key: Option<String>,
	select_only: bool,
	query: String,
	cursor: usize,
	max_visible: usize,
	focused: bool,
	current_context_tokens: Option<u64>,
	callbacks: ModelPickerCallbacks,
	registry_context_window: Box<dyn Fn(&str) -> Option<u64>>,
	theme: Theme,
	bindings: KeybindingsManager,
	panel_theme: OverlayTheme,
}

impl ModelPickerComponent {
	/// A picker over `registry`'s current catalog.
	pub fn new(
		theme: Theme,
		bindings: KeybindingsManager,
		registry: &mut dyn ModelPickerRegistry,
		callbacks: ModelPickerCallbacks,
		options: ModelPickerOptions,
	) -> Self {
		let select_only = options.select_only;
		let task_mode_key = if select_only || callbacks.on_pick_task.is_none() {
			None
		} else {
			options.task_mode_keys.first().cloned()
		};
		let mut picker = ModelPickerComponent {
			items: Vec::new(),
			quick_role_items: Vec::new(),
			quick_roles: HashMap::new(),
			config_error: None,
			current_selector: options.current_selector.clone(),
			current_quick_role_selector: options.current_quick_role.as_ref().map(|role| format!("@{role}")),
			task_selector: options.task_selector.clone(),
			role_mode: false,
			task_mode: false,
			task_mode_key,
			select_only,
			query: String::new(),
			cursor: 0,
			max_visible: MIN_VISIBLE,
			focused: false,
			callbacks,
			current_context_tokens: options.current_context_tokens,
			registry_context_window: Box::new(|_| None),
			theme,
			bindings,
			panel_theme: OverlayTheme::default(),
		};
		picker.build_quick_role_items(&options.quick_roles, &options.quick_role_order);
		// Hydrate synchronously from the current snapshot so the first Enter
		// acts on cached models instead of being dropped while a refresh runs.
		picker.reload(registry);
		if let Some(selector) = options.current_selector.as_deref() {
			picker.select_selector(selector);
		}
		picker
	}

	/// Bring the catalog up to date; the host repaints afterwards.
	pub fn refresh(&mut self, registry: &mut dyn ModelPickerRegistry) {
		match registry.refresh_if_stale() {
			Ok(true) => self.reload(registry),
			Ok(false) => {}
			Err(error) => self.config_error = Some(error),
		}
	}

	/// Re-read the registry snapshot and re-filter the list.
	pub fn reload(&mut self, registry: &dyn ModelPickerRegistry) {
		self.items = registry.items();
		self.config_error = None;
		self.clamp_cursor();
	}

	/// The search query.
	pub fn query(&self) -> &str {
		&self.query
	}

	/// Highlighted index within [`Self::visible_items`].
	pub fn cursor(&self) -> usize {
		self.cursor
	}

	/// The active mode.
	pub fn mode(&self) -> ModelPickerMode {
		if self.select_only {
			ModelPickerMode::Selection
		} else if self.task_mode {
			ModelPickerMode::Task
		} else if self.role_mode {
			ModelPickerMode::Role
		} else {
			ModelPickerMode::Session
		}
	}

	/// The highlighted model.
	pub fn selected(&self) -> Option<&ModelBrowserItem> {
		self.visible_items().into_iter().nth(self.cursor)
	}

	/// The rows currently shown, filtered by the query.
	pub fn visible_items(&self) -> Vec<&ModelBrowserItem> {
		let items = if self.role_mode { &self.quick_role_items } else { &self.items };
		filter_items(items, &self.query)
	}

	/// Rows the list window may show.
	pub fn max_visible(&self) -> usize {
		self.max_visible
	}

	/// Set the list window from the terminal height: 40% of `rows`, minus the
	/// chrome and the browser frame, never below [`MIN_VISIBLE`].
	pub fn set_terminal_rows(&mut self, rows: usize) {
		let rows = rows.max(16);
		let budget = (rows as f64 * HEIGHT_FRACTION) as usize;
		self.max_visible = budget.saturating_sub(CHROME_ROWS + BROWSER_FRAME_ROWS).max(MIN_VISIBLE);
	}

	/// Set the list window directly.
	pub fn set_max_visible(&mut self, rows: usize) {
		self.max_visible = rows.max(MIN_VISIBLE);
	}

	/// Whether Enter would go over the highlighted model's context window.
	pub fn is_over_context(&self, item: &ModelBrowserItem) -> bool {
		if self.select_only {
			return false;
		}
		let Some(tokens) = self.context_tokens() else {
			return false;
		};
		match (self.registry_context_window)(&item.selector) {
			Some(window) => tokens > window,
			None => false,
		}
	}

	fn context_tokens(&self) -> Option<u64> {
		self.current_context_tokens
	}

	/// Set the session token count used for the over-context check.
	pub fn set_current_context_tokens(&mut self, tokens: Option<u64>) {
		self.current_context_tokens = tokens;
	}

	/// The registry's context-window lookup, for hosts that know the windows.
	pub fn set_context_window_lookup(&mut self, lookup: Box<dyn Fn(&str) -> Option<u64>>) {
		self.registry_context_window = lookup;
	}

	/// Highlight `selector` in the current list.
	pub fn select_selector(&mut self, selector: &str) {
		if let Some(index) = self.visible_items().iter().position(|item| item.selector == selector) {
			self.cursor = index;
		}
	}

	/// Flip between session-model and Task-subagent targets.
	pub fn toggle_task_mode(&mut self) {
		self.task_mode = !self.task_mode;
		let target = if self.task_mode { self.task_selector.clone() } else { self.current_selector.clone() };
		self.cursor = 0;
		self.query.clear();
		if let Some(target) = target {
			self.select_selector(&target);
		}
	}

	/// The session token count used for the over-context check.
	pub fn current_context_tokens(&self) -> Option<u64> {
		self.current_context_tokens
	}

	/// Answer the callbacks with the highlighted row.
	pub fn confirm(&mut self) {
		let Some(item) = self.selected().cloned() else {
			return;
		};
		if let Some(role) = self.quick_roles.get(&item.selector).cloned() {
			if let Some(mut on_pick_role) = self.callbacks.on_pick_role.take() {
				on_pick_role(&role);
				self.callbacks.on_pick_role = Some(on_pick_role);
			}
			return;
		}
		if self.task_mode {
			if let Some(mut on_pick_task) = self.callbacks.on_pick_task.take() {
				on_pick_task(&item.model, &item.selector);
				self.callbacks.on_pick_task = Some(on_pick_task);
			}
			return;
		}
		let over_context = self.is_over_context(&item);
		if let Some(mut on_pick) = self.callbacks.on_pick.take() {
			on_pick(&item.model, &item.selector, over_context);
			self.callbacks.on_pick = Some(on_pick);
		}
	}

	/// Dismiss the picker.
	pub fn cancel(&mut self) {
		if let Some(on_cancel) = self.callbacks.on_cancel.take() {
			on_cancel();
			self.callbacks.on_cancel = Some(on_cancel);
		}
	}

	/// Route pointer events: a list row selects (and, on activate, confirms).
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		match event {
			NativeUiEvent::Action { act, .. } => match act.as_str() {
				"close" => self.cancel(),
				"clear" => self.query.clear(),
				"use" => self.confirm(),
				"task" => {
					if self.task_mode_key.is_some() && !self.role_mode {
						self.toggle_task_mode();
					}
				}
				_ => {}
			},
			NativeUiEvent::Select { item, .. } | NativeUiEvent::Activate { item, .. } => {
				let Some(index) = self.visible_items().iter().position(|row| &row.selector == item) else {
					return;
				};
				self.cursor = index;
				if matches!(event, NativeUiEvent::Activate { .. }) {
					self.confirm();
				}
			}
			_ => {}
		}
	}

	/// Build virtual `@role` rows in ctrl+p order.
	fn build_quick_role_items(&mut self, quick_roles: &[ResolvedRoleModel], quick_role_order: &[String]) {
		if quick_roles.is_empty() {
			return;
		}
		let order: Vec<String> = if quick_role_order.is_empty() {
			quick_roles.iter().map(|entry| entry.role.clone()).collect()
		} else {
			quick_role_order.to_vec()
		};
		let position = |role: &str| order.iter().position(|entry| entry == role).unwrap_or(usize::MAX);
		let mut items: Vec<ModelBrowserItem> = quick_roles
			.iter()
			.map(|entry| {
				let selector = entry.selector();
				self.quick_roles.insert(selector.clone(), entry.clone());
				ModelBrowserItem {
					provider: String::new(),
					id: selector.clone(),
					model: entry.model.clone(),
					selector,
				}
			})
			.collect();
		items.sort_by_key(|item| position(item.selector.trim_start_matches('@')));
		self.quick_role_items = items;
	}

	/// Switch list content only when a leading `@` changes the search mode.
	fn sync_mode_from_query(&mut self) {
		let role_mode = !self.select_only && !self.task_mode && self.query.starts_with('@');
		if role_mode == self.role_mode {
			return;
		}
		self.role_mode = role_mode;
		let current = if role_mode {
			self.current_quick_role_selector.clone()
		} else if self.task_mode {
			self.task_selector.clone()
		} else {
			self.current_selector.clone()
		};
		self.cursor = 0;
		if let Some(current) = current {
			self.select_selector(&current);
		}
	}

	fn clamp_cursor(&mut self) {
		let len = self.visible_items().len();
		if self.cursor >= len {
			self.cursor = len.saturating_sub(1);
		}
	}

	/// The footer hint for the active mode; keys resolve at render time.
	pub fn footer_hint(&self) -> String {
		let formatter = KeyHintFormatter::new(&self.theme);
		let up_down = editor_keys(&self.bindings, &self.theme, &["tui.select.up", "tui.select.down"]);
		let enter = formatter.format_key_hint("enter");
		let close = format!("{} close", editor_key(&self.bindings, &self.theme, "tui.select.cancel"));
		let mode = self.mode();
		let mut hint = match mode {
			ModelPickerMode::Role => format!("{up_down} roles · {enter} apply role model · type to search · {close}"),
			ModelPickerMode::Task => {
				format!("{up_down} models · {enter} use for Task subagents · type to search · {close}")
			}
			ModelPickerMode::Selection => format!("{up_down} models · {enter} select model · type to search · {close}"),
			ModelPickerMode::Session => {
				format!("{up_down} models · {enter} use for this session · type to search · @ quick roles · {close}")
			}
		};
		if let Some(key) = self.task_mode_key.as_deref() {
			if mode != ModelPickerMode::Role {
				hint.push_str(&format!(
					" · {} {}",
					formatter.format_key_hint(key),
					if self.task_mode { "session model" } else { "task model" }
				));
			}
		}
		hint
	}

	/// Where the highlighted row is within the window.
	fn window(&self, len: usize) -> (usize, usize) {
		if len <= self.max_visible {
			return (0, len);
		}
		let start = self.cursor.saturating_sub(self.max_visible / 2).min(len - self.max_visible);
		(start, start + self.max_visible)
	}

	/// The hint strip of the native card.
	fn describe_hints(&self) -> NativeNode {
		let mode = self.mode();
		let mut hints: Vec<Option<NativeHint>> = vec![
			action_hint(&self.bindings, &["tui.select.up", "tui.select.down"][..], mode.noun()),
			Some(NativeHint { keys: vec!["enter".to_string()], label: mode.confirm_label().to_string() }),
			Some(NativeHint { keys: Vec::new(), label: "type to search".to_string() }),
		];
		if mode == ModelPickerMode::Session {
			hints.push(Some(NativeHint { keys: vec!["@".to_string()], label: "quick roles".to_string() }));
		}
		hints.push(Some(NativeHint {
			keys: vec![interrupt_key(&self.bindings, &self.theme)],
			label: "close".to_string(),
		}));
		if let Some(key) = self.task_mode_key.as_deref() {
			if mode != ModelPickerMode::Role && mode != ModelPickerMode::Selection {
				hints.push(Some(NativeHint {
					keys: vec![key.to_string()],
					label: if self.task_mode { "session model".to_string() } else { "task model".to_string() },
				}));
			}
		}
		hints_row(&hints, "hints")
	}

	/// The status line for the card body.
	fn status_text(&self) -> NodeText {
		match self.config_error.as_deref() {
			Some(error) => NodeText::Spans(vec![span(error, Some("error"))]),
			None => {
				let tone = if self.task_mode { "error" } else { "muted" };
				NodeText::Spans(vec![span(self.mode().status_hint(), Some(tone))])
			}
		}
	}

	/// The list of rows, each labelled by its selector.
	fn describe_list(&self) -> NativeNode {
		let visible = self.visible_items();
		let items: Vec<NativeNode> = visible
			.iter()
			.map(|item| {
				let mut props = Props::new();
				props.set_str("label", item.selector.clone());
				if self.is_over_context(item) {
					props.set_bool("disabled", true);
				}
				node(NodeKind::Item, props, Vec::new(), Some(item.selector.as_str()))
			})
			.collect();
		let mut props = Props::new();
		if let Some(selected) = self.selected() {
			props.set_str("selected", selected.selector.clone());
		}
		if visible.is_empty() {
			props.insert(
				"empty",
				serde_json::Value::Array(vec![serde_json::Value::String(self.empty_text().to_string())]),
			);
		}
		node(NodeKind::List, props, items, Some("models"))
	}

	/// The empty-list message for the active mode.
	pub fn empty_text(&self) -> &'static str {
		if self.role_mode {
			"No quick roles configured"
		} else {
			"No models available"
		}
	}

	/// The fallback card for terminals without the `picker` kind.
	pub fn describe(&self, cx: &DescribeContext) -> NativeNode {
		if cx.supports(NodeKind::Picker) {
			return self.describe_picker(cx);
		}
		let mut props = Props::new();
		props.set_role(MODEL_PICKER_ROLE);
		props.set_head(NodeText::Plain(self.mode().title().to_string()));
		if self.task_mode {
			props.set_str("tone", "error");
		}
		let mut status_props = Props::new();
		status_props.set_str("wrap", "word");
		card(
			props,
			vec![
				text(self.status_text(), status_props),
				self.describe_list(),
				self.describe_hints(),
			],
		)
	}

	/// The `picker` sheet, sharing the shell's native search and list kinds.
	fn describe_picker(&self, _cx: &DescribeContext) -> NativeNode {
		let mode = self.mode();
		let mut props = Props::new();
		props.set_str("title", mode.title());
		props.set_str("icon", "cpu");
		props.set_str("noun", mode.noun());
		props.set_str("size", "md");
		props.set_str("layout", "rows");
		props.set_str("preview", "below");
		props.set_str(
			"placeholder",
			if mode == ModelPickerMode::Task || mode == ModelPickerMode::Selection {
				"Search models…"
			} else {
				"Search models, @ for quick roles…"
			},
		);
		props.set_text(self.empty_text());
		if let Some(selected) = self.selected() {
			props.set_str("selected", selected.selector.clone());
		}
		props.set_head(NodeText::Plain(mode.status_hint().to_string()));
		node(NodeKind::Picker, props, vec![self.describe_list()], Some("model-picker"))
	}
}

impl Component for ModelPickerComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = usize::from(width).saturating_sub(4);
		let status = match self.config_error.as_deref() {
			Some(error) => self.theme.fg("error", &format!(" {error}")).unwrap_or_else(|_| error.to_string()),
			None => {
				let raw = format!(" {}", self.mode().status_hint());
				self.theme
					.fg(if self.task_mode { "error" } else { "muted" }, &raw)
					.unwrap_or(raw)
			}
		};
		let title = if self.task_mode {
			"Switch Task Model"
		} else if self.select_only {
			"Select Model"
		} else {
			"Switch Model"
		};
		let mut out = vec![
			top_border(width, title, &self.panel_theme),
			row(&status, width, &self.panel_theme),
		];
		let visible = self.visible_items();
		if visible.is_empty() {
			let empty = format!("  {}", self.empty_text());
			let empty = self.theme.fg("dim", &empty).unwrap_or(empty);
			out.push(row(&empty, width, &self.panel_theme));
		} else {
			let (start, end) = self.window(visible.len());
			for index in start..end {
				let item = visible[index];
				let selected = index == self.cursor;
				let marker = if selected {
					self.theme.fg("accent", "▶ ").unwrap_or_else(|_| "▶ ".to_string())
				} else {
					"  ".to_string()
				};
				let tone = if self.is_over_context(item) {
					"dim"
				} else if selected {
					"accent"
				} else {
					"text"
				};
				let label = self
					.theme
					.fg(tone, &truncate_label(&item.selector, inner.saturating_sub(2)))
					.unwrap_or_else(|_| item.selector.clone());
				out.push(row(&format!("{marker}{label}"), width, &self.panel_theme));
			}
		}
		let footer = self.footer_hint();
		let footer = self.theme.fg("dim", &footer).unwrap_or(footer);
		out.push(row(&footer, width, &self.panel_theme));
		out.push(bottom_border(width, &self.panel_theme));
		out
	}

	fn handle_input(&mut self, key: &Key) {
		if let Some(task_key) = self.task_mode_key.as_deref() {
			if key_id(key).as_deref() == Some(task_key) {
				self.toggle_task_mode();
				return;
			}
		}
		match key {
			Key::Esc => self.cancel(),
			Key::Enter => self.confirm(),
			Key::Up => {
				if self.cursor > 0 {
					self.cursor -= 1;
				}
			}
			Key::Down => {
				let len = self.visible_items().len();
				if self.cursor + 1 < len {
					self.cursor += 1;
				}
			}
			Key::Backspace => {
				self.query.pop();
				self.cursor = 0;
				self.sync_mode_from_query();
				self.clamp_cursor();
			}
			_ => {
				if let Some(ch) = printable(key) {
					self.query.push(ch);
					self.cursor = 0;
					self.sync_mode_from_query();
					self.clamp_cursor();
				}
			}
		}
	}

	fn handle_paste(&mut self, text: &str) {
		self.query.push_str(text);
		self.cursor = 0;
		self.sync_mode_from_query();
		self.clamp_cursor();
	}

	fn invalidate(&mut self) {
		self.panel_theme = OverlayTheme::default();
	}
}

fn printable(key: &Key) -> Option<char> {
	match key {
		Key::Char(c) | Key::Shift(c) => Some(*c),
		_ => None,
	}
}

fn truncate_label(label: &str, width: usize) -> String {
	if label.chars().count() <= width || width == 0 {
		return label.to_string();
	}
	let mut out: String = label.chars().take(width.saturating_sub(1)).collect();
	out.push('…');
	out
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

	fn model(provider: &str, id: &str) -> SetupModel {
		SetupModel {
			provider: provider.to_string(),
			id: id.to_string(),
			name: id.to_string(),
			kind: crate::setup::scenes::types::ModelKind::Chat,
			priority: None,
		}
	}

	fn item(provider: &str, id: &str) -> ModelBrowserItem {
		let model = model(provider, id);
		ModelBrowserItem {
			provider: provider.to_string(),
			id: id.to_string(),
			selector: format!("{provider}/{id}"),
			model,
		}
	}

	struct FakeRegistry {
		items: Vec<ModelBrowserItem>,
		stale: bool,
	}

	impl ModelPickerRegistry for FakeRegistry {
		fn refresh_if_stale(&mut self) -> Result<bool, String> {
			let stale = self.stale;
			self.stale = false;
			Ok(stale)
		}

		fn items(&self) -> Vec<ModelBrowserItem> {
			self.items.clone()
		}
	}

	fn catalog() -> FakeRegistry {
		FakeRegistry {
			items: vec![item("anthropic", "sonnet"), item("openai", "gpt-5"), item("openai", "mini")],
			stale: false,
		}
	}

	fn role(name: &str, model: SetupModel) -> ResolvedRoleModel {
		ResolvedRoleModel {
			role: name.to_string(),
			model,
			thinking_level: None,
			explicit_thinking_level: false,
		}
	}

	fn options() -> ModelPickerOptions {
		ModelPickerOptions {
			current_selector: Some("openai/gpt-5".to_string()),
			quick_roles: vec![role("default", model("anthropic", "sonnet")), role("smol", model("openai", "mini"))],
			quick_role_order: vec!["smol".to_string(), "default".to_string()],
			..Default::default()
		}
	}

	fn picker(options: ModelPickerOptions) -> (ModelPickerComponent, Rc<RefCell<Vec<String>>>) {
		let picked = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&picked);
		let mut registry = catalog();
		let component = ModelPickerComponent::new(
			theme(),
			KeybindingsManager::new(),
			&mut registry,
			ModelPickerCallbacks {
				on_pick: Some(Box::new(move |_model, selector, over_context| {
					sink.borrow_mut().push(format!("pick:{selector}:{over_context}"));
				})),
				..Default::default()
			},
			options,
		);
		(component, picked)
	}

	#[test]
	fn filter_matches_selector_substrings_ignoring_case() {
		let items = vec![item("anthropic", "sonnet"), item("openai", "gpt-5")];
		let matches: Vec<&str> = filter_items(&items, "SON").iter().map(|i| i.selector.as_str()).collect();
		assert_eq!(matches, vec!["anthropic/sonnet"]);
		assert_eq!(filter_items(&items, "  ").len(), 2);
		assert!(filter_items(&items, "nope").is_empty());
	}

	#[test]
	fn constructor_preselects_the_current_selector() {
		let (picker, _picked) = picker(options());
		assert_eq!(picker.mode(), ModelPickerMode::Session);
		assert_eq!(picker.selected().map(|item| item.selector.as_str()), Some("openai/gpt-5"));
		assert_eq!(picker.query(), "");
	}

	#[test]
	fn typing_filters_and_enter_picks() {
		let (mut picker, picked) = picker(options());
		for ch in "mini".chars() {
			picker.handle_input(&Key::Char(ch));
		}
		assert_eq!(picker.visible_items().len(), 1);
		picker.handle_input(&Key::Enter);
		assert_eq!(picked.borrow().as_slice(), ["pick:openai/mini:false"]);
	}

	#[test]
	fn at_prefix_switches_to_quick_roles_in_cycle_order() {
		let (mut picker, picked) = picker(options());
		picker.handle_input(&Key::Char('@'));
		assert_eq!(picker.mode(), ModelPickerMode::Role);
		let order: Vec<&str> = picker.visible_items().iter().map(|item| item.selector.as_str()).collect();
		assert_eq!(order, vec!["@smol", "@default"]);
		picker.handle_input(&Key::Backspace);
		assert_eq!(picker.mode(), ModelPickerMode::Session);
		assert!(picked.borrow().is_empty());
	}

	#[test]
	fn over_context_models_are_flagged_on_pick() {
		let mut options = options();
		options.current_context_tokens = Some(300_000);
		let (mut picker, picked) = picker(options);
		picker.set_context_window_lookup(Box::new(|selector: &str| {
			if selector == "openai/gpt-5" { Some(200_000) } else { Some(400_000) }
		}));
		picker.set_current_context_tokens(Some(300_000));
		picker.handle_input(&Key::Enter);
		assert_eq!(picked.borrow().as_slice(), ["pick:openai/gpt-5:true"]);
	}

	#[test]
	fn escape_reports_a_cancel() {
		let cancelled = Rc::new(RefCell::new(0));
		let sink = Rc::clone(&cancelled);
		let mut registry = catalog();
		let mut component = ModelPickerComponent::new(
			theme(),
			KeybindingsManager::new(),
			&mut registry,
			ModelPickerCallbacks {
				on_cancel: Some(Box::new(move || *sink.borrow_mut() += 1)),
				..Default::default()
			},
			ModelPickerOptions::default(),
		);
		component.handle_input(&Key::Esc);
		component.handle_native_event(&NativeUiEvent::Action {
			key: Vec::new(),
			act: "close".to_string(),
			value: None,
			mods: Vec::new(),
		});
		assert_eq!(*cancelled.borrow(), 2);
	}

	#[test]
	fn task_mode_toggles_with_its_key_and_targets_task_picks() {
		let picked = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&picked);
		let mut registry = catalog();
		let mut component = ModelPickerComponent::new(
			theme(),
			KeybindingsManager::new(),
			&mut registry,
			ModelPickerCallbacks {
				on_pick_task: Some(Box::new(move |_model, selector| sink.borrow_mut().push(selector.to_string()))),
				..Default::default()
			},
			ModelPickerOptions {
				task_mode_keys: vec!["alt+p".to_string()],
				task_selector: Some("openai/mini".to_string()),
				..Default::default()
			},
		);
		component.handle_input(&Key::Alt('p'));
		assert_eq!(component.mode(), ModelPickerMode::Task);
		assert_eq!(component.selected().map(|item| item.selector.as_str()), Some("openai/mini"));
		component.handle_input(&Key::Enter);
		assert_eq!(picked.borrow().as_slice(), ["openai/mini"]);
	}

	#[test]
	fn selection_only_pickers_never_enter_role_or_task_mode() {
		let mut options = options();
		options.select_only = true;
		options.task_mode_keys = vec!["alt+p".to_string()];
		let (mut picker, _picked) = picker(options);
		assert_eq!(picker.mode(), ModelPickerMode::Selection);
		picker.handle_input(&Key::Char('@'));
		assert_eq!(picker.mode(), ModelPickerMode::Selection);
	}

	#[test]
	fn terminal_height_clamps_the_visible_window() {
		let (mut picker, _picked) = picker(options());
		picker.set_terminal_rows(16);
		assert_eq!(picker.max_visible(), MIN_VISIBLE);
		picker.set_terminal_rows(200);
		assert_eq!(picker.max_visible(), (200.0 * HEIGHT_FRACTION) as usize - CHROME_ROWS - BROWSER_FRAME_ROWS);
		picker.set_max_visible(1);
		assert_eq!(picker.max_visible(), MIN_VISIBLE);
	}

	#[test]
	fn render_frames_the_status_list_and_mode_hints() {
		let (mut picker, _picked) = picker(options());
		let lines = picker.render(60);
		assert!(lines[0].contains("Switch Model"));
		assert!(lines.iter().any(|line| line.contains(STATUS_HINT)));
		assert!(lines.iter().any(|line| line.contains("openai/gpt-5")));
		assert!(lines.iter().any(|line| line.contains("quick roles")));
		assert_eq!(lines.len(), CHROME_ROWS + 3);
	}

	#[test]
	fn a_stale_registry_refresh_reloads_the_list() {
		let mut registry = FakeRegistry {
			items: vec![item("openai", "old")],
			stale: true,
		};
		let mut component = ModelPickerComponent::new(
			theme(),
			KeybindingsManager::new(),
			&mut registry,
			ModelPickerCallbacks::default(),
			ModelPickerOptions::default(),
		);
		assert_eq!(component.visible_items().len(), 1);
		registry.items = vec![item("openai", "new"), item("openai", "newer")];
		component.refresh(&mut registry);
		assert_eq!(component.visible_items().len(), 2);
		assert!(component.config_error.is_none());
	}
}
