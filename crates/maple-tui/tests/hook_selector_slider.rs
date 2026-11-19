//! Port of `l/test/hook-selector-slider.test.ts`.
//!
//! The crate's selector is not a `Component`: it exposes `rows()`, `slider_line()`
//! and accessors, so rendering is asserted through those. The crate owns the
//! slider index instead of firing `onChange`, and binds `h`/`l` (the kitty
//! CSI-u equivalents) while a slider is focused.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::keys::Key;
use maple_tui::overlays::hook_selector::{
	HookSelectorCallbacks, HookSelectorComponent, HookSelectorOptions, HookSelectorSlider,
	HookSelectorSliderSegment,
};

fn model_slider(index: usize) -> HookSelectorSlider {
	HookSelectorSlider {
		caption: Some("continue with".to_string()),
		segments: vec![
			HookSelectorSliderSegment {
				label: "smol".to_string(),
				detail: Some("gpt-5-mini".to_string()),
			},
			HookSelectorSliderSegment {
				label: "default".to_string(),
				detail: Some("claude-sonnet".to_string()),
			},
			HookSelectorSliderSegment {
				label: "slow".to_string(),
				detail: Some("claude-opus".to_string()),
			},
		],
		index,
	}
}

struct Harness {
	component: HookSelectorComponent,
	selected: Rc<RefCell<Vec<String>>>,
	cancelled: Rc<RefCell<usize>>,
}

fn harness(slider: Option<HookSelectorSlider>) -> Harness {
	let selected = Rc::new(RefCell::new(Vec::new()));
	let cancelled = Rc::new(RefCell::new(0));
	let mut callbacks = HookSelectorCallbacks::default();
	callbacks.on_select = Some(Box::new(move |label| selected.borrow_mut().push(label.to_string())));
	callbacks.on_cancel = Some(Box::new(|| *cancelled.borrow_mut() += 1));
	let options = HookSelectorOptions {
		slider,
		callbacks,
		..HookSelectorOptions::default()
	};
	let component = HookSelectorComponent::new(
		"Plan mode - next step",
		vec!["Approve and execute".into(), "Refine plan".into()],
		options,
	);
	Harness {
		component,
		selected,
		cancelled,
	}
}

/// Slider line plus the option rows, mirroring the on-screen card.
fn text(component: &HookSelectorComponent) -> String {
	let mut out = component.slider_line();
	for row in component.rows() {
		out.push('\n');
		out.push_str(&row.text);
	}
	out
}

#[test]
fn renders_every_tier_label_plus_the_active_tier_detail() {
	let h = harness(Some(model_slider(1)));
	let rendered = text(&h.component);
	assert!(rendered.contains("smol"), "{rendered}");
	assert!(rendered.contains("default"), "{rendered}");
	assert!(rendered.contains("slow"), "{rendered}");
	assert!(rendered.contains("claude-sonnet"), "{rendered}");
	assert!(!rendered.contains("gpt-5-mini"), "{rendered}");
	assert!(!rendered.contains("claude-opus"), "{rendered}");
}

#[test]
fn right_arrow_advances_the_slider_and_the_displayed_model() {
	let mut h = harness(Some(model_slider(1)));
	h.component.handle_key(&Key::Right);
	assert_eq!(h.component.slider_index(), 2);
	let rendered = text(&h.component);
	assert!(rendered.contains("claude-opus"), "{rendered}");
	assert!(!rendered.contains("claude-sonnet"), "{rendered}");
}

#[test]
fn left_and_right_move_from_any_position_without_selecting() {
	let mut h = harness(Some(model_slider(2)));
	h.component.handle_key(&Key::Left);
	assert_eq!(h.component.slider_index(), 1);
	h.component.handle_key(&Key::Left);
	assert_eq!(h.component.slider_index(), 0);
	assert!(text(&h.component).contains("gpt-5-mini"));
	assert!(h.selected.borrow().is_empty());
	assert_eq!(*h.cancelled.borrow(), 0);
}

#[test]
fn slider_h_and_l_keys_move_in_both_directions() {
	let mut left = harness(Some(model_slider(1)));
	left.component.handle_key(&Key::Char('h'));
	assert_eq!(left.component.slider_index(), 0);

	let mut right = harness(Some(model_slider(1)));
	right.component.handle_key(&Key::Char('l'));
	assert_eq!(right.component.slider_index(), 2);
}

#[test]
fn clamps_at_both_edges_and_only_moves_on_real_change() {
	let mut h = harness(Some(model_slider(0)));
	h.component.handle_key(&Key::Left);
	assert_eq!(h.component.slider_index(), 0);
	h.component.handle_key(&Key::Right);
	h.component.handle_key(&Key::Right);
	assert_eq!(h.component.slider_index(), 2);
	h.component.handle_key(&Key::Right);
	assert_eq!(h.component.slider_index(), 2);
}

#[test]
fn clamps_a_constructor_index_beyond_the_segment_range() {
	let mut h = harness(Some(model_slider(99)));
	assert_eq!(h.component.slider_index(), 2);
	h.component.handle_key(&Key::Right);
	assert_eq!(h.component.slider_index(), 2);
	h.component.handle_key(&Key::Left);
	assert_eq!(h.component.slider_index(), 1);
}

#[test]
fn falls_back_to_left_right_callbacks_without_a_slider() {
	let left = Rc::new(RefCell::new(0));
	let right = Rc::new(RefCell::new(0));
	let mut callbacks = HookSelectorCallbacks::default();
	callbacks.on_left = Some(Box::new(|| *left.borrow_mut() += 1));
	callbacks.on_right = Some(Box::new(|| *right.borrow_mut() += 1));
	let options = HookSelectorOptions {
		callbacks,
		..HookSelectorOptions::default()
	};
	let mut component = HookSelectorComponent::new(
		"Plan mode - next step",
		vec!["Approve and execute".into(), "Refine plan".into()],
		options,
	);
	component.handle_key(&Key::Left);
	component.handle_key(&Key::Right);
	assert_eq!((*left.borrow(), *right.borrow()), (1, 1));
}

#[test]
fn fuzzy_filters_overflowing_option_lists_from_typed_input() {
	let selected = Rc::new(RefCell::new(Vec::new()));
	let mut callbacks = HookSelectorCallbacks::default();
	callbacks.on_select = Some(Box::new(move |label| selected.borrow_mut().push(label.to_string())));
	let options = HookSelectorOptions {
		max_visible: 3,
		callbacks,
		..HookSelectorOptions::default()
	};
	let mut component = HookSelectorComponent::new(
		"Choose provider",
		vec!["Ollama".into(), "Kagi".into(), "OpenCode Go".into(), "Tavily".into()],
		options,
	);
	component.handle_key(&Key::Char('o'));
	component.handle_key(&Key::Char('g'));
	assert_eq!(component.query(), "og");
	let rendered = text(&component);
	assert!(rendered.contains("OpenCode Go"), "{rendered}");
	assert!(!rendered.contains("Ollama"), "{rendered}");
	assert!(rendered.contains("Search: og"), "{rendered}");
	component.handle_key(&Key::Enter);
	assert_eq!(*selected.borrow(), vec!["OpenCode Go".to_string()]);
}
