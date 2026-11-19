//! Reference: `l/src/setup/scenes/composer.ts`.
//!
//! Select and persist the prompt composer layout.
//!
//! Deviations from the TypeScript original:
//! - `theme` is `host.ctx().theme()`; `getSelectListTheme` becomes
//!   [`SelectListTheme::default`].
//! - `getComposerShapeOptions` maps to
//!   `overlays::composer_shape_registry::get_composer_shape_options`.
//! - `renderComposerShapePreview` has no crate equivalent (the crate's preview
//!   needs a composer style registry that is not vendored), so a compact local
//!   preview paints the status top rule, a prompt row, and the bottom bar.
//! - `WizardStep` has no crate equivalent: intro, preview, and list render
//!   directly, and `maxLines` truncates the body.
//! - `#commit` is synchronous: `save_composer_shape` runs before `finish`.
//! - `Memo` is omitted.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::chrome::keybinding_hints::editor_key;
use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::describe::{ansi, col, item, list, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeText, Props};
use crate::overlays::composer_shape_registry::get_composer_shape_options;
use crate::setup::scenes::types::{
	ComposerPreviewStatusSource, SetupHost, SetupScene, SetupSceneController, SetupSceneHost,
	SetupSceneResult, SgrMouseEvent,
};

/// Mount the composer scene.
pub fn mount_composer_scene(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	Box::new(ComposerSceneController::new(host))
}

struct ComposerSceneController {
	host: Rc<dyn SetupSceneHost>,
	shapes: Vec<String>,
	select_list: SelectList,
	current_shape: Rc<RefCell<String>>,
	committing: Rc<Cell<bool>>,
}

impl ComposerSceneController {
	fn new(host: Box<dyn SetupSceneHost>) -> Self {
		let host: Rc<dyn SetupSceneHost> = Rc::from(host);
		let ctx = host.ctx();
		let choices = get_composer_shape_options();
		let shapes: Vec<String> = choices.iter().map(|choice| choice.value.clone()).collect();
		let items: Vec<SelectItem> = choices
			.iter()
			.enumerate()
			.map(|(index, choice)| {
				SelectItem::new(
					choice.value.clone(),
					format!("{}  {}", index + 1, choice.label),
				)
				.with_description(choice.description.clone().unwrap_or_default())
			})
			.collect();
		let configured = ctx.composer_shape();
		let initial = if shapes.iter().any(|shape| shape == configured) {
			configured.to_string()
		} else {
			"band".to_string()
		};
		let initial_index = shapes.iter().position(|shape| *shape == initial).unwrap_or(0);
		let mut select_list = SelectList::new(items, shapes.len(), SelectListTheme::default());
		select_list.set_selected_index(initial_index as isize);

		let committing = Rc::new(Cell::new(false));
		let current_shape = Rc::new(RefCell::new(initial));
		let preview_shape = current_shape.clone();
		let preview_host = host.clone();
		select_list.on_selection_change(move |item| {
			*preview_shape.borrow_mut() = item.key.clone();
			preview_host.request_render();
		});
		let commit_host = host.clone();
		let commit_flag = committing.clone();
		select_list.on_select(move |item| {
			if commit_flag.get() {
				return;
			}
			commit_flag.set(true);
			commit_host.ctx().save_composer_shape(&item.key);
			commit_host.finish(SetupSceneResult::Done);
		});
		let cancel_host = host.clone();
		select_list.on_cancel(move || cancel_host.finish(SetupSceneResult::Skipped));

		ComposerSceneController {
			host,
			shapes,
			select_list,
			current_shape,
			committing,
		}
	}

	fn preview_shape(&mut self, shape: String) {
		*self.current_shape.borrow_mut() = shape;
		self.host.request_render();
	}
}

/// Compact stand-in for `renderComposerShapePreview`: the status top rule, a
/// prompt row, and the bottom bar.
fn render_shape_preview(
	shape: &str,
	width: usize,
	status: Option<&dyn ComposerPreviewStatusSource>,
) -> Vec<String> {
	let mut lines = vec![String::new(), "Preview:".to_string()];
	if let Some(status) = status {
		let (top, _) = status.top_border(width, Some("Setup"));
		lines.push(top);
	}
	lines.push(format!("  ❯ Ask anything, edit files, run tools{}", "▌"));
	if let Some(status) = status {
		lines.push(status.bottom_bar(width, "full", Some("Setup")));
	}
	let _ = shape;
	lines
}

impl Component for ComposerSceneController {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_body(width, None)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.committing.get() {
			return;
		}
		let quick = match key {
			Key::Char(character) if character.is_ascii_digit() => {
				let index = (*character as u32).checked_sub('1' as u32).map(|digit| digit as isize);
				match index {
					Some(index) if (index as usize) < self.shapes.len() => Some(index),
					_ => None,
				}
			}
			_ => None,
		};
		if let Some(index) = quick {
			self.select_list.set_selected_index(index);
			if let Some(shape) = self.shapes.get(index as usize) {
				self.preview_shape(shape.clone());
			}
			return;
		}
		self.select_list.handle_input(key);
	}

	fn invalidate(&mut self) {
		self.select_list.invalidate();
	}
}

impl SetupSceneController for ComposerSceneController {
	fn title(&self) -> &str {
		"Choose composer shape"
	}

	fn subtitle(&self) -> Option<&str> {
		Some("Pick the prompt and status line layout for your workflow.")
	}

	fn render_body(&mut self, width: u16, max_lines: Option<usize>) -> Vec<String> {
		let ctx = self.host.ctx();
		let theme = ctx.theme();
		let confirm = editor_key(&KeybindingsManager::new(), theme, "tui.select.confirm");
		let mut lines = vec![theme
			.fg(
				"muted",
				&format!(
					"Select a layout; live preview updates below. Press {} to confirm.",
					confirm
				),
			)
			.unwrap_or_default()];
		lines.extend(render_shape_preview(
			&self.current_shape.borrow(),
			width.max(1) as usize,
			ctx.status_line(),
		));
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

	fn describe_scene(&mut self, cx: &DescribeContext) -> NativeNode {
		let ctx = self.host.ctx();
		let theme = ctx.theme();
		let confirm = editor_key(&KeybindingsManager::new(), theme, "tui.select.confirm");
		let mut intro = Props::new();
		intro.set_str("wrap", "word");
		let mut list_props = Props::new();
		list_props.set_role("omp.setup.composer.list");
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
		let preview = render_shape_preview(
			&self.current_shape.borrow(),
			cx.cols as usize,
			ctx.status_line(),
		);
		let mut preview_props = Props::new();
		preview_props.set_i64("cols", cx.cols as i64);
		let mut col_props = Props::new();
		col_props.set_str("gap", "sm");
		col_props.set_role("omp.setup.composer");
		col(
			vec![
				text(
					NodeText::Spans(vec![span(
						format!(
							"Select a layout; live preview updates below. Press {} to confirm.",
							confirm
						),
						Some("muted"),
					)]),
					intro,
				),
				list(items, list_props, Some("options")),
				col(
					vec![
						text(
							NodeText::Spans(vec![span("Preview:", Some("muted"))]),
							Props::new(),
						),
						ansi(preview.join("\n"), preview_props),
					],
					Props::new(),
				),
			],
			col_props,
		)
	}
}

/// Select and persist the prompt composer layout.
pub const COMPOSER_SETUP_SCENE: SetupScene = SetupScene {
	id: "composer-shape",
	title: "Choose composer shape",
	step: "Composer",
	min_version: 2,
	ansi_only: true,
	should_run: None,
	mount: mount_composer_scene,
};

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn scene_gate_and_geometry() {
		let scene = COMPOSER_SETUP_SCENE;
		assert_eq!(scene.id, "composer-shape");
		assert_eq!(scene.step, "Composer");
		assert_eq!(scene.min_version, 2);
		assert!(scene.ansi_only);
		assert!(scene.should_run.is_none());
	}

	#[test]
	fn builtin_shapes_start_with_band() {
		let options = get_composer_shape_options();
		assert!(!options.is_empty());
		assert_eq!(options[0].value, "band");
		assert!(options.iter().all(|option| !option.label.is_empty()));
	}

	#[test]
	fn preview_paints_a_prompt_row() {
		let lines = render_shape_preview("band", 60, None);
		assert!(lines.iter().any(|line| line.contains("❯")));
	}
}
