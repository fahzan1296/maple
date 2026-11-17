//! Reference: `l/src/overlays/show-images-selector.ts`.
//!
//! A two-row Yes/No picker for inline images.
//!
//! Deviations: matches [`crate::overlays::session_account_selector`] — the
//! panel chrome is drawn here instead of through `OverlayPanel`, and the
//! picker sheet is described from [`SelectPickerOptions`].

use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;

use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::chrome::select_list_mouse_routing::route_select_list_mouse_with_top_border;
use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::keys::Key;
use crate::mouse::MouseEvent;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{NativeDescribe, item, list};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::native::overlay::overlay_card;
use crate::native::picker::{
	PickerList, PickerView, SelectPickerOptions, docked_picker, route_select_list_picker,
	select_list_picker,
};

/// Semantic role of the picker panel.
pub const SHOW_IMAGES_ROLE: &str = "omp.overlay.show-images";

/// Most rows the picker shows at once.
pub const SHOW_IMAGES_MAX_VISIBLE: usize = 5;

struct SheetList {
	inner: RefCell<SelectList>,
}

impl SheetList {
	fn new(list: SelectList) -> Self {
		SheetList {
			inner: RefCell::new(list),
		}
	}
}

impl PickerList for SheetList {
	fn picker_view(&self) -> PickerView {
		let list = self.inner.borrow();
		PickerView {
			items: list.visible_items().to_vec(),
			selected: list.selected_key(),
			pending: None,
			query: list.filter().to_string(),
			cursor: list.filter().encode_utf16().count(),
		}
	}

	fn select_index(&mut self, index: usize) {
		self.inner.borrow_mut().set_selected_index(index as isize);
	}

	fn activate_index(&mut self, index: usize) {
		let mut list = self.inner.borrow_mut();
		list.set_selected_index(index as isize);
		list.click_item(index);
	}

	fn cancel(&mut self) {
		let mut list = self.inner.borrow_mut();
		list.handle_input(&Key::Esc);
	}
}

/// The show-images selector panel.
pub struct ShowImagesSelectorComponent {
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for ShowImagesSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ShowImagesSelectorComponent").finish()
	}
}

impl ShowImagesSelectorComponent {
	/// Build the selector, resting on `current_value`.
	pub fn new(
		current_value: bool,
		on_select: impl FnMut(bool) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		let items = vec![
			SelectItem::new("yes", "Yes").with_description("Show images inline in terminal"),
			SelectItem::new("no", "No").with_description("Show text placeholder instead"),
		];
		let mut list = SelectList::new(items, SHOW_IMAGES_MAX_VISIBLE, SelectListTheme::default());
		list.set_selected_index(if current_value { 0 } else { 1 });
		let mut on_select = on_select;
		list.on_select(move |item| on_select(item.key == "yes"));
		let mut on_cancel = on_cancel;
		list.on_cancel(move || on_cancel());
		let options = SelectPickerOptions {
			title: "Show images".to_string(),
			icon: Some("image".to_string()),
			noun: Some("options".to_string()),
			current: Some(vec![if current_value { "yes" } else { "no" }.to_string()]),
			..SelectPickerOptions::default()
		};
		ShowImagesSelectorComponent {
			list: SheetList::new(list),
			options,
			theme: OverlayTheme::default(),
		}
	}

	/// The panel title.
	pub fn title(&self) -> &'static str {
		"Show Images"
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &'static str {
		SHOW_IMAGES_ROLE
	}

	/// The picker options, for hosts that build the sheet themselves.
	pub fn picker_options(&self) -> &SelectPickerOptions {
		&self.options
	}

	/// The terminal list, for tests and hosts that drive it directly.
	pub fn select_list(&self) -> Ref<'_, SelectList> {
		self.list.inner.borrow()
	}

	/// The terminal list, mutably.
	pub fn select_list_mut(&self) -> RefMut<'_, SelectList> {
		self.list.inner.borrow_mut()
	}

	/// Route mouse selection through the title rows into the list.
	pub fn route_mouse(&mut self, event: &MouseEvent, line: u16, col: u16) {
		route_select_list_mouse_with_top_border(&mut self.list.inner.borrow_mut(), event, line, col);
	}

	/// Picker pointer events drive the list exactly as its keys do.
	pub fn handle_native_event(&mut self, event: &crate::native::picker::PickerUiEvent) {
		route_select_list_picker(&mut self.list, event, "");
	}
}

impl Component for ShowImagesSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let theme = &self.theme;
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let mut rows = vec![
			top_border(width, "Show Images", theme),
			row("", width, theme),
		];
		for line in self.list.inner.borrow_mut().render(inner) {
			rows.push(row(&line, width, theme));
		}
		rows.push(row("", width, theme));
		rows.push(row("enter select · esc cancel", width, theme));
		rows.push(bottom_border(width, theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		self.list.inner.borrow_mut().handle_input(key);
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		self.route_mouse(event, event.row, event.column);
	}
}

impl NativeDescribe for ShowImagesSelectorComponent {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		if cx.feature("picker") {
			let props = select_list_picker(&self.list, &self.options).to_props();
			return docked_picker(props, None);
		}
		let items: Vec<NativeNode> = self
			.list
			.inner
			.borrow()
			.visible_items()
			.iter()
			.map(|item_value| {
				let mut props = Props::new();
				props.set_str("label", item_value.title.clone());
				if let Some(description) = &item_value.description {
					props.set_str("detail", description.as_str());
				}
				item(item_value.key.clone(), props)
			})
			.collect();
		let mut list_props = Props::new();
		list_props.insert(
			"selected",
			serde_json::json!(self.list.inner.borrow().selected_key()),
		);
		overlay_card(
			SHOW_IMAGES_ROLE,
			Some(crate::native::node::Text::plain("Show Images")),
			vec![list(items, list_props, Some("options"))],
			None,
		)
	}
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;
	use std::rc::Rc;

	use super::*;

	#[test]
	fn rests_on_current_value_and_reports_options() {
		let picked = Rc::new(RefCell::new(None));
		let sel_picked = Rc::clone(&picked);
		let sel = ShowImagesSelectorComponent::new(true, move |v| *sel_picked.borrow_mut() = Some(v), || {});
		assert_eq!(sel.title(), "Show Images");
		assert_eq!(sel.native_role(), SHOW_IMAGES_ROLE);
		assert_eq!(sel.select_list().selected_key().as_deref(), Some("yes"));
		assert_eq!(sel.picker_options().current, Some(vec!["yes".to_string()]));
		assert!(picked.borrow().is_none());
	}

	#[test]
	fn selecting_no_reports_false() {
		let picked = Rc::new(RefCell::new(None));
		let sel_picked = Rc::clone(&picked);
		let mut sel = ShowImagesSelectorComponent::new(false, move |v| *sel_picked.borrow_mut() = Some(v), || {});
		assert_eq!(sel.select_list().selected_key().as_deref(), Some("no"));
		sel.select_list_mut().click_item(1);
		assert_eq!(*picked.borrow(), Some(false));
		assert_eq!(sel.picker_options().current, Some(vec!["no".to_string()]));
	}

	#[test]
	fn picker_events_drive_the_list() {
		let picked = Rc::new(RefCell::new(None));
		let sel_picked = Rc::clone(&picked);
		let mut sel = ShowImagesSelectorComponent::new(false, move |v| *sel_picked.borrow_mut() = Some(v), || {});
		sel.handle_native_event(&crate::native::picker::PickerUiEvent::Activate {
			key: String::new(),
			item: "yes".to_string(),
		});
		assert_eq!(sel.select_list().selected_key().as_deref(), Some("yes"));
		assert_eq!(*picked.borrow(), Some(true));
	}
}

/// Unused import guard: the picker `Arc` is part of `SelectPickerOptions`.
#[allow(dead_code)]
type UnusedArc = Arc<()>;
