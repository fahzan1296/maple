//! Reference: `l/src/setup/scenes/glyph.ts`.
//!
//! Preview and persist the terminal glyph preset.
//!
//! Deviations from the TypeScript original:
//! - `theme` is `host.ctx().theme()`; `getSymbolPreset`/`setSymbolPreset` become
//!   [`SetupHost::symbol_preset`] / [`SetupHost::preview_symbol_preset`] /
//!   [`SetupHost::save_symbol_preset`].
//! - `TERMINAL.glyphProtocol` becomes [`SetupHost::glyph_protocol`].
//! - `WizardStep` has no crate equivalent: intro and list render directly.
//! - `SelectList` has no native description, so [`describe_scene`] builds the
//!   `List`/`Item` nodes.
//! - The async preview race (`#previewRequest`) is gone: the preview applies
//!   synchronously.
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
use crate::theme::symbols::SymbolPreset;

const INTRO: &str = "If a row shows boxes, tofu, or misaligned icons, pick another.";

const GLYPH_PRESETS: [SymbolPreset; 3] = [
	SymbolPreset::Nerd,
	SymbolPreset::Unicode,
	SymbolPreset::Ascii,
];

const GLYPH_LABELS: [&str; 3] = ["Nerd Font", "Unicode", "ASCII"];

const GLYPH_SAMPLES: [&str; 3] = [
	"\u{f00c}  \u{e0b0}  \u{e019}  \u{f010b}  \u{e729}",
	"✔  ✖  📁  ⬢  ╭─╮  ├─  •  ⠋  →",
	"[+]  [-]  >  +  [D]  +-+  |-  *  ->",
];

fn glyph_items() -> Vec<SelectItem> {
	GLYPH_PRESETS
		.iter()
		.enumerate()
		.map(|(index, preset)| {
			let description = if *preset == SymbolPreset::Nerd {
				format!("{}  ╭─╮  ├─  ◆  ✔  ✖", GLYPH_SAMPLES[index])
			} else {
				GLYPH_SAMPLES[index].to_string()
			};
			SelectItem::new(preset.as_str().to_string(), format!("{}  {}", index + 1, GLYPH_LABELS[index]))
				.with_description(description)
		})
		.collect()
}

/// Mount the glyph scene.
pub fn mount_glyph_scene(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	Box::new(GlyphSceneController::new(host))
}

struct GlyphSceneController {
	host: Rc<dyn SetupSceneHost>,
	select_list: SelectList,
	committing: Rc<Cell<bool>>,
}

impl GlyphSceneController {
	fn new(host: Box<dyn SetupSceneHost>) -> Self {
		let host: Rc<dyn SetupSceneHost> = Rc::from(host);
		let ctx = host.ctx();
		let mut select_list = SelectList::new(
			glyph_items(),
			GLYPH_PRESETS.len(),
			SelectListTheme::default(),
		);
		let current = GLYPH_PRESETS
			.iter()
			.position(|preset| *preset == ctx.symbol_preset())
			.unwrap_or(0);
		select_list.set_selected_index(current as isize);
		let preview_host = host.clone();
		select_list.on_selection_change(move |item| {
			if let Some(preset) = GLYPH_PRESETS
				.iter()
				.find(|preset| preset.as_str() == item.key)
				.copied()
			{
				preview_host.ctx().preview_symbol_preset(preset);
				preview_host.request_render();
			}
		});
		let committing = Rc::new(Cell::new(false));
		let commit_host = host.clone();
		let commit_flag = committing.clone();
		select_list.on_select(move |item| {
			if commit_flag.get() {
				return;
			}
			commit_flag.set(true);
			if let Some(preset) = GLYPH_PRESETS
				.iter()
				.find(|preset| preset.as_str() == item.key)
				.copied()
			{
				commit_host.ctx().save_symbol_preset(preset);
			}
			commit_host.request_render();
			commit_host.finish(SetupSceneResult::Done);
		});
		let cancel_host = host.clone();
		select_list.on_cancel(move || cancel_host.finish(SetupSceneResult::Skipped));
		GlyphSceneController {
			host,
			select_list,
			committing,
		}
	}
}

impl Component for GlyphSceneController {
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
			Key::Char('3') => Some(2isize),
			_ => None,
		};
		if let Some(index) = quick {
			self.select_list.set_selected_index(index);
			if let Some(preset) = GLYPH_PRESETS.get(index as usize).copied() {
				self.host.ctx().preview_symbol_preset(preset);
			}
			self.host.request_render();
			return;
		}
		self.select_list.handle_input(key);
	}

	fn invalidate(&mut self) {
		self.select_list.invalidate();
	}
}

impl SetupSceneController for GlyphSceneController {
	fn title(&self) -> &str {
		"Choose glyph mode"
	}

	fn subtitle(&self) -> Option<&str> {
		Some("Pick the row that renders cleanly in your terminal.")
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
		list_props.set_role("omp.setup.glyph.list");
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
		col_props.set_role("omp.setup.glyph");
		col(
			vec![
				text(NodeText::Spans(vec![span(INTRO, Some("muted"))]), intro),
				list(items, list_props, Some("options")),
			],
			col_props,
		)
	}
}

/// Preview and persist the terminal glyph preset. Skipped once the Glyph
/// Protocol handshake confirmed the terminal renders omp's bundled icons.
pub const GLYPH_SETUP_SCENE: SetupScene = SetupScene {
	id: "glyph-mode",
	title: "Choose glyph mode",
	step: "Symbols",
	min_version: 1,
	ansi_only: true,
	should_run: Some(|ctx| !ctx.glyph_protocol()),
	mount: mount_glyph_scene,
};

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn glyph_items_cover_every_preset_in_order() {
		let items = glyph_items();
		assert_eq!(items.len(), 3);
		assert_eq!(items[0].key, "nerd");
		assert_eq!(items[1].key, "unicode");
		assert_eq!(items[2].key, "ascii");
		assert!(items[0].title.starts_with("1"));
		assert!(items[2].title.starts_with("3"));
	}

	#[test]
	fn scene_gate_and_geometry() {
		let scene = GLYPH_SETUP_SCENE;
		assert_eq!(scene.id, "glyph-mode");
		assert_eq!(scene.step, "Symbols");
		assert_eq!(scene.min_version, 1);
		assert!(scene.ansi_only);
		assert!(scene.should_run.is_some());
	}
}
