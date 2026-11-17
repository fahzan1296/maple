//! Port of `l/test/interactive-native.test.ts`.
//!
//! Deviations:
//! - The reference inspects native TSP `describe()` nodes for every component;
//!   the crate has no native describe tree, so assertions use the public
//!   component state and rendered rows instead.
//! - Masked input renders `*`, not the reference's `•`.
//! - Vim mode, terminal wrap rows, loader/countdown timers, and magic-keyword
//!   shimmer are host/Bun concerns without crate equivalents; the portable
//!   editing, masking, and menu contracts are tested below.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::ansi_scan::strip_ansi;
use maple_tui::component::Component;
use maple_tui::components::editor::Editor;
use maple_tui::components::input::Input;
use maple_tui::components::select_list::{SelectItem, SelectList};
use maple_tui::components::settings_list::{SettingItem, SettingsList};
use maple_tui::keys::Key;
use maple_tui::theme::{default_theme, tui_adapters};

fn select_items() -> Vec<SelectItem> {
	vec![
		SelectItem::new("a", "Alpha").with_label("Alpha"),
		SelectItem::new("b", "Beta").disabled(),
		SelectItem::new("c", "Gamma"),
		SelectItem::new("d", "Delta").with_confirmation("Press again to delete"),
	]
}

#[test]
fn changes_text_and_cursor_when_typing_and_only_cursor_when_moving() {
	let mut editor = Editor::new();
	editor.set_text("hello");
	assert_eq!(editor.cursor_column(), 5);

	editor.handle_input(&Key::Char('!'));
	assert_eq!(editor.get_text(), "hello!");
	assert_eq!(editor.cursor_column(), 6);

	editor.handle_input(&Key::Left);
	assert_eq!(editor.get_text(), "hello!");
	assert_eq!(editor.cursor_column(), 5);
}

#[test]
fn never_renders_a_masked_inputs_secret() {
	let mut input = Input::new();
	input.set_password(true);
	input.set_value("hunter2");
	input.handle_input(&Key::Left);

	let rendered = strip_ansi(&input.render(80).join("\n"));
	assert!(rendered.contains("*******"), "masked row: {rendered:?}");
	assert!(!rendered.contains("hunter2"));
	assert_eq!(input.cursor_viewport_column(80), 6);
}

#[test]
fn activates_the_same_item_by_keyboard_and_by_pointer() {
	let theme = default_theme();
	let list_theme = tui_adapters::get_select_list_theme(&theme);

	let selected: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
	let mut keyboard = SelectList::new(select_items(), 5, list_theme.clone());
	let sink = Rc::clone(&selected);
	keyboard.on_select(move |item| sink.borrow_mut().push(item.key.clone()));
	keyboard.handle_input(&Key::Down);
	keyboard.handle_input(&Key::Enter);
	assert_eq!(selected.borrow().as_slice(), ["c"]);
	assert_eq!(keyboard.selected_key().as_deref(), Some("c"));

	let pointer_selected: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
	let mut pointer = SelectList::new(select_items(), 5, list_theme);
	let sink = Rc::clone(&pointer_selected);
	pointer.on_select(move |item| sink.borrow_mut().push(item.key.clone()));
	pointer.click_item(2);
	assert_eq!(pointer_selected.borrow().as_slice(), ["c"]);
	assert_eq!(pointer.selected_key().as_deref(), Some("c"));
}

#[test]
fn keeps_the_confirmation_step_for_activation_of_a_confirm_required_item() {
	let theme = default_theme();
	let list_theme = tui_adapters::get_select_list_theme(&theme);
	let selected: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
	let mut list = SelectList::new(select_items(), 5, list_theme);
	let sink = Rc::clone(&selected);
	list.on_select(move |item| sink.borrow_mut().push(item.key.clone()));

	list.click_item(3);
	assert!(selected.borrow().is_empty(), "first activation must wait");
	let pending = strip_ansi(&list.render(80).join("\n"));
	assert!(pending.contains("Press again to delete"), "status: {pending:?}");

	list.click_item(3);
	assert_eq!(selected.borrow().as_slice(), ["d"]);
}

#[test]
fn settings_selection_moves_on_select_and_changes_value_when_picked_again() {
	let theme = default_theme();
	let list_theme = tui_adapters::get_settings_list_theme(&theme);
	let toggled: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
	let items = vec![
		SettingItem::new("one", "One", "on").with_values(vec!["on".to_string(), "off".to_string()]),
		SettingItem::new("two", "Two", "on").with_values(vec!["on".to_string(), "off".to_string()]),
	];
	let mut list = SettingsList::new(items, 5, list_theme);
	let sink = Rc::clone(&toggled);
	list.on_toggle(move |id, value| sink.borrow_mut().push((id.to_string(), value.to_string())));

	assert!(list.select_item("two"));
	assert_eq!(list.get_selected_item().map(|item| item.id.as_str()), Some("two"));
	assert!(toggled.borrow().is_empty());

	list.handle_input(&Key::Enter);
	assert_eq!(toggled.borrow().as_slice(), [("two".to_string(), "off".to_string())]);
	let rendered = strip_ansi(&list.render(80).join("\n"));
	assert!(rendered.contains("off"), "rows: {rendered:?}");
}

#[test]
#[ignore = "host Bun timers/TSP surface: loader repaint scheduling"]
fn schedules_no_loader_repaint_timer_while_a_tsp_surface_renders() {}

#[test]
#[ignore = "host Bun timers/TSP surface: loader timer chain"]
fn stops_a_running_loaders_timer_chain_once_a_tsp_surface_opens() {}

#[test]
#[ignore = "host Bun timers: countdown tick/expiry"]
fn counts_down_natively_without_a_per_second_tick() {}

#[test]
#[ignore = "host Bun timers/TSP surface: magic-keyword shimmer scheduling"]
fn schedules_no_magic_keyword_shimmer_frame_under_tsp() {}

#[test]
#[ignore = "no vim mode in this crate's Editor (documented out of scope)"]
fn leaves_vim_insert_unlabelled_so_the_host_keeps_native_editing() {}