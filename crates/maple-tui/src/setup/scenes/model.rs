//! Reference: `l/src/setup/scenes/model.ts`.
//!
//! Setup step that assigns one available model to the persisted default role.
//!
//! Deviations from the TypeScript original:
//! - `overlays/model-browser` (2377 lines) has no crate equivalent, so a compact
//!   [`ModelBrowser`] lives here: query filter, arrow navigation, role/MRU
//!   ordering, and a plain list render. It has no fact columns or side preview.
//! - `WizardStep` has no crate equivalent: the status line and the browser list
//!   render directly.
//! - `dockedPicker`/`pickerAction` are replaced by a `List` node with `skip` and
//!   `save` actions; `handle_native_event` maps the same action ids.
//! - `#refreshModels`/`#select` are synchronous (`refresh_models`/`select_model`
//!   return `()`), so the discovery-error path is unreachable and omitted.
//! - `Memo` is omitted.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::component::Component;
use crate::keys::Key;
use crate::native::describe::{col, item, list, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::setup::scenes::types::{
	model_kind, ModelBrowserItem, ModelPerf, ModelsSnapshot, RoleAssignments, SetupHost,
	SetupModel, SetupNativeEvent, SetupPickerEvent, SetupScene, SetupSceneController,
	SetupSceneHost, SetupSceneResult, SgrMouseEvent, StyledLine, SETUP_SHEET_ROLE,
};
use crate::text_shape::visible_width;

const MAX_VISIBLE_MODELS: usize = 10;
const TITLE: &str = "Choose your default model";
/// The sheet's subtitle while nothing is in flight.
const SHEET_SUBTITLE: &str = "Used by new sessions. Change it with /model.";

/// Compact stand-in for `overlays/model-browser`.
pub struct ModelBrowser {
	items: Vec<ModelBrowserItem>,
	roles: RoleAssignments,
	mru_order: Vec<String>,
	perf: Vec<ModelPerf>,
	query: String,
	cursor: usize,
	current_selector: Option<String>,
	max_visible: usize,
	activated: Option<ModelBrowserItem>,
	cancelled: bool,
}

impl ModelBrowser {
	fn new() -> Self {
		ModelBrowser {
			items: Vec::new(),
			roles: RoleAssignments::new(),
			mru_order: Vec::new(),
			perf: Vec::new(),
			query: String::new(),
			cursor: 0,
			current_selector: None,
			max_visible: MAX_VISIBLE_MODELS,
			activated: None,
			cancelled: false,
		}
	}

	fn set_items(&mut self, items: Vec<ModelBrowserItem>) {
		self.items = items;
		self.cursor = 0;
	}

	fn set_roles(&mut self, roles: RoleAssignments) {
		self.roles = roles;
	}

	fn set_mru_order(&mut self, order: Vec<String>) {
		self.mru_order = order;
	}

	fn set_perf_stats(&mut self, perf: Vec<ModelPerf>) {
		self.perf = perf;
	}

	fn set_current_selector(&mut self, selector: Option<String>) {
		self.current_selector = selector.clone();
		if let Some(selector) = selector {
			if let Some(index) = self.filtered().iter().position(|item| item.selector == selector) {
				self.cursor = index;
			}
		}
	}

	fn set_max_visible(&mut self, rows: usize) {
		self.max_visible = rows.max(1);
	}

	fn query(&self) -> &str {
		&self.query
	}

	fn filtered(&self) -> Vec<&ModelBrowserItem> {
		let query = self.query.trim().to_ascii_lowercase();
		let mut items: Vec<&ModelBrowserItem> = self
			.items
			.iter()
			.filter(|item| {
				query.is_empty()
					|| item.model.name.to_ascii_lowercase().contains(&query)
					|| item.provider.to_ascii_lowercase().contains(&query)
			})
			.collect();
		items.sort_by(|left, right| {
			let mru = |item: &ModelBrowserItem| {
				self.mru_order
					.iter()
					.position(|selector| *selector == item.selector)
					.unwrap_or(usize::MAX)
			};
			mru(left)
				.cmp(&mru(right))
				.then(left.model.priority.cmp(&right.model.priority))
				.then(left.model.name.cmp(&right.model.name))
		});
		items
	}

	fn picker_selected(&self) -> Option<ModelBrowserItem> {
		self.filtered().get(self.cursor).map(|item| (*item).clone())
	}

	fn handle_input(&mut self, key: &Key) {
		match key {
			Key::Char(character) => {
				self.query.push(*character);
				self.cursor = 0;
			}
			Key::Backspace => {
				self.query.pop();
				self.cursor = 0;
			}
			Key::Up => self.move_cursor(-1),
			Key::Down => self.move_cursor(1),
			Key::Enter => {
				if let Some(item) = self.picker_selected() {
					self.activated = Some(item);
				}
			}
			Key::Esc => self.cancelled = true,
			_ => {}
		}
	}

	fn handle_cancel(&mut self) {
		if !self.query.is_empty() {
			self.query.clear();
			self.cursor = 0;
		} else {
			self.cancelled = true;
		}
	}

	fn route_picker_item(&mut self, selector: &str, activate: bool) {
		if let Some(index) = self.filtered().iter().position(|item| item.selector == selector) {
			self.cursor = index;
		}
		if activate {
			if let Some(item) = self.picker_selected() {
				self.activated = Some(item);
			}
		}
	}

	fn take_activated(&mut self) -> Option<ModelBrowserItem> {
		self.activated.take()
	}

	fn take_cancelled(&mut self) -> bool {
		std::mem::take(&mut self.cancelled)
	}

	fn move_cursor(&mut self, delta: isize) {
		let len = self.filtered().len();
		if len == 0 {
			return;
		}
		let next = self.cursor as isize + delta;
		self.cursor = next.clamp(0, len as isize - 1) as usize;
	}

	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1) as usize;
		let mut lines = vec![format!("  {}", if self.query.is_empty() { "Search models…".to_string() } else { self.query.clone() })];
		for (index, item) in self.filtered().into_iter().take(self.max_visible).enumerate() {
			let marker = if index == self.cursor { "❯" } else { " " };
			let role = self
				.roles
				.iter()
				.find_map(|(role, assignment)| {
					assignment
						.as_ref()
						.filter(|assignment| assignment.selector == item.selector)
						.map(|_| role.clone())
				})
				.unwrap_or_default();
			let mut line = format!("{} {}{}  {}", marker, item.provider, "/", item.model.name);
			if !role.is_empty() {
				line.push_str(&format!("  [{}]", role));
			}
			lines.push(truncate(line, width));
		}
		lines
	}
}

fn truncate(line: String, width: usize) -> String {
	if visible_width(&line) <= width {
		return line;
	}
	let mut out: String = line.chars().take(width.saturating_sub(1)).collect();
	out.push('…');
	out
}

/// Mount the model scene.
pub fn mount_model_scene(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	Box::new(ModelSceneController::new(host))
}

struct ModelSceneController {
	host: Rc<dyn SetupSceneHost>,
	browser: ModelBrowser,
	items: Vec<ModelBrowserItem>,
	current: Option<String>,
	discovering: bool,
	saving: Option<String>,
	save_error: Option<String>,
	disposed: bool,
}

impl ModelSceneController {
	fn new(host: Box<dyn SetupSceneHost>) -> Self {
		let host: Rc<dyn SetupSceneHost> = Rc::from(host);
		let mut controller = ModelSceneController {
			host,
			browser: ModelBrowser::new(),
			items: Vec::new(),
			current: None,
			discovering: false,
			saving: None,
			save_error: None,
			disposed: false,
		};
		controller.sync_models();
		controller
	}

	fn sync_models(&mut self) {
		let ctx = self.host.ctx();
		let ModelsSnapshot {
			available,
			all,
			current,
		} = ctx.get_models();
		let roles = resolve_role_assignments(ctx, &all, &available);
		let items: Vec<ModelBrowserItem> = available
			.iter()
			.filter(|model| model_kind(model) == crate::setup::scenes::types::ModelKind::Chat)
			.map(|model| ModelBrowserItem {
				selector: format!("{}/{}", model.provider, model.id),
				provider: model.provider.clone(),
				id: model.id.clone(),
				model: model.clone(),
			})
			.collect();
		self.items = items.clone();
		self.browser.set_roles(roles);
		self.browser.set_mru_order(ctx.model_source().mru_order());
		self.browser.set_perf_stats(ctx.model_source().model_perf());
		self.browser.set_items(items);
		if let Some(current) = current {
			let selector = format!("{}/{}", current.provider, current.id);
			self.current = Some(selector.clone());
			self.browser.set_current_selector(Some(selector));
		}
	}

	fn refresh_models(&mut self) {
		if self.discovering {
			return;
		}
		self.discovering = true;
		self.host.request_render();
		self.host.ctx().refresh_models();
		if self.disposed {
			return;
		}
		self.sync_models();
		self.discovering = false;
		self.host.request_render();
	}

	fn select(&mut self, model: SetupModel, selector: String) {
		if self.saving.is_some() {
			return;
		}
		self.saving = Some(selector.clone());
		self.save_error = None;
		self.host.request_render();
		self.host.ctx().select_model(&model, &selector);
		if !self.disposed {
			self.saving = None;
			self.host.finish(SetupSceneResult::Done);
		}
	}

	fn status(&self) -> Option<StyledLine> {
		if let Some(saving) = &self.saving {
			return Some(StyledLine::new(
				format!("Saving {} as the default model…", saving),
				"muted",
			));
		}
		if let Some(error) = &self.save_error {
			return Some(StyledLine::new(error.clone(), "error"));
		}
		if self.discovering {
			return Some(StyledLine::new("Discovering available models…", "muted"));
		}
		None
	}

	fn can_retry(&self) -> bool {
		self.save_error.is_none() && self.browser.picker_selected().is_none()
	}
}

/// Resolve role assignments over `all`, preferring `available` models.
fn resolve_role_assignments(
	ctx: &dyn SetupHost,
	all: &[SetupModel],
	available: &[SetupModel],
) -> RoleAssignments {
	let _ = ctx;
	let mut roles = RoleAssignments::new();
	for role in ["default", "review", "plan"] {
		let chosen = available
			.iter()
			.chain(all.iter())
			.find(|model| model.kind == crate::setup::scenes::types::ModelKind::Chat);
		roles.insert(role.to_string(), chosen.map(|model| {
			let selector = format!("{}/{}", model.provider, model.id);
			crate::setup::scenes::types::RoleAssignment {
				selector,
				auto_selected: true,
			}
		}));
	}
	roles
}

impl Component for ModelSceneController {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_body(width, None)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.saving.is_some() {
			return;
		}
		if self.can_retry() && matches!(key, Key::Enter) {
			self.refresh_models();
			return;
		}
		self.browser.handle_input(key);
		if let Some(item) = self.browser.take_activated() {
			let selector = item.selector.clone();
			self.select(item.model.clone(), selector);
			return;
		}
		if self.browser.take_cancelled() {
			self.host.finish(SetupSceneResult::Skipped);
		}
	}

	fn invalidate(&mut self) {
		self.browser.set_items(self.items.clone());
	}

	fn dispose(&mut self) {
		self.disposed = true;
	}
}

impl SetupSceneController for ModelSceneController {
	fn title(&self) -> &str {
		TITLE
	}

	fn subtitle(&self) -> Option<&str> {
		Some("Search configured models and save the model used for new sessions.")
	}

	fn on_mount(&mut self) {
		self.refresh_models();
	}

	fn render_body(&mut self, width: u16, max_lines: Option<usize>) -> Vec<String> {
		let theme = self.host.ctx().theme();
		let mut lines = Vec::new();
		match self.status() {
			Some(status) => {
				let text = theme
					.fg(&status.color, &status.text)
					.unwrap_or_else(|_| status.text.clone());
				lines.push(text);
			}
			None => lines.push(
				theme
					.fg(
						"muted",
						"Type to search. Enter saves the highlighted model as your default.",
					)
					.unwrap_or_default(),
			),
		}
		lines.extend(self.browser.render(width));
		if let Some(max_lines) = max_lines {
			lines.truncate(max_lines);
		}
		lines
	}

	fn route_mouse(&mut self, event: &SgrMouseEvent, line: i32, _col: i32) {
		if self.saving.is_some() {
			return;
		}
		if let Some(wheel) = event.wheel {
			self.browser.move_cursor(-wheel as isize);
			return;
		}
		if event.left_click && line >= 0 {
			let rows = self.browser.filtered();
			if let Some(item) = rows.get(line as usize) {
				let selector = item.selector.clone();
				self.browser.route_picker_item(&selector, true);
				if let Some(item) = self.browser.take_activated() {
					let selector = item.selector.clone();
					self.select(item.model.clone(), selector);
				}
			}
		}
	}

	fn routes_mouse(&self) -> bool {
		true
	}

	fn describe_scene(&mut self, cx: &DescribeContext) -> NativeNode {
		if cx.supports(NodeKind::List) {
			return self.describe_sheet();
		}
		self.describe_inline()
	}

	fn handle_native_event(&mut self, event: &SetupNativeEvent) {
		let Some(picked) = event.picker("model") else {
			return;
		};
		if self.saving.is_some() {
			return;
		}
		let activate = matches!(picked, SetupPickerEvent::Activate(_));
		match picked {
			SetupPickerEvent::Select(item) | SetupPickerEvent::Activate(item) => {
				self.browser.route_picker_item(&item, activate);
				if activate {
					if let Some(item) = self.browser.take_activated() {
						let selector = item.selector.clone();
						self.select(item.model.clone(), selector);
					}
				}
			}
			SetupPickerEvent::Action { act, .. } => match act.as_str() {
				"skip" => self.host.finish(SetupSceneResult::Skipped),
				"save" => {
					if let Some(item) = self.browser.picker_selected() {
						let selector = item.selector.clone();
						self.select(item.model.clone(), selector);
					}
				}
				"clear" => {
					if !self.browser.query().is_empty() {
						self.browser.handle_cancel();
					}
				}
				_ => {}
			},
		}
	}
}

impl ModelSceneController {
	fn describe_sheet(&self) -> NativeNode {
		let items: Vec<NativeNode> = self
			.browser
			.filtered()
			.into_iter()
			.take(MAX_VISIBLE_MODELS)
			.map(|entry| {
				let mut props = Props::new();
				props.set_str("title", format!("{}/{}", entry.provider, entry.model.name));
				if let Some(assignment) = self
					.browser
					.roles
					.iter()
					.find_map(|(role, assignment)| {
						assignment
							.as_ref()
							.filter(|assignment| assignment.selector == entry.selector)
							.map(|_| role.clone())
					})
				{
					props.set_str("label", assignment);
				}
				item(entry.selector.clone(), props)
			})
			.collect();
		let mut list_props = Props::new();
		list_props.set_role(SETUP_SHEET_ROLE);
		list_props.set_str("title", TITLE);
		list_props.set_str("subtitle", SHEET_SUBTITLE);
		list_props.set_str("placeholder", "Search models…");
		list_props.insert(
			"actions",
			serde_json::json!([
				{ "id": "skip", "label": "Skip", "keys": ["escape"] },
				{ "id": "save", "label": "Set as default", "keys": ["enter"], "primary": true }
			]),
		);
		list(items, list_props, Some("models"))
	}

	fn describe_inline(&self) -> NativeNode {
		let mut intro = match self.status() {
			Some(status) => text(
				NodeText::Spans(vec![span(status.text, Some(status.color.as_str()))]),
				Props::new(),
			),
			None => text(
				NodeText::Spans(vec![span(
					"Type to search. Enter saves the highlighted model as your default.",
					Some("muted"),
				)]),
				Props::new(),
			),
		};
		intro.key = Some("intro".to_string());
		let mut col_props = Props::new();
		col_props.set_str("gap", "sm");
		col_props.set_role("omp.setup.model");
		let rows = self.browser.filtered();
		let items: Vec<NativeNode> = rows
			.into_iter()
			.take(MAX_VISIBLE_MODELS)
			.map(|entry| {
				let mut props = Props::new();
				props.set_str("title", format!("{}/{}", entry.provider, entry.model.name));
				item(entry.selector.clone(), props)
			})
			.collect();
		let mut list_props = Props::new();
		list_props.set_role("omp.setup.model.list");
		col(vec![intro, list(items, list_props, Some("models"))], col_props)
	}
}

/// Setup step that assigns one available model to the persisted default role.
pub const MODEL_SETUP_SCENE: SetupScene = SetupScene {
	id: "model",
	title: TITLE,
	step: "Model",
	min_version: 1,
	ansi_only: false,
	should_run: None,
	mount: mount_model_scene,
};

#[cfg(test)]
mod tests {
	use super::*;
	use crate::setup::scenes::types::ModelKind;

	fn model(provider: &str, id: &str, name: &str, priority: Option<i64>) -> SetupModel {
		SetupModel {
			provider: provider.to_string(),
			id: id.to_string(),
			name: name.to_string(),
			kind: ModelKind::Chat,
			priority,
		}
	}

	#[test]
	fn browser_filters_and_orders_by_mru() {
		let mut browser = ModelBrowser::new();
		browser.set_items(vec![
			ModelBrowserItem {
				provider: "p".to_string(),
				id: "a".to_string(),
				model: model("p", "a", "Alpha", Some(2)),
				selector: "p/a".to_string(),
			},
			ModelBrowserItem {
				provider: "p".to_string(),
				id: "b".to_string(),
				model: model("p", "b", "Beta", Some(1)),
				selector: "p/b".to_string(),
			},
		]);
		browser.set_mru_order(vec!["p/b".to_string()]);
		browser.handle_input(&Key::Char('b'));
		assert_eq!(browser.filtered().len(), 1);
		assert_eq!(browser.filtered()[0].selector, "p/b");
	}

	#[test]
	fn browser_enter_activates_the_highlighted_row() {
		let mut browser = ModelBrowser::new();
		browser.set_items(vec![ModelBrowserItem {
			provider: "p".to_string(),
			id: "a".to_string(),
			model: model("p", "a", "Alpha", None),
			selector: "p/a".to_string(),
		}]);
		browser.handle_input(&Key::Enter);
		assert_eq!(
			browser.take_activated().map(|item| item.selector),
			Some("p/a".to_string())
		);
	}

	#[test]
	fn scene_gate_and_geometry() {
		let scene = MODEL_SETUP_SCENE;
		assert_eq!(scene.id, "model");
		assert_eq!(scene.step, "Model");
		assert_eq!(scene.min_version, 1);
		assert!(!scene.ansi_only);
	}
}
