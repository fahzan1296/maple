//! Port of `l/test/wrapper-selector-mouse-offset.test.ts`.
//!
//! Every inline-picker wrapper mounts a single-line top border before its
//! [`SelectList`], so overlay-local pointer rows are offset by one. These guard
//! the off-by-one that would let a top-border click select the first row.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::mouse::{MouseButtonKind, MouseKind, MouseEvent};
use maple_tui::overlays::plugin_selector::{
	PluginInfo, PluginItem, PluginSelectorCallbacks, PluginSelectorComponent,
};
use maple_tui::overlays::queue_mode_selector::QueueModeSelectorComponent;
use maple_tui::overlays::theme_selector::ThemeSelectorComponent;
use maple_tui::overlays::thinking_selector::{ThinkingLevelOption, ThinkingSelectorComponent};

fn click(row: u16) -> MouseEvent {
	MouseEvent {
		kind: MouseKind::Down(MouseButtonKind::Left),
		column: 0,
		row,
		shift: false,
		ctrl: false,
		alt: false,
	}
}

#[test]
fn theme_selector_ignores_the_border_row_and_selects_the_first_theme_below_it() {
	let selected: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
	let sink = Rc::clone(&selected);
	let mut component = ThemeSelectorComponent::new(
		"alpha",
		&["alpha".to_string(), "beta".to_string()],
		move |value| *sink.borrow_mut() = Some(value),
		|| {},
		|| {},
	);
	component.render(80);

	component.route_mouse(&click(0), 0, 0);
	assert!(selected.borrow().is_none(), "border row must be inert");

	component.route_mouse(&click(1), 1, 0);
	assert_eq!(selected.borrow().as_deref(), Some("alpha"));
}

#[test]
fn thinking_selector_ignores_the_border_row_and_selects_the_first_level_below_it() {
	let selected: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
	let sink = Rc::clone(&selected);
	let levels = vec![
		ThinkingLevelOption {
			value: "low".to_string(),
			label: "Low".to_string(),
			description: None,
			depth: Some(0.25),
		},
		ThinkingLevelOption {
			value: "high".to_string(),
			label: "High".to_string(),
			description: None,
			depth: Some(0.75),
		},
	];
	let mut component =
		ThinkingSelectorComponent::new(Some("low"), levels, move |value| *sink.borrow_mut() = Some(value), || {});
	component.render(80);

	component.route_mouse(&click(0), 0, 0);
	assert!(selected.borrow().is_none(), "border row must be inert");

	component.route_mouse(&click(1), 1, 0);
	assert_eq!(selected.borrow().as_deref(), Some("low"));
}

#[test]
fn queue_mode_selector_ignores_the_border_row_and_selects_the_first_mode_below_it() {
	let selected: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
	let sink = Rc::clone(&selected);
	let mut component =
		QueueModeSelectorComponent::new("one-at-a-time", move |value| *sink.borrow_mut() = Some(value.to_string()), || {});
	component.render(80);

	component.route_mouse(&click(0), 0, 0);
	assert!(selected.borrow().is_none(), "border row must be inert");

	component.route_mouse(&click(1), 1, 0);
	assert_eq!(selected.borrow().as_deref(), Some("one-at-a-time"));
}

#[test]
fn plugin_selector_ignores_the_border_row_and_selects_the_first_plugin_below_it() {
	let selected: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
	let sink = Rc::clone(&selected);
	let plugins = vec![
		PluginItem::new(PluginInfo::new("alpha"), "marketplace"),
		PluginItem::new(PluginInfo::new("beta"), "marketplace"),
	];
	let callbacks = PluginSelectorCallbacks::new(
		move |name, _marketplace, _scope| *sink.borrow_mut() = Some(name.to_string()),
		|| {},
	);
	let mut component = PluginSelectorComponent::new(2, &plugins, &HashSet::new(), callbacks);
	component.render(80);

	component.route_mouse(&click(0), 0, 0);
	assert!(selected.borrow().is_none(), "border row must be inert");

	component.route_mouse(&click(1), 1, 0);
	assert_eq!(selected.borrow().as_deref(), Some("alpha"));
}
