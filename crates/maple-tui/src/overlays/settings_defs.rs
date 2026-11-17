//! Schema-independent display definitions for the settings overlay.
//!
//! Reference: `l/src/overlays/settings-defs.ts`.
//!
//! Deviations:
//! - `SymbolKey` from `theme/symbols` is not modeled as a Rust enum in this port;
//!   `TabMetadata.icon` is `String` (symbol key, e.g. `"tab.appearance"`).
//! - `WeakMap`-backed memoization is replaced by per-call recomputation for
//!   small entry lists (no global mutable cache); hosts may cache externally.
//! - `numeric_option`, `number_steps`, `get_display_default` mirror the TS API.

use std::collections::BTreeMap;

/// Setting tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SettingTab {
	Appearance,
	Model,
	Interaction,
	Context,
	Memory,
	Files,
	Shell,
	Tools,
	Tasks,
	Providers,
}

impl SettingTab {
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Appearance => "appearance",
			Self::Model => "model",
			Self::Interaction => "interaction",
			Self::Context => "context",
			Self::Memory => "memory",
			Self::Files => "files",
			Self::Shell => "shell",
			Self::Tools => "tools",
			Self::Tasks => "tasks",
			Self::Providers => "providers",
		}
	}

	#[must_use]
	pub fn parse(s: &str) -> Option<Self> {
		match s {
			"appearance" => Some(Self::Appearance),
			"model" => Some(Self::Model),
			"interaction" => Some(Self::Interaction),
			"context" => Some(Self::Context),
			"memory" => Some(Self::Memory),
			"files" => Some(Self::Files),
			"shell" => Some(Self::Shell),
			"tools" => Some(Self::Tools),
			"tasks" => Some(Self::Tasks),
			"providers" => Some(Self::Providers),
			_ => None,
		}
	}
}

/// Tab display metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabMetadata {
	pub label: String,
	pub icon: String,
}

/// Ordered list of tabs for UI rendering.
pub const SETTING_TABS: &[SettingTab] = &[
	SettingTab::Appearance,
	SettingTab::Model,
	SettingTab::Interaction,
	SettingTab::Context,
	SettingTab::Memory,
	SettingTab::Files,
	SettingTab::Shell,
	SettingTab::Tools,
	SettingTab::Tasks,
	SettingTab::Providers,
];

/// Tab display metadata.
pub fn tab_metadata() -> BTreeMap<String, TabMetadata> {
	let mut m = BTreeMap::new();
	let entries = [
		(SettingTab::Appearance, "Appearance", "tab.appearance"),
		(SettingTab::Model, "Model", "tab.model"),
		(SettingTab::Interaction, "Interaction", "tab.interaction"),
		(SettingTab::Context, "Context", "tab.context"),
		(SettingTab::Memory, "Memory", "tab.memory"),
		(SettingTab::Files, "Files", "tab.files"),
		(SettingTab::Shell, "Shell", "tab.shell"),
		(SettingTab::Tools, "Tools", "tab.tools"),
		(SettingTab::Tasks, "Tasks", "tab.tasks"),
		(SettingTab::Providers, "Providers", "tab.providers"),
	];
	for (tab, label, icon) in entries {
		m.insert(
			tab.as_str().to_string(),
			TabMetadata { label: label.to_string(), icon: icon.to_string() },
		);
	}
	m
}

/// One-sentence lead per tab.
pub fn tab_leads() -> BTreeMap<String, String> {
	BTreeMap::from([
		("appearance".into(), "Control theme, composer, display, and image behavior.".into()),
		("model".into(), "Tune model behavior, prompts, retries, and advisor settings.".into()),
		("interaction".into(), "Configure input, approvals, speech, and runtime interaction.".into()),
		("context".into(), "Adjust context retention, compaction, and related behavior.".into()),
		("memory".into(), "Configure memory integrations and auto-learning behavior.".into()),
		("files".into(), "Control file editing, reading, and search-related behavior.".into()),
		("shell".into(), "Tune shell, eval, and runtime execution settings.".into()),
		("tools".into(), "Manage built-in tool defaults and integrations.".into()),
		("tasks".into(), "Configure subagent, isolation, and task execution behavior.".into()),
		("providers".into(), "Manage provider endpoints, services, and auth settings.".into()),
	])
}

/// Ordered section groups per tab.
pub fn tab_groups() -> BTreeMap<String, Vec<String>> {
	BTreeMap::from([
		(
			"appearance".into(),
			vec![
				"Theme".into(),
				"Composer".into(),
				"Status Line".into(),
				"Display".into(),
				"Images".into(),
			],
		),
		(
			"model".into(),
			vec![
				"Thinking".into(),
				"Sampling".into(),
				"Prompt".into(),
				"Retry & Fallback".into(),
				"Advisor".into(),
				"Prewalk".into(),
				"Vision".into(),
			],
		),
		(
			"interaction".into(),
			vec![
				"Input".into(),
				"Approvals".into(),
				"Notifications".into(),
				"Speech".into(),
				"Collab".into(),
				"Stream".into(),
				"Magic Keywords".into(),
				"Startup & Updates".into(),
				"Power".into(),
				"Agent".into(),
				"Git".into(),
				"Skills".into(),
			],
		),
		(
			"context".into(),
			vec!["General".into(), "Compaction".into(), "Rules (TTSR)".into(), "Experimental".into()],
		),
		(
			"memory".into(),
			vec!["General".into(), "Auto-Learn".into(), "Mnemopi".into(), "Hindsight".into(), "Sharpshooter".into()],
		),
		(
			"files".into(),
			vec![
				"Editing".into(),
				"Reading".into(),
				"Read Summaries".into(),
				"LSP".into(),
			],
		),
		(
			"shell".into(),
			vec!["Bash".into(), "Eval & Runtimes".into()],
		),
		(
			"tools".into(),
			vec![
				"Available Tools".into(),
				"Todos".into(),
				"Grep & Browser".into(),
				"Find".into(),
				"Computer".into(),
				"IDA Pro".into(),
				"GitHub".into(),
				"Output Limits".into(),
				"Execution".into(),
				"Discovery & MCP".into(),
				"Extensions".into(),
				"Developer".into(),
			],
		),
		(
			"tasks".into(),
			vec!["Modes".into(), "Subagents".into(), "Isolation".into(), "Commands & Skills".into()],
		),
		(
			"providers".into(),
			vec![
				"Services".into(),
				"Fireworks".into(),
				"Tiny Model".into(),
				"Protocol".into(),
				"Timeouts".into(),
				"Privacy".into(),
			],
		),
	])
}

/// Submenu choice metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmenuOption {
	pub value: String,
	pub label: String,
	pub description: Option<String>,
}

/// Base UI metadata shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiBase {
	pub tab: Option<SettingTab>,
	pub group: Option<String>,
	pub label: String,
	pub description: String,
	pub warning: Option<String>,
	pub condition: Option<String>,
}

/// Structural schema entries supplied by the application host.
#[derive(Clone)]
pub struct SettingsDisplayEntry {
	pub path: String,
	pub schema_type: String,
	pub default_value: Option<serde_json::Value>,
	pub ui: Option<UiMetadata>,
	pub enum_values: Option<Vec<String>>,
	pub credential: bool,
	pub condition_fn: Option<std::sync::Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl std::fmt::Debug for SettingsDisplayEntry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SettingsDisplayEntry")
			.field("path", &self.path)
			.field("schema_type", &self.schema_type)
			.field("default_value", &self.default_value)
			.field("ui", &self.ui)
			.field("enum_values", &self.enum_values)
			.field("credential", &self.credential)
			.field("condition_fn", &self.condition_fn.is_some())
			.finish()
	}
}

/// UI metadata (wide shape).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiMetadata {
	pub tab: Option<SettingTab>,
	pub group: Option<String>,
	pub label: String,
	pub description: String,
	pub warning: Option<String>,
	pub condition: Option<String>,
	pub options: Option<Vec<SubmenuOption>>,
	pub options_runtime: bool,
	pub secret: bool,
	pub ordered: bool,
	pub editor: Option<String>,
}

/// Primitive value displayed by a settings control.
pub type SettingsDisplayValue = String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseSettingDef {
	pub path: String,
	pub default_value: Option<serde_json::Value>,
	pub schema_type: String,
	pub label: String,
	pub description: String,
	pub warning: Option<String>,
	pub tab: Option<SettingTab>,
	pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BooleanSettingDef {
	pub base: BaseSettingDef,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumSettingDef {
	pub base: BaseSettingDef,
	pub values: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmenuSettingDef {
	pub base: BaseSettingDef,
	pub options: Vec<SubmenuOption>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextInputSettingDef {
	pub base: BaseSettingDef,
	pub secret: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderLimitsSettingDef {
	pub base: BaseSettingDef,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiSelectSettingDef {
	pub base: BaseSettingDef,
	pub options: Vec<SubmenuOption>,
	pub ordered: bool,
}

/// Setting definition variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingDef {
	Boolean(BooleanSettingDef),
	Enum(EnumSettingDef),
	Submenu(SubmenuSettingDef),
	Text(TextInputSettingDef),
	ProviderLimits(ProviderLimitsSettingDef),
	MultiSelect(MultiSelectSettingDef),
}

impl SettingDef {
	#[must_use]
	pub fn base(&self) -> &BaseSettingDef {
		match self {
			Self::Boolean(d) => &d.base,
			Self::Enum(d) => &d.base,
			Self::Submenu(d) => &d.base,
			Self::Text(d) => &d.base,
			Self::ProviderLimits(d) => &d.base,
			Self::MultiSelect(d) => &d.base,
		}
	}
}

fn resolve_options(ui: &UiMetadata) -> Option<Vec<SubmenuOption>> {
	if ui.options_runtime {
		return Some(Vec::new());
	}
	ui.options.clone()
}

fn entry_to_setting_def(entry: &SettingsDisplayEntry) -> Option<SettingDef> {
	let ui = entry.ui.as_ref()?;
	let base = BaseSettingDef {
		path: entry.path.clone(),
		default_value: entry.default_value.clone(),
		schema_type: entry.schema_type.clone(),
		label: ui.label.clone(),
		description: ui.description.clone(),
		warning: ui.warning.clone(),
		tab: ui.tab,
		group: ui.group.clone(),
	};
	if entry.schema_type == "boolean" {
		return Some(SettingDef::Boolean(BooleanSettingDef { base }));
	}
	let options = resolve_options(ui);
	if entry.schema_type == "enum" {
		let has_options = options.is_some();
		let values = if has_options {
			options.unwrap_or_default().into_iter().map(|o| o.value).collect()
		} else {
			entry.enum_values.clone().unwrap_or_default()
		};
		if has_options {
			return Some(SettingDef::Submenu(SubmenuSettingDef {
				base,
				options: ui.options.clone().unwrap_or_default(),
			}));
		}
		return Some(SettingDef::Enum(EnumSettingDef { base, values }));
	}
	if entry.schema_type == "number" {
		if ui.options_runtime || ui.options.is_none() {
			return None;
		}
		return Some(SettingDef::Submenu(SubmenuSettingDef {
			base,
			options: ui.options.clone().unwrap_or_default(),
		}));
	}
	if entry.schema_type == "string" {
		if ui.options_runtime {
			return Some(SettingDef::Submenu(SubmenuSettingDef { base, options: Vec::new() }));
		}
		if let Some(opts) = &ui.options {
			return Some(SettingDef::Submenu(SubmenuSettingDef {
				base,
				options: opts.clone(),
			}));
		}
		return Some(SettingDef::Text(TextInputSettingDef {
			base,
			secret: entry.credential,
		}));
	}
	if entry.schema_type == "array" {
		if ui.editor.as_deref() == Some("json") {
			return Some(SettingDef::Text(TextInputSettingDef {
				base,
				secret: entry.credential,
			}));
		}
		let opts = ui.options.as_ref()?;
		if ui.options_runtime {
			return None;
		}
		return Some(SettingDef::MultiSelect(MultiSelectSettingDef {
			base,
			options: opts.clone(),
			ordered: ui.ordered,
		}));
	}
	if entry.schema_type == "record" {
		if entry.path == "providers.maxInFlightRequests" {
			return Some(SettingDef::ProviderLimits(ProviderLimitsSettingDef { base }));
		}
		return Some(SettingDef::Text(TextInputSettingDef { base, secret: false }));
	}
	None
}

/// Get all setting definitions with UI.
#[must_use]
pub fn get_all_setting_defs(entries: &[SettingsDisplayEntry]) -> Vec<SettingDef> {
	entries.iter().filter_map(entry_to_setting_def).collect()
}

/// Get settings ordered by their tab's section layout.
#[must_use]
pub fn get_settings_for_tab(entries: &[SettingsDisplayEntry], tab: SettingTab) -> Vec<SettingDef> {
	let defs = get_all_setting_defs(entries);
	let groups = tab_groups();
	let order = groups.get(tab.as_str()).cloned().unwrap_or_default();
	let mut filtered: Vec<_> = defs.into_iter().filter(|d| d.base().tab == Some(tab)).collect();
	filtered.sort_by_key(|d| {
		let Some(g) = &d.base().group else {
			return -1i64;
		};
		order.iter().position(|x| x == g).map(|i| i as i64).unwrap_or(order.len() as i64)
	});
	filtered
}

/// Find the display definition for a host setting path.
#[must_use]
pub fn get_setting_def(entries: &[SettingsDisplayEntry], path: &str) -> Option<SettingDef> {
	get_all_setting_defs(entries).into_iter().find(|d| d.base().path == path)
}

/// Numeric option value: `-1` for `default`, finite number otherwise.
#[must_use]
pub fn numeric_option(value: &str) -> Option<i64> {
	if value == "default" {
		return Some(-1);
	}
	value.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| n as i64)
}

/// Number hint for numeric submenu settings.
#[must_use]
pub fn number_steps(def: &SettingDef) -> Option<BTreeMap<String, String>> {
	let SettingDef::Submenu(sd) = def else {
		return None;
	};
	if sd.base.schema_type != "number" || sd.options.len() < 2 {
		return None;
	}
	let mut labels = BTreeMap::new();
	for opt in &sd.options {
		let n = numeric_option(&opt.value)?;
		labels.insert(n.to_string(), opt.label.clone());
	}
	Some(labels)
}

/// Format a setting's declared default for display.
#[must_use]
pub fn get_display_default(entries: &[SettingsDisplayEntry], path: &str) -> String {
	let Some(entry) = entries.iter().find(|e| e.path == path) else {
		return String::new();
	};
	match &entry.default_value {
		None => String::new(),
		Some(serde_json::Value::Bool(b)) => b.to_string(),
		Some(serde_json::Value::Null) => String::new(),
		Some(v) => v.to_string(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn setting_tab_parse_roundtrip() {
		assert_eq!(SettingTab::parse("appearance"), Some(SettingTab::Appearance));
		assert_eq!(SettingTab::Appearance.as_str(), "appearance");
		assert_eq!(SettingTab::parse("nope"), None);
	}

	#[test]
	fn numeric_option_handles_default_and_numbers() {
		assert_eq!(numeric_option("default"), Some(-1));
		assert_eq!(numeric_option("42"), Some(42));
		assert_eq!(numeric_option(""), None);
		assert_eq!(numeric_option("abc"), None);
	}

	#[test]
	fn get_display_default_formats_bool() {
		let entries = vec![SettingsDisplayEntry {
			path: "a".into(),
			schema_type: "boolean".into(),
			default_value: Some(serde_json::json!(true)),
			ui: None,
			enum_values: None,
			credential: false,
			condition_fn: None,
		}];
		assert_eq!(get_display_default(&entries, "a"), "true");
	}
}
