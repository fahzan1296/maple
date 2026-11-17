//! Port of `l/test/mouse.test.ts`.
//!
//! The crate exposes the same SGR report model under
//! `setup::scenes::types`. The reference's `routeSelectListMouse` target
//! (handleWheel/hitTest/setHoverIndex/clickItem) has no crate equivalent — the
//! crate routes directly against `SelectList` — so those five cases are
//! ignored with that missing API named.

use std::cell::Cell;

use maple_tui::setup::scenes::types::{parse_sgr_mouse, route_sgr_mouse_input};

#[test]
fn returns_none_for_non_mouse_input() {
	assert!(parse_sgr_mouse("a").is_none());
	assert!(parse_sgr_mouse("\x1b[A").is_none());
	assert!(parse_sgr_mouse("\x1b[<bogus").is_none());
}

#[test]
fn decodes_left_clicks_with_zero_based_coordinates() {
	let event = parse_sgr_mouse("\x1b[<0;5;9M").expect("click");
	assert_eq!(event.button, 0);
	assert_eq!(event.col, 4);
	assert_eq!(event.row, 8);
	assert!(!event.release);
	assert_eq!(event.wheel, None);
	assert!(!event.motion);
	assert!(event.left_click);
}

#[test]
fn decodes_releases_as_non_clicks() {
	let event = parse_sgr_mouse("\x1b[<0;5;9m").expect("release");
	assert!(event.release);
	assert!(!event.left_click);
}

#[test]
fn decodes_wheel_direction_from_the_low_button_bit() {
	assert_eq!(parse_sgr_mouse("\x1b[<64;1;1M").unwrap().wheel, Some(-1));
	assert_eq!(parse_sgr_mouse("\x1b[<65;1;1M").unwrap().wheel, Some(1));
	assert!(!parse_sgr_mouse("\x1b[<65;1;1M").unwrap().left_click);
}

#[test]
fn does_not_read_a_horizontal_wheel_report_as_a_vertical_direction() {
	for report in ["\x1b[<66;1;1M", "\x1b[<67;1;1M", "\x1b[<70;1;1M"] {
		let event = parse_sgr_mouse(report).expect("report");
		assert_eq!(event.wheel, None);
		assert!(!event.left_click);
		assert!(!event.motion);
	}
	assert_eq!(parse_sgr_mouse("\x1b[<69;1;1M").unwrap().wheel, Some(1));
}

#[test]
fn decodes_motion_reports_without_treating_them_as_clicks() {
	let event = parse_sgr_mouse("\x1b[<35;10;3M").expect("motion");
	assert!(event.motion);
	assert!(!event.left_click);
	assert_eq!(event.wheel, None);
}

#[test]
fn route_returns_false_and_does_not_call_the_handler_for_non_mouse_input() {
	let called = Cell::new(false);
	let handled = route_sgr_mouse_input("a", &mut |_event| {
		called.set(true);
	});
	assert!(!handled);
	assert!(!called.get());
}

#[test]
fn route_decodes_and_forwards_an_sgr_mouse_report() {
	let received: Cell<Option<(i32, i32, bool)>> = Cell::new(None);
	let handled = route_sgr_mouse_input("\x1b[<0;2;3M", &mut |event| {
		received.set(Some((event.col, event.row, event.left_click)));
	});
	assert!(handled);
	let (col, row, left_click) = received.get().expect("forwarded event");
	assert_eq!(row, 2);
	assert_eq!(col, 1);
	assert!(left_click);
}

#[test]
#[ignore = "missing API: route_select_list_mouse(target) SelectListMouseTarget"]
fn route_select_list_mouse_forwards_wheel_notches() {}

#[test]
#[ignore = "missing API: route_select_list_mouse(target) SelectListMouseTarget"]
fn route_select_list_mouse_hovers_the_hit_tested_row_on_motion() {}

#[test]
#[ignore = "missing API: route_select_list_mouse(target) SelectListMouseTarget"]
fn route_select_list_mouse_clears_hover_when_motion_misses_a_row() {}

#[test]
#[ignore = "missing API: route_select_list_mouse(target) SelectListMouseTarget"]
fn route_select_list_mouse_clicks_the_hit_tested_row() {}

#[test]
#[ignore = "missing API: route_select_list_mouse(target) SelectListMouseTarget"]
fn route_select_list_mouse_ignores_release_events() {}
