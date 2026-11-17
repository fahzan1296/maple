//! Route overlay-local mouse input past a panel's top border into its
//! [`SelectList`] child.
//!
//! Reference: `l/src/chrome/select-list-mouse-routing.ts`, with the shared
//! routing pattern from `l/src/mouse.ts` (`routeSelectListMouse`).
//!
//! Deviations:
//! - The reference's `SgrMouseEvent` carries `wheel`/`motion`/`leftClick`
//!   flags; the Rust [`MouseEvent`] folds those into [`MouseKind`], so the
//!   flags are derived from `kind`.
//! - `SelectList::hit_test` returns the item rather than its index, so the
//!   index is recovered by identity within `visible_items`.

use crate::components::select_list::SelectList;
use crate::mouse::{MouseButtonKind, MouseKind, MouseEvent};

/// Route overlay-local mouse input past a panel's top border into its
/// [`SelectList`] child. `line`/`col` are overlay-local; the child starts one
/// row lower.
pub fn route_select_list_mouse_with_top_border(
	select_list: &mut SelectList,
	event: &MouseEvent,
	line: u16,
	_col: u16,
) {
	route_select_list_mouse(select_list, event, line as i64 - 1);
}

/// Shared routing: wheel scrolls, pointer motion hovers, a left click selects
/// and activates. Returns true when the event was consumed.
fn route_select_list_mouse(select_list: &mut SelectList, event: &MouseEvent, line: i64) -> bool {
	let index = if line >= 0 {
		select_list.hit_test(line as usize).and_then(|item| {
			select_list
				.visible_items()
				.iter()
				.position(|candidate| std::ptr::eq(candidate, item))
		})
	} else {
		None
	};

	match event.kind {
		MouseKind::ScrollUp => {
			select_list.handle_wheel(-1);
			true
		}
		MouseKind::ScrollDown => {
			select_list.handle_wheel(1);
			true
		}
		MouseKind::Moved | MouseKind::Drag(_) => {
			select_list.set_hovered_index(index);
			true
		}
		MouseKind::Down(MouseButtonKind::Left) => {
			if let Some(index) = index {
				select_list.click_item(index);
				return true;
			}
			false
		}
		_ => false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::component::Component;
	use crate::components::select_list::{SelectItem, SelectListTheme};
	use std::cell::Cell;
	use std::rc::Rc;

	fn list(count: usize) -> SelectList {
		let items: Vec<SelectItem> = (0..count)
			.map(|index| SelectItem::new(format!("k{index}"), format!("item {index}")))
			.collect();
		SelectList::new(items, 8, SelectListTheme::default())
	}

	fn event(kind: MouseKind, row: u16) -> MouseEvent {
		MouseEvent {
			kind,
			column: 0,
			row,
			shift: false,
			ctrl: false,
			alt: false,
		}
	}

	#[test]
	fn wheel_scrolls_the_selection_from_the_top_border() {
		let mut list = list(3);
		list.render(20);
		route_select_list_mouse_with_top_border(
			&mut list,
			&event(MouseKind::ScrollDown, 0),
			1,
			0,
		);
		assert_eq!(list.selected_index(), 1);
		route_select_list_mouse_with_top_border(&mut list, &event(MouseKind::ScrollUp, 0), 1, 0);
		assert_eq!(list.selected_index(), 0);
	}

	#[test]
	fn motion_highlights_the_row_under_the_pointer() {
		let mut list = list(3);
		list.render(20);
		route_select_list_mouse_with_top_border(&mut list, &event(MouseKind::Moved, 2), 2, 0);
		assert_eq!(list.hovered_index(), Some(1));
		route_select_list_mouse_with_top_border(&mut list, &event(MouseKind::Moved, 9), 9, 0);
		assert_eq!(list.hovered_index(), None);
	}

	#[test]
	fn left_click_selects_and_activates_the_hit_row() {
		let activated = Rc::new(Cell::new(0usize));
		let counter = Rc::clone(&activated);
		let mut list = list(3);
		list.on_select(move |item| {
			counter.set(item.key.parse().unwrap_or(0));
		});
		list.render(20);
		route_select_list_mouse_with_top_border(
			&mut list,
			&event(MouseKind::Down(MouseButtonKind::Left), 1),
			1,
			0,
		);
		assert_eq!(list.selected_index(), 0);
		assert_eq!(activated.get(), 0);
	}

	#[test]
	fn a_click_below_the_last_row_is_ignored() {
		let mut list = list(2);
		list.render(20);
		route_select_list_mouse_with_top_border(
			&mut list,
			&event(MouseKind::Down(MouseButtonKind::Left), 1),
			20,
			0,
		);
		assert_eq!(list.selected_index(), 0);
	}

	#[test]
	fn non_left_buttons_do_nothing() {
		let mut list = list(3);
		list.render(20);
		route_select_list_mouse_with_top_border(
			&mut list,
			&event(MouseKind::Down(MouseButtonKind::Right), 0),
			1,
			0,
		);
		assert_eq!(list.selected_index(), 0);
		assert_eq!(list.hovered_index(), None);
	}
}
