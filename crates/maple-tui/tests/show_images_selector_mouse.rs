//! Port of `l/test/show-images-selector-mouse.test.ts`.
//!
//! The wrapper mounts a single-line top border before its [`SelectList`], so
//! routed component-local lines are offset by one. Regression guard for the
//! off-by-one that would let a top-border click select the first row.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::mouse::{MouseButtonKind, MouseKind, MouseEvent};
use maple_tui::overlays::show_images_selector::ShowImagesSelectorComponent;

fn left_click(row: u16) -> MouseEvent {
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
fn ignores_a_click_on_the_top_border_row() {
	let selected: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
	let sink = Rc::clone(&selected);
	let mut component = ShowImagesSelectorComponent::new(true, move |value| *sink.borrow_mut() = Some(value), || {});
	component.render(80);

	component.route_mouse(&left_click(0), 0, 0);

	assert!(selected.borrow().is_none(), "border row must be inert");
}

#[test]
fn selects_the_first_item_when_the_row_below_the_border_is_clicked() {
	let selected: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
	let sink = Rc::clone(&selected);
	let mut component = ShowImagesSelectorComponent::new(true, move |value| *sink.borrow_mut() = Some(value), || {});
	component.render(80);

	component.route_mouse(&left_click(1), 1, 0);

	// First SelectList row is "Yes" → true.
	assert_eq!(*selected.borrow(), Some(true));
}
