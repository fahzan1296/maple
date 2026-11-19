//! Thinking-level selector overlay.
//!
//! Reference: `l/src/overlays/thinking-selector.ts`.
//!
//! Deviations:
//! - No process-global theme/keybindings or native `SelectListSheet` event
//!   router; picker description uses `select_list_picker`/`docked_picker` and
//!   mouse/key routing is handled by the hosted `SelectList`.
//! - Thinking effort metadata is injected as [`ThinkingLevelOption`] values so
//!   this overlay does not depend on catalog crates.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
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
	docked_picker, route_select_list_picker, select_list_picker, PickerColumn, PickerList,
	PickerUiEvent, PickerView, SelectPickerOptions, PICKER_KEY,
};

/// Synthetic auto level id.
pub const AUTO_THINKING: &str = "auto";

/// One selectable thinking level.
#[derive(Clone, Debug, PartialEq)]
pub struct ThinkingLevelOption {
	/// Configured level id.
	pub value: String,
	/// Display label.
	pub label: String,
	/// Optional detail.
	pub description: Option<String>,
	/// Depth meter in `[0.0, 1.0]`; `None` for auto.
	pub depth: Option<f32>,
}

/// Build select-list items from the configured levels.
#[must_use]
pub fn build_thinking_items(available: &[ThinkingLevelOption]) -> Vec<SelectItem> {
	available
		.iter()
		.map(|level| {
			let mut item = SelectItem::new(level.value.clone(), level.label.clone());
			if let Some(description) = &level.description {
				item = item.with_description(description.clone());
			}
			item
		})
		.collect()
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

/// Thinking level selector.
pub struct ThinkingSelectorComponent {
	title: String,
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for ThinkingSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ThinkingSelectorComponent")
			.field("title", &self.title)
			.finish()
	}
}

impl ThinkingSelectorComponent {
	/// Create a selector over `available_levels`, preselecting `current_level`.
	pub fn new<F, C>(
		current_level: Option<&str>,
		available_levels: Vec<ThinkingLevelOption>,
		on_select: F,
		on_cancel: C,
	) -> Self
	where
		F: FnMut(String) + 'static,
		C: FnMut() + 'static,
	{
		let items = build_thinking_items(&available_levels);
		let mut list = SelectList::new(items.clone(), items.len().max(1), SelectListTheme::default());
		if let Some(current) = current_level {
			if let Some(index) = items.iter().position(|item| item.key == current) {
				list.set_selected_index(index as isize);
			}
		}
		let on_select = std::rc::Rc::new(RefCell::new(on_select));
		let on_cancel = std::rc::Rc::new(RefCell::new(on_cancel));
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
		let depths: HashMap<String, f32> = available_levels
			.into_iter()
			.filter_map(|level| level.depth.map(|depth| (level.value, depth)))
			.collect();
		let options = SelectPickerOptions {
			title: "Thinking level".to_string(),
			subtitle: None,
			icon: Some("brain".to_string()),
			noun: Some("levels".to_string()),
			searchable: false,
			decorate: Some(Arc::new(move |item: &SelectItem| {
				depths.get(&item.key).map(|depth| {
					let mut props = Props::new();
					let mut facts = serde_json::Map::new();
					facts.insert(
						"depth".to_string(),
						serde_json::Value::Number(
							serde_json::Number::from_f64(f64::from(*depth))
								.unwrap_or_else(|| serde_json::Number::from(0)),
						),
					);
					props.insert("facts", serde_json::Value::Object(facts));
					props
				})
			})),
			columns: Some(vec![PickerColumn {
				id: "depth".to_string(),
				head: None,
				align: None,
				grow: None,
				priority: None,
				format: Some("bar".to_string()),
			}]),
			current: current_level.map(|level| vec![level.to_string()]),
			confirm: None,
		};
		Self {
			title: "Thinking Level".to_string(),
			list: SheetList::new(list),
			options,
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

impl Component for ThinkingSelectorComponent {
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

impl NativeDescribe for ThinkingSelectorComponent {
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

	fn levels() -> Vec<ThinkingLevelOption> {
		vec![
			ThinkingLevelOption {
				value: AUTO_THINKING.to_string(),
				label: "auto".to_string(),
				description: Some("automatic".to_string()),
				depth: None,
			},
			ThinkingLevelOption {
				value: "low".to_string(),
				label: "low".to_string(),
				description: None,
				depth: Some(0.25),
			},
			ThinkingLevelOption {
				value: "high".to_string(),
				label: "high".to_string(),
				description: None,
				depth: Some(1.0),
			},
		]
	}

	#[test]
	fn preselects_current_level() {
		let selector = ThinkingSelectorComponent::new(Some("high"), levels(), |_| {}, || {});
		assert_eq!(selector.select_list().selected_key().as_deref(), Some("high"));
	}

	#[test]
	fn enter_reports_selected_level() {
		let calls = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&calls);
		let mut selector = ThinkingSelectorComponent::new(
			Some(AUTO_THINKING),
			levels(),
			move |level| sink.borrow_mut().push(level),
			|| {},
		);
		selector.handle_input(&Key::Down);
		selector.handle_input(&Key::Enter);
		assert_eq!(&*calls.borrow(), &["low".to_string()]);
	}

	#[test]
	fn escape_cancels() {
		let cancelled = Rc::new(RefCell::new(0usize));
		let sink = Rc::clone(&cancelled);
		let mut selector = ThinkingSelectorComponent::new(Some(AUTO_THINKING), levels(), |_| {}, move || {
			*sink.borrow_mut() += 1;
		});
		selector.handle_input(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 1);
	}

	#[test]
	fn picker_description_is_docked_when_supported() {
		let selector = ThinkingSelectorComponent::new(Some("low"), levels(), |_| {}, || {});
		let mut cx = DescribeContext::new(40);
		cx.enable_feature("picker");
		let described = selector.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.children[0].kind, NodeKind::Picker);
	}

	#[test]
	fn fallback_description_is_rows() {
		let selector = ThinkingSelectorComponent::new(Some("low"), levels(), |_| {}, || {});
		let described = selector.describe_native(&DescribeContext::new(40));
		assert_eq!(described.kind, NodeKind::Rows);
	}

	#[test]
	fn build_items_preserve_values_and_descriptions() {
		let items = build_thinking_items(&levels());
		assert_eq!(items[0].key, AUTO_THINKING);
		assert_eq!(items[0].description.as_deref(), Some("automatic"));
		assert_eq!(items[2].key, "high");
	}
}
