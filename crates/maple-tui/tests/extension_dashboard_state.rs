//! Port of `l/test/extension-dashboard-state.test.ts`.

use maple_tui::overlays::extensions::state_manager::apply_disabled_extensions_to_state;
use maple_tui::overlays::extensions::types::{
	DashboardState, DisabledReason, Extension, ExtensionKind, ExtensionProvider, ExtensionSource,
	ExtensionState, ProviderTab,
};
use serde_json::json;

fn extension(id: &str) -> Extension {
	let name = id.trim_start_matches("skill:").to_string();
	Extension {
		id: id.to_string(),
		kind: ExtensionKind::Skill,
		name: name.clone(),
		display_name: name,
		description: None,
		trigger: None,
		path: format!("/tmp/{id}"),
		source: ExtensionSource {
			provider: "native".to_string(),
			provider_name: "Native".to_string(),
			level: "native".to_string(),
		},
		state: ExtensionState::Active,
		disabled_reason: None,
		shadowed_by: None,
		raw: json!({}),
	}
}

fn dashboard_state(extensions: Vec<Extension>) -> DashboardState {
	DashboardState {
		tabs: vec![ProviderTab {
			id: "all".to_string(),
			label: "ALL".to_string(),
			enabled: true,
			count: extensions.len(),
		}],
		active_tab_index: 0,
		tab_filtered: extensions.clone(),
		search_filtered: extensions.clone(),
		search_query: String::new(),
		list_index: 0,
		scroll_offset: 0,
		selected: extensions.first().cloned(),
		extensions,
	}
}

#[test]
fn applies_item_disabled_state_to_every_visible_dashboard_slice() {
	let alpha = extension("skill:alpha");
	let state = dashboard_state(vec![alpha, extension("skill:beta")]);
	let providers: Vec<ExtensionProvider> = Vec::new();

	let next = apply_disabled_extensions_to_state(state, &["skill:alpha".to_string()], &providers);

	for slice in [&next.extensions, &next.tab_filtered, &next.search_filtered] {
		assert_eq!(slice[0].state, ExtensionState::Disabled);
		assert_eq!(slice[0].disabled_reason, Some(DisabledReason::ItemDisabled));
		assert_eq!(slice[1].state, ExtensionState::Active);
	}
	let selected = next.selected.expect("selection survives");
	assert_eq!(selected.id, "skill:alpha");
	assert_eq!(selected.state, ExtensionState::Disabled);
	assert_eq!(selected.disabled_reason, Some(DisabledReason::ItemDisabled));
}

#[test]
fn restores_a_previously_item_disabled_shadowed_extension_as_shadowed() {
	let mut shadowed = extension("skill:shadowed");
	shadowed.state = ExtensionState::Disabled;
	shadowed.disabled_reason = Some(DisabledReason::ItemDisabled);
	shadowed.shadowed_by = Some("skill:shadowing".to_string());
	let state = dashboard_state(vec![shadowed]);
	let providers: Vec<ExtensionProvider> = Vec::new();

	let next = apply_disabled_extensions_to_state(state, &[], &providers);

	assert_eq!(next.extensions[0].state, ExtensionState::Shadowed);
	assert_eq!(next.extensions[0].disabled_reason, Some(DisabledReason::Shadowed));
	assert_eq!(next.extensions[0].shadowed_by.as_deref(), Some("skill:shadowing"));
	let selected = next.selected.expect("selection survives");
	assert_eq!(selected.state, ExtensionState::Shadowed);
	assert_eq!(selected.disabled_reason, Some(DisabledReason::Shadowed));
}
