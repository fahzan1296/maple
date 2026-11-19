//! Theme selector overlay.
//!
//! Reference: `l/src/overlays/theme-selector.ts`.
//!
//! Deviations:
//! - No process-global theme registry; themes and the initial selection are
//!   provided by the host.
//! - Preview/selection callbacks receive theme ids as strings.
//! - Native picker description is built with `select_list_picker`/`docked_picker`.

use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;

use crate::chrome::overlay_box::{bottom_border, row, top_border, OverlayTheme};
use crate::chrome::select_list_mouse_routing::route_select_list_mouse_with_top_border;
use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::keys::Key;
use crate::mouse::MouseEvent;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::native::picker::{
	docked_picker, route_select_list_picker, select_list_picker, PickerList, PickerUiEvent,
	PickerView, SelectPickerOptions, PICKER_KEY,
};

/// Picker options for a theme list (selector and settings submenu).
#[must_use]
pub fn theme_picker_options(title: &str, current: &str) -> SelectPickerOptions {
	let current = current.to_string();
	SelectPickerOptions {
		title: title.to_string(),
		subtitle: None,
		icon: Some("ink".to_string()),
		noun: Some("themes".to_string()),
		searchable: true,
		decorate: Some(Arc::new(|item: &SelectItem| {
			let mut props = Props::new();
			let mut mark = serde_json::Map::new();
			let text: String = item.key.chars().take(2).collect();
			mark.insert("text".to_string(), serde_json::Value::String(text));
			mark.insert("seed".to_string(), serde_json::Value::String(item.key.clone()));
			props.insert("mark", serde_json::Value::Object(mark));
			props.set_str("detail", "");
			Some(props)
		})),
		columns: None,
		current: Some(vec![current]),
		confirm: Some("Apply".to_string()),
	}
}

struct SheetList {
	inner: RefCell<SelectList>,
}

impl SheetList {
	fn new(list: SelectList) -> Self {
		Self {
			inner: RefCell::new(list),
		}
	}

	fn borrow(&self) -> Ref<'_, SelectList> {
		self.inner.borrow()
	}

	fn borrow_mut(&self) -> RefMut<'_, SelectList> {
		self.inner.borrow_mut()
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
		self.inner.borrow_mut().handle_input(&Key::Esc);
	}
}

/// Theme selector panel.
pub struct ThemeSelectorComponent {
	title: String,
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for ThemeSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ThemeSelectorComponent")
			.field("title", &self.title)
			.finish()
	}
}

impl ThemeSelectorComponent {
	/// Create a selector preloaded with `themes`.
	pub fn new<F, C, P>(
		current_theme: &str,
		themes: &[String],
		on_select: F,
		on_cancel: C,
		on_preview: P,
	) -> Self
	where
		F: FnMut(String) + 'static,
		C: FnMut() + 'static,
		P: FnMut(String) + 'static,
	{
		let items: Vec<SelectItem> = themes
			.iter()
			.map(|name| {
				let mut item = SelectItem::new(name.clone(), name.clone());
				if name == current_theme {
					item = item.with_description("(current)".to_string());
				}
				item
			})
			.collect();
		let mut list = SelectList::new(items.clone(), 10, SelectListTheme::default());
		if let Some(index) = themes.iter().position(|name| name == current_theme) {
			list.set_selected_index(index as isize);
		}
		let on_select = std::rc::Rc::new(RefCell::new(on_select));
		let on_cancel = std::rc::Rc::new(RefCell::new(on_cancel));
		let on_preview = std::rc::Rc::new(RefCell::new(on_preview));
		{
			let sink = std::rc::Rc::clone(&on_select);
			list.set_on_select(Some(Box::new(move |item: &SelectItem| {
				(sink.borrow_mut())(item.key.clone());
			})));
		}
		{
			let sink = std::rc::Rc::clone(&on_cancel);
			list.set_on_cancel(Some(Box::new(move || {
				(sink.borrow_mut())();
			})));
		}
		{
			let sink = std::rc::Rc::clone(&on_preview);
			list.set_on_selection_change(Some(Box::new(move |item: &SelectItem| {
				(sink.borrow_mut())(item.key.clone());
			})));
		}
		Self {
			title: "Theme".to_string(),
			list: SheetList::new(list),
			options: theme_picker_options("Theme", current_theme),
			theme: OverlayTheme::default(),
		}
	}

	/// The hosted selection list.
	#[must_use]
	pub fn select_list(&self) -> Ref<'_, SelectList> {
		self.list.borrow()
	}

	/// Route a mouse event through the list body below the top border.
	pub fn route_mouse(&mut self, event: &MouseEvent, line: u16, col: u16) {
		route_select_list_mouse_with_top_border(&mut self.list.borrow_mut(), event, line, col);
	}

	/// Route a native picker event onto the list.
	pub fn handle_picker_event(&mut self, event: &PickerUiEvent) -> bool {
		route_select_list_picker(&mut self.list, event, PICKER_KEY)
	}

	fn rows(&self, width: u16) -> Vec<String> {
		let inner = width.saturating_sub(4).max(1);
		let list_rows = self.list.borrow_mut().render(inner);
		let mut rows = vec![top_border(width, &self.title, &self.theme)];
		for line in list_rows {
			rows.push(row(&line, width, &self.theme));
		}
		rows.push(bottom_border(width, &self.theme));
		rows
	}
}

impl Component for ThemeSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.list.borrow_mut().handle_input(key);
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		self.route_mouse(event, event.row, event.column);
	}
}

impl NativeDescribe for ThemeSelectorComponent {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		if cx.feature("picker") {
			let props = select_list_picker(&self.list, &self.options).to_props();
			return docked_picker(props, None);
		}
		let cols = cx.cols.max(1);
		let lines: Vec<String> = self
			.rows(cols)
			.into_iter()
			.map(|line| line.replace(crate::native::describe::CURSOR_MARKER, ""))
			.collect();
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cols));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	fn themes() -> Vec<String> {
		vec!["light".into(), "dark".into(), "forest".into()]
	}

	#[test]
	fn preselects_current_theme() {
		let selector = ThemeSelectorComponent::new("dark", &themes(), |_| {}, || {}, |_| {});
		assert_eq!(selector.select_list().selected_key().as_deref(), Some("dark"));
	}

	#[test]
	fn selection_change_previews() {
		let previews = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&previews);
		let mut selector = ThemeSelectorComponent::new(
			"light",
			&themes(),
			|_| {},
			|| {},
			move |name| sink.borrow_mut().push(name),
		);
		selector.handle_input(&Key::Down);
		assert_eq!(&*previews.borrow(), &["dark".to_string()]);
	}

	#[test]
	fn enter_applies_selected_theme() {
		let applied = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&applied);
		let mut selector = ThemeSelectorComponent::new(
			"light",
			&themes(),
			move |name| sink.borrow_mut().push(name),
			|| {},
			|_| {},
		);
		selector.handle_input(&Key::Down);
		selector.handle_input(&Key::Enter);
		assert_eq!(&*applied.borrow(), &["dark".to_string()]);
	}

	#[test]
	fn escape_cancels() {
		let cancelled = Rc::new(RefCell::new(0usize));
		let sink = Rc::clone(&cancelled);
		let mut selector = ThemeSelectorComponent::new("light", &themes(), |_| {}, move || {
			*sink.borrow_mut() += 1;
		}, |_| {});
		selector.handle_input(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 1);
	}

	#[test]
	fn picker_options_mark_current_and_apply() {
		let options = theme_picker_options("Theme", "dark");
		assert_eq!(options.title, "Theme");
		assert_eq!(options.confirm.as_deref(), Some("Apply"));
		assert!(options.searchable);
		assert_eq!(options.current, Some(vec!["dark".to_string()]));
		assert!(options.decorate.is_some());
	}

	#[test]
	fn picker_description_is_docked_when_supported() {
		let selector = ThemeSelectorComponent::new("dark", &themes(), |_| {}, || {}, |_| {});
		let mut cx = DescribeContext::new(40);
		cx.enable_feature("picker");
		let described = selector.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.children[0].kind, NodeKind::Picker);
	}
}
