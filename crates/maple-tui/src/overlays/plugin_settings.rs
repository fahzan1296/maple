//! Reference: `l/src/overlays/plugin-settings.ts`.
//!
//! Plugin settings UI: a hierarchical interface over the two plugin
//! registries — an npm-style runtime plugin list and the marketplace install
//! list — whose rows open a per-plugin detail list of enablement toggles,
//! manifest feature toggles and manifest-declared settings.
//!
//! The three views (`PluginListComponent`, `PluginDetailComponent`,
//! `MarketplacePluginDetailComponent`) are also exposed as native settings
//! pages ([`PluginPrefsPage`]) for hosts that draw a prefs sheet instead of the
//! text-mode box.
//!
//! Deviations:
//! - The host's plugin managers are async `Promise`s in the reference; here
//!   they are the synchronous, fallible traits [`PluginSettingsManager`],
//!   [`PluginSettingsMarketplaceManager`] and [`PluginSettingsHost`]. The host
//!   owns the I/O and the `requestRender` scheduling.
//! - `PrefsControl`/`PrefsSection`/`PrefsEditing` come from the wire package in
//!   the reference; the crate has no prefs wire types, so they are defined
//!   here with the fields this module reads.
//! - The crate's `SettingsList` has no submenu callbacks, so a setting's value
//!   editor is described ([`PluginConfigField`]) and composed by the host from
//!   `components::form` instead of being built inline.
//! - The reference reads the process-global theme; text-mode rows and hints are
//!   uncoloured here and the host paints them.

use std::collections::{BTreeMap, HashMap};

use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::components::settings_list::{SettingItem, SettingsList};
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Span, Text as NodeText};
use crate::native::overlay::{action_hint, hints_row, overlay_card};

/// Semantic role of every plugin settings panel.
pub const PLUGIN_SETTINGS_ROLE: &str = "omp.overlay.plugin-settings";
/// Row id of the plugin enablement toggle.
pub const PLUGIN_ENABLED_ID: &str = "__enabled__";
/// Most plugin rows the list shows at once.
pub const PLUGIN_LIST_MAX_VISIBLE: usize = 8;
/// Most setting rows a detail list shows at once.
pub const PLUGIN_DETAIL_MAX_VISIBLE: usize = 10;
/// Stand-in shown for a secret setting that has a value.
pub const SECRET_PLACEHOLDER: &str = "••••••••";
/// Stand-in shown for a setting with no value.
pub const UNSET_VALUE: &str = "(not set)";

/// A plugin row's control on the native settings page: open the plugin's
/// settings.
#[must_use]
pub fn plugin_configure() -> PrefsControl {
	PrefsControl::Action {
		label: "Configure".to_string(),
		act: "open".to_string(),
	}
}

/// One control of a native prefs row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefsControl {
	/// An on/off switch.
	Switch {
		/// Whether the switch is on.
		on: bool,
	},
	/// A button that sends `act` (with the row's id as its value).
	Action {
		/// Button label.
		label: String,
		/// Action id.
		act: String,
	},
}

impl PrefsControl {
	/// The wire props of this control.
	#[must_use]
	pub fn to_props(&self) -> Props {
		let mut props = Props::new();
		match self {
			PrefsControl::Switch { on } => {
				props.set_str("k", "switch");
				props.set_bool("on", *on);
			}
			PrefsControl::Action { label, act } => {
				props.set_str("k", "action");
				props.set_str("label", label.as_str());
				props.set_str("act", act.as_str());
			}
		}
		props
	}
}

/// One row of a native prefs section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefsRow {
	/// Row identity (the setting id).
	pub id: String,
	/// Row label, trimmed.
	pub label: String,
	/// Secondary text.
	pub hint: Option<String>,
	/// Warning text.
	pub warning: Option<String>,
	/// The row's control.
	pub control: PrefsControl,
}

impl PrefsRow {
	/// The wire props of this row.
	#[must_use]
	pub fn to_props(&self) -> Props {
		let mut props = Props::new();
		props.set_str("id", self.id.as_str());
		props.set_str("label", self.label.as_str());
		if let Some(hint) = &self.hint {
			props.set_str("hint", hint.as_str());
		}
		if let Some(warning) = &self.warning {
			props.set_str("warning", warning.as_str());
		}
		props.insert("control", self.control.to_props().to_value());
		props
	}
}

/// One section of a native prefs page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefsSection {
	/// Section identity.
	pub id: String,
	/// Section heading.
	pub title: String,
	/// Section rows.
	pub rows: Vec<PrefsRow>,
}

/// An open value editor drawn over the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefsEditing {
	/// A setting row's editor is open.
	Row {
		/// The row whose editor is open.
		id: String,
	},
}

/// The plugins tab as a native settings page draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginPrefsPage {
	/// Lead paragraph.
	pub lead: String,
	/// Sections in order.
	pub sections: Vec<PrefsSection>,
	/// Focused row id.
	pub focus: Option<String>,
	/// The open value editor, if any.
	pub editing: Option<PrefsEditing>,
	/// An open value editor without a native control, drawn over the page.
	pub editor: Option<NativeNode>,
}

impl PluginPrefsPage {
	/// A page with no sections (still loading, or nothing to show).
	#[must_use]
	pub fn empty(lead: impl Into<String>) -> Self {
		PluginPrefsPage {
			lead: lead.into(),
			sections: Vec::new(),
			focus: None,
			editing: None,
			editor: None,
		}
	}
}

/// A plugin detail list as native page sections: on/off rows as switches, value
/// rows as an edit button showing the value.
#[must_use]
pub fn prefs_detail_page(title: &str, lead: &str, list: Option<&SettingsList>) -> PluginPrefsPage {
	let Some(list) = list else {
		return PluginPrefsPage::empty(lead);
	};
	let rows: Vec<PrefsRow> = list
		.items()
		.iter()
		.map(|item| {
			let is_switch = item.values.as_ref().is_some_and(|values| {
				values.len() == 2
					&& values.iter().any(|v| v.as_str() == "true")
					&& values.iter().any(|v| v.as_str() == "false")
			});
			let control = if is_switch {
				PrefsControl::Switch {
					on: item.value == "true",
				}
			} else {
				let label = item.value.trim();
				PrefsControl::Action {
					label: if label.is_empty() { "Edit…".to_string() } else { label.to_string() },
					act: "edit".to_string(),
				}
			};
			PrefsRow {
				id: item.id.clone(),
				label: item.label.trim().to_string(),
				hint: item.description.clone(),
				warning: item.warning.clone(),
				control,
			}
		})
		.collect();
	PluginPrefsPage {
		lead: lead.to_string(),
		sections: vec![PrefsSection {
			id: "plugin".to_string(),
			title: title.to_string(),
			rows,
		}],
		focus: list.get_selected_item().map(|item| item.id.clone()),
		editing: None,
		editor: None,
	}
}

/// A native page event on a plugin detail list, through the list's own code
/// paths: a switch change applies the value, a row click selects it, an
/// activation opens its editor, and `close` on the open editor dismisses it.
pub fn prefs_detail_event(list: Option<&mut SettingsList>, event: &NativeUiEvent) -> bool {
	let Some(list) = list else {
		return false;
	};
	match event {
		NativeUiEvent::Change { item, value, .. } => match value {
			serde_json::Value::Bool(flag) => {
				list.apply_value(item, if *flag { "true" } else { "false" });
				true
			}
			_ => false,
		},
		NativeUiEvent::Select { item, .. } => {
			if list.select_item(item) {
				return true;
			}
			list.select_item(item)
		}
		NativeUiEvent::Activate { item, .. } => list.select_item(item),
		NativeUiEvent::Action { act, value, .. } => {
			if act != "edit" {
				return false;
			}
			match value.as_ref().and_then(|value| value.as_str()) {
				Some(id) => list.select_item(id),
				None => false,
			}
		}
		_ => false,
	}
}

/// The declared type and metadata of one plugin setting.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginSettingSchema {
	/// A free-form string.
	String {
		/// Description shown to the user.
		description: Option<String>,
		/// Whether the value is a secret.
		secret: bool,
		/// Value used when none is stored.
		default: Option<String>,
	},
	/// A number, optionally bounded.
	Number {
		/// Description shown to the user.
		description: Option<String>,
		/// Whether the value is a secret.
		secret: bool,
		/// Value used when none is stored.
		default: Option<f64>,
		/// Smallest accepted value.
		min: Option<f64>,
		/// Largest accepted value.
		max: Option<f64>,
	},
	/// A boolean.
	Boolean {
		/// Description shown to the user.
		description: Option<String>,
		/// Whether the value is a secret.
		secret: bool,
		/// Value used when none is stored.
		default: Option<bool>,
	},
	/// One of a fixed set of spellings.
	Enum {
		/// Description shown to the user.
		description: Option<String>,
		/// Whether the value is a secret.
		secret: bool,
		/// Value used when none is stored.
		default: Option<String>,
		/// Accepted spellings.
		values: Vec<String>,
	},
}

impl PluginSettingSchema {
	/// The declared description.
	#[must_use]
	pub fn description(&self) -> Option<&str> {
		match self {
			PluginSettingSchema::String { description, .. }
			| PluginSettingSchema::Number { description, .. }
			| PluginSettingSchema::Boolean { description, .. }
			| PluginSettingSchema::Enum { description, .. } => description.as_deref(),
		}
	}

	/// Whether the value is a secret.
	#[must_use]
	pub fn secret(&self) -> bool {
		match self {
			PluginSettingSchema::String { secret, .. }
			| PluginSettingSchema::Number { secret, .. }
			| PluginSettingSchema::Boolean { secret, .. }
			| PluginSettingSchema::Enum { secret, .. } => *secret,
		}
	}

	/// The type spelling (`string`, `number`, `boolean`, `enum`).
	#[must_use]
	pub fn type_name(&self) -> &'static str {
		match self {
			PluginSettingSchema::String { .. } => "string",
			PluginSettingSchema::Number { .. } => "number",
			PluginSettingSchema::Boolean { .. } => "boolean",
			PluginSettingSchema::Enum { .. } => "enum",
		}
	}

	/// The declared default as display text.
	#[must_use]
	pub fn default_text(&self) -> Option<String> {
		match self {
			PluginSettingSchema::String { default, .. } | PluginSettingSchema::Enum { default, .. } => {
				default.clone()
			}
			PluginSettingSchema::Number { default, .. } => default.map(|value| format!("{value}")),
			PluginSettingSchema::Boolean { default, .. } => default.map(|value| value.to_string()),
		}
	}

	/// The type hint shown under a value editor (`Type: number (1..10)`).
	#[must_use]
	pub fn type_hint(&self) -> String {
		let mut hint = format!("Type: {}", self.type_name());
		if let PluginSettingSchema::Number { min, max, .. } = self {
			if min.is_some() || max.is_some() {
				let min = min.map(|value| format!("{value}")).unwrap_or_default();
				let max = max.map(|value| format!("{value}")).unwrap_or_default();
				hint.push_str(&format!(" ({min}..{max})"));
			}
		}
		hint
	}
}

/// One manifest feature a plugin can switch on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginFeature {
	/// Description shown to the user.
	pub description: Option<String>,
	/// Whether the feature is on when the plugin has no explicit list.
	pub default: bool,
}

/// The manifest fields the settings UI reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginManifest {
	/// Plugin description.
	pub description: Option<String>,
	/// Declared features.
	pub features: BTreeMap<String, PluginFeature>,
	/// Declared settings.
	pub settings: BTreeMap<String, PluginSettingSchema>,
}

/// Installed runtime plugin fields consumed by the settings UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstalledPlugin {
	/// Plugin name.
	pub name: String,
	/// Installed version.
	pub version: String,
	/// Whether the plugin is enabled.
	pub enabled: bool,
	/// Explicitly enabled features; `None` means the manifest defaults apply.
	pub enabled_features: Option<Vec<String>>,
	/// The plugin's manifest.
	pub manifest: PluginManifest,
}

/// One installed marketplace entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstalledPluginEntry {
	/// Where the plugin is installed.
	pub install_path: String,
	/// Installed version.
	pub version: String,
	/// Install timestamp.
	pub installed_at: String,
	/// Last update timestamp.
	pub last_updated: String,
	/// Git commit the install came from.
	pub git_commit_sha: Option<String>,
	/// Whether this entry is enabled; `None` counts as enabled.
	pub enabled: Option<bool>,
}

/// Installed marketplace metadata consumed by the settings UI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstalledPluginSummary {
	/// Marketplace plugin id (`name@marketplace`).
	pub id: String,
	/// Scope of the install.
	pub scope: Option<crate::overlays::plugin_selector::PluginScope>,
	/// Set when a project install shadows this one.
	pub shadowed_by: Option<String>,
	/// Installed entries; the first is the effective install.
	pub entries: Vec<InstalledPluginEntry>,
}

/// Runtime plugin manager capabilities required by the settings UI.
pub trait PluginSettingsManager {
	/// Every installed runtime plugin.
	fn list(&mut self) -> Result<Vec<InstalledPlugin>, String>;
	/// One plugin by name, optionally from a specific install path.
	fn get_plugin(&mut self, name: &str, path: Option<&str>) -> Result<Option<InstalledPlugin>, String>;
	/// The plugin's stored setting values.
	fn get_plugin_settings(&mut self, name: &str) -> Result<HashMap<String, serde_json::Value>, String>;
	/// Enable or disable a plugin.
	fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String>;
	/// The plugin's explicitly enabled features.
	fn get_enabled_features(&mut self, name: &str) -> Result<Option<Vec<String>>, String>;
	/// Replace the plugin's enabled features; `None` restores the defaults.
	fn set_enabled_features(&mut self, name: &str, features: Option<Vec<String>>) -> Result<(), String>;
	/// Store one setting value.
	fn set_plugin_setting(&mut self, name: &str, key: &str, value: serde_json::Value) -> Result<(), String>;
}

/// Marketplace manager capabilities required by the settings UI.
pub trait PluginSettingsMarketplaceManager {
	/// Every installed marketplace plugin.
	fn list_installed_plugins(&mut self) -> Result<Vec<InstalledPluginSummary>, String>;
	/// Enable or disable a marketplace plugin.
	fn set_plugin_enabled(
		&mut self,
		plugin_id: &str,
		enabled: bool,
		scope: Option<crate::overlays::plugin_selector::PluginScope>,
	) -> Result<(), String>;
}

/// Host-owned managers and plugin identifier parsing.
pub trait PluginSettingsHost {
	/// The runtime plugin manager.
	fn manager(&mut self) -> &mut dyn PluginSettingsManager;
	/// Build a marketplace manager.
	fn create_marketplace_manager(&mut self) -> Result<Box<dyn PluginSettingsMarketplaceManager>, String>;
	/// The plugin name inside a marketplace plugin id.
	fn parse_plugin_id(&self, id: &str) -> Option<String>;
}

/// Forward a key to `input`, but cancel via `on_cancel` on Escape.
///
/// The reference decodes Escape through `matchesKey` because the fullscreen
/// overlay runs the kitty keyboard protocol; the crate's [`Key`] already
/// carries the decoded key, so the check is [`Key::Esc`].
pub fn handle_input_or_escape(key: &Key, input: &mut dyn Component, on_cancel: &mut dyn FnMut()) {
	if matches!(key, Key::Esc) {
		on_cancel();
		return;
	}
	input.handle_input(key);
}

/// One row in the unified plugin list.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginListEntry {
	/// An npm-style runtime plugin.
	Npm(InstalledPlugin),
	/// A marketplace-installed plugin.
	Marketplace(InstalledPluginSummary),
}

/// What a plugin list selection reports.
pub struct PluginListCallbacks {
	/// Runs with the chosen runtime plugin.
	pub on_npm_select: Box<dyn FnMut(&InstalledPlugin)>,
	/// Runs with the chosen marketplace plugin.
	pub on_marketplace_select: Box<dyn FnMut(&InstalledPluginSummary)>,
	/// Runs when the list is dismissed.
	pub on_cancel: Box<dyn FnMut()>,
}

impl PluginListCallbacks {
	/// Build the callback trio.
	pub fn new(
		on_npm_select: impl FnMut(&InstalledPlugin) + 'static,
		on_marketplace_select: impl FnMut(&InstalledPluginSummary) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		PluginListCallbacks {
			on_npm_select: Box::new(on_npm_select),
			on_marketplace_select: Box::new(on_marketplace_select),
			on_cancel: Box::new(on_cancel),
		}
	}
}

/// True when the marketplace summary's first entry is not explicitly disabled:
/// a missing `enabled` flag means enabled, matching `/plugins list`.
#[must_use]
pub fn marketplace_enabled(summary: &InstalledPluginSummary) -> bool {
	summary.entries.first().and_then(|entry| entry.enabled) != Some(false)
}

/// The features a plugin has switched on, honouring the manifest defaults when
/// the plugin has no explicit list.
#[must_use]
pub fn effective_enabled_features(plugin: &InstalledPlugin) -> Vec<String> {
	match &plugin.enabled_features {
		Some(features) => features.clone(),
		None => plugin
			.manifest
			.features
			.iter()
			.filter(|(_, feature)| feature.default)
			.map(|(name, _)| name.clone())
			.collect(),
	}
}

/// The stored or default display text of one setting, masked when secret.
#[must_use]
pub fn setting_display_value(
	schema: &PluginSettingSchema,
	value: Option<&serde_json::Value>,
) -> String {
	let text = match value {
		Some(serde_json::Value::String(text)) => Some(text.clone()),
		Some(serde_json::Value::Number(number)) => Some(number.to_string()),
		Some(serde_json::Value::Bool(flag)) => Some(flag.to_string()),
		Some(serde_json::Value::Null) | None => schema.default_text(),
		Some(other) => Some(other.to_string()),
	};
	let empty = text.is_none();
	let text = text.unwrap_or_else(|| UNSET_VALUE.to_string());
	if schema.secret() && !empty && text != "false" {
		return SECRET_PLACEHOLDER.to_string();
	}
	text
}

/// One manifest setting as the detail list shows it, plus what its editor needs.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginConfigField {
	/// A boolean setting: a two-value row.
	Bool {
		/// The list row.
		item: SettingItem,
	},
	/// An enum setting: a two-value-free row with a choice editor.
	Enum {
		/// The setting key.
		key: String,
		/// The list row.
		item: SettingItem,
		/// Values the editor offers.
		values: Vec<String>,
	},
	/// A string or number setting: a row with a text editor.
	Value {
		/// The setting key.
		key: String,
		/// The list row.
		item: SettingItem,
		/// The declared schema (for the editor's type hint, secret flag, range).
		schema: PluginSettingSchema,
	},
}

impl PluginConfigField {
	/// The list row.
	#[must_use]
	pub fn item(&self) -> &SettingItem {
		match self {
			PluginConfigField::Bool { item }
			| PluginConfigField::Enum { item, .. }
			| PluginConfigField::Value { item, .. } => item,
		}
	}

	/// The row identity (`config:<key>`).
	#[must_use]
	pub fn id(&self) -> &str {
		self.item().id.as_str()
	}
}

/// The manifest settings of `plugin` as detail rows, ordered as declared.
#[must_use]
pub fn plugin_config_items(
	plugin: &InstalledPlugin,
	settings: &HashMap<String, serde_json::Value>,
) -> Vec<PluginConfigField> {
	let mut fields = Vec::with_capacity(plugin.manifest.settings.len());
	for (key, schema) in &plugin.manifest.settings {
		let current = settings.get(key);
		let description = schema
			.description()
			.map(str::to_string)
			.unwrap_or_else(|| format!("Configure {key}"));
		let id = format!("config:{key}");
		let label = format!("  {key}");
		match schema {
			PluginSettingSchema::Boolean { .. } => {
				let on = match current {
					Some(serde_json::Value::Bool(flag)) => *flag,
					_ => schema.default_text().as_deref() == Some("true"),
				};
				fields.push(PluginConfigField::Bool {
					item: SettingItem::new(id, label, if on { "true" } else { "false" })
						.with_description(description)
						.with_values(vec!["true".to_string(), "false".to_string()]),
				});
			}
			PluginSettingSchema::Enum { values, .. } => {
				let display = setting_display_value(schema, current);
				let current_value = if display == UNSET_VALUE {
					schema.default_text().unwrap_or_default()
				} else {
					display
				};
				fields.push(PluginConfigField::Enum {
					key: key.clone(),
					item: SettingItem::new(id, label, current_value).with_description(description),
					values: values.clone(),
				});
			}
			PluginSettingSchema::String { .. } | PluginSettingSchema::Number { .. } => {
				fields.push(PluginConfigField::Value {
					key: key.clone(),
					item: SettingItem::new(id, label, setting_display_value(schema, current))
						.with_description(description),
					schema: schema.clone(),
				});
			}
		}
	}
	fields
}

/// The rows of a plugin's detail list: the enablement toggle, one toggle per
/// manifest feature, then the manifest settings.
#[must_use]
pub fn plugin_detail_items(
	plugin: &InstalledPlugin,
	settings: &HashMap<String, serde_json::Value>,
) -> (Vec<SettingItem>, Vec<PluginConfigField>) {
	let mut items = vec![SettingItem::new(PLUGIN_ENABLED_ID, "Enabled", if plugin.enabled { "true" } else { "false" })
		.with_description("Enable or disable this plugin")
		.with_values(vec!["true".to_string(), "false".to_string()])];
	let enabled = effective_enabled_features(plugin);
	for (name, feature) in &plugin.manifest.features {
		let description = feature
			.description
			.clone()
			.unwrap_or_else(|| format!("Enable {name} feature"));
		items.push(
			SettingItem::new(
				format!("feature:{name}"),
				format!("  {name}"),
				if enabled.iter().any(|enabled| enabled == name) { "true" } else { "false" },
			)
			.with_description(description)
			.with_values(vec!["true".to_string(), "false".to_string()]),
		);
	}
	let fields = plugin_config_items(plugin, settings);
	items.extend(fields.iter().map(|field| field.item().clone()));
	(items, fields)
}

/// The rows an add or remove of features produces, applied to the plugin.
pub fn toggle_feature(plugin: &mut InstalledPlugin, feature: &str, enabled: bool) {
	let mut current: Vec<String> = effective_enabled_features(plugin);
	if enabled {
		if !current.iter().any(|name| name == feature) {
			current.push(feature.to_string());
		}
	} else {
		current.retain(|name| name != feature);
	}
	plugin.enabled_features = Some(current);
}

/// What a plugin detail change reports.
pub struct PluginDetailCallbacks {
	/// Enablement changed.
	pub on_enabled_change: Box<dyn FnMut(bool)>,
	/// A feature was switched.
	pub on_feature_change: Box<dyn FnMut(&str, bool)>,
	/// A setting value changed.
	pub on_config_change: Box<dyn FnMut(&str, serde_json::Value)>,
	/// Go back to the plugin list.
	pub on_back: Box<dyn FnMut()>,
}

impl PluginDetailCallbacks {
	/// Build the callback set.
	pub fn new(
		on_enabled_change: impl FnMut(bool) + 'static,
		on_feature_change: impl FnMut(&str, bool) + 'static,
		on_config_change: impl FnMut(&str, serde_json::Value) + 'static,
		on_back: impl FnMut() + 'static,
	) -> Self {
		PluginDetailCallbacks {
			on_enabled_change: Box::new(on_enabled_change),
			on_feature_change: Box::new(on_feature_change),
			on_config_change: Box::new(on_config_change),
			on_back: Box::new(on_back),
		}
	}
}

/// One row of the unified plugin list, with status, kind, version and scope.
#[must_use]
pub fn plugin_entry_item(entry: &PluginListEntry) -> SelectItem {
	match entry {
		PluginListEntry::Npm(plugin) => {
			let feature_count = plugin.manifest.features.len();
			let enabled_count = plugin
				.enabled_features
				.as_ref()
				.map(Vec::len)
				.unwrap_or(feature_count);
			let mut details = format!("[npm] · v{}", plugin.version);
			if feature_count > 0 {
				details.push_str(&format!(" · {enabled_count}/{feature_count} features"));
			}
			SelectItem::new(plugin_entry_value(entry), format!("{} {}", status_word(plugin.enabled), plugin.name))
				.with_description(details)
		}
		PluginListEntry::Marketplace(summary) => {
			let enabled = marketplace_enabled(summary);
			let scope = summary.scope.map(|scope| scope.as_str()).unwrap_or("-");
			let mut details = format!("[marketplace] [{scope}] · v{}", entry_version(summary));
			if let Some(shadowed_by) = &summary.shadowed_by {
				details.push_str(&format!(" · shadowed by {shadowed_by}"));
			}
			let shadow = if summary.shadowed_by.is_some() { " (shadowed)" } else { "" };
			SelectItem::new(
				plugin_entry_value(entry),
				format!("{} {}{shadow}", status_word(enabled), summary.id),
			)
			.with_description(details)
		}
	}
}

fn status_word(enabled: bool) -> &'static str {
	if enabled { "Enabled" } else { "Disabled" }
}

fn entry_version(summary: &InstalledPluginSummary) -> &str {
	summary.entries.first().map(|entry| entry.version.as_str()).unwrap_or("?")
}

/// Stable list value of an entry; combined with [`find_entry_by_value`] it keeps
/// lookup correct when the same plugin id exists in both scopes.
#[must_use]
pub fn plugin_entry_value(entry: &PluginListEntry) -> String {
	match entry {
		PluginListEntry::Npm(plugin) => format!("npm:{}", plugin.name),
		PluginListEntry::Marketplace(summary) => format!(
			"mkt:{}:{}",
			summary.scope.map(|scope| scope.as_str()).unwrap_or("-"),
			summary.id
		),
	}
}

/// The entry with `value`, if any.
#[must_use]
pub fn find_entry_by_value<'a>(entries: &'a [PluginListEntry], value: &str) -> Option<&'a PluginListEntry> {
	entries.iter().find(|entry| plugin_entry_value(entry) == value)
}

/// The plugin list as one native page section.
#[must_use]
pub fn plugin_list_page(entries: &[PluginListEntry], focus: Option<String>) -> PluginPrefsPage {
	let rows: Vec<PrefsRow> = entries
		.iter()
		.map(|entry| match entry {
			PluginListEntry::Npm(plugin) => {
				let features = plugin.manifest.features.len();
				let mut parts = vec![status_word(plugin.enabled).to_string(), "npm".to_string(), format!("v{}", plugin.version)];
				if features > 0 {
					let enabled = plugin.enabled_features.as_ref().map(Vec::len).unwrap_or(features);
					parts.push(format!("{enabled}/{features} features"));
				}
				PrefsRow {
					id: plugin_entry_value(entry),
					label: plugin.name.clone(),
					hint: Some(parts.join(" · ")),
					warning: None,
					control: plugin_configure(),
				}
			}
			PluginListEntry::Marketplace(summary) => {
				let parts = vec![
					status_word(marketplace_enabled(summary)).to_string(),
					"marketplace".to_string(),
					summary.scope.map(|scope| scope.as_str()).unwrap_or("-").to_string(),
					format!("v{}", entry_version(summary)),
				];
				PrefsRow {
					id: plugin_entry_value(entry),
					label: summary.id.clone(),
					hint: Some(parts.join(" · ")),
					warning: summary
						.shadowed_by
						.as_ref()
						.map(|shadowed_by| format!("Shadowed by the {shadowed_by} install")),
					control: plugin_configure(),
				}
			}
		})
		.collect();
	let lead = if rows.is_empty() {
		"No plugins installed. Install one with omp plugin install <package>, or <name>@<marketplace>."
	} else {
		"Plugins installed for you and this project. Configure one to turn it or its features on and off."
	};
	PluginPrefsPage {
		lead: lead.to_string(),
		sections: if rows.is_empty() {
			Vec::new()
		} else {
			vec![PrefsSection {
				id: "installed".to_string(),
				title: "Installed".to_string(),
				rows,
			}]
		},
		focus,
		editing: None,
		editor: None,
	}
}

/// Shows installed plugins from both registries with enable/disable status,
/// scope tag and shadow indicator.
pub struct PluginListComponent {
	entries: Vec<PluginListEntry>,
	list: SelectList,
	role: String,
	theme: crate::chrome::overlay_box::OverlayTheme,
	callbacks: std::rc::Rc<std::cell::RefCell<PluginListCallbacks>>,
}

impl std::fmt::Debug for PluginListComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PluginListComponent")
			.field("entries", &self.entries.len())
			.finish()
	}
}

impl PluginListComponent {
	/// Build the list over `entries`.
	pub fn new(entries: Vec<PluginListEntry>, callbacks: PluginListCallbacks) -> Self {
		let items: Vec<SelectItem> = entries.iter().map(plugin_entry_item).collect();
		let max_visible = items.len().min(PLUGIN_LIST_MAX_VISIBLE);
		let mut list = SelectList::new(items, max_visible.max(1), SelectListTheme::default());
		let callbacks = std::rc::Rc::new(std::cell::RefCell::new(callbacks));
		let lookup = entries.clone();
		let select_sink = std::rc::Rc::clone(&callbacks);
		list.on_select(move |item| match find_entry_by_value(&lookup, &item.key) {
			Some(PluginListEntry::Npm(plugin)) => (select_sink.borrow_mut().on_npm_select)(plugin),
			Some(PluginListEntry::Marketplace(summary)) => {
				(select_sink.borrow_mut().on_marketplace_select)(summary);
			}
			None => {}
		});
		let cancel_sink = std::rc::Rc::clone(&callbacks);
		list.on_cancel(move || (cancel_sink.borrow_mut().on_cancel)());
		PluginListComponent {
			entries,
			list,
			role: PLUGIN_SETTINGS_ROLE.to_string(),
			theme: crate::chrome::overlay_box::OverlayTheme::default(),
			callbacks,
		}
	}

	/// The entries in order.
	pub fn entries(&self) -> &[PluginListEntry] {
		&self.entries
	}

	/// The terminal list, for hosts that drive it directly.
	pub fn select_list(&self) -> &SelectList {
		&self.list
	}

	/// The terminal list, mutably.
	pub fn select_list_mut(&mut self) -> &mut SelectList {
		&mut self.list
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The collapsed title.
	pub fn title(&self) -> &str {
		"Plugins"
	}

	/// The installed plugins as one native page section.
	#[must_use]
	pub fn prefs_page(&self) -> PluginPrefsPage {
		plugin_list_page(&self.entries, self.list.selected_key())
	}

	/// A native page event: a row click moves the selection, Configure (or a
	/// double click) opens the plugin.
	pub fn handle_prefs_event(&mut self, event: &NativeUiEvent) -> bool {
		match event {
			NativeUiEvent::Select { item, .. } => {
				let found = self
					.list
					.visible_items()
					.iter()
					.position(|row| &row.key == item);
				match found {
					Some(index) => {
						self.list.set_selected_index(index as isize);
						true
					}
					None => false,
				}
			}
			NativeUiEvent::Activate { item, .. } => self.activate(item),
			NativeUiEvent::Action { act, value, .. } if act == "open" => match value.as_ref().and_then(|v| v.as_str()) {
				Some(id) => self.activate(id),
				None => false,
			},
			_ => false,
		}
	}

	fn activate(&mut self, id: &str) -> bool {
		let entry = find_entry_by_value(&self.entries, id).cloned();
		let mut callbacks = self.callbacks.borrow_mut();
		match entry {
			Some(PluginListEntry::Npm(plugin)) => {
				(callbacks.on_npm_select)(&plugin);
				true
			}
			Some(PluginListEntry::Marketplace(summary)) => {
				(callbacks.on_marketplace_select)(&summary);
				true
			}
			None => false,
		}
	}

	/// Text rows for `width`, including the rounded border chrome.
	fn rows(&mut self, width: u16) -> Vec<String> {
		use crate::chrome::overlay_box::{bottom_border, row, top_border};
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let mut rows = vec![top_border(width, "Plugins", &self.theme), row("", width, &self.theme)];
		if self.entries.is_empty() {
			rows.push(row("No plugins installed", width, &self.theme));
			rows.push(row("Install npm plugins:        omp plugin install <package>", width, &self.theme));
			rows.push(row(
				"Install marketplace plugins: omp plugin install <name>@<marketplace>",
				width,
				&self.theme,
			));
		} else {
			for line in self.list.render(inner) {
				rows.push(row(&line, width, &self.theme));
			}
		}
		rows.push(row("", width, &self.theme));
		rows.push(row("enter to configure · esc to go back", width, &self.theme));
		rows.push(bottom_border(width, &self.theme));
		rows
	}
}

impl Component for PluginListComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.list.handle_input(key);
	}
}

impl NativeDescribe for PluginListComponent {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let body = if self.entries.is_empty() {
			let mut children = vec![crate::native::describe::text(
				NodeText::spans(vec![Span::styled("No plugins installed", "muted")]),
				Props::new(),
			)];
			children.push(hints_row(
				&[action_hint(
					&crate::keybindings::KeybindingsManager::new(),
					"tui.select.cancel",
					"go back",
				)],
				"plugin-list-hints",
			));
			children
		} else {
			let items: Vec<NativeNode> = self
				.entries
				.iter()
				.map(|entry| {
					let item = plugin_entry_item(entry);
					let mut props = Props::new();
					let dot = match entry {
						PluginListEntry::Npm(plugin) if plugin.enabled => "success",
						PluginListEntry::Npm(_) => "dim",
						PluginListEntry::Marketplace(summary) if marketplace_enabled(summary) => "success",
						PluginListEntry::Marketplace(_) => "dim",
					};
					props.set_spans(vec![
						Span::styled("● ", dot),
						Span::styled(item.title.clone(), "default"),
					]);
					if let Some(description) = &item.description {
						props.set_str("description", description.as_str());
					}
					crate::native::describe::item(plugin_entry_value(entry), props)
				})
				.collect();
			let mut list_props = Props::new();
			list_props.insert("selected", serde_json::json!(self.list.selected_key()));
			list_props.set_i64("max", PLUGIN_LIST_MAX_VISIBLE as i64);
			vec![crate::native::describe::list(items, list_props, Some("plugins"))]
		};
		overlay_card(&self.role, Some(NodeText::plain("Plugins")), body, None)
	}
}

/// Shows detail settings for a single runtime plugin: the enablement toggle,
/// its feature toggles and its manifest settings.
pub struct PluginDetailComponent {
	plugin: InstalledPlugin,
	settings: HashMap<String, serde_json::Value>,
	items: Vec<SettingItem>,
	config: Vec<PluginConfigField>,
	role: String,
	list: Option<SettingsList>,
	callbacks: PluginDetailCallbacks,
}

impl std::fmt::Debug for PluginDetailComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PluginDetailComponent")
			.field("plugin", &self.plugin.name)
			.field("items", &self.items.len())
			.finish()
	}
}

impl PluginDetailComponent {
	/// Build the detail list for `plugin` from its stored `settings`.
	pub fn new(
		plugin: InstalledPlugin,
		settings: HashMap<String, serde_json::Value>,
		callbacks: PluginDetailCallbacks,
	) -> Self {
		let (items, config) = plugin_detail_items(&plugin, &settings);
		let max_visible = items.len().min(PLUGIN_DETAIL_MAX_VISIBLE);
		let mut list = SettingsList::new(
			items.clone(),
			max_visible,
			crate::components::settings_list::SettingsListTheme::default(),
		);
		list.set_hint(Some("enter to edit · esc to go back".to_string()));
		PluginDetailComponent {
			plugin,
			settings,
			items,
			config,
			role: PLUGIN_SETTINGS_ROLE.to_string(),
			list: Some(list),
			callbacks,
		}
	}

	/// The plugin being edited.
	pub fn plugin(&self) -> &InstalledPlugin {
		&self.plugin
	}

	/// The detail rows in order.
	pub fn items(&self) -> &[SettingItem] {
		&self.items
	}

	/// The manifest settings, with what their editors need.
	pub fn config_fields(&self) -> &[PluginConfigField] {
		&self.config
	}

	/// The terminal settings list.
	pub fn settings_list(&self) -> Option<&SettingsList> {
		self.list.as_ref()
	}

	/// The terminal settings list, mutably.
	pub fn settings_list_mut(&mut self) -> Option<&mut SettingsList> {
		self.list.as_mut()
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// Apply a row change through the plugin's callbacks, keeping the local
	/// plugin state in step with the host.
	pub fn apply_change(&mut self, id: &str, new_value: &str) {
		let on = new_value == "true";
		if id == PLUGIN_ENABLED_ID {
			(self.callbacks.on_enabled_change)(on);
			self.plugin.enabled = on;
		} else if let Some(feature) = id.strip_prefix("feature:") {
			(self.callbacks.on_feature_change)(feature, on);
			toggle_feature(&mut self.plugin, feature, on);
		} else if let Some(key) = id.strip_prefix("config:") {
			if let Some(PluginSettingSchema::Boolean { .. }) = self.plugin.manifest.settings.get(key) {
				(self.callbacks.on_config_change)(key, serde_json::Value::Bool(on));
			}
		}
	}

	/// Submit a text/number value for `key`, parsing numbers as numbers.
	pub fn submit_config_value(&mut self, key: &str, value: &str) {
		let parsed = match self.plugin.manifest.settings.get(key) {
			Some(PluginSettingSchema::Number { .. }) => match value.parse::<f64>() {
				Ok(number) => serde_json::Number::from_f64(number)
					.map(serde_json::Value::Number)
					.unwrap_or(serde_json::Value::String(value.to_string())),
				Err(_) => serde_json::Value::String(value.to_string()),
			},
			_ => serde_json::Value::String(value.to_string()),
		};
		self.settings.insert(key.to_string(), parsed.clone());
		(self.callbacks.on_config_change)(key, parsed);
	}

	/// Go back to the plugin list.
	pub fn back(&mut self) {
		(self.callbacks.on_back)();
	}

	/// This plugin's settings as a native page section.
	#[must_use]
	pub fn prefs_page(&self) -> PluginPrefsPage {
		let lead = self
			.plugin
			.manifest
			.description
			.clone()
			.unwrap_or_else(|| "Turn the plugin and its features on and off, and change its settings.".to_string());
		prefs_detail_page(&self.plugin.name, &lead, self.list.as_ref())
	}

	/// A native page event on this plugin's settings.
	pub fn handle_prefs_event(&mut self, event: &NativeUiEvent) -> bool {
		let Self { list, .. } = self;
		prefs_detail_event(list.as_mut(), event)
	}
}

impl Component for PluginDetailComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		use crate::chrome::overlay_box::{bottom_border, row, top_border};
		let theme = crate::chrome::overlay_box::OverlayTheme::default();
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let mut rows = vec![top_border(width, &self.plugin.name, &theme), row("", width, &theme)];
		match self.list.as_mut() {
			Some(list) => {
				for line in list.render(inner) {
					rows.push(row(&line, width, &theme));
				}
			}
			None => rows.push(row("Loading settings…", width, &theme)),
		}
		rows.push(bottom_border(width, &theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		let mut on_back = std::mem::replace(&mut self.callbacks.on_back, Box::new(|| {}));
		let list = self.list.as_mut();
		match list {
			Some(list) => handle_input_or_escape(key, list, &mut on_back),
			None => {
				if matches!(key, Key::Esc) {
					on_back();
				}
			}
		}
		self.callbacks.on_back = on_back;
	}
}

/// Detail view for a marketplace plugin, including the settings declared by its
/// runtime package and the install metadata from the registry.
pub struct MarketplacePluginDetailComponent {
	plugin: InstalledPluginSummary,
	runtime: Option<InstalledPlugin>,
	items: Vec<SettingItem>,
	list: SettingsList,
	role: String,
	theme: crate::chrome::overlay_box::OverlayTheme,
}

impl std::fmt::Debug for MarketplacePluginDetailComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MarketplacePluginDetailComponent")
			.field("plugin", &self.plugin.id)
			.finish()
	}
}

impl MarketplacePluginDetailComponent {
	/// Build the detail for `plugin`; the runtime manifest settings load later
	/// through [`set_runtime`](MarketplacePluginDetailComponent::set_runtime).
	pub fn new(plugin: InstalledPluginSummary) -> Self {
		let items = marketplace_settings_rows(&plugin);
		let list = SettingsList::new(
			items.clone(),
			items.len().min(PLUGIN_DETAIL_MAX_VISIBLE),
			crate::components::settings_list::SettingsListTheme::default(),
		);
		MarketplacePluginDetailComponent {
			plugin,
			runtime: None,
			items,
			list,
			role: PLUGIN_SETTINGS_ROLE.to_string(),
			theme: crate::chrome::overlay_box::OverlayTheme::default(),
		}
	}

	/// Attach the runtime package's manifest so its settings appear.
	pub fn set_runtime(
		&mut self,
		runtime: InstalledPlugin,
		settings: &HashMap<String, serde_json::Value>,
	) {
		self.items = runtime_settings_rows(&runtime, settings);
		self.list.set_items(self.items.clone());
		self.runtime = Some(runtime);
	}

	/// The marketplace summary being edited.
	pub fn plugin(&self) -> &InstalledPluginSummary {
		&self.plugin
	}

	/// The detail rows in order.
	pub fn items(&self) -> &[SettingItem] {
		&self.items
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// Enable or disable the marketplace install.
	pub fn set_enabled(&mut self, enabled: bool) {
		for entry in &mut self.plugin.entries {
			entry.enabled = Some(enabled);
		}
		let value = if enabled { "true" } else { "false" };
		let item = &mut self.items[0];
		item.value = value.to_string();
		self.list.update_value(PLUGIN_ENABLED_ID, value);
	}

	/// This plugin's settings as a native page section, with its install facts as
	/// the lead.
	#[must_use]
	pub fn prefs_page(&self) -> PluginPrefsPage {
		let lead = marketplace_detail_facts(&self.plugin).join(" · ");
		let rows: Vec<PrefsRow> = self
			.items
			.iter()
			.map(|item| {
				let is_switch = item.values.as_ref().is_some_and(|values| values.len() == 2);
				PrefsRow {
					id: item.id.clone(),
					label: item.label.trim().to_string(),
					hint: item.description.clone(),
					warning: item.warning.clone(),
					control: if is_switch {
						PrefsControl::Switch { on: item.value == "true" }
					} else {
						PrefsControl::Action {
							label: if item.value.trim().is_empty() {
								"Edit…".to_string()
							} else {
								item.value.trim().to_string()
							},
							act: "edit".to_string(),
						}
					},
				}
			})
			.collect();
		PluginPrefsPage {
			lead,
			sections: vec![PrefsSection {
				id: "plugin".to_string(),
				title: self.plugin.id.clone(),
				rows,
			}],
			focus: None,
			editing: None,
			editor: None,
		}
	}

	fn rebuild(&mut self) {
		self.items = marketplace_settings_rows(&self.plugin);
	}

	/// The plugin reference the host's config-change callback needs.
	pub fn runtime_name(&self) -> Option<&str> {
		self.runtime.as_ref().map(|runtime| runtime.name.as_str())
	}
}

impl Component for MarketplacePluginDetailComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		use crate::chrome::overlay_box::{bottom_border, row, top_border};
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let mut rows = vec![
			top_border(width, &self.plugin.id, &self.theme),
			row("", width, &self.theme),
		];
		for fact in marketplace_detail_facts(&self.plugin) {
			rows.push(row(&fact, width, &self.theme));
		}
		for line in self.list.render(inner) {
			rows.push(row(&line, width, &self.theme));
		}
		rows.push(bottom_border(width, &self.theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		self.list.handle_input(key);
	}
}

/// The install facts of a marketplace plugin, as the page lead.
#[must_use]
pub fn marketplace_detail_facts(summary: &InstalledPluginSummary) -> Vec<String> {
	let entry = summary.entries.first();
	let mut facts = vec![
		format!("{} install", summary.scope.map(|scope| scope.as_str()).unwrap_or("-")),
		format!("v{}", entry.map(|entry| entry.version.as_str()).unwrap_or("?")),
	];
	if let Some(entry) = entry {
		if !entry.install_path.is_empty() {
			facts.push(shorten_path(&entry.install_path));
		}
	}
	if let Some(shadowed_by) = &summary.shadowed_by {
		facts.push(format!("shadowed by the {shadowed_by} install"));
	}
	facts
}

/// The `key  value` install summary rows of the marketplace detail list.
#[must_use]
pub fn marketplace_summary_rows(summary: &InstalledPluginSummary) -> Vec<String> {
	let entry = summary.entries.first();
	let mut rows = vec![
		format!("version       {}", entry.map(|entry| entry.version.as_str()).unwrap_or("(unknown)")),
		format!("scope         {}", summary.scope.map(|scope| scope.as_str()).unwrap_or("-")),
		format!(
			"install path  {}",
			entry
				.map(|entry| if entry.install_path.is_empty() {
					"(unknown)".to_string()
				} else {
					shorten_path(&entry.install_path)
				})
				.unwrap_or_else(|| "(unknown)".to_string())
		),
		format!("installed at  {}", entry.map(|entry| entry.installed_at.as_str()).unwrap_or("(unknown)")),
		format!("last updated  {}", entry.map(|entry| entry.last_updated.as_str()).unwrap_or("(unknown)")),
	];
	if let Some(sha) = entry.and_then(|entry| entry.git_commit_sha.as_deref()) {
		rows.push(format!("git sha       {sha}"));
	}
	rows
}

/// The marketplace install's own settings rows: the enablement toggle plus the
/// runtime plugin's manifest settings.
#[must_use]
pub fn marketplace_settings_rows(summary: &InstalledPluginSummary) -> Vec<SettingItem> {
	vec![SettingItem::new(
		PLUGIN_ENABLED_ID,
		"Enabled",
		if marketplace_enabled(summary) { "true" } else { "false" },
	)
	.with_description("Enable or disable this marketplace plugin")
	.with_values(vec!["true".to_string(), "false".to_string()])]
}

fn runtime_settings_rows(
	runtime: &InstalledPlugin,
	settings: &HashMap<String, serde_json::Value>,
) -> Vec<SettingItem> {
	let mut rows = marketplace_settings_rows(&InstalledPluginSummary {
		id: runtime.name.clone(),
		scope: None,
		shadowed_by: None,
		entries: Vec::new(),
	});
	rows[0].value = if runtime.enabled { "true" } else { "false" }.to_string();
	rows.extend(
		plugin_config_items(runtime, settings)
			.into_iter()
			.map(|field| field.item().clone()),
	);
	rows
}

/// Shorten a home-anchored path to `~/…`.
#[must_use]
pub fn shorten_path(path: &str) -> String {
	if let Some(home) = std::env::var_os("HOME") {
		let home = home.to_string_lossy().to_string();
		if !home.is_empty() {
			if let Some(rest) = path.strip_prefix(&home) {
				return format!("~{rest}");
			}
		}
	}
	path.to_string()
}

/// Which view the plugin settings tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginSettingsView {
	/// The unified plugin list.
	List,
	/// A runtime plugin's detail.
	NpmDetail,
	/// A marketplace plugin's detail.
	MarketplaceDetail,
}

/// What the plugin settings tab reports back to the settings selector.
pub struct PluginSettingsCallbacks {
	/// Close the whole tab.
	pub on_close: Box<dyn FnMut()>,
	/// A plugin changed on disk; the tab should reload.
	pub on_plugin_changed: Box<dyn FnMut()>,
}

impl PluginSettingsCallbacks {
	/// Build the callback pair.
	pub fn new(on_close: impl FnMut() + 'static, on_plugin_changed: impl FnMut() + 'static) -> Self {
		PluginSettingsCallbacks {
			on_close: Box::new(on_close),
			on_plugin_changed: Box::new(on_plugin_changed),
		}
	}
}

/// Top-level plugin settings: owns the list/detail view state and the host's
/// managers.
pub struct PluginSettingsComponent {
	host: Box<dyn PluginSettingsHost>,
	callbacks: PluginSettingsCallbacks,
	view: PluginSettingsView,
	entries: Vec<PluginListEntry>,
	detail_plugin: Option<InstalledPlugin>,
	detail_summary: Option<InstalledPluginSummary>,
	detail_list: Option<SettingsList>,
	loading: bool,
}

impl std::fmt::Debug for PluginSettingsComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PluginSettingsComponent")
			.field("view", &self.view)
			.field("entries", &self.entries.len())
			.finish()
	}
}

impl PluginSettingsComponent {
	/// Build the tab over `host` and load both registries.
	pub fn new(host: Box<dyn PluginSettingsHost>, callbacks: PluginSettingsCallbacks) -> Self {
		let mut component = PluginSettingsComponent {
			host,
			callbacks,
			view: PluginSettingsView::List,
			entries: Vec::new(),
			detail_plugin: None,
			detail_summary: None,
			detail_list: None,
			loading: true,
		};
		component.show_plugin_list();
		component
	}

	/// The view currently shown.
	pub fn view(&self) -> PluginSettingsView {
		self.view
	}

	/// The unified plugin entries in order.
	pub fn entries(&self) -> &[PluginListEntry] {
		&self.entries
	}

	/// Whether the registries are still loading.
	pub fn is_loading(&self) -> bool {
		self.loading
	}

	/// Reload both registries and return to the list.
	///
	/// Either registry may fail to load (corrupt JSON, missing project root) and
	/// the user still benefits from the other half, so a failure only empties
	/// that half instead of taking the tab down.
	pub fn show_plugin_list(&mut self) {
		self.view = PluginSettingsView::List;
		self.detail_plugin = None;
		self.detail_summary = None;
		self.detail_list = None;
		let npm = self.host.manager().list().unwrap_or_default();
		let marketplace = match self.host.create_marketplace_manager() {
			Ok(mut manager) => manager.list_installed_plugins().unwrap_or_default(),
			Err(_) => Vec::new(),
		};
		let mut entries: Vec<PluginListEntry> = Vec::with_capacity(npm.len() + marketplace.len());
		entries.extend(npm.into_iter().map(PluginListEntry::Npm));
		entries.extend(marketplace.into_iter().map(PluginListEntry::Marketplace));
		self.entries = entries;
		self.loading = false;
	}

	/// Open a runtime plugin's detail view, loading its stored setting values.
	///
	/// Unreadable settings are treated as empty so the tab still opens and the
	/// user can fix the plugin rather than being locked out.
	pub fn show_plugin_detail(&mut self, plugin: InstalledPlugin) {
		self.view = PluginSettingsView::NpmDetail;
		self.loading = false;
		let settings = self
			.host
			.manager()
			.get_plugin_settings(&plugin.name)
			.unwrap_or_default();
		let (items, _) = plugin_detail_items(&plugin, &settings);
		let mut list = SettingsList::new(
			items,
			PLUGIN_DETAIL_MAX_VISIBLE,
			crate::components::settings_list::SettingsListTheme::default(),
		);
		list.set_hint(Some("enter to edit · esc to go back".to_string()));
		self.detail_list = Some(list);
		self.detail_plugin = Some(plugin);
		self.detail_summary = None;
	}

	/// Open a marketplace plugin's detail view, including the runtime package's
	/// settings when the package can be resolved.
	pub fn show_marketplace_detail(&mut self, plugin: InstalledPluginSummary) {
		self.view = PluginSettingsView::MarketplaceDetail;
		self.loading = false;
		let name = self
			.host
			.parse_plugin_id(&plugin.id)
			.unwrap_or_else(|| plugin.id.clone());
		let path = plugin
			.entries
			.first()
			.map(|entry| entry.install_path.as_str())
			.filter(|path| !path.is_empty());
		let items = match self.host.manager().get_plugin(&name, path) {
			Ok(Some(runtime)) => {
				let settings = self
					.host
					.manager()
					.get_plugin_settings(&runtime.name)
					.unwrap_or_default();
				let mut items = marketplace_settings_rows(&plugin);
				items[0].value = if runtime.enabled { "true" } else { "false" }.to_string();
				items.extend(
					plugin_config_items(&runtime, &settings)
						.into_iter()
						.map(|field| field.item().clone()),
				);
				self.detail_plugin = Some(runtime);
				items
			}
			_ => {
				self.detail_plugin = None;
				marketplace_settings_rows(&plugin)
			}
		};
		let mut list = SettingsList::new(
			items,
			PLUGIN_DETAIL_MAX_VISIBLE,
			crate::components::settings_list::SettingsListTheme::default(),
		);
		list.set_hint(Some("enter to edit · esc to go back".to_string()));
		self.detail_list = Some(list);
		self.detail_summary = Some(plugin);
	}

	/// Persist a plugin's enablement, then notify the host.
	pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
		let stored = self.host.manager().set_enabled(name, enabled).is_ok();
		if stored {
			(self.callbacks.on_plugin_changed)();
		}
		stored
	}

	/// Persist one of a plugin's features, then notify the host.
	pub fn set_enabled_feature(&mut self, name: &str, feature: &str, enabled: bool) -> bool {
		let manager = self.host.manager();
		let mut features = manager.get_enabled_features(name).unwrap_or_default().unwrap_or_default();
		if enabled {
			if !features.iter().any(|current| current == feature) {
				features.push(feature.to_string());
			}
		} else {
			features.retain(|current| current != feature);
		}
		let stored = manager.set_enabled_features(name, Some(features)).is_ok();
		if stored {
			(self.callbacks.on_plugin_changed)();
		}
		stored
	}

	/// Persist one of a plugin's settings, then notify the host.
	pub fn set_plugin_setting(&mut self, name: &str, key: &str, value: serde_json::Value) -> bool {
		let stored = self.host.manager().set_plugin_setting(name, key, value).is_ok();
		if stored {
			(self.callbacks.on_plugin_changed)();
		}
		stored
	}

	/// Enable or disable a marketplace install, then notify the host.
	pub fn set_marketplace_enabled(
		&mut self,
		plugin_id: &str,
		enabled: bool,
		scope: Option<crate::overlays::plugin_selector::PluginScope>,
	) -> bool {
		let Ok(mut manager) = self.host.create_marketplace_manager() else {
			return false;
		};
		let stored = manager.set_plugin_enabled(plugin_id, enabled, scope).is_ok();
		if stored {
			(self.callbacks.on_plugin_changed)();
		}
		stored
	}

	/// The current view as a native settings page.
	#[must_use]
	pub fn prefs_page(&self) -> PluginPrefsPage {
		if self.loading {
			return PluginPrefsPage::empty("Loading installed plugins…");
		}
		match self.view {
			PluginSettingsView::List => plugin_list_page(&self.entries, None),
			PluginSettingsView::NpmDetail => {
				let plugin = self.detail_plugin.as_ref();
				let title = plugin.map(|plugin| plugin.name.as_str()).unwrap_or("Plugin");
				let lead = plugin
					.and_then(|plugin| plugin.manifest.description.clone())
					.unwrap_or_else(|| {
						"Turn the plugin and its features on and off, and change its settings.".to_string()
					});
				prefs_detail_page(title, &lead, self.detail_list.as_ref())
			}
			PluginSettingsView::MarketplaceDetail => {
				let summary = self.detail_summary.as_ref();
				let lead = summary
					.map(|summary| marketplace_detail_facts(summary).join(" · "))
					.unwrap_or_default();
				let title = summary.map(|summary| summary.id.as_str()).unwrap_or("Plugin");
				prefs_detail_page(title, &lead, self.detail_list.as_ref())
			}
		}
	}

	/// A native page event in the current view. In a detail view, switch changes
	/// are persisted through the host before the local row is updated.
	pub fn handle_prefs_event(&mut self, event: &NativeUiEvent) -> bool {
		if self.view == PluginSettingsView::List {
			let id = match event {
				NativeUiEvent::Activate { item, .. } => item,
				NativeUiEvent::Action { act, value, .. } if act == "open" => {
					match value.as_ref().and_then(|value| value.as_str()) {
						Some(id) => id,
						None => return false,
					}
				}
				_ => return false,
			};
			let Some(entry) = find_entry_by_value(&self.entries, id).cloned() else {
				return false;
			};
			match entry {
				PluginListEntry::Npm(plugin) => self.show_plugin_detail(plugin),
				PluginListEntry::Marketplace(summary) => self.show_marketplace_detail(summary),
			}
			return true;
		}
		if let NativeUiEvent::Change { item, value, .. } = event {
			self.persist_detail_change(item, value);
		}
		prefs_detail_event(self.detail_list.as_mut(), event)
	}

	/// Persist one detail row's new value through the host's managers.
	pub fn persist_detail_change(&mut self, id: &str, value: &serde_json::Value) {
		let new_value = match value {
			serde_json::Value::Bool(flag) => flag.to_string(),
			serde_json::Value::String(text) => text.clone(),
			serde_json::Value::Number(number) => number.to_string(),
			_ => return,
		};
		let Some(plugin) = self.detail_plugin.as_ref() else {
			return;
		};
		let name = plugin.name.clone();
		let config_key = id.strip_prefix("config:").map(str::to_string);
		let schema = config_key
			.as_ref()
			.and_then(|key| plugin.manifest.settings.get(key))
			.cloned();
		let marketplace = match self.view {
			PluginSettingsView::MarketplaceDetail => self
				.detail_summary
				.as_ref()
				.map(|summary| (summary.id.clone(), summary.scope)),
			_ => None,
		};
		if id == PLUGIN_ENABLED_ID {
			let enabled = new_value == "true";
			if let Some((plugin_id, scope)) = marketplace {
				self.set_marketplace_enabled(&plugin_id, enabled, scope);
			} else {
				self.set_enabled(&name, enabled);
			}
		} else if let Some(feature) = id.strip_prefix("feature:") {
			self.set_enabled_feature(&name, feature, new_value == "true");
		} else if let Some(key) = config_key {
			let value = match schema {
				Some(PluginSettingSchema::Boolean { .. }) => {
					serde_json::Value::Bool(new_value == "true")
				}
				Some(PluginSettingSchema::Number { .. }) => match new_value.parse::<f64>() {
					Ok(number) => serde_json::Number::from_f64(number)
						.map_or_else(|| serde_json::Value::String(new_value.clone()), serde_json::Value::Number),
					Err(_) => serde_json::Value::String(new_value.clone()),
				},
				_ => serde_json::Value::String(new_value.clone()),
			};
			self.set_plugin_setting(&name, &key, value);
		}
		if self.view == PluginSettingsView::NpmDetail {
			if let Some(plugin) = self.detail_plugin.as_mut() {
				if id == PLUGIN_ENABLED_ID {
					plugin.enabled = new_value == "true";
				} else if let Some(feature) = id.strip_prefix("feature:") {
					toggle_feature(plugin, feature, new_value == "true");
				}
			}
		}
	}

	/// Close the whole tab.
	pub fn close(&mut self) {
		(self.callbacks.on_close)();
	}

	/// Forward a key to the current view; Escape goes back to the list from a
	/// detail view, and closes the tab while the list has nothing to act on, so
	/// the tab is always dismissible.
	pub fn handle_input(&mut self, key: &Key) {
		if self.loading || self.entries.is_empty() {
			if matches!(key, Key::Esc) {
				self.close();
			}
			return;
		}
		if self.view != PluginSettingsView::List && matches!(key, Key::Esc) {
			self.show_plugin_list();
			return;
		}
		if let Some(list) = self.detail_list.as_mut() {
			list.handle_input(key);
		}
	}
}

impl Component for PluginSettingsComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1) as usize;
		if self.loading {
			return vec![crate::text_shape::truncate_to_width(
				"Loading installed plugins…",
				width,
				crate::text_shape::Ellipsis::Omit,
			)];
		}
		match self.view {
			PluginSettingsView::List => {
				if self.entries.is_empty() {
					return vec![crate::text_shape::truncate_to_width(
						"No plugins installed.",
						width,
						crate::text_shape::Ellipsis::Omit,
					)];
				}
				let items: Vec<SelectItem> = self.entries.iter().map(plugin_entry_item).collect();
				let mut list = SelectList::new(
					items,
					PLUGIN_LIST_MAX_VISIBLE.min(self.entries.len().max(1)),
					SelectListTheme::default(),
				);
				list.render(width as u16)
			}
			PluginSettingsView::NpmDetail | PluginSettingsView::MarketplaceDetail => {
				if let Some(list) = self.detail_list.as_mut() {
					return list.render(width as u16);
				}
				vec![crate::text_shape::truncate_to_width(
					"No plugin details.",
					width,
					crate::text_shape::Ellipsis::Omit,
				)]
			}
		}
	}

	fn handle_input(&mut self, key: &Key) {
		PluginSettingsComponent::handle_input(self, key);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::plugin_selector::PluginScope;

	fn schema_boolean(default: bool) -> PluginSettingSchema {
		PluginSettingSchema::Boolean {
			description: None,
			secret: false,
			default: Some(default),
		}
	}

	fn plugin() -> InstalledPlugin {
		let mut features = BTreeMap::new();
		features.insert(
			"lint".to_string(),
			PluginFeature {
				description: Some("Lint plans".to_string()),
				default: true,
			},
		);
		features.insert(
			"deploy".to_string(),
			PluginFeature {
				description: None,
				default: false,
			},
		);
		let mut settings = BTreeMap::new();
		settings.insert("verbose".to_string(), schema_boolean(false));
		settings.insert(
			"mode".to_string(),
			PluginSettingSchema::Enum {
				description: Some("Run mode".to_string()),
				secret: false,
				default: Some("fast".to_string()),
				values: vec!["fast".to_string(), "slow".to_string()],
			},
		);
		settings.insert(
			"token".to_string(),
			PluginSettingSchema::String {
				description: None,
				secret: true,
				default: None,
			},
		);
		settings.insert(
			"retries".to_string(),
			PluginSettingSchema::Number {
				description: None,
				secret: false,
				default: Some(3.0),
				min: Some(1.0),
				max: Some(9.0),
			},
		);
		InstalledPlugin {
			name: "review".to_string(),
			version: "1.4.0".to_string(),
			enabled: true,
			enabled_features: None,
			manifest: PluginManifest {
				description: Some("Reviews plans".to_string()),
				features,
				settings,
			},
		}
	}

	fn summary(id: &str, scope: PluginScope, enabled: Option<bool>) -> InstalledPluginSummary {
		InstalledPluginSummary {
			id: id.to_string(),
			scope: Some(scope),
			shadowed_by: None,
			entries: vec![InstalledPluginEntry {
				install_path: "/home/dev/.maple/plugins/x".to_string(),
				version: "2.0.0".to_string(),
				installed_at: "2026-01-01".to_string(),
				last_updated: "2026-02-01".to_string(),
				git_commit_sha: Some("abc123".to_string()),
				enabled,
			}],
		}
	}

	#[test]
	fn entry_values_keep_scopes_apart() {
		let npm = PluginListEntry::Npm(plugin());
		let user = PluginListEntry::Marketplace(summary("x@official", PluginScope::User, None));
		let project = PluginListEntry::Marketplace(summary("x@official", PluginScope::Project, None));
		assert_eq!(plugin_entry_value(&npm), "npm:review");
		assert_eq!(plugin_entry_value(&user), "mkt:user:x@official");
		assert_eq!(plugin_entry_value(&project), "mkt:project:x@official");
		let entries = vec![npm, user, project];
		assert!(matches!(
			find_entry_by_value(&entries, "mkt:project:x@official"),
			Some(PluginListEntry::Marketplace(_))
		));
		assert!(find_entry_by_value(&entries, "mkt:user:missing").is_none());
	}

	#[test]
	fn a_missing_enabled_flag_counts_as_enabled() {
		let default = summary("x@official", PluginScope::User, None);
		assert!(marketplace_enabled(&default));
		let disabled = summary("x@official", PluginScope::User, Some(false));
		assert!(!marketplace_enabled(&disabled));
		let enabled = summary("x@official", PluginScope::User, Some(true));
		assert!(marketplace_enabled(&enabled));
	}

	#[test]
	fn feature_defaults_apply_when_the_list_is_null() {
		let mut plugin = plugin();
		assert_eq!(effective_enabled_features(&plugin), vec!["lint".to_string()]);
		plugin.enabled_features = Some(vec!["deploy".to_string()]);
		assert_eq!(effective_enabled_features(&plugin), vec!["deploy".to_string()]);
		toggle_feature(&mut plugin, "lint", true);
		toggle_feature(&mut plugin, "deploy", false);
		assert_eq!(plugin.enabled_features.as_deref(), Some(&["lint".to_string()][..]));
	}

	#[test]
	fn detail_rows_start_with_enablement_then_features_then_settings() {
		let (items, fields) = plugin_detail_items(&plugin(), &HashMap::new());
		let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
		assert_eq!(
			ids,
			vec![
				PLUGIN_ENABLED_ID,
				"feature:deploy",
				"feature:lint",
				"config:mode",
				"config:retries",
				"config:token",
				"config:verbose",
			]
		);
		assert_eq!(items[0].value, "true");
		assert_eq!(items[1].value, "false");
		assert_eq!(items[2].value, "true");
		assert_eq!(items[2].label, "  lint");
		assert_eq!(items[2].description.as_deref(), Some("Lint plans"));
		assert_eq!(fields.len(), 4);
		assert!(matches!(fields[0], PluginConfigField::Enum { .. }));
		assert!(matches!(fields[1], PluginConfigField::Value { .. }));
		assert!(matches!(fields[3], PluginConfigField::Bool { .. }));
		assert_eq!(fields[1].item().value, "3");
	}

	#[test]
	fn secrets_are_masked_and_unset_settings_say_so() {
		let secret = PluginSettingSchema::String {
			description: None,
			secret: true,
			default: None,
		};
		assert_eq!(setting_display_value(&secret, None), UNSET_VALUE);
		assert_eq!(
			setting_display_value(&secret, Some(&serde_json::json!("hunter2"))),
			SECRET_PLACEHOLDER
		);
		let plain = PluginSettingSchema::String {
			description: None,
			secret: false,
			default: Some("fallback".to_string()),
		};
		assert_eq!(setting_display_value(&plain, None), "fallback");
		assert_eq!(setting_display_value(&plain, Some(&serde_json::json!("set"))), "set");
	}

	#[test]
	fn type_hints_name_the_type_and_range() {
		let number = PluginSettingSchema::Number {
			description: None,
			secret: false,
			default: None,
			min: Some(1.0),
			max: Some(9.0),
		};
		assert_eq!(number.type_hint(), "Type: number (1..9)");
		assert_eq!(
			PluginSettingSchema::Enum {
				description: None,
				secret: false,
				default: None,
				values: vec!["a".to_string()],
			}
			.type_hint(),
			"Type: enum"
		);
	}

	#[test]
	fn list_rows_show_status_kind_and_version() {
		let mut npm = plugin();
		npm.enabled_features = Some(vec!["lint".to_string()]);
		let npm_item = plugin_entry_item(&PluginListEntry::Npm(npm));
		assert_eq!(npm_item.title, "Enabled review");
		let details = npm_item.description.clone().unwrap_or_default();
		assert!(details.contains("[npm]"), "{details}");
		assert!(details.contains("1/2 features"), "{details}");

		let mut shadowed = summary("x@official", PluginScope::Project, Some(false));
		shadowed.shadowed_by = Some("user".to_string());
		let market_item = plugin_entry_item(&PluginListEntry::Marketplace(shadowed));
		assert_eq!(market_item.title, "Disabled x@official (shadowed)");
		let details = market_item.description.clone().unwrap_or_default();
		assert!(details.contains("[marketplace] [project]"), "{details}");
		assert!(details.contains("v2.0.0"), "{details}");
		assert!(details.contains("shadowed by user"), "{details}");
	}

	#[test]
	fn list_page_reports_a_hint_and_shadow_warning() {
		let mut shadowed = summary("x@official", PluginScope::Project, None);
		shadowed.shadowed_by = Some("user".to_string());
		let page = plugin_list_page(
			&[PluginListEntry::Npm(plugin()), PluginListEntry::Marketplace(shadowed)],
            Some("npm:review".to_string()),
		);
		assert_eq!(page.sections.len(), 1);
		assert_eq!(page.sections[0].id, "installed");
		assert_eq!(page.sections[0].rows.len(), 2);
		assert_eq!(page.sections[0].rows[0].label, "review");
		assert_eq!(
			page.sections[0].rows[0].hint.as_deref(),
			Some("Enabled · npm · v1.4.0 · 0/2 features")
		);
		assert_eq!(
			page.sections[0].rows[1].warning.as_deref(),
			Some("Shadowed by the user install")
		);
		assert_eq!(page.focus.as_deref(), Some("npm:review"));
		let empty = plugin_list_page(&[], None);
		assert!(empty.sections.is_empty());
		assert!(empty.lead.contains("No plugins installed"));
	}

	#[test]
	fn detail_page_turns_booleans_into_switches() {
		let (items, _) = plugin_detail_items(&plugin(), &HashMap::new());
		let mut list = SettingsList::new(items, 8, crate::components::settings_list::SettingsListTheme::default());
		list.select_item("config:mode");
		let page = prefs_detail_page("review", "Reviews plans", Some(&list));
		assert_eq!(page.sections.len(), 1);
		assert_eq!(page.sections[0].title, "review");
		assert_eq!(page.focus.as_deref(), Some("config:mode"));
		let enabled = &page.sections[0].rows[0];
		assert_eq!(enabled.label, "Enabled");
		assert_eq!(enabled.control, PrefsControl::Switch { on: true });
		let mode = page.sections[0]
			.rows
			.iter()
			.find(|row| row.id == "config:mode")
			.expect("mode row");
		assert_eq!(
			mode.control,
			PrefsControl::Action {
				label: "fast".to_string(),
				act: "edit".to_string()
			}
		);
		let empty = prefs_detail_page("review", "lead", None);
		assert!(empty.sections.is_empty());
		assert_eq!(empty.lead, "lead");
	}

	#[test]
	fn detail_events_apply_switch_values() {
		let (items, _) = plugin_detail_items(&plugin(), &HashMap::new());
		let mut list = SettingsList::new(items, 8, crate::components::settings_list::SettingsListTheme::default());
		let event = NativeUiEvent::Change {
			key: Vec::new(),
			item: "feature:deploy".to_string(),
			value: serde_json::json!(true),
		};
		assert!(prefs_detail_event(Some(&mut list), &event));
		let row = list
			.items()
			.iter()
			.find(|item| item.id == "feature:deploy")
			.expect("feature row");
		assert_eq!(row.value, "true");
		let ignored = NativeUiEvent::Change {
			key: Vec::new(),
			item: "feature:deploy".to_string(),
			value: serde_json::json!("text"),
		};
		assert!(!prefs_detail_event(Some(&mut list), &ignored));
		assert!(!prefs_detail_event(None, &ignored));
	}

	#[test]
	fn changing_a_row_updates_local_plugin_state() {
		let enabled = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let features = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let config = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let (enabled_sink, feature_sink, config_sink) =
			(std::rc::Rc::clone(&enabled), std::rc::Rc::clone(&features), std::rc::Rc::clone(&config));
		let mut component = PluginDetailComponent::new(
			plugin(),
			HashMap::new(),
			PluginDetailCallbacks::new(
				move |on| enabled_sink.borrow_mut().push(on),
				move |name, on| feature_sink.borrow_mut().push((name.to_string(), on)),
				move |key, value| config_sink.borrow_mut().push((key.to_string(), value)),
				|| {},
			),
		);
		component.apply_change(PLUGIN_ENABLED_ID, "false");
		component.apply_change("feature:lint", "false");
		component.apply_change("config:verbose", "true");
		component.submit_config_value("retries", "5");
		component.submit_config_value("token", "abc");
		assert_eq!(&*enabled.borrow(), &[false]);
		assert_eq!(&*features.borrow(), &[("lint".to_string(), false)]);
		let config = config.borrow();
		assert_eq!(config.len(), 3);
		assert_eq!(config[0], ("verbose".to_string(), serde_json::json!(true)));
		assert_eq!(config[1].0, "retries");
		assert_eq!(config[1].1, serde_json::json!(5.0));
		assert_eq!(config[2], ("token".to_string(), serde_json::json!("abc")));
		assert!(!component.plugin().enabled);
		assert_eq!(component.plugin().enabled_features.as_deref(), Some(&["deploy".to_string()][..]));
	}

	#[test]
	fn marketplace_detail_reports_install_facts() {
		let mut shadowed = summary("x@official", PluginScope::Project, Some(false));
		shadowed.shadowed_by = Some("user".to_string());
		let facts = marketplace_detail_facts(&shadowed);
		assert_eq!(facts[0], "project install");
		assert_eq!(facts[1], "v2.0.0");
		assert!(facts[2].ends_with(".maple/plugins/x"), "{facts:?}");
		assert_eq!(facts[3], "shadowed by the user install");
		let rows = marketplace_summary_rows(&shadowed);
		assert_eq!(rows[0], "version       2.0.0");
		assert_eq!(rows[1], "scope         project");
		assert!(rows[4].starts_with("last updated  2026-02-01"));
		assert_eq!(rows[5], "git sha       abc123");
		let mut component = MarketplacePluginDetailComponent::new(shadowed);
		assert_eq!(component.items().len(), 1);
		assert_eq!(component.items()[0].value, "false");
		component.set_enabled(true);
		assert_eq!(component.items()[0].value, "true");
		component.set_runtime(plugin(), &HashMap::new());
		let ids: Vec<&str> = component.items().iter().map(|item| item.id.as_str()).collect();
		assert_eq!(ids[0], PLUGIN_ENABLED_ID);
		assert!(ids.contains(&"config:mode"));
	}

	#[test]
	fn list_component_activates_the_entry_under_the_value() {
		let picked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = std::rc::Rc::clone(&picked);
		let market_sink = std::rc::Rc::clone(&picked);
		let mut component = PluginListComponent::new(
			vec![
				PluginListEntry::Npm(plugin()),
				PluginListEntry::Marketplace(summary("x@official", PluginScope::User, None)),
			],
			PluginListCallbacks::new(
				move |plugin| sink.borrow_mut().push(format!("npm:{}", plugin.name)),
				move |summary| market_sink.borrow_mut().push(format!("mkt:{}", summary.id)),
				|| {},
			),
		);
		assert!(component.handle_prefs_event(&NativeUiEvent::Action {
			key: Vec::new(),
			act: "open".to_string(),
			value: Some(serde_json::json!("mkt:user:x@official")),
			mods: Vec::new(),
		}));
		assert_eq!(&*picked.borrow(), &["mkt:x@official".to_string()]);
		assert_eq!(component.prefs_page().sections[0].rows.len(), 2);
	}

	#[test]
	fn the_tab_loads_both_registries_and_stays_dismissible() {
		struct Manager {
			plugins: Vec<InstalledPlugin>,
		}

		impl PluginSettingsManager for Manager {
			fn list(&mut self) -> Result<Vec<InstalledPlugin>, String> {
				Ok(self.plugins.clone())
			}
			fn get_plugin(&mut self, _name: &str, _path: Option<&str>) -> Result<Option<InstalledPlugin>, String> {
				Ok(None)
			}
			fn get_plugin_settings(&mut self, _name: &str) -> Result<HashMap<String, serde_json::Value>, String> {
				Ok(HashMap::new())
			}
			fn set_enabled(&mut self, _name: &str, _enabled: bool) -> Result<(), String> {
				Ok(())
			}
			fn get_enabled_features(&mut self, _name: &str) -> Result<Option<Vec<String>>, String> {
				Ok(None)
			}
			fn set_enabled_features(&mut self, _name: &str, _features: Option<Vec<String>>) -> Result<(), String> {
				Ok(())
			}
			fn set_plugin_setting(&mut self, _name: &str, _key: &str, _value: serde_json::Value) -> Result<(), String> {
				Ok(())
			}
		}

		struct Market;

		impl PluginSettingsMarketplaceManager for Market {
			fn list_installed_plugins(&mut self) -> Result<Vec<InstalledPluginSummary>, String> {
				Err("registry is corrupt".to_string())
			}
			fn set_plugin_enabled(
				&mut self,
				_plugin_id: &str,
				_enabled: bool,
				_scope: Option<PluginScope>,
			) -> Result<(), String> {
				Ok(())
			}
		}

		struct Host {
			manager: Manager,
			changed: usize,
		}

		impl PluginSettingsHost for Host {
			fn manager(&mut self) -> &mut dyn PluginSettingsManager {
				&mut self.manager
			}
			fn create_marketplace_manager(&mut self) -> Result<Box<dyn PluginSettingsMarketplaceManager>, String> {
				Ok(Box::new(Market))
			}
			fn parse_plugin_id(&self, id: &str) -> Option<String> {
				id.split('@').next().map(str::to_string)
			}
		}

		let closed = std::rc::Rc::new(std::cell::RefCell::new(0));
		let closed_sink = std::rc::Rc::clone(&closed);
		let mut component = PluginSettingsComponent::new(
			Box::new(Host {
				manager: Manager {
					plugins: vec![plugin()],
				},
				changed: 0,
			}),
			PluginSettingsCallbacks::new(move || *closed_sink.borrow_mut() += 1, || {}),
		);
		assert!(!component.is_loading());
		assert_eq!(component.entries().len(), 1);
		assert_eq!(component.prefs_page().sections[0].rows.len(), 1);
		assert!(component.set_enabled("review", false));
		component.show_plugin_detail(plugin());
		assert_eq!(component.view(), PluginSettingsView::NpmDetail);
		let page = component.prefs_page();
		assert_eq!(page.sections[0].title, "review");
		assert_eq!(page.sections[0].rows[0].label, "Enabled");
		assert!(component.handle_prefs_event(&NativeUiEvent::Change {
			key: Vec::new(),
			item: "config:mode".to_string(),
			value: serde_json::json!("slow"),
		}));
		component.handle_input(&Key::Esc);
		assert_eq!(component.view(), PluginSettingsView::List);
		assert_eq!(*closed.borrow(), 0, "Escape inside the list only navigates");
	}
}
