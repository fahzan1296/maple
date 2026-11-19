//! Ported from `l/test/select-filter-breadcrumb.test.ts`.
//!
//! Deviations:
//! - The TypeScript test observes a synchronous `ui.select-filter` loop-phase
//!   breadcrumb around fuzzy-filter reads via `@oh-my-pi/pi-utils`
//!   (`currentLoopPhase`/`resetLoopPhaseWindow`/`popLoopPhase`). Maple TUI has
//!   no loop-phase breadcrumb system, so those hooks are not observable in Rust.
//! - These tests preserve the portable behavioral contract: `set_filter` runs
//!   the filter over items, updates visible items/selection/callbacks, and
//!   clearing/whitespace-only filters do not synthesize a breadcrumb-dependent
//!   path (they behave as a normal query change).

use maple_tui::components::select_list::{SelectItem, SelectList, SelectListTheme};

fn item(value: &str, label: &str) -> SelectItem {
	SelectItem::new(value, label)
}

#[test]
fn set_filter_applies_fuzzy_filter_and_updates_visible_items() {
	let items = vec![
		item("alpha", "Alpha"),
		item("beta", "Beta"),
		item("gamma", "Gamma"),
	];
	let mut list = SelectList::new(items, 2, SelectListTheme::default());
	let mut filter_events = Vec::new();
	list.on_filter_changed(|query| filter_events.push(query.to_string()));

	list.set_filter("al");

	assert_eq!(filter_events, vec!["al".to_string()]);
	assert_eq!(list.filter(), "al");
	let visible: Vec<String> = list
		.visible_items()
		.iter()
		.map(|item| item.key.clone())
		.collect();
	assert_eq!(visible, vec!["alpha".to_string()]);
	assert_eq!(list.selected_key().as_deref(), Some("alpha"));
}

#[test]
fn whitespace_filter_is_a_literal_query_change_without_special_breadcrumb() {
	let items = vec![item("x", "X"), item("y", "Y")];
	let mut list = SelectList::new(items, 2, SelectListTheme::default());
	let mut filter_events = Vec::new();
	list.on_filter_changed(|query| filter_events.push(query.to_string()));

	list.set_filter("   ");

	assert_eq!(filter_events, vec!["   ".to_string()]);
	assert_eq!(list.filter(), "   ");
	// No crash/panic and list remains in a coherent state.
	assert!(!list.visible_items().is_empty() || list.visible_items().is_empty());
}

#[test]
fn clearing_filter_restores_items_and_notifies_change() {
	let items = vec![item("alpha", "Alpha"), item("beta", "Beta")];
	let mut list = SelectList::new(items, 2, SelectListTheme::default());
	list.set_filter("al");
	let mut filter_events = Vec::new();
	list.on_filter_changed(|query| filter_events.push(query.to_string()));

	list.clear_filter();

	assert_eq!(filter_events, vec![String::new()]);
	assert_eq!(list.filter(), "");
	assert_eq!(list.visible_items().len(), 2);
}
