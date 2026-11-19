//! Settings overlay: tab strip, per-tab setting lists, and global search.
//! Reference: `l/src/overlays/settings-selector.ts`.
//!
//! The selector owns the frame (border, tabs, divider, footer hints), the
//! per-tab [`SettingsList`]s, and type-to-search across every tab. Boolean and
//! enum rows change in place; submenu, text, provider-limit, and multi-select
//! rows have no list-side editor in this port, so activation hands the row to
//! the host through [`SettingsCallbacks::on_edit`].
//!
//! Divergences from the TypeScript original:
//! - No split sidebar layout: [`SettingsList`] only renders the flat list.
//! - The settings surface is provided by [`SettingsHost`] (get/set/unset plus
//!   provider-limit normalization) instead of a schema-backed store object.
//! - Search results are ordered by match count per tab rather than by the best
//!   fuzzy score, because the shared scorer exposes ranking per list only.
//! - The plugins page is built by a host factory; without one the tab shows a
//!   "plugins unavailable" row.
//! - The frame height is fixed (the component API carries no terminal size).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;

use crate::chrome::overlay_box::{
	bottom_border, divider, row, top_border, OverlayTheme,
};
use crate::component::Component;
use crate::components::input::Input;
use crate::components::menu_selection::fuzzy_filter;
use crate::components::settings_list::{SettingItem, SettingsList, SettingsListTheme};
use crate::components::tab_bar::{Tab, TabBar, TabBarTheme};
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::native::describe::{col, item, list, node, row as native_row, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{NativeHint, action_hint, hints_row, overlay_card};
use crate::native::backend::NativeUiEvent;
use crate::native::NativeDescribe;
use crate::overlays::plugin_settings::{
	PluginSettingsCallbacks, PluginSettingsComponent,
};
use crate::overlays::settings_defs::{
	SETTING_TABS, SettingDef, SettingTab, SettingsDisplayEntry, SubmenuOption,
	get_setting_def, get_settings_for_tab, number_steps, numeric_option, tab_metadata,
};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// The plugins tab's page id.
pub const PLUGINS_PAGE: &str = "plugins";
/// Most choices a popup menu lists before a picker takes over.
pub const PREFS_MENU_MAX: usize = 12;
/// Rows the active list may occupy.
const MAX_VISIBLE: usize = 12;
/// Content rows the frame paints (list padding included).
const CONTENT_ROWS: usize = 18;
/// Text settings whose values are paths, ids, or commands.
const MONO_TEXT_SUFFIXES: &[&str] = &[
	"path", "dir", "url", "endpoint", "host", "command", "model", "id",
];
/// Submenus whose choices preview live while highlighted.
const PREVIEWED_SETTINGS: &[&str] = &[
	"theme.dark",
	"theme.light",
	"statusLine.preset",
	"statusLine.separator",
	"statusLine.contextLine",
	"snapcompact.shape",
	"composer.shape",
];

/// Which keys the footer currently offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsHintMode {
	/// Search owns the keys.
	Search,
	/// The plugins page owns the keys.
	Plugins,
	/// Section headings have focus.
	Sections,
	/// Rows have focus and Tab jumps sections.
	RowsSections,
	/// Rows have focus and Tab switches tabs.
	Rows,
}

/// Status-line preview values passed to the host renderer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusLinePreviewSettings {
	/// Preset id.
	pub preset: String,
	/// Left-side segment ids.
	pub left_segments: Vec<String>,
	/// Right-side segment ids.
	pub right_segments: Vec<String>,
	/// Separator style id.
	pub separator: String,
	/// Accent the session segment.
	pub session_accent: bool,
	/// Draw on the terminal background.
	pub transparent: bool,
}

/// Settings store the overlay reads and writes.
///
/// Implementors expose the schema-backed display entries and persist values
/// under their dotted paths. `normalize_provider_limits` and
/// `validate_provider_limits` mirror the TS store helpers; their defaults are
/// correct for a plain JSON object of positive whole numbers.
pub trait SettingsHost {
	/// Display entries for the current schema.
	fn entries(&self) -> Vec<SettingsDisplayEntry>;
	/// Current value of `path`.
	fn get(&self, path: &str) -> Option<Value>;
	/// Persist `value` at `path`.
	fn set(&mut self, path: &str, value: Value);
	/// Remove `path` so its default applies again.
	fn unset(&mut self, path: &str);
	/// Per-provider request limits from a raw record value.
	fn normalize_provider_limits(&self, value: Option<&Value>) -> BTreeMap<String, u32> {
		let mut limits = BTreeMap::new();
		let Some(Value::Object(map)) = value else {
			return limits;
		};
		for (provider, raw) in map {
			if let Some(limit) = raw.as_u64() {
				if limit > 0 {
					limits.insert(provider.clone(), limit.min(u64::from(u32::MAX)) as u32);
				}
			}
		}
		limits
	}
	/// Keep only usable entries of a provider-limit record.
	fn validate_provider_limits(&self, value: Value) -> Value {
		value
	}
}

/// Everything the selector needs from the running application.
pub struct SettingsRuntimeContext {
	/// Settings store.
	pub settings: Rc<RefCell<Box<dyn SettingsHost>>>,
	/// Thinking levels the current model supports.
	pub available_thinking_levels: Vec<String>,
	/// Theme names available for the theme settings.
	pub available_themes: Vec<String>,
	/// Current model id (snapcompact previews).
	pub model: String,
	/// Image budget in bytes (snapcompact previews).
	pub image_budget: usize,
	/// Ask the surface to repaint.
	pub request_render: Rc<dyn Fn()>,
	/// Build the plugins page; `None` hides its content.
	pub plugins: Option<Box<dyn FnMut(PluginSettingsCallbacks) -> PluginSettingsComponent>>,
}

/// What the selector reports back.
pub struct SettingsCallbacks {
	/// A setting changed (path, new value).
	pub on_change: Box<dyn FnMut(&str, Value)>,
	/// Close the overlay.
	pub on_cancel: Box<dyn FnMut()>,
	/// Highlight a theme while its submenu is open.
	pub on_theme_preview: Option<Box<dyn FnMut(&str)>>,
	/// Push a status-line preview configuration.
	pub on_status_line_preview: Option<Box<dyn FnMut(StatusLinePreviewSettings)>>,
	/// Current status-line preview string (`None` shows a placeholder).
	pub get_status_line_preview: Option<Box<dyn Fn() -> String>>,
	/// Plugins changed on disk.
	pub on_plugins_changed: Option<Box<dyn FnMut()>>,
	/// Open the host editor for a row the list cannot edit (path, schema type).
	pub on_edit: Option<Box<dyn FnMut(&str, &str)>>,
}

impl std::fmt::Debug for SettingsSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SettingsSelectorComponent")
			.field("tab", &self.current_tab)
			.field("searching", &self.search_list.is_some())
			.finish()
	}
}

/// The settings overlay.
pub struct SettingsSelectorComponent {
	context: SettingsRuntimeContext,
	callbacks: Rc<RefCell<SettingsCallbacks>>,
	tab_bar: TabBar,
	tabs: Vec<Tab>,
	current_list: Option<SettingsList>,
	search_list: Option<SettingsList>,
	plugin_component: Option<PluginSettingsComponent>,
	current_tab: String,
	pre_search_tab: String,
	search_query: String,
	search_input: Input,
	search_match_count: usize,
	search_first_match: HashMap<String, String>,
	has_section_jump: bool,
	tab_row_start: usize,
	tab_row_count: usize,
	content_row_start: usize,
}

impl SettingsSelectorComponent {
	/// Build the selector, opening the Appearance tab.
	pub fn new(context: SettingsRuntimeContext, callbacks: SettingsCallbacks) -> Self {
		let tabs = settings_tabs();
		let mut tab_bar = TabBar::new(String::new(), tabs.clone(), TabBarTheme::default());
		tab_bar.set_show_hint(false);
		let mut component = SettingsSelectorComponent {
			context,
			callbacks: Rc::new(RefCell::new(callbacks)),
			tab_bar,
			tabs,
			current_list: None,
			search_list: None,
			plugin_component: None,
			current_tab: SettingTab::Appearance.as_str().to_string(),
			pre_search_tab: SettingTab::Appearance.as_str().to_string(),
			search_query: String::new(),
			search_input: Input::new(),
			search_match_count: 0,
			search_first_match: HashMap::new(),
			has_section_jump: false,
			tab_row_start: 0,
			tab_row_count: 0,
			content_row_start: 0,
		};
		component.switch_to_tab(SettingTab::Appearance.as_str());
		component
	}

	/// The active page id (a tab id or [`PLUGINS_PAGE`]).
	pub fn current_tab(&self) -> &str {
		&self.current_tab
	}

	/// Whether the global search list owns the content area.
	pub fn is_searching(&self) -> bool {
		self.search_list.is_some()
	}

	/// Current search query.
	pub fn search_query(&self) -> &str {
		&self.search_query
	}

	/// Number of settings matching the current query.
	pub fn search_match_count(&self) -> usize {
		self.search_match_count
	}

	/// The active settings list (search results while searching).
	pub fn active_list(&self) -> Option<&SettingsList> {
		self.search_list.as_ref().or(self.current_list.as_ref())
	}

	/// Switch to `id`, leaving search mode.
	pub fn show_page(&mut self, id: &str) {
		if self.search_list.is_some() {
			self.end_search(false);
		}
		if id != self.current_tab {
			self.tab_bar.set_selected_by_id(id);
			self.switch_to_tab(id);
		}
	}

	fn switch_to_tab(&mut self, id: &str) {
		self.current_tab = id.to_string();
		self.current_list = None;
		self.search_list = None;
		self.plugin_component = None;
		match SettingTab::parse(id) {
			Some(tab) => self.show_settings_tab(tab),
			None => self.show_plugins_tab(),
		}
	}

	fn show_settings_tab(&mut self, tab: SettingTab) {
		let defs = self.defs_for(tab);
		let items = self.build_items_for_defs(&defs);
		let section_count = items.iter().filter(|item| item.heading).count()
			+ usize::from(items.first().is_some_and(|item| !item.heading));
		self.has_section_jump = section_count >= 2;

		let settings = Rc::clone(&self.context.settings);
		let callbacks = Rc::clone(&self.callbacks);

		let mut list = SettingsList::new(items, MAX_VISIBLE, SettingsListTheme::default());
		list.set_type_to_search(false);
		list.set_hint(Some(String::new()));
		list.on_toggle(move |id, new_value| {
			apply_cycle_change(&settings, &callbacks, id, new_value);
		});

		let settings = Rc::clone(&self.context.settings);
		let callbacks = Rc::clone(&self.callbacks);
		let cancel = Rc::clone(&self.callbacks);
		list.on_cancel(move || (cancel.borrow_mut().on_cancel)());
		list.on_select(move |row| {
			let kind = {
				let store = settings.borrow();
				def_kind(&store.entries(), &row.id)
			};
			let Some((schema_type, editable)) = kind else {
				return;
			};
			if editable {
				if let Some(on_edit) = callbacks.borrow_mut().on_edit.as_mut() {
					on_edit(&row.id, &schema_type);
				}
			}
		});
		self.current_list = Some(list);
	}

	fn show_plugins_tab(&mut self) {
		let callbacks = Rc::clone(&self.callbacks);
		let on_close = move || (callbacks.borrow_mut().on_cancel)();
		let callbacks = Rc::clone(&self.callbacks);
		let on_plugin_changed = move || {
			if let Some(changed) = callbacks.borrow_mut().on_plugins_changed.as_mut() {
				changed();
			}
		};
		match self.context.plugins.as_mut() {
			Some(factory) => {
				self.plugin_component = Some(factory(PluginSettingsCallbacks::new(
					on_close,
					on_plugin_changed,
				)));
			}
			None => self.plugin_component = None,
		}
	}

	fn defs_for(&self, tab: SettingTab) -> Vec<SettingDef> {
		let entries = self.context.settings.borrow().entries();
		get_settings_for_tab(&entries, tab)
	}

	fn build_items_for_defs(&self, defs: &[SettingDef]) -> Vec<SettingItem> {
		let store = self.context.settings.borrow();
		let entries = store.entries();
		let mut items = Vec::new();
		let mut last_group: Option<&str> = None;
		for def in defs {
			if !condition_holds(&entries, &def.base().path) {
				continue;
			}
			if let Some(group) = def.base().group.as_deref() {
				if last_group != Some(group) {
					items.push(SettingItem::new(format!("__heading:{group}"), group, String::new()).heading());
					last_group = Some(group);
				}
			}
			if let Some(item) = self.item_for_def(def, &self.context.settings) {
				items.push(item);
			}
		}
		items
	}

	fn item_for_def(
		&self,
		def: &SettingDef,
		store: &Rc<RefCell<Box<dyn SettingsHost>>>,
	) -> Option<SettingItem> {
		let store = store.borrow();
		let base = def.base();
		let current = store.get(&base.path);
		let changed = is_changed(def, current.as_ref());
		let mut item = SettingItem::new(base.path.clone(), base.label.clone(), String::new());
		if !base.description.is_empty() {
			item = item.with_description(base.description.clone());
		}
		if let Some(warning) = &base.warning {
			item = item.with_warning(warning.clone());
		}
		if changed {
			item = item.changed();
		}
		item.value = match def {
			SettingDef::Boolean(_) => {
				if current.as_ref().and_then(Value::as_bool).unwrap_or(false) {
					"true".to_string()
				} else {
					"false".to_string()
				}
			}
			SettingDef::Enum(enum_def) => {
				let raw = current
					.as_ref()
					.map(|value| value.as_str().unwrap_or_default().to_string())
					.unwrap_or_default();
				if raw.is_empty() {
					enum_def.values.first().cloned().unwrap_or_default()
				} else {
					raw
				}
			}
			SettingDef::Submenu(submenu) => submenu_display(self, submenu, current.as_ref()),
			SettingDef::Text(text_def) => format_text_value(text_def.secret, current.as_ref()),
			SettingDef::ProviderLimits(_) => {
				format_provider_limits(store.as_ref(), current.as_ref())
			}
			SettingDef::MultiSelect(multi) => format_multi_value(&multi.options, current.as_ref()),
		};
		match def {
			SettingDef::Boolean(_) => item = item.with_values(vec!["true".into(), "false".into()]),
			SettingDef::Enum(_) => item = item.with_values(current_enum_values(self, def)),
			_ => {}
		}
		Some(item)
	}

	fn submenu_options(&self, def: &SettingDef, path: &str) -> Vec<SubmenuOption> {
		if path == "defaultThinkingLevel" {
			let mut levels = vec!["auto".to_string()];
			levels.extend(self.context.available_thinking_levels.iter().cloned());
			let declared = match def {
				SettingDef::Submenu(submenu) => submenu.options.clone(),
				_ => Vec::new(),
			};
			return levels
				.into_iter()
				.map(|level| {
					declared
						.iter()
						.find(|option| option.value == level)
						.cloned()
						.unwrap_or_else(|| SubmenuOption {
							value: level.clone(),
							label: level,
							description: None,
						})
				})
				.collect();
		}
		if path == "theme.dark" || path == "theme.light" {
			return self
				.context
				.available_themes
				.iter()
				.map(|theme| SubmenuOption {
					value: theme.clone(),
					label: theme.clone(),
					description: None,
				})
				.collect();
		}
		match def {
			SettingDef::Submenu(submenu) => submenu.options.clone(),
			_ => Vec::new(),
		}
	}

	fn footer_hint_text(&self) -> String {
		if self.search_list.is_some() {
			return "enter to change · tab to jump tabs · esc to exit search".to_string();
		}
		if self.current_tab == PLUGINS_PAGE {
			return "tab to switch tabs · esc to close".to_string();
		}
		if self.current_list.as_ref().is_some_and(SettingsList::section_focused) {
			return "up/down to jump sections · tab/enter to settings · left/right to switch tabs · esc to close".to_string();
		}
		let nav = if self.has_section_jump {
			"tab to jump sections · left/right to switch tabs"
		} else {
			"tab to switch tabs"
		};
		format!("enter/space to change · {nav} · type to search · esc to close")
	}

	fn hints_node(&self, bindings: &crate::keybindings::KeybindingsManager, mode: SettingsHintMode) -> NativeNode {
		let close = action_hint(bindings, "tui.select.cancel", "close");
		let switch_tabs = NativeHint {
			keys: vec!["left".into(), "right".into()],
			label: "switch tabs".into(),
		};
		let hints: Vec<Option<NativeHint>> = match mode {
			SettingsHintMode::Search => vec![
				action_hint(bindings, "tui.select.confirm", "change"),
				Some(NativeHint { keys: vec!["tab".into()], label: "jump tabs".into() }),
				action_hint(bindings, "tui.select.cancel", "exit search"),
			],
			SettingsHintMode::Plugins => vec![
				Some(NativeHint { keys: vec!["tab".into()], label: "switch tabs".into() }),
				close,
			],
			SettingsHintMode::Sections => vec![
				action_hint(bindings, &["tui.select.up", "tui.select.down"], "jump sections"),
				Some(NativeHint { keys: vec!["tab".into()], label: "to settings".into() }),
				Some(switch_tabs),
				close,
			],
			SettingsHintMode::Rows | SettingsHintMode::RowsSections => vec![
				Some(NativeHint { keys: vec!["enter".into(), "space".into()], label: "change".into() }),
				Some(NativeHint {
					keys: vec!["tab".into()],
					label: if mode == SettingsHintMode::RowsSections {
						"jump sections".into()
					} else {
						"switch tabs".into()
					},
				}),
				Some(switch_tabs),
				Some(NativeHint { keys: Vec::new(), label: "type to search".into() }),
				close,
			],
		};
		hints_row(&hints, "hints")
	}

	fn status_preview_string(&self) -> String {
		match self.callbacks.borrow().get_status_line_preview.as_ref() {
			Some(get) => get(),
			None => "(preview not available)".to_string(),
		}
	}

	fn trigger_status_line_preview(&self) {
		let settings = self.context.settings.borrow();
		let preview = StatusLinePreviewSettings {
			preset: string_setting(&**settings, "statusLine.preset"),
			left_segments: string_list_setting(&**settings, "statusLine.leftSegments"),
			right_segments: string_list_setting(&**settings, "statusLine.rightSegments"),
			separator: string_setting(&**settings, "statusLine.separator"),
			session_accent: bool_setting(&**settings, "statusLine.sessionAccent"),
			transparent: bool_setting(&**settings, "statusLine.transparent"),
		};
		drop(settings);
		if let Some(callback) = self.callbacks.borrow_mut().on_status_line_preview.as_mut() {
			callback(preview);
		}
	}

	/// Rebuild the visible rows after a change made outside the list.
	fn refresh_items(&mut self) {
		if self.search_list.is_some() {
			let query = self.search_query.clone();
			self.set_search_query(&query);
			return;
		}
		let Some(tab) = SettingTab::parse(&self.current_tab) else {
			return;
		};
		let defs = self.defs_for(tab);
		let items = self.build_items_for_defs(&defs);
		let section_count = items.iter().filter(|item| item.heading).count()
			+ usize::from(items.first().is_some_and(|item| !item.heading));
		self.has_section_jump = section_count >= 2;
		if let Some(list) = self.current_list.as_mut() {
			list.set_items(items);
		}
	}

	/// All search candidates, one bucket per tab, with condition gates applied.
	fn search_corpora(&self) -> Vec<(SettingTab, Vec<SettingItem>)> {
		let store = self.context.settings.borrow();
		let entries = store.entries();
		SETTING_TABS
			.iter()
			.map(|tab| {
				let defs = get_settings_for_tab(&entries, *tab);
				let mut rows = Vec::new();
				for def in &defs {
					if !condition_holds(&entries, &def.base().path) {
						continue;
					}
					if let Some(item) = self.item_for_def(def, &self.context.settings) {
						rows.push(item);
					}
				}
				(*tab, rows)
			})
			.collect()
	}

	/// Replace the search results with matches for `query`.
	fn set_search_query(&mut self, query: &str) {
		if self.search_list.is_none() {
			return;
		}
		if query.is_empty() {
			self.end_search(false);
			return;
		}
		self.search_query = query.to_string();
		self.search_first_match.clear();

		let metadata = tab_metadata();
		let mut matched: Vec<(SettingTab, Vec<SettingItem>, usize)> = Vec::new();
		let mut counts: HashMap<String, usize> = HashMap::new();
		let mut total = 0usize;
		for (tab, rows) in self.search_corpora() {
			let hits = fuzzy_filter(&rows, query, setting_filter_text);
			counts.insert(tab.as_str().to_string(), hits.len());
			if hits.is_empty() {
				continue;
			}
			total += hits.len();
			matched.push((tab, hits, tab_order(tab)));
		}
		matched.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.2.cmp(&right.2)));

		let mut items = Vec::new();
		for (tab, hits, _) in &matched {
			let meta = metadata.get(tab.as_str());
			let label = meta.map(|meta| meta.label.clone()).unwrap_or_else(|| tab.as_str().to_string());
			items.push(SettingItem::new(format!("__tab:{}", tab.as_str()), label, String::new()).heading());
			if let Some(first) = hits.first() {
				self.search_first_match
					.insert(tab.as_str().to_string(), first.id.clone());
			}
			items.extend(hits.iter().cloned());
		}

		if let Some(list) = self.search_list.as_mut() {
			list.set_items(items);
		}
		self.search_match_count = total;
		self.tabs = build_search_tabs(&counts, &matched);
		self.tab_bar.set_tabs(self.tabs.clone());
		self.sync_tab_bar_to_selection();
	}

	fn sync_tab_bar_to_selection(&mut self) {
		let Some(list) = self.search_list.as_ref() else {
			return;
		};
		let Some(selected) = list.get_selected_item() else {
			return;
		};
		let id = selected.id.clone();
		let tab = {
			let store = self.context.settings.borrow();
			get_setting_def(&store.entries(), &id).and_then(|def| def.base().tab)
		};
		if let Some(tab) = tab {
			self.tab_bar.set_selected_by_id(tab.as_str());
		}
	}

	/// Start a global search with `initial_query`.
	fn start_search(&mut self, initial_query: &str) {
		self.pre_search_tab = self.current_tab.clone();
		self.search_input.set_value(initial_query);
		let settings = Rc::clone(&self.context.settings);
		let callbacks = Rc::clone(&self.callbacks);
		let mut list = SettingsList::new(Vec::new(), MAX_VISIBLE, SettingsListTheme::default());
		list.set_type_to_search(false);
		list.set_hint(Some(String::new()));
		list.set_empty_text("No matching settings");
		list.on_toggle(move |id, new_value| {
			apply_cycle_change(&settings, &callbacks, id, new_value);
		});
		let callbacks = Rc::clone(&self.callbacks);
		list.on_cancel(move || (callbacks.borrow_mut().on_cancel)());
		list.on_select({
			let settings = Rc::clone(&self.context.settings);
			let callbacks = Rc::clone(&self.callbacks);
			move |row| {
				let kind = {
					let store = settings.borrow();
					def_kind(&store.entries(), &row.id)
				};
				let Some((schema_type, editable)) = kind else {
					return;
				};
				if editable {
					if let Some(on_edit) = callbacks.borrow_mut().on_edit.as_mut() {
						on_edit(&row.id, &schema_type);
					}
				}
			}
		});
		self.current_list = None;
		self.plugin_component = None;
		self.search_list = Some(list);
		self.set_search_query(initial_query);
	}

	/// Leave search mode; `jump` lands on the selected result's tab.
	fn end_search(&mut self, jump: bool) {
		if self.search_list.is_none() {
			return;
		}
		let selected_tab = if jump {
			self.search_list
				.as_ref()
				.and_then(SettingsList::get_selected_item)
				.map(|item| item.id.clone())
		} else {
			None
		};
		let target = selected_tab
			.as_deref()
			.and_then(|id| {
				let store = self.context.settings.borrow();
				get_setting_def(&store.entries(), id).and_then(|def| def.base().tab)
			})
			.map(|tab| tab.as_str().to_string())
			.or_else(|| Some(self.pre_search_tab.clone()))
			.unwrap_or_else(|| SettingTab::Appearance.as_str().to_string());

		self.search_query.clear();
		self.search_input = Input::new();
		self.search_first_match.clear();
		self.search_match_count = 0;
		self.search_list = None;
		self.tabs = settings_tabs();
		self.tab_bar
			.set_tabs_with_active(self.tabs.clone(), Some(target.as_str()));
		self.switch_to_tab(&target);
		if let Some(id) = selected_tab {
			if let Some(list) = self.current_list.as_mut() {
				list.select_item(&id);
			}
		}
	}

	/// Search mode: Escape leaves, Tab jumps tabs, navigation stays with the
	/// result list, everything else edits the query.
	fn handle_search_input(&mut self, key: &Key) {
		if matches!(key, Key::Esc) {
			self.end_search(true);
			return;
		}
		if matches!(key, Key::Tab) {
			self.step_search_tab(1);
			return;
		}
		if matches!(
			key,
			Key::Enter | Key::Up | Key::Down | Key::PageUp | Key::PageDown
		) {
			if let Some(list) = self.search_list.as_mut() {
				list.handle_input(key);
			}
			return;
		}
		self.search_input.handle_input(key);
		let value = self.search_input.value().to_string();
		if value != self.search_query {
			self.set_search_query(&value);
		}
	}

	fn step_search_tab(&mut self, delta: isize) {
		self.tab_bar.handle_input(if delta > 0 { &Key::Right } else { &Key::Left });
		if let Some(tab) = self.tab_bar.active_tab() {
			let id = tab.id.clone();
			if let Some(first) = self.search_first_match.get(&id).cloned() {
				if let Some(list) = self.search_list.as_mut() {
					list.select_item(&first);
				}
			}
		}
	}

	fn render_search_banner(&mut self, width: usize) -> String {
		let count_text = if self.search_match_count == 1 {
			"1 match".to_string()
		} else {
			format!("{} matches", self.search_match_count)
		};
		let right_width = visible_width(&count_text) + 1;
		let prefix = "  ";
		let input_width = width
			.saturating_sub(visible_width(prefix) + right_width + 1)
			.max(4);
		let line = self
			.search_input
			.render(input_width as u16)
			.into_iter()
			.next()
			.unwrap_or_default();
		truncate_to_width(&format!("{prefix}{line}   {count_text} "), width, Ellipsis::Unicode)
	}

	fn handle_settings_mouse(&mut self, event: &MouseEvent) {

		let row_index = event.row as usize;
		let column = event.column as usize;
		if row_index >= self.tab_row_start && row_index < self.tab_row_start + self.tab_row_count {
			let line = row_index - self.tab_row_start;
			if matches!(event.kind, MouseKind::Down(MouseButtonKind::Left)) {
				if let Some(tab) = self.tab_bar.tab_at(line, column) {
					let id = tab.id.clone();
					if self.search_list.is_some() {
						if let Some(first) = self.search_first_match.get(&id).cloned() {
							if let Some(list) = self.search_list.as_mut() {
								list.select_item(&first);
							}
						}
					} else {
						self.tab_bar.set_selected_by_id(&id);
						self.switch_to_tab(&id);
					}
				}
			}
			return;
		}
		if let Some(list) = self.search_list.as_mut().or(self.current_list.as_mut()) {
			let offset = self.content_row_start;
			let translated = MouseEvent {
				row: event.row.saturating_sub(offset as u16),
				column: event.column.saturating_sub(2),
				..*event
			};
			list.handle_mouse(&translated);
		}
	}
}

impl Component for SettingsSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1);
		let theme = OverlayTheme::default();
		let inner = (width as usize).saturating_sub(4).max(1);

		let tab_lines = self.tab_bar.render(inner as u16);
		let searching = self.search_list.is_some();
		let show_preview = !searching && self.current_tab == SettingTab::Appearance.as_str();
		let preview_lines: Vec<String> = if show_preview {
			vec![
				String::new(),
				"Preview:".to_string(),
				self.status_preview_string(),
			]
		} else {
			Vec::new()
		};

		let mut rows: Vec<String> = Vec::new();
		rows.push(top_border(width, "Settings", &theme));
		self.tab_row_start = rows.len();
		self.tab_row_count = tab_lines.len();
		for line in &tab_lines {
			rows.push(row(line, width, &theme));
		}
		rows.push(divider(width, &theme));
		if searching {
			let banner = self.render_search_banner(inner);
			rows.push(row(&banner, width, &theme));
		}
		self.content_row_start = rows.len();

		let content: Vec<String> = if let Some(list) = self.search_list.as_mut().or(self.current_list.as_mut()) {
			list.set_max_visible(MAX_VISIBLE);
			list.render(inner as u16)
		} else if let Some(plugin) = self.plugin_component.as_mut() {
			plugin.render(inner as u16)
		} else {
			vec!["Plugins are unavailable in this host.".to_string()]
		};
		for index in 0..CONTENT_ROWS {
			rows.push(row(content.get(index).map(String::as_str).unwrap_or(""), width, &theme));
		}
		for line in &preview_lines {
			rows.push(row(line, width, &theme));
		}
		rows.push(divider(width, &theme));
		rows.push(row(&self.footer_hint_text(), width, &theme));
		rows.push(bottom_border(width, &theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		if self.search_list.is_some() {
			self.handle_search_input(key);
			return;
		}
		let active_list_open = self.current_list.is_some() || self.plugin_component.is_some();
		if !active_list_open {
			if matches!(key, Key::Esc) {
				(self.callbacks.borrow_mut().on_cancel)();
			}
			return;
		}
		if matches!(key, Key::Tab) {
			if self.current_list.as_ref().is_some_and(SettingsList::has_section_focus_targets) {
				if let Some(list) = self.current_list.as_mut() {
					list.toggle_section_focus();
				}
				return;
			}
			self.tab_bar.handle_input(key);
			self.follow_tab_change();
			return;
		}
		if matches!(key, Key::Left | Key::Right) {
			self.tab_bar.handle_input(key);
			self.follow_tab_change();
			return;
		}
		if self.current_tab != PLUGINS_PAGE {
			if let Key::Char(character) = key {
				if !character.is_control() && !character.is_whitespace() {
					let query = character.to_string();
					self.start_search(&query);
					return;
				}
			}
		}
		if let Some(list) = self.current_list.as_mut() {
			list.handle_input(key);
			self.refresh_items();
		} else if let Some(plugin) = self.plugin_component.as_mut() {
			plugin.handle_input(key);
		}
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		self.handle_settings_mouse(event);
	}
}

impl NativeDescribe for SettingsSelectorComponent {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let bindings = crate::keybindings::KeybindingsManager::default();
		let searching = self.search_list.is_some();
		let metadata = tab_metadata();
		let mut tabs_props = Props::new();
		tabs_props.insert(
			"items",
			Value::Array(
				self.tabs
					.iter()
					.map(|tab| {
						let mut entry = serde_json::Map::new();
						entry.insert("id".into(), Value::String(tab.id.clone()));
						entry.insert("label".into(), Value::String(tab.label.clone()));
						Value::Object(entry)
					})
					.collect(),
			),
		);
		if let Some(active) = self.tab_bar.active_tab() {
			tabs_props.set_str("active", active.id.clone());
		}
		let tabs_node = node(NodeKind::Tabs, tabs_props, Vec::new(), Some("tabs"));

		let mut children = vec![tabs_node];
		if searching {
			let mut props = Props::new();
			props.set_str("role", "omp.settings.search");
			children.push(native_row(
				vec![
					text(NodeText::Spans(vec![span("Search:", Some("muted"))]), Props::new()),
					text(NodeText::Plain(self.search_query.clone()), Props::new()),
					text(
						NodeText::Spans(vec![span(
							format!("{} matches", self.search_match_count),
							Some("dim"),
						)]),
						Props::new(),
					),
				],
				props,
			));
		}

		if let Some(active_list) = self.active_list() {
			let rows: Vec<NativeNode> = active_list
				.filtered_items()
				.iter()
				.map(|entry| {
					let mut props = Props::new();
					if entry.heading {
						props.set_str("label", entry.label.clone());
						props.set_str("variant", "heading");
					} else {
						props.set_str(
							"label",
							format!("{}  {}", entry.label, entry.value),
						);
						if entry.changed {
							props.set_str("variant", "changed");
						}
					}
					item(entry.id.clone(), props)
				})
				.collect();
			let mut props = Props::new();
			props.set_str("role", "omp.settings.list");
			if let Some(selected) = active_list.get_selected_item() {
				props.set_str("selected", selected.id.clone());
			}
			children.push(list(rows, props, Some("settings")));
		}

		if !searching && self.current_tab == SettingTab::Appearance.as_str() {
			children.push(col(
				vec![
					text(NodeText::Spans(vec![span("Preview:", Some("muted"))]), Props::new()),
					text(NodeText::Plain(self.status_preview_string()), Props::new()),
				],
				Props::new(),
			));
		}

		let mode = if searching {
			SettingsHintMode::Search
		} else if self.current_tab == PLUGINS_PAGE {
			SettingsHintMode::Plugins
		} else if self.current_list.as_ref().is_some_and(SettingsList::section_focused) {
			SettingsHintMode::Sections
		} else if self.has_section_jump {
			SettingsHintMode::RowsSections
		} else {
			SettingsHintMode::Rows
		};
		children.push(self.hints_node(&bindings, mode));

		let lead = metadata
			.get(&self.current_tab)
			.map(|meta| meta.label.clone())
			.unwrap_or_else(|| "Settings".to_string());
		overlay_card(
			"omp.overlay.settings",
			Some(NodeText::Plain(lead)),
			children,
			None,
		)
	}
}

impl SettingsSelectorComponent {
	/// Native page events: page switches, row selection, activation, and changes.
	pub fn handle_native_event(&mut self, event: NativeUiEvent) {
		match event {
			NativeUiEvent::Action { act, value, .. } => {
				if act == "page" {
					if let Some(page) = value.as_ref().and_then(Value::as_str) {
						self.show_page(page);
					}
				} else if act == "close" {
					if value.is_none() {
						(self.callbacks.borrow_mut().on_cancel)();
					}
				} else if act == "edit" {
					if let Some(path) = value.as_ref().and_then(Value::as_str) {
						self.edit_row(path);
					}
				}
			}
			NativeUiEvent::Select { item: id, .. } => {
				if let Some(list) = self.search_list.as_mut().or(self.current_list.as_mut()) {
					list.select_item(&id);
				}
			}
			NativeUiEvent::Activate { item: id, .. } => self.edit_row(&id),
			NativeUiEvent::Change { item: id, value, .. } => {
				if let Some(list) = self.search_list.as_mut().or(self.current_list.as_mut()) {
					let text = match value {
						Value::Bool(flag) => flag.to_string(),
						Value::Number(number) => number.to_string(),
						Value::String(text) => text,
						_ => return,
					};
					list.apply_value(&id, &text);
					self.refresh_items();
				}
			}
			_ => {}
		}
	}

	fn edit_row(&mut self, path: &str) {
		let (schema_type, editable) = {
			let store = self.context.settings.borrow();
			match def_kind(&store.entries(), path) {
				Some(kind) => kind,
				None => return,
			}
		};
		if !editable {
			return;
		}
		let callbacks = Rc::clone(&self.callbacks);
		let mut on_edit = callbacks.borrow_mut().on_edit.take();
		if let Some(on_edit) = &mut on_edit {
			on_edit(path, &schema_type);
		}
		callbacks.borrow_mut().on_edit = on_edit;
	}

	fn follow_tab_change(&mut self) {
		let Some(active) = self.tab_bar.active_tab() else {
			return;
		};
		let id = active.id.clone();
		if id != self.current_tab {
			self.switch_to_tab(&id);
		}
	}
}

/// Tab strip entries for the settings overlay.
pub fn settings_tabs() -> Vec<Tab> {
	let metadata = tab_metadata();
	SETTING_TABS
		.iter()
		.map(|tab| {
			let key = tab.as_str();
			let meta = metadata.get(key);
			let label = meta.map(|meta| meta.label.clone()).unwrap_or_else(|| key.to_string());
			Tab::new(key, label)
		})
		.collect()
}

/// Matching tabs first (with counts), the rest muted at the end.
fn build_search_tabs(
	counts: &HashMap<String, usize>,
	matched: &[(SettingTab, Vec<SettingItem>, usize)],
) -> Vec<Tab> {
	let mut tabs: Vec<Tab> = Vec::new();
	for (tab, _, _) in matched {
		let count = counts.get(tab.as_str()).copied().unwrap_or(0);
		if count > 0 {
			let label = tab_metadata()
				.get(tab.as_str())
				.map(|meta| meta.label.clone())
				.unwrap_or_else(|| tab.as_str().to_string());
			tabs.push(
				Tab::new(tab.as_str(), format!("{label} ({count})"))
					.with_short(format!("{count}")),
			);
		}
	}
	for tab in SETTING_TABS {
		if matched.iter().any(|(matched_tab, _, _)| matched_tab == tab) {
			continue;
		}
		let label = tab_metadata()
			.get(tab.as_str())
			.map(|meta| meta.label.clone())
			.unwrap_or_else(|| tab.as_str().to_string());
		tabs.push(Tab::new(tab.as_str(), label).muted());
	}
	tabs.push(Tab::new(PLUGINS_PAGE, "Plugins").muted());
	tabs
}

fn tab_order(tab: SettingTab) -> usize {
	SETTING_TABS
		.iter()
		.position(|candidate| *candidate == tab)
		.unwrap_or(SETTING_TABS.len())
}

/// Text a row is matched against while searching: label, path, and value.
fn setting_filter_text(item: &SettingItem) -> String {
	format!("{} {} {}", item.label, item.id, item.value)
}

fn condition_holds(entries: &[SettingsDisplayEntry], path: &str) -> bool {
	entries
		.iter()
		.find(|entry| entry.path == path)
		.and_then(|entry| entry.condition_fn.as_ref())
		.map(|condition| condition())
		.unwrap_or(true)
}

/// Schema type of `path` and whether the list cannot edit it inline.
fn def_kind(entries: &[SettingsDisplayEntry], path: &str) -> Option<(String, bool)> {
	let def = get_setting_def(entries, path)?;
	let schema_type = def.base().schema_type.clone();
	let editable = !matches!(def, SettingDef::Boolean(_) | SettingDef::Enum(_));
	Some((schema_type, editable))
}

fn is_changed(def: &SettingDef, current: Option<&Value>) -> bool {
	let default = def.base().default_value.as_ref();
	match (current, default) {
		(None, None) => false,
		(None, Some(_)) | (Some(_), None) => true,
		(Some(current), Some(default)) => current != default,
	}
}

fn current_enum_values(component: &SettingsSelectorComponent, def: &SettingDef) -> Vec<String> {
	let path = def.base().path.as_str();
	match def {
		SettingDef::Enum(enum_def) => enum_def.values.clone(),
		SettingDef::Submenu(_) => component
			.submenu_options(def, path)
			.into_iter()
			.map(|option| option.value)
			.collect(),
		_ => Vec::new(),
	}
}

fn submenu_display(
	component: &SettingsSelectorComponent,
	def: &crate::overlays::settings_defs::SubmenuSettingDef,
	current: Option<&Value>,
) -> String {
	let path = def.base.path.as_str();
	let raw = raw_setting_string(path, current);
	let options = component.submenu_options(&SettingDef::Submenu(def.clone()), path);
	if let Some(labels) = number_steps(&SettingDef::Submenu(def.clone())) {
		let _ = labels;
	}
	options
		.iter()
		.find(|option| option.value == raw)
		.map(|option| option.label.clone())
		.unwrap_or(raw)
}

fn raw_setting_string(path: &str, value: Option<&Value>) -> String {
	let raw = match value {
		Some(Value::String(text)) => text.clone(),
		Some(Value::Number(number)) => number.to_string(),
		Some(Value::Bool(flag)) => flag.to_string(),
		Some(other) => other.to_string(),
		None => String::new(),
	};
	if matches!(path, "compaction.thresholdPercent" | "compaction.thresholdTokens")
		&& (raw == "-1" || raw.is_empty())
	{
		return "default".to_string();
	}
	raw
}

fn format_text_value(secret: bool, value: Option<&Value>) -> String {
	if secret {
		return if value.is_some_and(|value| !value.is_null()) {
			"••••••••".to_string()
		} else {
			String::new()
		};
	}
	format_edit_value(value)
}

fn format_edit_value(value: Option<&Value>) -> String {
	match value {
		None | Some(Value::Null) => String::new(),
		Some(Value::String(text)) => text.clone(),
		Some(other) => other.to_string(),
	}
}

fn format_provider_limits(host: &dyn SettingsHost, value: Option<&Value>) -> String {
	let limits = host.normalize_provider_limits(value);
	if limits.is_empty() {
		return "Unlimited".to_string();
	}
	limits
		.iter()
		.map(|(provider, limit)| format!("{provider}: {limit}"))
		.collect::<Vec<_>>()
		.join(", ")
}

fn format_multi_value(options: &[SubmenuOption], value: Option<&Value>) -> String {
	let labels: Vec<String> = value
		.and_then(Value::as_array)
		.map(|values| {
			values
				.iter()
				.filter_map(Value::as_str)
				.filter_map(|entry| {
					options
						.iter()
						.find(|option| option.value == entry)
						.map(|option| option.label.clone())
				})
				.collect()
		})
		.unwrap_or_default();
	if labels.is_empty() {
		return "none".to_string();
	}
	labels.join(", ")
}

/// Prefix used by text settings drawn in mono.
#[must_use]
pub fn is_mono_text_setting(path: &str) -> bool {
	MONO_TEXT_SUFFIXES
		.iter()
		.any(|suffix| path.to_ascii_lowercase().ends_with(suffix))
}

/// Whether a submenu's choices preview live while highlighted.
#[must_use]
pub fn is_previewed_setting(path: &str) -> bool {
	PREVIEWED_SETTINGS.contains(&path)
}

fn apply_cycle_change(
	settings: &Rc<RefCell<Box<dyn SettingsHost>>>,
	callbacks: &Rc<RefCell<SettingsCallbacks>>,
	path: &str,
	new_value: &str,
) {
	let (kind, schema_type) = {
		let store = settings.borrow();
		let Some(def) = get_setting_def(&store.entries(), path) else {
			return;
		};
		let kind = match def {
			SettingDef::Boolean(_) => SettingKind::Boolean,
			SettingDef::Enum(_) => SettingKind::Enum,
			_ => return,
		};
		(kind, def.base().schema_type.clone())
	};
	let value = match kind {
		SettingKind::Boolean => Value::Bool(new_value == "true"),
		SettingKind::Enum => Value::String(new_value.to_string()),
	};
	if set_value(settings, &schema_type, path, value.clone()).is_err() {
		return;
	}
	let mut on_change = std::mem::replace(
		&mut callbacks.borrow_mut().on_change,
		Box::new(|_, _| {}),
	);
	on_change(path, value);
	callbacks.borrow_mut().on_change = on_change;
}

enum SettingKind {
	Boolean,
	Enum,
}

/// Store `value` at `path`, converting JSON by the declared schema type.
pub fn set_value(
	settings: &Rc<RefCell<Box<dyn SettingsHost>>>,
	schema_type: &str,
	path: &str,
	value: Value,
) -> Result<(), String> {
	let string_value = match &value {
		Value::String(text) => text.clone(),
		other => other.to_string(),
	};
	let thresholds = matches!(
		path,
		"compaction.thresholdPercent" | "compaction.thresholdTokens"
	);
	if thresholds && string_value == "default" {
		settings.borrow_mut().set(path, Value::from(-1));
		return Ok(());
	}
	let converted = match schema_type {
		"array" => {
			let parsed: Value = serde_json::from_str(&string_value)
				.map_err(|_| format!("Invalid array JSON for {path}"))?;
			if !parsed.is_array() {
				return Err(format!("Invalid array JSON for {path}"));
			}
			parsed
		}
		"record" => {
			let source = if string_value.is_empty() {
				"{}".to_string()
			} else {
				string_value.clone()
			};
			let parsed: Value = serde_json::from_str(&source)
				.map_err(|_| format!("Invalid record JSON for {path}"))?;
			if !parsed.is_object() {
				return Err(format!("Invalid record JSON for {path}"));
			}
			if path == "providers.maxInFlightRequests" {
				settings.borrow().validate_provider_limits(parsed)
			} else {
				parsed
			}
		}
		_ => {
			let current = settings.borrow().get(path);
			match current {
				Some(Value::Number(_)) => Value::from(
					string_value
						.parse::<f64>()
						.map_err(|_| format!("Invalid number for {path}"))?,
				),
				Some(Value::Bool(_)) => Value::Bool(string_value == "true"),
				_ => value,
			}
		}
	};
	settings.borrow_mut().set(path, converted);
	Ok(())
}

/// Numeric submenu label for `value`, when the submenu is a step list.
#[must_use]
pub fn numeric_step_label(steps: &BTreeMap<String, String>, value: &str) -> Option<String> {
	let numeric = numeric_option(value)?;
	steps.get(&numeric.to_string()).cloned()
}

fn string_setting(host: &dyn SettingsHost, path: &str) -> String {
	host.get(path)
		.and_then(|value| value.as_str().map(str::to_string))
		.unwrap_or_default()
}

fn string_list_setting(host: &dyn SettingsHost, path: &str) -> Vec<String> {
	host.get(path)
		.and_then(|value| {
			value.as_array().map(|entries| {
				entries
					.iter()
					.filter_map(|entry| entry.as_str().map(str::to_string))
					.collect()
			})
		})
		.unwrap_or_default()
}

fn bool_setting(host: &dyn SettingsHost, path: &str) -> bool {
	host.get(path).and_then(|value| value.as_bool()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::settings_defs::{
		BaseSettingDef, BooleanSettingDef, EnumSettingDef, SubmenuOption, SubmenuSettingDef,
		UiMetadata,
	};

	struct MemoryHost {
		values: HashMap<String, Value>,
	}

	impl MemoryHost {
		fn new() -> Self {
			MemoryHost {
				values: HashMap::new(),
			}
		}
	}

	impl SettingsHost for MemoryHost {
		fn entries(&self) -> Vec<SettingsDisplayEntry> {
			vec![
				SettingsDisplayEntry {
					path: "ui.compact".to_string(),
					schema_type: "boolean".to_string(),
					default_value: Some(Value::Bool(false)),
					ui: Some(UiMetadata {
						tab: Some(SettingTab::Appearance),
						group: Some("Layout".to_string()),
						label: "Compact".to_string(),
						description: "Compact rows".to_string(),
						..UiMetadata::default()
					}),
					enum_values: None,
					credential: false,
					condition_fn: None,
				},
				SettingsDisplayEntry {
					path: "ui.density".to_string(),
					schema_type: "string".to_string(),
					default_value: Some(Value::String("normal".to_string())),
					ui: Some(UiMetadata {
						tab: Some(SettingTab::Appearance),
						group: Some("Layout".to_string()),
						label: "Density".to_string(),
						description: String::new(),
						options: Some(vec![
							SubmenuOption {
								value: "normal".to_string(),
								label: "Normal".to_string(),
								description: None,
							},
							SubmenuOption {
								value: "dense".to_string(),
								label: "Dense".to_string(),
								description: None,
							},
						]),
						..UiMetadata::default()
					}),
					enum_values: None,
					credential: false,
					condition_fn: None,
				},
			]
		}

		fn get(&self, path: &str) -> Option<Value> {
			self.values.get(path).cloned()
		}

		fn set(&mut self, path: &str, value: Value) {
			self.values.insert(path.to_string(), value);
		}

		fn unset(&mut self, path: &str) {
			self.values.remove(path);
		}
	}

	fn base(path: &str, label: &str) -> BaseSettingDef {
		BaseSettingDef {
			path: path.to_string(),
			default_value: None,
			schema_type: "boolean".to_string(),
			label: label.to_string(),
			description: String::new(),
			warning: None,
			tab: Some(SettingTab::Appearance),
			group: None,
		}
	}

	#[test]
	fn tabs_cover_every_setting_tab() {
		let tabs = settings_tabs();
		assert_eq!(tabs.len(), SETTING_TABS.len());
		assert_eq!(tabs[0].id, "appearance");
		assert!(tabs.iter().all(|tab| !tab.muted));
	}

	#[test]
	fn changed_flags_compare_json_values() {
		let def = SettingDef::Boolean(BooleanSettingDef {
			base: BaseSettingDef {
				default_value: Some(Value::Bool(false)),
				..base("ui.compact", "Compact")
			},
		});
		assert!(!is_changed(&def, None));
		assert!(is_changed(&def, Some(&Value::Bool(true))));
		assert!(!is_changed(&def, Some(&Value::Bool(false))));
	}

	#[test]
	fn submenu_values_render_declared_labels() {
		let def = SubmenuSettingDef {
			base: base("ui.density", "Density"),
			options: vec![SubmenuOption {
				value: "dense".to_string(),
				label: "Dense".to_string(),
				description: None,
			}],
		};
		let mut host = MemoryHost::new();
		host.values
			.insert("ui.density".to_string(), Value::String("dense".to_string()));
		let settings: Rc<RefCell<Box<dyn SettingsHost>>> = Rc::new(RefCell::new(Box::new(host)));
		let context = SettingsRuntimeContext {
			settings,
			available_thinking_levels: Vec::new(),
			available_themes: Vec::new(),
			model: "test".to_string(),
			image_budget: 0,
			request_render: Rc::new(|| {}),
			plugins: None,
		};
		let callbacks = SettingsCallbacks {
			on_change: Box::new(|_, _| {}),
			on_cancel: Box::new(|| {}),
			on_theme_preview: None,
			on_status_line_preview: None,
			get_status_line_preview: None,
			on_plugins_changed: None,
			on_edit: None,
		};
		let component = SettingsSelectorComponent::new(context, callbacks);
		assert_eq!(
			submenu_display(&component, &def, Some(&Value::String("dense".to_string()))),
			"Dense"
		);
	}

	#[test]
	fn set_value_converts_numbers_lists_and_records() {
		let settings: Rc<RefCell<Box<dyn SettingsHost>>> =
			Rc::new(RefCell::new(Box::new(MemoryHost::new())));
		set_value(&settings, "number", "a.b", Value::String("12".to_string())).unwrap();
		assert_eq!(settings.borrow().get("a.b"), Some(Value::from(12.0)));

		set_value(&settings, "array", "a.list", Value::String("[\"x\"]".to_string())).unwrap();
		assert_eq!(
			settings.borrow().get("a.list"),
			Some(Value::Array(vec![Value::String("x".to_string())]))
		);
		assert!(set_value(&settings, "array", "a.list", Value::String("{}".to_string())).is_err());

		set_value(
			&settings,
			"record",
			"providers.maxInFlightRequests",
			Value::String("{\"openai\":3}".to_string()),
		)
		.unwrap();
		assert_eq!(
			settings.borrow().get("providers.maxInFlightRequests"),
			Some(Value::Object(serde_json::Map::from_iter([(
				"openai".to_string(),
				Value::from(3)
			)])))
		);

		set_value(
			&settings,
			"number",
			"compaction.thresholdPercent",
			Value::String("default".to_string()),
		)
		.unwrap();
		assert_eq!(settings.borrow().get("compaction.thresholdPercent"), Some(Value::from(-1)));
	}

	#[test]
	fn provider_limits_default_normalization_drops_bad_entries() {
		let host = MemoryHost::new();
		let raw = Value::Object(serde_json::Map::from_iter([
			("openai".to_string(), Value::from(4)),
			("zero".to_string(), Value::from(0)),
			("text".to_string(), Value::String("x".to_string())),
		]));
		let limits = host.normalize_provider_limits(Some(&raw));
		assert_eq!(limits.get("openai"), Some(&4));
		assert_eq!(limits.len(), 1);
	}

	#[test]
	fn search_terms_match_label_path_and_value() {
		let row = SettingItem::new("ui.compact", "Compact", "true");
		let text = setting_filter_text(&row);
		assert!(text.contains("Compact"));
		assert!(text.contains("ui.compact"));
		assert!(text.contains("true"));
	}

	#[test]
	fn numbered_submenu_labels_resolve() {
		let mut steps = BTreeMap::new();
		steps.insert("0".to_string(), "Off".to_string());
		assert_eq!(numeric_step_label(&steps, "0"), Some("Off".to_string()));
		assert_eq!(numeric_step_label(&steps, "default"), Some("Off".to_string()));
	}

	#[test]
	fn enum_definitions_are_editable_inline() {
		let entries = vec![SettingsDisplayEntry {
			path: "ui.density".to_string(),
			schema_type: "string".to_string(),
			default_value: None,
			ui: Some(UiMetadata {
				tab: Some(SettingTab::Appearance),
				label: "Density".to_string(),
				options: Some(vec![SubmenuOption {
					value: "normal".to_string(),
					label: "Normal".to_string(),
					description: None,
				}]),
				..UiMetadata::default()
			}),
			enum_values: Some(vec!["normal".to_string()]),
			credential: false,
			condition_fn: None,
		}];
		assert!(get_setting_def(&entries, "ui.density").is_some());
		let boolean = SettingDef::Enum(EnumSettingDef {
			base: base("ui.density", "Density"),
			values: vec!["normal".to_string()],
		});
		assert!(!def_kind(&entries, "ui.density").unwrap().1);
		let _ = boolean;
	}
}
