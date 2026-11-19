//! Types for the Extension Control Center dashboard.
//!
//! Reference: `l/src/overlays/extensions/types.ts`.
//!
//! Deviations:
//! - `raw` fields use `serde_json::Value` instead of `unknown`.
//! - `ExtensionKind` is a string-enum-like Rust enum; parsing from arbitrary
//!   extension IDs is best-effort in [`parse_extension_id`].
//! - `source_from_meta` is an identity-style helper over the same struct shape.

use serde_json::Value;

/// Discovery snapshot used by provider controls, independent of the loader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionProvider {
	pub id: String,
	pub display_name: String,
	pub enabled: bool,
	pub user_source_enabled: bool,
	pub foreign_user_source: bool,
}

/// Extension kinds matching capability types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionKind {
	ExtensionModule,
	Skill,
	Rule,
	Tool,
	Mcp,
	Prompt,
	Instruction,
	ContextFile,
	Hook,
	SlashCommand,
}

impl ExtensionKind {
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			Self::ExtensionModule => "extension-module",
			Self::Skill => "skill",
			Self::Rule => "rule",
			Self::Tool => "tool",
			Self::Mcp => "mcp",
			Self::Prompt => "prompt",
			Self::Instruction => "instruction",
			Self::ContextFile => "context-file",
			Self::Hook => "hook",
			Self::SlashCommand => "slash-command",
		}
	}

	#[must_use]
	pub fn parse(s: &str) -> Option<Self> {
		match s {
			"extension-module" => Some(Self::ExtensionModule),
			"skill" => Some(Self::Skill),
			"rule" => Some(Self::Rule),
			"tool" => Some(Self::Tool),
			"mcp" => Some(Self::Mcp),
			"prompt" => Some(Self::Prompt),
			"instruction" => Some(Self::Instruction),
			"context-file" => Some(Self::ContextFile),
			"hook" => Some(Self::Hook),
			"slash-command" => Some(Self::SlashCommand),
			_ => None,
		}
	}
}

/// Extension state (active, disabled, or shadowed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionState {
	Active,
	Disabled,
	Shadowed,
}

/// Reason why an extension is disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisabledReason {
	ProviderDisabled,
	UserOptIn,
	ItemDisabled,
	Shadowed,
}

/// Source metadata for an extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionSource {
	pub provider: String,
	pub provider_name: String,
	pub level: String,
}

/// Unified extension representation for the dashboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
	pub id: String,
	pub kind: ExtensionKind,
	pub name: String,
	pub display_name: String,
	pub description: Option<String>,
	pub trigger: Option<String>,
	pub path: String,
	pub source: ExtensionSource,
	pub state: ExtensionState,
	pub disabled_reason: Option<DisabledReason>,
	pub shadowed_by: Option<String>,
	pub raw: Value,
}

impl Default for Extension {
	fn default() -> Self {
		Self {
			id: String::new(),
			kind: ExtensionKind::ExtensionModule,
			name: String::new(),
			display_name: String::new(),
			description: None,
			trigger: None,
			path: String::new(),
			source: ExtensionSource {
				provider: String::new(),
				provider_name: String::new(),
				level: String::new(),
			},
			state: ExtensionState::Active,
			disabled_reason: None,
			shadowed_by: None,
			raw: Value::Null,
		}
	}
}

/// True when this row is a lower-precedence copy of another same-name item.
#[must_use]
pub fn is_shadowed_extension(ext: &Extension) -> bool {
	if ext.state == ExtensionState::Shadowed || ext.shadowed_by.is_some() {
		return true;
	}
	ext.raw
		.get("_shadowed")
		.and_then(Value::as_bool)
		.unwrap_or(false)
}

/// Tree node types for sidebar hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeNodeType {
	Provider,
	Kind,
	Item,
}

/// Sidebar tree node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
	pub id: String,
	pub label: String,
	pub node_type: TreeNodeType,
	pub enabled: bool,
	pub collapsed: bool,
	pub children: Vec<TreeNode>,
	pub count: Option<usize>,
}

/// Flattened tree item for navigation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatTreeItem {
	pub node: TreeNode,
	pub depth: usize,
	pub index: usize,
}

/// Focus region in the tabbed dashboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusRegion {
	Tabs,
	List,
}

/// Provider tab representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderTab {
	pub id: String,
	pub label: String,
	pub enabled: bool,
	pub count: usize,
}

/// Tabbed dashboard state.
#[derive(Clone, Debug, PartialEq)]
pub struct DashboardState {
	pub tabs: Vec<ProviderTab>,
	pub active_tab_index: usize,
	pub extensions: Vec<Extension>,
	pub tab_filtered: Vec<Extension>,
	pub search_filtered: Vec<Extension>,
	pub search_query: String,
	pub list_index: usize,
	pub scroll_offset: usize,
	pub selected: Option<Extension>,
}

/// Create extension ID from kind and name.
#[must_use]
pub fn make_extension_id(kind: ExtensionKind, name: &str) -> String {
	format!("{}:{name}", kind.as_str())
}

/// Parse extension ID into kind and name.
#[must_use]
pub fn parse_extension_id(id: &str) -> Option<(ExtensionKind, String)> {
	let (kind_str, name) = id.split_once(':')?;
	let kind = ExtensionKind::parse(kind_str)?;
	Some((kind, name.to_string()))
}

/// Map source metadata to extension source shape (identity helper).
#[must_use]
pub fn source_from_meta(meta: &ExtensionSource) -> ExtensionSource {
	meta.clone()
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	fn sample_ext(state: ExtensionState, raw: Value) -> Extension {
		Extension {
			id: "tool:x".into(),
			kind: ExtensionKind::Tool,
			name: "x".into(),
			display_name: "X".into(),
			description: None,
			trigger: None,
			path: "/x".into(),
			source: ExtensionSource {
				provider: "p".into(),
				provider_name: "P".into(),
				level: "project".into(),
			},
			state,
			disabled_reason: None,
			shadowed_by: None,
			raw,
		}
	}

	#[test]
	fn extension_kind_roundtrips_known_strings() {
		for (s, k) in [
			("extension-module", ExtensionKind::ExtensionModule),
			("skill", ExtensionKind::Skill),
			("slash-command", ExtensionKind::SlashCommand),
			("mcp", ExtensionKind::Mcp),
		] {
			assert_eq!(ExtensionKind::parse(s), Some(k));
			assert_eq!(k.as_str(), s);
		}
		assert_eq!(ExtensionKind::parse("nope"), None);
	}

	#[test]
	fn make_and_parse_extension_id() {
		let id = make_extension_id(ExtensionKind::Rule, "my-rule");
		assert_eq!(id, "rule:my-rule");
		let (kind, name) = parse_extension_id(&id).unwrap();
		assert_eq!(kind, ExtensionKind::Rule);
		assert_eq!(name, "my-rule");
		assert!(parse_extension_id("invalid").is_none());
	}

	#[test]
	fn is_shadowed_extension_checks_state_flag_and_raw() {
		let mut ext = sample_ext(ExtensionState::Active, json!({}));
		assert!(!is_shadowed_extension(&ext));
		ext.shadowed_by = Some("other".into());
		assert!(is_shadowed_extension(&ext));
		let ext2 = sample_ext(ExtensionState::Active, json!({"_shadowed": true}));
		assert!(is_shadowed_extension(&ext2));
		let ext3 = sample_ext(ExtensionState::Shadowed, json!({}));
		assert!(is_shadowed_extension(&ext3));
	}

	#[test]
	fn source_from_meta_clones() {
		let src = ExtensionSource {
			provider: "a".into(),
			provider_name: "A".into(),
			level: "user".into(),
		};
		assert_eq!(source_from_meta(&src), src);
	}
}
