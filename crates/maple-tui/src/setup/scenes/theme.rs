//! Reference: `l/src/setup/scenes/theme.ts`.
//!
//! Preview and persist the terminal color theme.
//!
//! Deviations from the TypeScript original:
//! - `theme.ts` singletons are reached through [`SetupHostContext`] methods
//!   (`current_theme_name`, `available_themes`, `preview_theme`,
//!   `enable_auto_theme`, `is_light_theme`, `save_theme`, `save_symbol_preset`,
//!   `save_color_blind_mode`) and the host's [`Theme`].
//! - `getSelectListTheme` maps to the crate's default select-list theme; the
//!   list is a small local [`ThemeList`] because `SelectList` callbacks are
//!   `'static` and cannot borrow the scene.
//! - `WizardStep`/`Memo` are omitted; intro, preview, list, and status render
//!   directly.
//! - `previewTheme` has no `ephemeral` flag here; preview is always ephemeral
//!   and `commit` persists.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::chrome::keybinding_hints::editor_key;
use crate::component::Component;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::describe::{col, item, list, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::text_shape::pad_to_width;
use crate::setup::scenes::types::{
	SetupHost, SetupScene, SetupSceneController, SetupSceneHost, SetupSceneResult, SgrMouseEvent,
	StyledLine,
};
use crate::text_shape::visible_width;
use crate::theme::symbols::SymbolPreset;
use crate::theme::Theme;

/// The built-in quick picks, mirroring `CURATED_ITEMS`.
const CURATED_ITEMS: &[(&str, &str, &str)] = &[
	("auto", "Match terminal", "Titanium in dark terminals, Light in light terminals"),
	("theme:titanium", "Titanium", "Default dark theme"),
	("theme:light", "Light", "Default light theme"),
	("colorblind", "Colorblind colors", "Adjust red/green contrast"),
	("ansi", "ANSI-safe", "ASCII glyphs with the dark terminal theme"),
	("browse", "Browse all…", "Show every built-in and custom theme"),
];

const THEME_PREVIEW_ROLE: &str = "omp.setup.theme.preview";
const THEME_ROLE: &str = "omp.setup.theme";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemeMode {
	Curated,
	All,
}

fn fill_styled_line(content: &str, width: usize) -> String {
	let mut out = String::from(content);
	let visible = visible_width(content);
	if width > visible {
		out.push_str(&" ".repeat(width - visible));
	}
	out
}

fn truncate(line: &str, width: usize) -> String {
	if line.chars().count() > width {
		line.chars().take(width).collect()
	} else {
		line.to_string()
	}
}

/// `theme.fg`, falling back to the unstyled text when the token is unknown.
fn fg(theme: &Theme, color: &str, text: &str) -> String {
	theme.fg(color, text).unwrap_or_else(|_| text.to_string())
}

/// `theme.bg`, falling back to the unstyled text when the token is unknown.
fn bg(theme: &Theme, background: &str, text: &str) -> String {
	theme.bg(background, text).unwrap_or_else(|_| text.to_string())
}

fn render_mock_status_line(theme: &Theme, width: u16) -> String {
	let width = width as usize;
	let sep = fg(theme, "statusLineSep", &format!(" {} ", theme.sep().pipe));
	let left = format!(
		"{}{}{}{}",
		fg(theme, "statusLineModel", &format!("{} sonnet", theme.icon().model)),
		sep,
		fg(theme, "statusLinePath", "~/project"),
		fg(theme, "statusLineGitDirty", &format!("{} main +2", theme.icon().git)),
	);
	let right = format!(
		"{}{}{}",
		fg(theme, "statusLineContext", &format!("{} 42%", theme.icon().context)),
		sep,
		fg(theme, "statusLineCost", &format!("{} 0.18", theme.icon().cost)),
	);
	let inner_width = width.saturating_sub(2).max(1);
	let gap = " ".repeat(
		inner_width
			.saturating_sub(visible_width(&left))
			.saturating_sub(visible_width(&right))
			.saturating_sub(2)
			.max(1),
	);
	let padded = pad_to_width(&format!(" {left}{gap}{right} "), width);
	bg(theme, "statusLineBg", &padded)
}

fn render_mock_editor(theme: &Theme, width: u16) -> Vec<String> {
	let box_round = &theme.box_round();
	let inner_width = (width as usize).saturating_sub(2).max(1);
	let horizontal: String = box_round.horizontal.repeat(inner_width);
	let top = fg(
		theme,
		"borderAccent",
		&format!("{}{}{}", box_round.top_left, horizontal, box_round.top_right),
	);
	let bottom = fg(
		theme,
		"borderMuted",
		&format!("{}{}{}", box_round.bottom_left, horizontal, box_round.bottom_right),
	);
	let prompt = format!(
		"{} {}{}",
		fg(theme, "accent", ">"),
		fg(theme, "text", "Ask anything, edit files, run tools"),
		theme.inverse(" "),
	);
	let hint = fg(
		theme,
		"dim",
		&format!(
			"{} send · {} newline · / commands",
			editor_key(&KeybindingsManager::new(), theme, "tui.input.submit"),
			editor_key(&KeybindingsManager::new(), theme, "tui.input.newLine")
		),
	);
	vec![
		top,
		format!(
			"{}{}{}",
			fg(theme, "borderAccent", &box_round.vertical),
			pad_to_width(&prompt, inner_width),
			fg(theme, "borderAccent", &box_round.vertical)
		),
		format!(
			"{}{}{}",
			fg(theme, "borderMuted", &box_round.vertical),
			fill_styled_line(&hint, inner_width),
			fg(theme, "borderMuted", &box_round.vertical)
		),
		bottom,
	]
}

fn render_theme_preview(theme: &Theme, width: u16) -> Vec<String> {
	let preview_width = (width as usize).clamp(24, 88) as u16;
	let mut lines = vec![
		theme.bold("Preview"),
		format!(
			"{}  {}  {}  {}",
			fg(theme, "success", &format!("{} success", theme.status().success)),
			fg(theme, "warning", &format!("{} warning", theme.status().warning)),
			fg(theme, "error", &format!("{} error", theme.status().error)),
			fg(theme, "accent", "accent"),
		),
		String::new(),
		fg(theme, "muted", "Status line"),
	];
	lines.push(render_mock_status_line(theme, preview_width));
	lines.push(fg(theme, "muted", "Editor"));
	lines.extend(render_mock_editor(theme, preview_width));
	lines
}

fn segment_props(theme: &Theme, icon: &str, text: &str, color: &str, side: &str) -> Props {
	let mut props = Props::new();
	props.set_str("spans", &format!("{icon} {text}"));
	props.set_str("tone", color);
	props.set_str("side", side);
	let _ = theme;
	props
}

fn describe_theme_preview(theme: &Theme) -> NativeNode {
	let mut col_props = Props::new();
	col_props.set_role(THEME_PREVIEW_ROLE);
	let mut children = vec![
		text(NodeText::Spans(vec![span("Preview", Some("strong"))]), Props::new()),
		text(
			NodeText::Spans(vec![
				span(format!("{} success", theme.status().success), Some("success")),
				span("  ", None),
				span(format!("{} warning", theme.status().warning), Some("warning")),
				span("  ", None),
				span(format!("{} error", theme.status().error), Some("error")),
				span("  ", None),
				span("accent", Some("accent")),
			]),
			Props::new(),
		),
		text(NodeText::Spans(vec![span("Status line", Some("muted"))]), Props::new()),
	];
	let mut status_props = Props::new();
	status_props.set_str("kind", "status");
	let segments = vec![
		node(
			"seg",
			segment_props(theme, &theme.icon().model, "sonnet", "statusLineModel", "left"),
			Vec::new(),
			None,
		),
		node(
			"seg",
			segment_props(theme, "", "~/project", "statusLinePath", "left"),
			Vec::new(),
			None,
		),
		node(
			"seg",
			segment_props(theme, &theme.icon().git, "main +2", "statusLineGitDirty", "left"),
			Vec::new(),
			None,
		),
		node(
			"seg",
			segment_props(theme, &theme.icon().context, "42%", "statusLineContext", "right"),
			Vec::new(),
			None,
		),
		node(
			"seg",
			segment_props(theme, &theme.icon().cost, "0.18", "statusLineCost", "right"),
			Vec::new(),
			None,
		),
	];
	children.push(node("status", status_props, segments, None));
	children.push(text(NodeText::Spans(vec![span("Editor", Some("muted"))]), Props::new()));
	let mut input_props = Props::new();
	input_props.set_str("text", "Ask anything, edit files, run tools");
	input_props.set_i64("cursor", "Ask anything, edit files, run tools".len() as i64);
	input_props.set_str("prompt", ">");
	input_props.set_str("readonly", "true");
	children.push(node("input", input_props, Vec::new(), None));
	children.push(text(
		NodeText::Spans(vec![span(
			format!(
				"{} send · {} newline · / commands",
				editor_key(&KeybindingsManager::new(), theme, "tui.input.submit"),
				editor_key(&KeybindingsManager::new(), theme, "tui.input.newLine")
			),
			Some("dim"),
		)]),
		Props::new(),
	));
	col(children, col_props)
}

struct ThemeList {
	items: Vec<(String, String, String)>,
	selected: usize,
	message: Option<String>,
}

enum ListOutcome {
	Moved,
	Confirm,
	Cancel,
}

impl ThemeList {
	fn new(items: Vec<(String, String, String)>, selected: usize) -> Self {
		ThemeList {
			items,
			selected,
			message: None,
		}
	}

	fn selected_value(&self) -> Option<&str> {
		self.items.get(self.selected).map(|item| item.0.as_str())
	}

	fn handle_input(&mut self, key: &Key) -> Option<ListOutcome> {
		match key {
			Key::Up => {
				if self.selected > 0 {
					self.selected -= 1;
				}
				Some(ListOutcome::Moved)
			}
			Key::Down => {
				if self.selected + 1 < self.items.len() {
					self.selected += 1;
				}
				Some(ListOutcome::Moved)
			}
			Key::Enter => Some(ListOutcome::Confirm),
			Key::Esc => Some(ListOutcome::Cancel),
			_ => None,
		}
	}

	fn render(&self, theme: &Theme, width: u16) -> Vec<String> {
		let width = width.max(1) as usize;
		let mut lines = Vec::new();
		if let Some(message) = &self.message {
			lines.push(theme.fg("error", message).unwrap_or_else(|_| message.clone()));
		}
		for (index, (_, label, description)) in self.items.iter().enumerate() {
			let marker = if index == self.selected { "❯" } else { " " };
			let line = format!("{} {}  {}", marker, label, description);
			lines.push(truncate(&line, width));
		}
		lines
	}
}

pub fn mount_theme_scene(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	Box::new(ThemeScene::new(host))
}

struct ThemeScene {
	host: Rc<dyn SetupSceneHost>,
	mode: Cell<ThemeMode>,
	list: RefCell<ThemeList>,
	loading: Cell<bool>,
	message: RefCell<Option<String>>,
	disposed: Cell<bool>,
	original_theme: RefCell<String>,
	original_symbol_preset: Cell<SymbolPreset>,
	original_color_blind: Cell<bool>,
}

impl ThemeScene {
	fn new(host: Box<dyn SetupSceneHost>) -> Self {
		let host: Rc<dyn SetupSceneHost> = Rc::from(host);
		let ctx = host.ctx();
		let original_theme = ctx.current_theme_name().unwrap_or_default();
		let original_symbol_preset = ctx.symbol_preset();
		let original_color_blind = ctx.color_blind_mode();
		let curated = CURATED_ITEMS
			.iter()
			.map(|(value, label, description)| {
				(value.to_string(), label.to_string(), description.to_string())
			})
			.collect();
		let selected = current_curated_index(&original_theme);
		ThemeScene {
			host,
			mode: Cell::new(ThemeMode::Curated),
			list: RefCell::new(ThemeList::new(curated, selected)),
			loading: Cell::new(false),
			message: RefCell::new(None),
			disposed: Cell::new(false),
			original_theme: RefCell::new(original_theme),
			original_symbol_preset: Cell::new(original_symbol_preset),
			original_color_blind: Cell::new(original_color_blind),
		}
	}

	fn apply_presentation(&self, symbol_preset: SymbolPreset, color_blind: bool) {
		let ctx = self.host.ctx();
		ctx.save_symbol_preset(symbol_preset);
		ctx.save_color_blind_mode(color_blind);
	}

	fn preview(&self, value: &str) {
		*self.message.borrow_mut() = None;
		let ctx = self.host.ctx();
		if value == "browse" {
			self.host.request_render();
			return;
		}
		let symbol = self.original_symbol_preset.get();
		let blind = self.original_color_blind.get();
		let mut failed: Option<String> = None;
		match value {
			"auto" => {
				self.apply_presentation(symbol, blind);
				ctx.enable_auto_theme();
			}
			"colorblind" => self.apply_presentation(symbol, true),
			"ansi" => {
				self.apply_presentation(SymbolPreset::Ascii, blind);
				if let Err(error) = ctx.preview_theme("dark-terminal") {
					failed = Some(error);
				}
			}
			_ => {
				if let Some(name) = value.strip_prefix("theme:") {
					self.apply_presentation(symbol, blind);
					if let Err(error) = ctx.preview_theme(name) {
						failed = Some(error);
					}
				}
			}
		}
		if let Some(error) = failed {
			*self.message.borrow_mut() = Some(error);
		}
		self.host.request_render();
	}

	fn commit(&self, value: &str) {
		let ctx = self.host.ctx();
		let symbol = self.original_symbol_preset.get();
		let blind = self.original_color_blind.get();
		match value {
			"auto" => {
				ctx.save_theme("dark", "titanium");
				ctx.save_theme("light", "light");
				self.apply_presentation(symbol, blind);
				ctx.enable_auto_theme();
			}
			"colorblind" => {
				ctx.save_color_blind_mode(true);
				self.apply_presentation(symbol, true);
			}
			"ansi" => {
				ctx.save_symbol_preset(SymbolPreset::Ascii);
				ctx.save_theme("dark", "dark-terminal");
				self.apply_presentation(SymbolPreset::Ascii, blind);
				ctx.enable_auto_theme();
			}
			_ => {
				if let Some(name) = value.strip_prefix("theme:") {
					self.apply_presentation(symbol, blind);
					if ctx.is_light_theme(name) {
						ctx.save_theme("light", name);
					} else {
						ctx.save_theme("dark", name);
					}
					let _ = ctx.preview_theme(name);
				}
			}
		}
	}

	fn show_all_themes(&self) {
		if self.loading.get() {
			return;
		}
		self.loading.set(true);
		*self.message.borrow_mut() = None;
		self.host.request_render();
		let ctx = self.host.ctx();
		let themes = ctx.available_themes();
		if self.disposed.get() {
			return;
		}
		let original = self.original_theme.borrow().clone();
		let items: Vec<(String, String, String)> = themes
			.iter()
			.map(|name| {
				let description = if *name == original {
					"current".to_string()
				} else {
					String::new()
				};
				(format!("theme:{}", name), name.clone(), description)
			})
			.collect();
		let selected = themes.iter().position(|name| *name == original).unwrap_or(0);
		self.mode.set(ThemeMode::All);
		*self.list.borrow_mut() = ThemeList::new(items, selected);
		self.loading.set(false);
		self.host.request_render();
	}

	fn restore_preview(&self) {
		let symbol = self.original_symbol_preset.get();
		let blind = self.original_color_blind.get();
		self.apply_presentation(symbol, blind);
		let original = self.original_theme.borrow().clone();
		if !original.is_empty() {
			let _ = self.host.ctx().preview_theme(&original);
		}
		self.host.request_render();
	}

	fn status_lines(&self) -> Vec<StyledLine> {
		match &*self.message.borrow() {
			Some(message) => vec![StyledLine::new(message.clone(), "error")],
			None => Vec::new(),
		}
	}
}

fn current_curated_index(current: &str) -> usize {
	match current {
		"titanium" => 1,
		"light" => 2,
		_ => 0,
	}
}

impl Component for ThemeScene {
	fn render(&mut self, width: u16) -> Vec<String> {
		let theme = self.host.ctx().theme();
		let mut lines = vec![
			theme
				.fg(
					"muted",
					"Theme changes preview live. Nothing is saved until you press Enter.",
				)
				.unwrap_or_default(),
			theme
				.fg(
					"dim",
					if self.mode.get() == ThemeMode::All {
						"Browsing all themes · Esc returns to curated choices"
					} else {
						"Esc skips this step"
					},
				)
				.unwrap_or_default(),
			String::new(),
		];
		lines.extend(render_theme_preview(theme, width));
		lines.push(String::new());
		lines.extend(self.list.borrow().render(theme, width));
		lines.extend(self.status_lines().into_iter().map(|line| line.text));
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		if let Key::Char(c) = key {
			if let Some(digit) = c.to_digit(10) {
				if digit >= 1 && digit <= 9 {
					self.list.borrow_mut().selected = (digit - 1) as usize;
					if let Some(value) = self.list.borrow().selected_value().map(str::to_string) {
						self.preview(&value);
					}
					return;
				}
			}
		}
		match self.list.borrow_mut().handle_input(key) {
			Some(ListOutcome::Moved) => {
				if let Some(value) = self.list.borrow().selected_value().map(str::to_string) {
					self.preview(&value);
				}
				self.host.request_render();
			}
			Some(ListOutcome::Confirm) => {
				let value = self
					.list
					.borrow()
					.selected_value()
					.map(str::to_string)
					.unwrap_or_default();
				if value == "browse" {
					self.show_all_themes();
					return;
				}
				self.commit(&value);
				self.host.finish(SetupSceneResult::Done);
			}
			Some(ListOutcome::Cancel) => {
				if self.mode.get() == ThemeMode::All {
					self.mode.set(ThemeMode::Curated);
					let curated = CURATED_ITEMS
						.iter()
						.map(|(value, label, description)| {
							(value.to_string(), label.to_string(), description.to_string())
						})
						.collect();
					let original = self.original_theme.borrow().clone();
					*self.list.borrow_mut() =
						ThemeList::new(curated, current_curated_index(&original));
					self.host.request_render();
					return;
				}
				self.restore_preview();
				self.host.finish(SetupSceneResult::Skipped);
			}
			None => {}
		}
	}

	fn invalidate(&mut self) {}

	fn dispose(&mut self) {
		self.disposed.set(true);
	}
}

impl SetupSceneController for ThemeScene {
	fn title(&self) -> &str {
		"Pick a theme"
	}

	fn subtitle(&self) -> Option<&str> {
		Some("Move through the list to preview; Enter saves the highlighted choice.")
	}

	fn render_body(&mut self, width: u16, max_lines: Option<usize>) -> Vec<String> {
		let mut lines = self.render(width);
		if let Some(max_lines) = max_lines {
			lines.truncate(max_lines);
		}
		lines
	}

	fn route_mouse(&mut self, event: &SgrMouseEvent, _line: i32, _col: i32) {
		if let Some(wheel) = event.wheel {
			match wheel.cmp(&0) {
				std::cmp::Ordering::Less => {
					self.list.borrow_mut().handle_input(&Key::Up);
				}
				std::cmp::Ordering::Greater => {
					self.list.borrow_mut().handle_input(&Key::Down);
				}
				std::cmp::Ordering::Equal => {}
			}
			self.host.request_render();
		}
	}

	fn routes_mouse(&self) -> bool {
		true
	}

	fn describe_scene(&mut self, cx: &DescribeContext) -> NativeNode {
		let theme = self.host.ctx().theme();
		let mut children: Vec<NativeNode> = Vec::new();
		let mut intro_props = Props::new();
		intro_props.set_str("gap", "sm");
		children.push(col(
			vec![
				text(
					NodeText::Spans(vec![span(
						"Theme changes preview live. Nothing is saved until you press Enter.",
						Some("muted"),
					)]),
					Props::new(),
				),
				text(
					NodeText::Spans(vec![span(
						if self.mode.get() == ThemeMode::All {
							"Browsing all themes · Esc returns to curated choices"
						} else {
							"Esc skips this step"
						},
						Some("dim"),
					)]),
					Props::new(),
				),
			],
			intro_props,
		));
		children.push(describe_theme_preview(theme));
		if self.loading.get() {
			let mut props = Props::new();
			props.set_str("label", "Loading themes…");
			props.set_str("tone", "dim");
			children.push(node("spinner", props, Vec::new(), Some("loading")));
		} else {
			let items: Vec<NativeNode> = self
				.list
				.borrow()
				.items
				.iter()
				.map(|(value, label, description)| {
					let mut props = Props::new();
					props.set_str("title", label);
					if !description.is_empty() {
						props.set_str("description", description);
					}
					item(value.clone(), props)
				})
				.collect();
			let mut list_props = Props::new();
			list_props.set_role("omp.setup.theme.list");
			children.push(list(items, list_props, Some("themes")));
		}
		for status in self.status_lines() {
			children.push(text(
				NodeText::Spans(vec![span(status.text, Some(status.color.as_str()))]),
				Props::new(),
			));
		}
		let mut col_props = Props::new();
		col_props.set_str("gap", "sm");
		col_props.set_role(THEME_ROLE);
		let _ = cx;
		col(children, col_props)
	}
}

/// Preview and persist the terminal color theme.
pub const THEME_SETUP_SCENE: SetupScene = SetupScene {
	id: "theme",
	title: "Pick a theme",
	step: "Theme",
	min_version: 1,
	ansi_only: true,
	should_run: None,
	mount: mount_theme_scene,
};

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn scene_gate_and_geometry() {
		let scene = THEME_SETUP_SCENE;
		assert_eq!(scene.id, "theme");
		assert_eq!(scene.step, "Theme");
		assert_eq!(scene.min_version, 1);
		assert!(scene.ansi_only);
	}

	#[test]
	fn curated_index_matches_current_theme() {
		assert_eq!(current_curated_index("titanium"), 1);
		assert_eq!(current_curated_index("light"), 2);
		assert_eq!(current_curated_index("other"), 0);
	}

	#[test]
	fn list_moves_and_confirms() {
		let mut list = ThemeList::new(
			vec![
				("a".to_string(), "A".to_string(), String::new()),
				("b".to_string(), "B".to_string(), String::new()),
			],
			0,
		);
		list.handle_input(&Key::Down);
		assert_eq!(list.selected_value(), Some("b"));
		assert!(matches!(list.handle_input(&Key::Enter), Some(ListOutcome::Confirm)));
		assert!(matches!(list.handle_input(&Key::Esc), Some(ListOutcome::Cancel)));
	}
}
