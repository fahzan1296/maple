//! Port of `l/test/menu-window-nearest.test.ts`.

use maple_tui::components::menu_selection::{
	get_menu_window, MenuAlignment, MenuWindowOptions,
};

fn nearest(current_start_index: usize) -> MenuWindowOptions {
	MenuWindowOptions {
		alignment: MenuAlignment::Nearest,
		current_start_index: Some(current_start_index),
	}
}

fn centered() -> MenuWindowOptions {
	MenuWindowOptions {
		alignment: MenuAlignment::Center,
		current_start_index: None,
	}
}

#[test]
fn defaults_to_centered_windowing_when_alignment_is_unspecified_or_center() {
	let row_counts = [3; 10];
	let centered_default = get_menu_window(&row_counts, 6, 15, None);
	let centered_explicit = get_menu_window(&row_counts, 6, 15, Some(&centered()));
	let nearest = get_menu_window(&row_counts, 6, 15, Some(&nearest(0)));

	assert_eq!(
		centered_default,
		centered_explicit,
		"default and explicit center must agree"
	);
	assert_eq!(centered_default.start_index, 4);
	assert_eq!(centered_default.end_index, 9);
	assert_eq!(centered_default.row_offset, 12);
	assert_eq!(centered_default.total_rows, 30);
	assert_eq!(nearest.start_index, 2);
	assert_eq!(nearest.end_index, 7);
	assert_eq!(nearest.row_offset, 6);
	assert_eq!(nearest.total_rows, 30);
}

#[test]
fn nearest_does_not_scroll_while_cursor_stays_in_viewport() {
	let row_counts = [3; 10];
	let mut current_start = 0;

	for selected in 0..=4 {
		let win = get_menu_window(&row_counts, selected, 15, Some(&nearest(current_start)));
		assert_eq!(win.start_index, 0);
		assert_eq!(win.end_index, 5);
		current_start = win.start_index;
	}
}

#[test]
fn nearest_scrolls_one_item_at_a_time_past_the_bottom_edge() {
	let row_counts = [3; 10];
	let mut current_start = 0;

	let win5 = get_menu_window(&row_counts, 5, 15, Some(&nearest(current_start)));
	assert_eq!(win5.start_index, 1);
	assert_eq!(win5.end_index, 6);
	current_start = win5.start_index;

	let win6 = get_menu_window(&row_counts, 6, 15, Some(&nearest(current_start)));
	assert_eq!(win6.start_index, 2);
	assert_eq!(win6.end_index, 7);
}

#[test]
fn nearest_keeps_viewport_stationary_moving_up_until_the_top_edge() {
	let row_counts = [3; 10];
	let mut current_start = 2;

	for selected in (2..=6).rev() {
		let win = get_menu_window(&row_counts, selected, 15, Some(&nearest(current_start)));
		assert_eq!(win.start_index, 2);
		assert_eq!(win.end_index, 7);
		current_start = win.start_index;
	}

	let win1 = get_menu_window(&row_counts, 1, 15, Some(&nearest(current_start)));
	assert_eq!(win1.start_index, 1);
	assert_eq!(win1.end_index, 6);
	current_start = win1.start_index;

	let win0 = get_menu_window(&row_counts, 0, 15, Some(&nearest(current_start)));
	assert_eq!(win0.start_index, 0);
	assert_eq!(win0.end_index, 5);
}

#[test]
fn nearest_handles_variable_row_heights() {
	let row_counts = [3, 4, 3, 4, 3, 4, 3, 4];
	let budget = 14;

	let win3 = get_menu_window(&row_counts, 3, budget, Some(&nearest(0)));
	assert_eq!(win3.start_index, 0);
	assert_eq!(win3.end_index, 4);

	let win4 = get_menu_window(&row_counts, 4, budget, Some(&nearest(0)));
	assert_eq!(win4.start_index, 1);
	assert_eq!(win4.end_index, 5);
}

#[test]
fn nearest_retains_an_oversized_item_taller_than_the_budget() {
	let row_counts = [3, 20, 3];
	let win = get_menu_window(&row_counts, 1, 10, Some(&nearest(0)));
	assert_eq!(win.start_index, 1);
	assert_eq!(win.end_index, 2);
}