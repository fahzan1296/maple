//! Reference: `l/src/setup/scenes/updates.ts`.
//!
//! Opt in to background self-update (`update.auto`).
//!
//! Deviations from the TypeScript original:
//! - `theme` is `host.ctx().theme()`, not the module singleton; the select-list
//!   theme is [`SelectListTheme::default`] (`getSelectListTheme` has no crate
//!   equivalent).
//! - `WizardStep` has no crate equivalent: the intro line and the list are
//!   rendered directly, and `maxLines` truncates the body.
//! - `SelectList` has no native description, so [`describe_scene`] builds the
//!   `List`/`Item` nodes from `select_list.items()`.
//! - `#commit` is synchronous: `save_auto_update` runs before `finish`.
//! - `Memo` is omitted.

use std::cell::Cell;
use std::rc::Rc;

use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::keys::Key;
use crate::native::describe::{col, item, list, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeText, Props};
use crate::setup::scenes::types::{
	SetupHost, SetupScene, SetupSceneController, SetupSceneHost, SetupSceneResult, SgrMouseEvent,
};

const INTRO: &str =
	"Updates install through whatever installed omp: the standalone binary, Homebrew, mise, bun, or npm. Change this later in /settings.";

const UPDATE_ITEMS: [(&str, &str, &str); 2] = [
	(
		"auto",
		"1  Update automatically",
		"Installs in the background; the next launch runs it",
	),
	(
		"notify",
		"2  Notify me",
		"Show a notice on startup; run omp update yourself",
	),
];

fn update_items() -> Vec<SelectItem> {
	UPDATE_ITEMS
		.iter()
		.map(|(key, title, description)| {
			SelectItem::new(key.to_string(), title.to_string())
				.with_description(description.to_string())
		})
		.collect()
}

/// Mount the updates scene.
pub fn mount_updates_scene(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	Box::new(UpdatesSceneController::new(host))
}

struct UpdatesSceneController {
	host: Rc<dyn SetupSceneHost>,
	select_list: SelectList,
	committing: Rc<Cell<bool>>,
}

impl UpdatesSceneController {
	fn new(host: Box<dyn SetupSceneHost>) -> Self {
		let host: Rc<dyn SetupSceneHost> = Rc::from(host);
		let ctx = host.ctx();
		let mut select_list = SelectList::new(
			update_items(),
			UPDATE_ITEMS.len(),
			SelectListTheme::default(),
		);
		select_list.set_selected_index(if ctx.auto_update() { 0 } else { 1 });
		let committing = Rc::new(Cell::new(false));
		let finish_host = host.clone();
		let commit_flag = committing.clone();
		select_list.on_select(move |item| {
			if commit_flag.get() {
				return;
			}
			commit_flag.set(true);
			let enabled = item.key == "auto";
			finish_host.ctx().save_auto_update(enabled);
			finish_host.finish(SetupSceneResult::Done);
		});
		let cancel_host = host.clone();
		select_list.on_cancel(move || cancel_host.finish(SetupSceneResult::Skipped));
		UpdatesSceneController {
			host,
			select_list,
			committing,
		}
	}
}

impl Component for UpdatesSceneController {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_body(width, None)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.committing.get() {
			return;
		}
		let quick = match key {
			Key::Char('1') => Some(0isize),
			Key::Char('2') => Some(1isize),
			_ => None,
		};
		if let Some(index) = quick {
			self.select_list.set_selected_index(index);
			self.host.request_render();
			return;
		}
		self.select_list.handle_input(key);
	}

	fn invalidate(&mut self) {
		self.select_list.invalidate();
	}
}

impl SetupSceneController for UpdatesSceneController {
	fn title(&self) -> &str {
		"Keep omp up to date"
	}

	fn subtitle(&self) -> Option<&str> {
		Some("Choose how new releases reach you.")
	}

	fn render_body(&mut self, width: u16, max_lines: Option<usize>) -> Vec<String> {
		let theme = self.host.ctx().theme();
		let mut lines = vec![theme.fg("muted", INTRO).unwrap_or_else(|_| INTRO.to_string())];
		lines.extend(self.select_list.render(width));
		if let Some(max_lines) = max_lines {
			lines.truncate(max_lines);
		}
		lines
	}

	fn route_mouse(&mut self, event: &SgrMouseEvent, line: i32, _col: i32) {
		if self.committing.get() {
			return;
		}
		if let Some(wheel) = event.wheel {
			self.select_list.handle_wheel(wheel as isize);
			return;
		}
		if event.left_click && line >= 0 {
			if let Some(hit) = self.select_list.hit_test(line as usize) {
				let key = hit.key.clone();
				if let Some(index) = self
					.select_list
					.visible_items()
					.iter()
					.position(|item| item.key == key)
				{
					self.select_list.click_item(index);
				}
			}
		}
	}

	fn routes_mouse(&self) -> bool {
		true
	}

	fn describe_scene(&mut self, _cx: &DescribeContext) -> NativeNode {
		let mut intro = Props::new();
		intro.set_str("wrap", "word");
		let mut list_props = Props::new();
		list_props.set_role("omp.setup.updates.list");
		let items: Vec<NativeNode> = self
			.select_list
			.items()
			.iter()
			.map(|entry| {
				let mut props = Props::new();
				props.set_str("title", entry.title.as_str());
				if let Some(description) = &entry.description {
					props.set_str("detail", description.as_str());
				}
				item(entry.key.clone(), props)
			})
			.collect();
		let mut col_props = Props::new();
		col_props.set_str("gap", "sm");
		col_props.set_role("omp.setup.updates");
		col(
			vec![
				text(NodeText::Spans(vec![span(INTRO, Some("muted"))]), intro),
				list(items, list_props, Some("options")),
			],
			col_props,
		)
	}
}

/// Opt in to background self-update (`update.auto`). Skipped where omp cannot
/// update itself, notably under Tern, which keeps its omp current on its own.
pub const UPDATES_SETUP_SCENE: SetupScene = SetupScene {
	id: "auto-update",
	title: "Keep omp up to date",
	step: "Updates",
	min_version: 3,
	ansi_only: false,
	should_run: Some(|ctx| ctx.can_auto_update()),
	mount: mount_updates_scene,
};

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn update_items_are_numbered_and_keyed() {
		let items = update_items();
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].key, "auto");
		assert_eq!(items[1].key, "notify");
		assert!(items[0].title.starts_with("1"));
		assert!(items[1].title.starts_with("2"));
	}

	#[test]
	fn scene_gate_and_geometry() {
		let scene = UPDATES_SETUP_SCENE;
		assert_eq!(scene.id, "auto-update");
		assert_eq!(scene.step, "Updates");
		assert_eq!(scene.min_version, 3);
		assert!(!scene.ansi_only);
		assert!(scene.should_run.is_some());
	}
}
