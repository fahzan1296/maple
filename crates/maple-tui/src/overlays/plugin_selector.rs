//! Reference: `l/src/overlays/plugin-selector.ts`.
//!
//! Interactive marketplace plugin selector: shows the plugins of every
//! configured marketplace in a list, where selecting one installs it and Esc
//! cancels. Natively the same list is described as a docked picker sheet whose
//! rows carry version, install state and scope as badges.
//!
//! Deviations:
//! - The reference reads the process-global theme through `getSelectListTheme()`;
//!   maple-tui has no global UI state, so the list uses
//!   [`SelectListTheme::default`].
//! - `TspPickerItem` badges/facts are wire props here, as
//!   [`crate::native::picker`] models them.
//! - The callbacks object becomes [`PluginSelectorCallbacks`], injected by the
//!   host (no globals).

use std::cell::{Ref, RefCell, RefMut};
use std::collections::{HashMap, HashSet};
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
	docked_picker, route_select_list_picker, select_list_picker, PickerColumn, PickerList, PickerUiEvent,
	PickerView, SelectPickerOptions,
};

/// Most plugin rows the list shows at once.
pub const PLUGIN_SELECTOR_MAX_VISIBLE: usize = 20;
/// Value of the disabled row shown when no plugins exist.
pub const EMPTY_PLUGIN_ID: &str = "__empty__";

/// Which settings file an installed plugin lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginScope {
	/// The user-level marketplace configuration.
	User,
	/// The project-level marketplace configuration.
	Project,
}

impl PluginScope {
	/// The scope spelling used in plugin ids and labels.
	pub fn as_str(self) -> &'static str {
		match self {
			PluginScope::User => "user",
			PluginScope::Project => "project",
		}
	}

	/// Parse a scope spelling.
	pub fn parse(value: &str) -> Option<PluginScope> {
		match value {
			"user" => Some(PluginScope::User),
			"project" => Some(PluginScope::Project),
			_ => None,
		}
	}
}

/// A marketplace plugin's catalogue entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginInfo {
	/// Plugin name.
	pub name: String,
	/// Version offered by the marketplace.
	pub version: Option<String>,
	/// One-line description.
	pub description: Option<String>,
}

impl PluginInfo {
	/// An entry named `name`.
	pub fn new(name: impl Into<String>) -> Self {
		PluginInfo {
			name: name.into(),
			version: None,
			description: None,
		}
	}

	/// Set the offered version.
	pub fn with_version(mut self, version: impl Into<String>) -> Self {
		self.version = Some(version.into());
		self
	}

	/// Set the description.
	pub fn with_description(mut self, description: impl Into<String>) -> Self {
		self.description = Some(description.into());
		self
	}
}

/// One selectable plugin row: the catalogue entry, its marketplace, and the
/// scope of that marketplace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginItem {
	/// The catalogue entry.
	pub plugin: PluginInfo,
	/// Marketplace the plugin comes from.
	pub marketplace: String,
	/// Scope of this entry, appended to the label and forwarded to `on_select`.
	pub scope: Option<PluginScope>,
}

impl PluginItem {
	/// A user-scope entry from `marketplace`.
	pub fn new(plugin: PluginInfo, marketplace: impl Into<String>) -> Self {
		PluginItem {
			plugin,
			marketplace: marketplace.into(),
			scope: None,
		}
	}

	/// Attach the marketplace's scope.
	pub fn with_scope(mut self, scope: PluginScope) -> Self {
		self.scope = Some(scope);
		self
	}
}

/// What a plugin selection reports.
pub struct PluginSelectorCallbacks {
	/// Runs with the chosen plugin's name, marketplace and scope.
	pub on_select: Box<dyn FnMut(&str, &str, Option<PluginScope>)>,
	/// Runs when the selector is dismissed.
	pub on_cancel: Box<dyn FnMut()>,
}

impl PluginSelectorCallbacks {
	/// Build the callback pair.
	pub fn new(
		on_select: impl FnMut(&str, &str, Option<PluginScope>) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		PluginSelectorCallbacks {
			on_select: Box::new(on_select),
			on_cancel: Box::new(on_cancel),
		}
	}
}

/// A row badge: text plus an optional tone token (`success`, `accent`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginBadge {
	/// Badge text.
	pub text: String,
	/// Tone token the host paints the badge with.
	pub tone: Option<&'static str>,
}

/// The id a plugin row carries: `name@marketplace`, plus `#scope` when scoped.
#[must_use]
pub fn plugin_row_id(name: &str, marketplace: &str, scope: Option<PluginScope>) -> String {
	match scope {
		Some(scope) => format!("{name}@{marketplace}#{}", scope.as_str()),
		None => format!("{name}@{marketplace}"),
	}
}

/// The parts of a plugin row id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginId {
	/// Plugin name.
	pub name: String,
	/// Marketplace.
	pub marketplace: String,
	/// Scope suffix, when the id carries one.
	pub scope: Option<PluginScope>,
}

/// Split a plugin row id; `None` when it is not a plugin id at all.
#[must_use]
pub fn split_plugin_id(id: &str) -> Option<PluginId> {
	// value format: "name@marketplace" or "name@marketplace#scope"
	let (base, scope) = match id.split_once('#') {
		Some((base, suffix)) => (base, PluginScope::parse(suffix)),
		None => (id, None),
	};
	let at = base.rfind('@')?;
	if at == 0 {
		return None;
	}
	Some(PluginId {
		name: base[..at].to_string(),
		marketplace: base[at + 1..].to_string(),
		scope,
	})
}

/// The badges of a plugin row: offered version, then install state, then scope.
#[must_use]
pub fn plugin_badges(plugin: &PluginInfo, installed: bool, scope: Option<PluginScope>) -> Vec<PluginBadge> {
	let mut badges = Vec::new();
	if let Some(version) = &plugin.version {
		badges.push(PluginBadge {
			text: version.clone(),
			tone: None,
		});
	}
	if installed {
		badges.push(PluginBadge {
			text: "installed".to_string(),
			tone: Some("success"),
		});
	}
	if let Some(scope) = scope {
		badges.push(PluginBadge {
			text: scope.as_str().to_string(),
			tone: Some("accent"),
		});
	}
	badges
}

/// The text-mode label of a plugin row: version, install state and scope as
/// bracketed suffixes.
#[must_use]
pub fn plugin_label(plugin: &PluginInfo, installed: bool, scope: Option<PluginScope>) -> String {
	let version = plugin
		.version
		.as_ref()
		.map(|version| format!("@{version}"))
		.unwrap_or_default();
	let status = if installed { " [installed]" } else { "" };
	let scope = scope
		.map(|scope| format!(" [{}]", scope.as_str()))
		.unwrap_or_default();
	format!("{}{version}{status}{scope}", plugin.name)
}

/// Why the empty row says no plugins exist.
#[must_use]
pub fn plugin_empty_description(marketplace_count: usize) -> &'static str {
	if marketplace_count == 0 {
		"Add a marketplace first: /marketplace add <source>"
	} else {
		"Configured marketplaces have no plugins"
	}
}

/// Build the rows of the plugin list, including the disabled empty row.
#[must_use]
pub fn build_plugin_items(
	plugins: &[PluginItem],
	installed_ids: &HashSet<String>,
	marketplace_count: usize,
) -> Vec<SelectItem> {
	let mut items: Vec<SelectItem> = plugins
		.iter()
		.map(|item| {
			let id = plugin_row_id(&item.plugin.name, &item.marketplace, item.scope);
			let installed =
				installed_ids.contains(&format!("{}@{}", item.plugin.name, item.marketplace));
			let mut row = SelectItem::new(id, plugin_label(&item.plugin, installed, item.scope))
				.with_label(item.marketplace.clone());
			if let Some(description) = &item.plugin.description {
				row = row.with_description(description.clone());
			}
			row
		})
		.collect();
	if items.is_empty() {
		items.push(
			SelectItem::new(EMPTY_PLUGIN_ID, "No plugins available")
				.with_description(plugin_empty_description(marketplace_count))
				.disabled(),
		);
	}
	items
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

/// A marketplace plugin selector panel.
pub struct PluginSelectorComponent {
	title: String,
	role: String,
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for PluginSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PluginSelectorComponent")
			.field("title", &self.title)
			.field("role", &self.role)
			.finish()
	}
}

impl PluginSelectorComponent {
	/// Build the selector over `plugins`.
	///
	/// `installed_ids` holds `name@marketplace` ids that are already installed;
	/// `marketplace_count` only shapes the empty-row copy.
	pub fn new(
		marketplace_count: usize,
		plugins: &[PluginItem],
		installed_ids: &HashSet<String>,
		callbacks: PluginSelectorCallbacks,
	) -> Self {
		// The sheet draws version, install state and scope as badges instead of
		// bracketed label text.
		let mut badges_by_value: HashMap<String, Vec<PluginBadge>> = HashMap::new();
		for item in plugins {
			let id = plugin_row_id(&item.plugin.name, &item.marketplace, item.scope);
			let installed = installed_ids.contains(&format!("{}@{}", item.plugin.name, item.marketplace));
			badges_by_value.insert(id, plugin_badges(&item.plugin, installed, item.scope));
		}

		let items = build_plugin_items(plugins, installed_ids, marketplace_count);
		let max_visible = items.len().min(PLUGIN_SELECTOR_MAX_VISIBLE);
		let mut list = SelectList::new(items, max_visible, SelectListTheme::default());
		let PluginSelectorCallbacks {
			mut on_select,
			on_cancel,
		} = callbacks;
		list.on_select(move |item| {
			let Some(plugin) = split_plugin_id(&item.key) else {
				return;
			};
			on_select(&plugin.name, &plugin.marketplace, plugin.scope);
		});
		list.on_cancel(on_cancel);

		let badges = Arc::new(badges_by_value);
		let options = SelectPickerOptions {
			title: "Plugins".to_string(),
			icon: Some("plus".to_string()),
			noun: Some("plugins".to_string()),
			confirm: Some("Install".to_string()),
			decorate: Some(Arc::new(move |item: &SelectItem| {
				let plugin = split_plugin_id(&item.key)?;
				let mut props = Props::new();
				props.set_str("label", plugin.name);
				let badges = badges.get(&item.key).cloned().unwrap_or_default();
				props.insert(
					"badges",
					serde_json::Value::Array(
						badges
							.into_iter()
							.map(|badge| {
								let mut value = serde_json::Map::new();
								value.insert("text".to_string(), serde_json::Value::String(badge.text));
								if let Some(tone) = badge.tone {
									value.insert("tone".to_string(), serde_json::Value::String(tone.to_string()));
								}
								serde_json::Value::Object(value)
							})
							.collect(),
					),
				);
				if let Some(marketplace) = &item.label {
					props.insert(
						"facts",
						serde_json::json!({ "marketplace": marketplace }),
					);
				}
				Some(props)
			})),
			columns: Some(vec![PickerColumn {
				id: "marketplace".to_string(),
				head: None,
				format: Some("dim".to_string()),
				align: None,
				grow: None,
				priority: None,
			}]),
			..SelectPickerOptions::default()
		};

		PluginSelectorComponent {
			title: "Plugins".to_string(),
			role: "omp.overlay.plugins".to_string(),
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

impl Component for PluginSelectorComponent {
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

impl NativeDescribe for PluginSelectorComponent {
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

	fn installed(ids: &[&str]) -> HashSet<String> {
		ids.iter().map(|id| (*id).to_string()).collect()
	}

	fn catalogue() -> Vec<PluginItem> {
		vec![
			PluginItem::new(
				PluginInfo::new("lint").with_version("1.2.0").with_description("Lints plans"),
				"official",
			),
			PluginItem::new(PluginInfo::new("deploy"), "official").with_scope(PluginScope::Project),
		]
	}

	#[test]
	fn row_ids_round_trip_through_split() {
		let id = plugin_row_id("lint", "official", None);
		assert_eq!(id, "lint@official");
		assert_eq!(
			split_plugin_id(&id),
			Some(PluginId {
				name: "lint".to_string(),
				marketplace: "official".to_string(),
				scope: None,
			})
		);
		let scoped = plugin_row_id("deploy", "official", Some(PluginScope::Project));
		assert_eq!(scoped, "deploy@official#project");
		assert_eq!(
			split_plugin_id(&scoped).and_then(|plugin| plugin.scope),
			Some(PluginScope::Project)
		);
		assert_eq!(split_plugin_id(EMPTY_PLUGIN_ID), None);
		assert_eq!(split_plugin_id("@official"), None);
	}

	#[test]
	fn badges_report_version_install_state_and_scope() {
		let plugin = PluginInfo::new("lint").with_version("1.2.0");
		let badges = plugin_badges(&plugin, true, Some(PluginScope::User));
		assert_eq!(
			badges,
			vec![
				PluginBadge {
					text: "1.2.0".to_string(),
					tone: None
				},
				PluginBadge {
					text: "installed".to_string(),
					tone: Some("success")
				},
				PluginBadge {
					text: "user".to_string(),
					tone: Some("accent")
				},
			]
		);
		assert!(plugin_badges(&PluginInfo::new("bare"), false, None).is_empty());
	}

	#[test]
	fn labels_append_version_install_state_and_scope() {
		let plugin = PluginInfo::new("lint").with_version("1.2.0");
		assert_eq!(
			plugin_label(&plugin, true, Some(PluginScope::User)),
			"lint@1.2.0 [installed] [user]"
		);
		assert_eq!(plugin_label(&PluginInfo::new("bare"), false, None), "bare");
	}

	#[test]
	fn empty_catalogue_yields_one_disabled_row() {
		let items = build_plugin_items(&[], &installed(&[]), 0);
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].key, EMPTY_PLUGIN_ID);
		assert_eq!(items[0].title, "No plugins available");
		assert!(items[0].disabled);
		assert_eq!(
			items[0].description.as_deref(),
			Some("Add a marketplace first: /marketplace add <source>")
		);
		let items = build_plugin_items(&[], &installed(&[]), 2);
		assert_eq!(
			items[0].description.as_deref(),
			Some("Configured marketplaces have no plugins")
		);
	}

	#[test]
	fn catalogue_rows_carry_marketplace_tag_and_description() {
		let items = build_plugin_items(&catalogue(), &installed(&["lint@official"]), 1);
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].key, "lint@official");
		assert_eq!(items[0].title, "lint@1.2.0 [installed]");
		assert_eq!(items[0].label.as_deref(), Some("official"));
		assert_eq!(items[0].description.as_deref(), Some("Lints plans"));
		assert_eq!(items[1].key, "deploy@official#project");
		assert_eq!(items[1].title, "deploy [project]");
	}

	#[test]
	fn selector_starts_on_the_first_row_and_reports_installs() {
		let calls = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&calls);
		let mut selector = PluginSelectorComponent::new(
			1,
			&catalogue(),
			&installed(&["lint@official"]),
			PluginSelectorCallbacks::new(
				move |name, marketplace, scope| {
					sink.borrow_mut()
						.push((name.to_string(), marketplace.to_string(), scope));
				},
				|| {},
			),
		);
		assert_eq!(
			selector.select_list().selected_key().as_deref(),
			Some("lint@official")
		);
		selector.handle_input(&Key::Down);
		selector.handle_input(&Key::Enter);
		assert_eq!(
			&*calls.borrow(),
			&[(
				"deploy".to_string(),
				"official".to_string(),
				Some(PluginScope::Project)
			)]
		);
	}

	#[test]
	fn the_empty_row_cannot_be_activated() {
		let calls = Rc::new(RefCell::new(0usize));
		let sink = Rc::clone(&calls);
		let mut selector = PluginSelectorComponent::new(
			0,
			&[],
			&installed(&[]),
			PluginSelectorCallbacks::new(move |_, _, _| *sink.borrow_mut() += 1, || {}),
		);
		selector.handle_input(&Key::Enter);
		assert_eq!(*calls.borrow(), 0);
	}

	#[test]
	fn escape_cancels() {
		let cancelled = Rc::new(RefCell::new(0usize));
		let sink = Rc::clone(&cancelled);
		let mut selector = PluginSelectorComponent::new(
			1,
			&catalogue(),
			&installed(&[]),
			PluginSelectorCallbacks::new(|_, _, _| {}, move || *sink.borrow_mut() += 1),
		);
		selector.handle_input(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 1);
	}

	#[test]
	fn renders_a_titled_bordered_list() {
		let mut selector = PluginSelectorComponent::new(
			1,
			&catalogue(),
			&installed(&[]),
			PluginSelectorCallbacks::new(|_, _, _| {}, || {}),
		);
		let rows = selector.render(60);
		assert!(rows[0].contains("Plugins"), "{rows:?}");
		assert_eq!(rows.len(), 4);
		assert!(rows[1].contains("lint"), "{rows:?}");
		assert!(rows[2].contains("deploy"), "{rows:?}");
	}

	#[test]
	fn picker_description_is_a_docked_sheet_with_marketplace_column() {
		let selector = PluginSelectorComponent::new(
			1,
			&catalogue(),
			&installed(&["lint@official"]),
			PluginSelectorCallbacks::new(|_, _, _| {}, || {}),
		);
		let mut cx = DescribeContext::new(60);
		cx.enable_feature("picker");
		let described = selector.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.children[0].kind, NodeKind::Picker);
		let props = select_list_picker(&selector.list, selector.picker_options());
		assert_eq!(props.title, "Plugins");
		assert_eq!(props.noun, "plugins");
		assert_eq!(props.items.len(), 2);
		assert_eq!(props.items[0].id, "lint@official");
		assert_eq!(props.items[0].label, "lint");
		let columns = props.columns.unwrap_or_default();
		assert_eq!(columns[0].id, "marketplace");
		assert_eq!(columns[0].format.as_deref(), Some("dim"));
	}
}
