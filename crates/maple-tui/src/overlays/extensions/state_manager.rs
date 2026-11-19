//! Pure dashboard tree, filtering, and selection state.
//!
//! Reference: `l/src/overlays/extensions/state-manager.ts`.
//!
//! Deviations:
//! - Fuzzy filtering uses [`crate::fuzzy::fuzzy_match`] per extension instead of
//!   the TS `FuzzyQuery`/`FuzzyText` prepared objects and WeakMap cache. Behavior
//!   is equivalent in intent (all query tokens must match searchable text).
//! - `providers`/`extensions` are borrowed/owned Rust values from
//!   [`crate::overlays::extensions::types`].

use crate::fuzzy::fuzzy_match;
use crate::overlays::extensions::types::{
	is_shadowed_extension, DashboardState, DisabledReason, Extension, ExtensionKind,
	ExtensionProvider, ExtensionState, FlatTreeItem, ProviderTab, TreeNode, TreeNodeType,
};

/// Resolve display state and disabled reason for an extension source.
#[must_use]
pub fn resolve_extension_state(
	source_provider: &str,
	source_level: &str,
	is_disabled: bool,
	is_shadowed: bool,
	providers: &[ExtensionProvider],
) -> (ExtensionState, Option<DisabledReason>) {
	if is_disabled {
		return (ExtensionState::Disabled, Some(DisabledReason::ItemDisabled));
	}
	if is_shadowed {
		return (ExtensionState::Shadowed, Some(DisabledReason::Shadowed));
	}
	let provider = providers.iter().find(|provider| provider.id == source_provider);
	if provider.is_some_and(|p| !p.enabled) {
		return (
			ExtensionState::Disabled,
			Some(DisabledReason::ProviderDisabled),
		);
	}
	if source_level == "user" && provider.is_some_and(|p| !p.user_source_enabled) {
		return (ExtensionState::Disabled, Some(DisabledReason::UserOptIn));
	}
	(ExtensionState::Active, None)
}

/// Build sidebar tree from extensions.
#[must_use]
pub fn build_sidebar_tree(extensions: &[Extension], providers: &[ExtensionProvider]) -> Vec<TreeNode> {
	use std::collections::BTreeMap;
	let mut by_provider: BTreeMap<String, BTreeMap<String, Vec<&Extension>>> = BTreeMap::new();
	for ext in extensions {
		let provider_id = ext.source.provider.clone();
		let by_kind = by_provider.entry(provider_id).or_default();
		by_kind
			.entry(ext.kind.as_str().to_string())
			.or_default()
			.push(ext);
	}

	let mut tree = Vec::new();
	for provider in providers {
		if provider.id == "native" {
			continue;
		}
		let mut kind_nodes = Vec::new();
		let mut total_count = 0usize;
		if let Some(by_kind) = by_provider.get(&provider.id) {
			for (kind_str, exts) in by_kind {
				total_count += exts.len();
				let kind = ExtensionKind::parse(kind_str).unwrap_or(ExtensionKind::Tool);
				kind_nodes.push(TreeNode {
					id: format!("{}:{kind_str}", provider.id),
					label: get_kind_display_name(kind),
					node_type: TreeNodeType::Kind,
					enabled: provider.enabled,
					collapsed: true,
					children: Vec::new(),
					count: Some(exts.len()),
				});
			}
			kind_nodes.sort_by(|a, b| b.count.unwrap_or(0).cmp(&a.count.unwrap_or(0)));
		}
		tree.push(TreeNode {
			id: provider.id.clone(),
			label: provider.display_name.clone(),
			node_type: TreeNodeType::Provider,
			enabled: provider.enabled,
			collapsed: false,
			children: kind_nodes,
			count: Some(total_count),
		});
	}
	tree
}

/// Flatten tree for keyboard navigation.
#[must_use]
pub fn flatten_tree(tree: &[TreeNode]) -> Vec<FlatTreeItem> {
	fn walk(node: &TreeNode, depth: usize, index: &mut usize, flat: &mut Vec<FlatTreeItem>) {
		let current = *index;
		*index += 1;
		flat.push(FlatTreeItem {
			node: node.clone(),
			depth,
			index: current,
		});
		if !node.collapsed {
			for child in &node.children {
				walk(child, depth + 1, index, flat);
			}
		}
	}
	let mut flat = Vec::new();
	let mut index = 0usize;
	for node in tree {
		walk(node, 0, &mut index, &mut flat);
	}
	flat
}

/// Apply fuzzy filter to extensions.
#[must_use]
pub fn apply_filter(extensions: &[Extension], query: &str) -> Vec<Extension> {
	let tokens: Vec<String> = query
		.split_whitespace()
		.filter(|t| !t.is_empty())
		.map(|t| t.to_lowercase())
		.collect();
	if tokens.is_empty() {
		return extensions.to_vec();
	}
	extensions
		.iter()
		.filter(|ext| {
			let searchable = [
				ext.name.as_str(),
				ext.display_name.as_str(),
				ext.description.as_deref().unwrap_or(""),
				ext.trigger.as_deref().unwrap_or(""),
				ext.source.provider_name.as_str(),
				ext.kind.as_str(),
			]
			.join(" ");
			tokens
				.iter()
				.all(|token| fuzzy_match(token, &searchable).matches)
		})
		.cloned()
		.collect()
}

fn get_kind_display_name(kind: ExtensionKind) -> String {
	match kind {
		ExtensionKind::ExtensionModule => "Extension Modules".to_string(),
		ExtensionKind::Skill => "Skills".to_string(),
		ExtensionKind::Rule => "Rules".to_string(),
		ExtensionKind::Tool => "Tools".to_string(),
		ExtensionKind::Mcp => "MCP Servers".to_string(),
		ExtensionKind::Prompt => "Prompts".to_string(),
		ExtensionKind::Instruction => "Instructions".to_string(),
		ExtensionKind::ContextFile => "Context Files".to_string(),
		ExtensionKind::Hook => "Hooks".to_string(),
		ExtensionKind::SlashCommand => "Slash Commands".to_string(),
	}
}

/// Build provider tabs from extensions.
#[must_use]
pub fn build_provider_tabs(extensions: &[Extension], providers: &[ExtensionProvider]) -> Vec<ProviderTab> {
	use std::collections::HashMap;
	let mut count_by_provider: HashMap<String, usize> = HashMap::new();
	for ext in extensions {
		*count_by_provider.entry(ext.source.provider.clone()).or_insert(0) += 1;
	}
	let mut tabs = vec![ProviderTab {
		id: "all".to_string(),
		label: "ALL".to_string(),
		enabled: true,
		count: extensions.len(),
	}];
	for provider in providers {
		if provider.id == "native" {
			continue;
		}
		let count = count_by_provider.get(&provider.id).copied().unwrap_or(0);
		tabs.push(ProviderTab {
			id: provider.id.clone(),
			label: provider.display_name.clone(),
			enabled: provider.enabled,
			count,
		});
	}
	tabs.sort_by(|a, b| {
		if a.id == "all" {
			return std::cmp::Ordering::Less;
		}
		if b.id == "all" {
			return std::cmp::Ordering::Greater;
		}
		let category = |t: &ProviderTab| {
			if t.count == 0 && t.enabled {
				2
			} else if !t.enabled {
				1
			} else {
				0
			}
		};
		let ac = category(a);
		let bc = category(b);
		ac.cmp(&bc).then_with(|| b.count.cmp(&a.count))
	});
	tabs
}

/// Filter extensions by provider tab.
#[must_use]
pub fn filter_by_provider(extensions: &[Extension], provider_id: &str) -> Vec<Extension> {
	if provider_id == "all" {
		return extensions.to_vec();
	}
	extensions
		.iter()
		.filter(|ext| ext.source.provider == provider_id)
		.cloned()
		.collect()
}

/// Apply setting-backed item disable overrides to an existing dashboard state.
#[must_use]
pub fn apply_disabled_extensions_to_state(
	mut state: DashboardState,
	disabled_ids: &[String],
	providers: &[ExtensionProvider],
) -> DashboardState {
	use std::collections::HashSet;
	let disabled: HashSet<&str> = disabled_ids.iter().map(String::as_str).collect();
	let update_extension = |ext: &Extension| -> Extension {
		if disabled.contains(ext.id.as_str()) {
			if ext.state == ExtensionState::Disabled
				&& ext.disabled_reason == Some(DisabledReason::ItemDisabled)
			{
				return ext.clone();
			}
			let mut out = ext.clone();
			out.state = ExtensionState::Disabled;
			out.disabled_reason = Some(DisabledReason::ItemDisabled);
			return out;
		}
		if ext.state != ExtensionState::Disabled
			|| ext.disabled_reason != Some(DisabledReason::ItemDisabled)
		{
			return ext.clone();
		}
		let (next_state, next_reason) = resolve_extension_state(
			&ext.source.provider,
			&ext.source.level,
			false,
			is_shadowed_extension(ext),
			providers,
		);
		let mut out = ext.clone();
		out.state = next_state;
		out.disabled_reason = next_reason;
		out
	};
	state.extensions = state.extensions.iter().map(update_extension).collect();
	state.tab_filtered = state.tab_filtered.iter().map(update_extension).collect();
	state.search_filtered = state.search_filtered.iter().map(update_extension).collect();
	state.selected = state.selected.as_ref().map(update_extension);
	state
}

/// Create initial dashboard state.
#[must_use]
pub fn create_initial_state(extensions: Vec<Extension>, providers: &[ExtensionProvider]) -> DashboardState {
	let tabs = build_provider_tabs(&extensions, providers);
	let tab_filtered = extensions.clone();
	let search_filtered = tab_filtered.clone();
	let selected = search_filtered.first().cloned();
	DashboardState {
		tabs,
		active_tab_index: 0,
		extensions,
		tab_filtered,
		search_filtered,
		search_query: String::new(),
		list_index: 0,
		scroll_offset: 0,
		selected,
	}
}

/// Tab identifier for the dashboard (`"all"` or a provider id).
pub type TabId = String;

/// Id of the aggregate tab.
pub const ALL_TAB_ID: &str = "all";

/// Whether a key switches provider tabs (Tab/Shift+Tab/Left/Right).
#[must_use]
pub fn is_tab_switch_key(key: &str) -> bool {
	matches!(key, "tab" | "shift+tab" | "left" | "right" | "ctrl+left" | "ctrl+right")
}

/// Switch state to the given tab/provider id and recompute filtered/selection.
#[must_use]
pub fn switch_to_tab(mut state: DashboardState, tab: TabId) -> DashboardState {
	let provider_id = if tab == ALL_TAB_ID { "all".to_string() } else { tab };
	state.active_tab_index = state
		.tabs
		.iter()
		.position(|t| t.id == provider_id)
		.unwrap_or(state.active_tab_index);
	state.tab_filtered = filter_by_provider(&state.extensions, &provider_id);
	state.search_filtered = apply_filter(&state.tab_filtered, &state.search_query);
	if state
		.selected
		.as_ref()
		.and_then(|sel| state.search_filtered.iter().find(|e| e.id == sel.id))
		.is_none()
	{
		state.selected = state.search_filtered.first().cloned();
	}
	state.list_index = state
		.selected
		.as_ref()
		.and_then(|sel| state.search_filtered.iter().position(|e| e.id == sel.id))
		.unwrap_or(0);
	state.scroll_offset = 0;
	state
}

/// Refresh state after toggle.
#[must_use]
pub fn refresh_state(
	state: DashboardState,
	extensions: Vec<Extension>,
	providers: &[ExtensionProvider],
) -> DashboardState {
	let tabs = build_provider_tabs(&extensions, providers);
	let active_tab = state.tabs.get(state.active_tab_index);
	let provider_id = active_tab.map(|t| t.id.as_str()).unwrap_or("all");
	let tab_filtered = filter_by_provider(&extensions, provider_id);
	let search_filtered = apply_filter(&tab_filtered, &state.search_query);
	let new_active_tab_index = tabs.iter().position(|t| t.id == provider_id).unwrap_or(0);
	let selected_id = state.selected.as_ref().map(|e| e.id.clone());
	let mut selected = selected_id
		.as_ref()
		.and_then(|id| search_filtered.iter().find(|e| &e.id == id).cloned());
	if selected.is_none() && !search_filtered.is_empty() {
		let idx = state.list_index.min(search_filtered.len().saturating_sub(1));
		selected = search_filtered.get(idx).cloned();
	}
	let list_index = selected
		.as_ref()
		.and_then(|s| search_filtered.iter().position(|e| e.id == s.id))
		.unwrap_or(0);
	DashboardState {
		tabs,
		active_tab_index: new_active_tab_index,
		extensions,
		tab_filtered,
		search_filtered,
		search_query: state.search_query,
		list_index,
		scroll_offset: state.scroll_offset,
		selected,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::extensions::types::ExtensionSource;
	use serde_json::json;

	fn provider(id: &str, enabled: bool, user_source_enabled: bool) -> ExtensionProvider {
		ExtensionProvider {
			id: id.into(),
			display_name: id.into(),
			enabled,
			user_source_enabled,
			foreign_user_source: false,
		}
	}

	fn ext(id: &str, kind: ExtensionKind, provider: &str) -> Extension {
		Extension {
			id: id.into(),
			kind,
			name: id.into(),
			display_name: id.into(),
			description: None,
			trigger: None,
			path: "/x".into(),
			source: ExtensionSource {
				provider: provider.into(),
				provider_name: provider.into(),
				level: "project".into(),
			},
			state: ExtensionState::Active,
			disabled_reason: None,
			shadowed_by: None,
			raw: json!({}),
		}
	}

	#[test]
	fn resolve_extension_state_prefers_item_disabled() {
		let providers = vec![provider("p", true, true)];
		let (state, reason) = resolve_extension_state("p", "project", true, false, &providers);
		assert_eq!(state, ExtensionState::Disabled);
		assert_eq!(reason, Some(DisabledReason::ItemDisabled));
	}

	#[test]
	fn build_provider_tabs_includes_all_and_sorts() {
		let providers = vec![provider("a", true, true), provider("b", false, true)];
		let extensions = vec![ext("a1", ExtensionKind::Tool, "a")];
		let tabs = build_provider_tabs(&extensions, &providers);
		assert_eq!(tabs[0].id, "all");
		assert!(tabs.iter().any(|t| t.id == "a"));
		assert!(tabs.iter().any(|t| t.id == "b"));
	}

	#[test]
	fn flatten_tree_walks_visible_children() {
		let tree = vec![TreeNode {
			id: "p".into(),
			label: "P".into(),
			node_type: TreeNodeType::Provider,
			enabled: true,
			collapsed: false,
			children: vec![TreeNode {
				id: "k".into(),
				label: "K".into(),
				node_type: TreeNodeType::Kind,
				enabled: true,
				collapsed: true,
				children: vec![],
				count: Some(1),
			}],
			count: Some(1),
		}];
		let flat = flatten_tree(&tree);
		assert_eq!(flat.len(), 2);
		assert_eq!(flat[0].depth, 0);
		assert_eq!(flat[1].depth, 1);
	}

	#[test]
	fn apply_filter_matches_all_tokens() {
		let exts = vec![ext("bash-tool", ExtensionKind::Tool, "p")];
		let filtered = apply_filter(&exts, "bash tool");
		assert_eq!(filtered.len(), 1);
		let filtered_none = apply_filter(&exts, "bash missingtoken");
		assert!(filtered_none.is_empty());
	}
}
