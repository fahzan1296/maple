//! Port of `l/test/extension-list-mouse.test.ts`.

use std::sync::{Arc, Mutex};

use maple_tui::component::Component;
use maple_tui::overlays::extensions::extension_dashboard::build_tab_bar_tabs;
use maple_tui::overlays::extensions::extension_list::{ExtensionList, ExtensionListCallbacks};
use maple_tui::overlays::extensions::types::{
	Extension, ExtensionKind, ExtensionSource, ExtensionState, ProviderTab,
};

fn source(level: &str) -> ExtensionSource {
	ExtensionSource {
		provider: "native".to_string(),
		provider_name: "Native".to_string(),
		level: level.to_string(),
	}
}

fn skill(name: &str) -> Extension {
	Extension {
		id: format!("skill:{name}"),
		kind: ExtensionKind::Skill,
		name: name.to_string(),
		display_name: name.to_string(),
		path: format!("/tmp/skill-{name}"),
		source: source("native"),
		state: ExtensionState::Active,
		raw: serde_json::json!({}),
		..Extension::default()
	}
}

type Toggles = Arc<Mutex<Vec<(String, bool)>>>;
type Selected = Arc<Mutex<Vec<Option<String>>>>;

fn build_list(toggles: Toggles, selected: Selected) -> ExtensionList {
	let callbacks = ExtensionListCallbacks {
		on_toggle: Some(Arc::new(move |id: &str, enabled: bool| {
			toggles.lock().unwrap().push((id.to_string(), enabled));
		})),
		on_selection_change: Some(Arc::new(move |ext: Option<Extension>| {
			selected.lock().unwrap().push(ext.map(|e| e.id));
		})),
		..Default::default()
	};
	let mut list = ExtensionList::new(vec![skill("alpha"), skill("beta"), skill("gamma")], callbacks, 15);
	list.set_focused(true);
	let _ = list.render(40); // populate hit rows and the visible-count window
	list
}

#[test]
fn hit_test_skips_banner_and_separator() {
	let list = build_list(Arc::default(), Arc::default());
	assert_eq!(list.hit_test(0), None);
	assert_eq!(list.hit_test(1), None);
	assert_eq!(list.hit_test(2), Some(0)); // kind header
	assert_eq!(list.hit_test(3), Some(1)); // alpha
	assert_eq!(list.hit_test(5), Some(3)); // gamma
	assert_eq!(list.hit_test(6), None);
}

#[test]
fn click_selects_then_toggles_selected_row() {
	let toggles: Toggles = Arc::default();
	let selected: Selected = Arc::default();
	let mut list = build_list(toggles.clone(), selected.clone());

	assert!(list.handle_click(3));
	assert_eq!(list.get_selected_extension().unwrap().id, "skill:alpha");
	assert_eq!(selected.lock().unwrap().len(), 1);
	assert!(toggles.lock().unwrap().is_empty());

	assert!(list.handle_click(3));
	assert_eq!(
		toggles.lock().unwrap().as_slice(),
		&[("skill:alpha".to_string(), false)]
	);
}

#[test]
fn clicks_outside_rows_are_noops() {
	let toggles: Toggles = Arc::default();
	let mut list = build_list(toggles.clone(), Arc::default());
	assert!(!list.handle_click(0));
	assert!(!list.handle_click(9));
	assert!(list.get_selected_extension().is_none()); // selection still on the header
	assert!(toggles.lock().unwrap().is_empty());
}

#[test]
fn wheel_moves_selection_like_arrows() {
	let selected: Selected = Arc::default();
	let mut list = build_list(Arc::default(), selected.clone());
	list.handle_click(3);
	list.handle_wheel(1);
	assert_eq!(list.get_selected_extension().unwrap().id, "skill:beta");
	list.handle_wheel(-1);
	assert_eq!(list.get_selected_extension().unwrap().id, "skill:alpha");
	assert_eq!(
		selected.lock().unwrap().as_slice(),
		&[
			Some("skill:alpha".to_string()),
			Some("skill:beta".to_string()),
			Some("skill:alpha".to_string()),
		]
	);
}

fn provider_tab(id: &str, label: &str, enabled: bool, count: usize) -> ProviderTab {
	ProviderTab {
		id: id.to_string(),
		label: label.to_string(),
		enabled,
		count,
	}
}

#[test]
fn empty_enabled_providers_are_muted() {
	let tabs = build_tab_bar_tabs(&[
		provider_tab("all", "ALL", true, 5),
		provider_tab("skills", "Skills", true, 3),
		provider_tab("empty", "Empty", true, 0),
		provider_tab("off", "Off", false, 2),
	]);
	let find = |id: &str| tabs.iter().find(|t| t.id == id).unwrap();

	assert!(!find("all").muted);
	assert_eq!(find("all").label, "ALL (5)");
	assert!(!find("skills").muted);
	assert!(find("skills").label.contains("Skills (3)"));
	assert!(find("empty").muted);
	assert!(!find("off").muted);
	assert!(find("off").label.contains("Off (2)"));
}

#[test]
fn newlines_in_triggers_collapse_to_one_row() {
	let dirty = Extension {
		id: "rule:dirty".to_string(),
		kind: ExtensionKind::Rule,
		name: "dirty".to_string(),
		display_name: "dirty".to_string(),
		path: "/tmp/dirty.md".to_string(),
		source: source("native"),
		state: ExtensionState::Active,
		trigger: Some("src/**/*.ts\nextra".to_string()),
		raw: serde_json::json!({}),
		..Extension::default()
	};
	let mut list = ExtensionList::new(vec![dirty], ExtensionListCallbacks::default(), 15);
	list.set_focused(true);
	let lines = list.render(80);
	assert!(lines.iter().all(|line| !line.contains('\n')));
	assert_eq!(lines.join("\n").split('\n').count(), lines.len());
}

#[test]
fn project_rows_show_the_project_name() {
	let project = Extension {
		path: "/home/sf/worlds/personal/.omp/skills/gog/SKILL.md".to_string(),
		source: source("project"),
		..skill("gog")
	};
	let mut list = ExtensionList::new(vec![project], ExtensionListCallbacks::default(), 15);
	list.set_focused(true);
	let text = list.render(80).join("\n");
	assert!(text.contains("personal"));
}

#[test]
fn context_files_without_omp_do_not_invent_a_project() {
	let ext = Extension {
		id: "context-file:project:AGENTS.md".to_string(),
		kind: ExtensionKind::ContextFile,
		name: "AGENTS.md".to_string(),
		display_name: "AGENTS.md".to_string(),
		trigger: Some("project".to_string()),
		path: "/home/sf/worlds/personal/AGENTS.md".to_string(),
		source: source("project"),
		raw: serde_json::json!({ "level": "project" }),
		..Extension::default()
	};
	let mut list = ExtensionList::new(vec![ext], ExtensionListCallbacks::default(), 15);
	list.set_focused(true);
	let text = list.render(80).join("\n");
	assert!(text.contains("AGENTS.md"));
	assert!(!text.contains("project · personal"));
}
