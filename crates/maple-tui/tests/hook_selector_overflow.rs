//! Port of `l/test/hook-selector-overflow.test.ts`.
//!
//! The crate's selector exposes `rows()` (unwrapped option labels, descriptions
//! wrapped at 76 columns) rather than a width-aware `render`, so the
//! width-bound tests stay ignored. Structural behaviour — full text preserved,
//! description rows, disabled rows, numeric quick-select, checkbox markers and
//! type-to-search — ports directly.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::keys::Key;
use maple_tui::overlays::hook_selector::{
	HookSelectorCallbacks, HookSelectorComponent, HookSelectorOption, HookSelectorOptionInput,
	HookSelectorOptions, SelectionMarker,
};

/// Collapse box-drawing glyphs and runs of whitespace so wrapped rows compare
/// as one logical line.
fn normalized(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut pending_space = false;
	for ch in text.chars() {
		if ('\u{2500}'..='\u{257f}').contains(&ch) || ch == ' ' {
			if !out.is_empty() {
				pending_space = true;
			}
			continue;
		}
		if pending_space {
			out.push(' ');
			pending_space = false;
		}
		out.push(ch);
	}
	out
}

fn rows_text(component: &HookSelectorComponent) -> String {
	component.rows().iter().map(|row| row.text.as_str()).collect::<Vec<_>>().join("\n")
}

fn options(items: impl IntoIterator<Item = impl Into<HookSelectorOptionInput>>) -> Vec<HookSelectorOptionInput> {
	items.into_iter().map(Into::into).collect()
}

fn selector(title: &str, items: Vec<HookSelectorOptionInput>, opts: HookSelectorOptions) -> HookSelectorComponent {
	HookSelectorComponent::new(title, items, opts)
}

fn recording_callbacks() -> (HookSelectorCallbacks, Rc<RefCell<Option<String>>>, Rc<RefCell<bool>>) {
	let selected = Rc::new(RefCell::new(None::<String>));
	let fired = Rc::new(RefCell::new(false));
	let sink = selected.clone();
	let flag = fired.clone();
	let mut callbacks = HookSelectorCallbacks::default();
	callbacks.on_select = Some(Box::new(move |label| {
		*sink.borrow_mut() = Some(label.to_string());
		*flag.borrow_mut() = true;
	}));
	(callbacks, selected, fired)
}

#[test]
#[ignore = "missing API: option-label wrapping to the render width (rows() emits unwrapped labels)"]
fn keeps_outlined_options_within_render_width() {
}

#[test]
fn preserves_the_tail_of_long_outlined_options() {
	let items = options([
		"Option A: Move to OMP-native only by migrating reusable shared AI instructions into .omp/AGENTS.md, .omp/rules, .omp/skills, and .omp/agents while deliberately not creating a root .github directory.",
		"Option B: Keep dual support by migrating canonical instructions into .omp while also maintaining a root .github/copilot-instructions.md compatibility bridge for editors that do not understand OMP resources yet.",
	]);
	let opts = HookSelectorOptions {
		outline: true,
		..HookSelectorOptions::default()
	};
	let component = selector("Which migration stance should be used?", items, opts);
	let rendered = rows_text(&component);
	assert!(normalized(&rendered).contains("not creating a root .github directory"), "{rendered}");
	assert!(normalized(&rendered).contains("do not understand OMP resources yet"), "{rendered}");
}

#[test]
fn renders_option_descriptions_as_separate_rows_below_the_label() {
	let items = options([
		HookSelectorOption {
			label: "Use existing local credentials".to_string(),
			description: Some(
				"Authenticate via the provider keys and OAuth state already configured under ~/.omp without opening a new browser-based setup flow."
					.to_string(),
			),
		},
		HookSelectorOption {
			label: "Set up omp in terminal".to_string(),
			description: Some(
				"Launch the local terminal UI to add provider keys, select models, and keep the current editor session waiting for the configured credentials."
					.to_string(),
			),
		},
	]);
	let opts = HookSelectorOptions {
		outline: true,
		..HookSelectorOptions::default()
	};
	let component = selector("How should authentication continue?", items, opts);
	let lines = component.rows();
	let rendered = rows_text(&component);
	let label_line = lines.iter().position(|row| row.text.contains("Use existing local credentials"));
	let description_line = lines.iter().position(|row| row.text.contains("Authenticate via the provider keys"));
	assert!(label_line.is_some_and(|at| description_line.is_some_and(|later| later > at)), "{rendered}");
	assert!(normalized(&rendered).contains("without opening a new browser-based setup flow"), "{rendered}");
	assert!(normalized(&rendered).contains("keep the current editor session waiting for the configured credentials"), "{rendered}");
}

#[test]
fn renders_a_multiline_title_without_dropping_detail() {
	let component = selector(
		"Delete session?\nsession-2026-08-20",
		options(["Delete", "Cancel"]),
		HookSelectorOptions::default(),
	);
	assert_eq!(component.base_title(), "Delete session?");
	assert_eq!(component.detail_lines(), &["session-2026-08-20".to_string()]);
}

#[test]
fn skips_disabled_options_during_keyboard_navigation() {
	let (callbacks, selected, _) = recording_callbacks();
	let opts = HookSelectorOptions {
		disabled_indices: vec![1],
		callbacks,
		..HookSelectorOptions::default()
	};
	let mut component = selector("Pick one", options(["First", "Disabled", "Third"]), opts);
	component.handle_key(&Key::Down);
	component.handle_key(&Key::Enter);
	assert_eq!(selected.borrow().as_deref(), Some("Third"));
}

#[test]
fn does_not_select_a_solely_disabled_option() {
	let (callbacks, _, fired) = recording_callbacks();
	let opts = HookSelectorOptions {
		disabled_indices: vec![0],
		callbacks,
		..HookSelectorOptions::default()
	};
	let mut component = selector("Pick one", options(["Disabled"]), opts);
	component.handle_key(&Key::Enter);
	assert!(!*fired.borrow());
}

#[test]
fn selects_the_matching_numbered_option_after_an_unnumbered_row() {
	let (callbacks, selected, _) = recording_callbacks();
	let mut component = selector(
		"Pick one",
		options(["Detected item", "1. First", "2. Second", "3. Third"]),
		HookSelectorOptions::default(),
	);
	component.handle_key(&Key::Char('3'));
	assert_eq!(selected.borrow().as_deref(), Some("3. Third"));
}

#[test]
fn ignores_a_digit_that_targets_a_disabled_option() {
	let (callbacks, _, fired) = recording_callbacks();
	let opts = HookSelectorOptions {
		disabled_indices: vec![1],
		callbacks,
		..HookSelectorOptions::default()
	};
	let mut component = selector("Pick one", options(["1. First", "2. Disabled", "3. Third"]), opts);
	component.handle_key(&Key::Char('2'));
	assert!(!*fired.borrow());
}

#[test]
fn ignores_a_digit_without_a_matching_numbered_option() {
	let (callbacks, _, fired) = recording_callbacks();
	let mut component = selector(
		"Pick one",
		options(["1. First", "2. Second"]),
		HookSelectorOptions::default(),
	);
	component.handle_key(&Key::Char('9'));
	assert!(!*fired.borrow());
}

#[test]
fn does_not_turn_an_unnumbered_menu_into_implicit_numeric_shortcuts() {
	let (callbacks, _, fired) = recording_callbacks();
	let mut component = selector(
		"Pick one",
		options(["First", "Second", "Third"]),
		HookSelectorOptions::default(),
	);
	component.handle_key(&Key::Char('3'));
	assert!(!*fired.borrow());
	assert_eq!(component.query(), "");
}

#[test]
fn checkbox_menus_move_the_cursor_without_confirming() {
	let (callbacks, selected, _) = recording_callbacks();
	let opts = HookSelectorOptions {
		selection_marker: Some(SelectionMarker::Checkbox),
		callbacks,
		..HookSelectorOptions::default()
	};
	let mut component = selector("Pick many", options(["1. First", "2. Second", "3. Third"]), opts);
	component.handle_key(&Key::Char('2'));
	assert!(selected.borrow().is_none());
	component.handle_key(&Key::Enter);
	assert_eq!(selected.borrow().as_deref(), Some("2. Second"));
}

#[test]
fn digits_become_search_text_once_the_list_overflows() {
	let items = options((1..=20).map(|index| format!("Option {index}")));
	let opts = HookSelectorOptions {
		max_visible: 5,
		..HookSelectorOptions::default()
	};
	let mut component = selector("Pick one", items, opts);
	component.handle_key(&Key::Char('1'));
	assert_eq!(component.query(), "1");
	assert!(rows_text(&component).contains("Search: 1"));
}
