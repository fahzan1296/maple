//! Queue-mode selector: the two delivery modes for messages sent while the
//! agent works, drawn as a bordered list and described as a native picker
//! sheet when the terminal supports one.
//!
//! Reference: `l/src/overlays/queue-mode-selector.ts`.
//!
//! Deviations:
//! - The reference reads the process-global theme through
//!   `getSelectListTheme()`; maple-tui has no global UI state, so the list uses
//!   [`SelectListTheme::default`].
//! - `cx.supports("picker")` is [`DescribeContext::feature`]`("picker")`.
//! - `handleNativeEvent` has no crate event router; picker events are routed
//!   through [`crate::native::picker::route_select_list_picker`] from
//!   [`QueueModeSelectorComponent::handle_picker_event`].
//! - The crate's `SelectList` has no `pickerView`, so [`SheetList`] adapts it to
//!   [`PickerList`] (the crate's `picker` module documents this host-side
//!   adaptation). The pending-confirmation key is not exposed by `SelectList`,
//!   so [`PickerView::pending`] is always `None`.
//! - The reference's user-facing subtitle names the host product; the copy is
//!   rebranded to maple.

use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;

use crate::chrome::overlay_box::{bottom_border, row, top_border, OverlayTheme};
use crate::chrome::select_list_mouse_routing::route_select_list_mouse_with_top_border;
use crate::component::Component;
use crate::keys::Key;
use crate::mouse::MouseEvent;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::native::picker::{
	docked_picker, route_select_list_picker, select_list_picker, PickerList, PickerUiEvent,
	PickerView, SelectPickerOptions,
};
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};

/// Deliver queued messages one by one.
pub const QUEUE_MODE_ONE_AT_A_TIME: &str = "one-at-a-time";
/// Deliver every queued message in one turn.
pub const QUEUE_MODE_ALL: &str = "all";

/// The queued-message delivery mode chosen in the queue-mode selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueMode {
	/// Process queued messages one by one (recommended).
	OneAtATime,
	/// Process all queued messages at once.
	All,
}

impl QueueMode {
	/// The value spelling of the mode.
	pub fn as_str(self) -> &'static str {
		match self {
			QueueMode::OneAtATime => QUEUE_MODE_ONE_AT_A_TIME,
			QueueMode::All => QUEUE_MODE_ALL,
		}
	}

	/// Parse a value spelling; `None` when it is not a queue mode.
	pub fn parse(value: &str) -> Option<QueueMode> {
		match value {
			QUEUE_MODE_ONE_AT_A_TIME => Some(QueueMode::OneAtATime),
			QUEUE_MODE_ALL => Some(QueueMode::All),
			_ => None,
		}
	}
}

/// A [`SelectList`] that also satisfies the picker's [`PickerList`] contract.
///
/// The crate's `SelectList` port does not expose `pickerView`, its caret, its
/// pending key, or its cancel/selection callbacks, so a host adapts it here.
struct SheetList {
	inner: RefCell<SelectList>,
}

impl SheetList {
	fn new(list: SelectList) -> Self {
		SheetList {
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
		let mut list = self.inner.borrow_mut();
		Component::handle_input(&mut *list, &Key::Esc);
	}
}

/// A queue-mode selector panel.
pub struct QueueModeSelectorComponent {
	title: String,
	role: String,
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for QueueModeSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("QueueModeSelectorComponent")
			.field("title", &self.title)
			.field("role", &self.role)
			.finish()
	}
}

impl QueueModeSelectorComponent {
	/// Build the selector with `current_mode` preselected.
	///
	/// `on_select` receives the chosen mode's value spelling; `on_cancel` runs
	/// when the selection is dismissed.
	pub fn new(
		current_mode: &str,
		mut on_select: impl FnMut(&str) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		let items = vec![
			SelectItem::new(QUEUE_MODE_ONE_AT_A_TIME, "one-at-a-time").with_description(
				"Process queued messages one by one (recommended)",
			),
			SelectItem::new(QUEUE_MODE_ALL, "all")
				.with_description("Process all queued messages at once"),
		];
		let mut list = SelectList::new(items, 2, SelectListTheme::default());
		if let Some(index) = list
			.items()
			.iter()
			.position(|item| item.key == current_mode)
		{
			list.set_selected_index(index as isize);
		}
		list.on_select(move |item| on_select(item.key.as_str()));
		list.on_cancel(on_cancel);
		let options = SelectPickerOptions {
			title: "Queue mode".to_string(),
			subtitle: Some("How messages you send while maple works are delivered".to_string()),
			icon: Some("list".to_string()),
			noun: Some("modes".to_string()),
			current: Some(vec![current_mode.to_string()]),
			decorate: Some(Arc::new(|item| {
				let mut props = Props::new();
				if item.key == QUEUE_MODE_ONE_AT_A_TIME {
					props.set_str("label", "One at a time");
					props.set_str("detail", "Each queued message gets its own turn");
					props.insert(
						"badges",
						serde_json::Value::Array(vec![serde_json::json!({
							"text": "recommended"
						})]),
					);
				} else {
					props.set_str("label", "All at once");
					props.set_str("detail", "Queued messages are sent together in one turn");
				}
				Some(props)
			})),
			..SelectPickerOptions::default()
		};
		QueueModeSelectorComponent {
			title: "Queue Mode".to_string(),
			role: "omp.overlay.queue-mode".to_string(),
			list: SheetList::new(list),
			options,
			theme: OverlayTheme::default(),
		}
	}

	/// The collapsed title shown in the top border.
	pub fn title(&self) -> &str {
		&self.title
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The terminal list, for hosts that drive it directly.
	pub fn select_list(&self) -> Ref<'_, SelectList> {
		self.list.borrow()
	}

	/// The terminal list, mutably.
	pub fn select_list_mut(&mut self) -> RefMut<'_, SelectList> {
		self.list.borrow_mut()
	}

	/// The picker options used for the native description.
	pub fn picker_options(&self) -> &SelectPickerOptions {
		&self.options
	}

	/// Replace the panel theme.
	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	/// The panel theme.
	pub fn theme(&self) -> &OverlayTheme {
		&self.theme
	}

	/// Route overlay-local pointer input past the top border into the list.
	pub fn route_mouse(&mut self, event: &MouseEvent, line: u16, col: u16) {
		route_select_list_mouse_with_top_border(&mut self.list.borrow_mut(), event, line, col);
	}

	/// Route a native picker event onto the list.
	pub fn handle_picker_event(&mut self, event: &PickerUiEvent) -> bool {
		route_select_list_picker(&mut self.list, event, "")
	}

	/// Rows for `width`, including the rounded border chrome.
	fn rows(&self, width: u16) -> Vec<String> {
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let list_rows = self.list.borrow_mut().render(inner);
		let mut rows = vec![top_border(width, &self.title, &self.theme)];
		for line in list_rows {
			rows.push(row(&line, width, &self.theme));
		}
		rows.push(bottom_border(width, &self.theme));
		rows
	}
}

impl Component for QueueModeSelectorComponent {
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

impl NativeDescribe for QueueModeSelectorComponent {
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
	use std::rc::Rc;
	use crate::mouse::{MouseButtonKind, MouseKind};

	fn value_of(item: &SelectItem) -> &'static str {
		match item.key.as_str() {
			QUEUE_MODE_ONE_AT_A_TIME => QUEUE_MODE_ONE_AT_A_TIME,
			_ => QUEUE_MODE_ALL,
		}
	}

	#[test]
	fn parses_and_spells_both_modes() {
		assert_eq!(QueueMode::parse("all"), Some(QueueMode::All));
		assert_eq!(
			QueueMode::parse("one-at-a-time"),
			Some(QueueMode::OneAtATime)
		);
		assert_eq!(QueueMode::parse("nope"), None);
		assert_eq!(QueueMode::All.as_str(), "all");
		assert_eq!(QueueMode::OneAtATime.as_str(), "one-at-a-time");
	}

	#[test]
	fn preselects_the_current_mode() {
		let selector = QueueModeSelectorComponent::new("all", |_| {}, || {});
		assert_eq!(selector.select_list().selected_key().as_deref(), Some("all"));
		let selector = QueueModeSelectorComponent::new("one-at-a-time", |_| {}, || {});
		assert_eq!(
			selector.select_list().selected_key().as_deref(),
			Some("one-at-a-time")
		);
	}

	#[test]
	fn unknown_current_mode_leaves_the_first_row_selected() {
		let selector = QueueModeSelectorComponent::new("missing", |_| {}, || {});
		assert_eq!(
			selector.select_list().selected_key().as_deref(),
			Some("one-at-a-time")
		);
	}

	#[test]
	fn rows_are_a_bordered_two_row_list() {
		let mut selector = QueueModeSelectorComponent::new("all", |_| {}, || {});
		let rows = selector.render(40);
		assert_eq!(rows.len(), 4);
		assert!(rows[0].contains("Queue Mode"), "{rows:?}");
		assert!(rows[3].contains('╚') || rows[3].contains('└'), "{rows:?}");
		assert!(rows[1].contains("one-at-a-time"), "{rows:?}");
		assert!(rows[2].contains("all"), "{rows:?}");
	}

	#[test]
	fn enter_reports_the_selected_mode_once() {
		let calls = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&calls);
		let mut selector = QueueModeSelectorComponent::new(
			"one-at-a-time",
			move |mode| sink.borrow_mut().push(mode),
			|| {},
		);
		selector.handle_input(&Key::Down);
		selector.handle_input(&Key::Enter);
		assert_eq!(&*calls.borrow(), &[QUEUE_MODE_ALL]);
	}

	#[test]
	fn escape_cancels_without_selecting() {
		let mut cancelled = 0;
		let mut selector = QueueModeSelectorComponent::new("all", |_| {}, || cancelled += 1);
		selector.handle_input(&Key::Esc);
		assert_eq!(cancelled, 1);
	}

	#[test]
	fn picker_description_is_a_docked_sheet() {
		let selector = QueueModeSelectorComponent::new("all", |_| {}, || {});
		let mut cx = DescribeContext::new(40);
		cx.enable_feature("picker");
		let described = selector.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.children[0].kind, NodeKind::Picker);
	}

	#[test]
	fn fallback_description_is_rows() {
		let selector = QueueModeSelectorComponent::new("all", |_| {}, || {});
		let cx = DescribeContext::new(40);
		let described = selector.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Rows);
	}

	#[test]
	fn click_routes_past_the_top_border_and_activates() {
		let calls = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&calls);
		let mut selector = QueueModeSelectorComponent::new(
			"one-at-a-time",
			move |mode| sink.borrow_mut().push(mode),
			|| {},
		);
		let event = MouseEvent {
			kind: MouseKind::Down(MouseButtonKind::Left),
			column: 3,
			row: 2,
			shift: false,
			ctrl: false,
			alt: false,
		};
		selector.route_mouse(&event, 2, 3);
		assert_eq!(&*calls.borrow(), &[QUEUE_MODE_ALL]);
	}

	#[test]
	fn select_list_maps_items_to_value_spellings() {
		let selector = QueueModeSelectorComponent::new("all", |_| {}, || {});
		let list = selector.select_list();
		let values: Vec<&str> = list.items().iter().map(value_of).collect();
		assert_eq!(values, vec!["one-at-a-time", "all"]);
	}
}
