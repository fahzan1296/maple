//! Extension list (inventory with master switch and fuzzy search).
//!
//! Reference: `l/src/overlays/extensions/extension-list.ts`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::component::Component;
use crate::components::input::Input;
use crate::keys::{is_arrow_down, is_arrow_up, is_enter, Key};
use crate::text_shape::{truncate_to_width, Ellipsis};

use super::display_text::sanitize_display_line;
use super::inspector_model::{
	join_list_hints, live_tools_for_extension, project_list_hint,
};
use super::mcp_runtime::{
	snapshot_mcp_runtime, McpConnectionHealth, McpRuntimeOpts, McpRuntimeSource, McpServerDisplay,
};
use super::types::{
	is_shadowed_extension, Extension, ExtensionKind, ExtensionProvider, ExtensionState,
};

/// Callbacks provided by the embedding application/dashboard.
#[derive(Default, Clone)]
pub struct ExtensionListCallbacks {
	pub on_selection_change: Option<Arc<dyn Fn(Option<Extension>) + Send + Sync>>,
	pub on_toggle: Option<Arc<dyn Fn(&str, bool) + Send + Sync>>,
	pub on_master_toggle: Option<Arc<dyn Fn(&str) + Send + Sync>>,
	pub on_user_source_toggle: Option<Arc<dyn Fn(&str) + Send + Sync>>,
	pub get_providers: Option<Arc<dyn Fn() -> Vec<ExtensionProvider> + Send + Sync>>,
	pub tool_source: Option<Arc<dyn super::inspector_model::ToolRuntimeSource + Send + Sync>>,
	pub mcp_source: Option<Arc<dyn McpRuntimeSource + Send + Sync>>,
}

const DEFAULT_MAX_VISIBLE: usize = 15;

/// Picker/list view of the inventory (semantic equivalent of TS pickerView).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionListPickerView {
	pub order_ids: Vec<String>,
	pub selected: Option<String>,
	pub hits: HashMap<String, Vec<(usize, usize)>>,
	pub grouped: bool,
}

/// Selected row switch state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionListSwitch {
	pub on: bool,
	pub blocked: Option<String>,
}

/// Flattened list item for rendering/navigation.
#[derive(Debug, Clone, PartialEq)]
pub enum ListItem {
	Master {
		provider_id: String,
		provider_name: String,
		enabled: bool,
	},
	UserSource {
		provider_id: String,
		provider_name: String,
		enabled: bool,
	},
	KindHeader {
		kind: ExtensionKind,
		label: String,
		icon: String,
		count: usize,
	},
	Extension {
		item: Extension,
	},
}

impl ListItem {
	pub fn key(&self) -> String {
		match self {
			Self::Master { provider_id, .. } => format!("master:{provider_id}"),
			Self::UserSource { provider_id, .. } => format!("user-source:{provider_id}"),
			Self::KindHeader { kind, .. } => format!("kind-header:{:?}", kind),
			Self::Extension { item } => format!("extension:{}:{}", item.id, item.path),
		}
	}

	pub fn native_id(&self) -> String {
		stable_key(&self.key())
	}

	pub fn search_text(&self) -> String {
		match self {
			Self::Master { provider_name, .. } => format!("Enable {provider_name} Master Switch"),
			Self::UserSource { provider_name, .. } => {
				format!("Load ~/ {provider_name} config opt-in project config always loads")
			}
			Self::KindHeader { label, count, .. } => format!("{label} {count}"),
			Self::Extension { item } => {
				let mut text = item.display_name.clone();
				if let Some(trigger) = &item.trigger {
					text.push(' ');
					text.push_str(trigger);
				}
				text.push(' ');
				text.push_str(&item.id);
				text.push(' ');
				text.push_str(kind_label(item.kind));
				text
			}
		}
	}
}

fn stable_key(input: &str) -> String {
	let mut hash: u64 = 0xcbf29ce484222325;
	for b in input.as_bytes() {
		hash ^= u64::from(*b);
		hash = hash.wrapping_mul(0x100000001b3);
	}
	format!("{hash:016x}")
}

fn kind_label(kind: ExtensionKind) -> &'static str {
	match kind {
		ExtensionKind::ExtensionModule => "Extension Modules",
		ExtensionKind::Skill => "Skills",
		ExtensionKind::Tool => "Tools",
		ExtensionKind::SlashCommand => "Commands",
		ExtensionKind::Rule => "Rules",
		ExtensionKind::Mcp => "MCP Servers",
		ExtensionKind::Hook => "Hooks",
		ExtensionKind::Prompt => "Prompts",
		ExtensionKind::ContextFile => "Context",
		ExtensionKind::Instruction => "Instructions",
	}
}

fn kind_icon(kind: ExtensionKind) -> &'static str {
	match kind {
		ExtensionKind::ExtensionModule | ExtensionKind::Tool => "🧩",
		ExtensionKind::Skill => "★",
		ExtensionKind::SlashCommand => "⌘",
		ExtensionKind::Rule => "§",
		ExtensionKind::Mcp => "⛁",
		ExtensionKind::Hook => "⚓",
		ExtensionKind::Prompt => "❝",
		ExtensionKind::ContextFile => "▣",
		ExtensionKind::Instruction => "i",
	}
}

fn fuzzy_contains(haystack: &str, needle: &str) -> bool {
	if needle.is_empty() {
		return true;
	}
	let h: Vec<char> = haystack.to_lowercase().chars().collect();
	let n: Vec<char> = needle.to_lowercase().chars().collect();
	let mut i = 0usize;
	for ch in n {
		let mut found = false;
		while i < h.len() {
			if h[i] == ch {
				i += 1;
				found = true;
				break;
			}
			i += 1;
		}
		if !found {
			return false;
		}
	}
	true
}

fn fuzzy_indices(haystack: &str, needle: &str) -> Option<Vec<usize>> {
	if needle.is_empty() {
		return Some(Vec::new());
	}
	let h_chars: Vec<char> = haystack.chars().collect();
	let n_chars: Vec<char> = needle.chars().collect();
	let mut out = Vec::new();
	let mut i = 0usize;
	for nc in n_chars {
		let mut found = None;
		while i < h_chars.len() {
			if h_chars[i].to_lowercase().next() == nc.to_lowercase().next() {
				found = Some(i);
				i += 1;
				break;
			}
			i += 1;
		}
		let idx = found?;
		out.push(idx);
	}
	Some(out)
}

fn ranges_from_indices(indices: &[usize]) -> Vec<(usize, usize)> {
	if indices.is_empty() {
		return Vec::new();
	}
	let mut out = Vec::new();
	let mut start = indices[0];
	let mut end = indices[0] + 1;
	for &i in &indices[1..] {
		if i == end {
			end += 1;
		} else {
			out.push((start, end));
			start = i;
			end = i + 1;
		}
	}
	out.push((start, end));
	out
}

/// Inventory list component.
pub struct ExtensionList {
	extensions: Vec<Extension>,
	callbacks: ExtensionListCallbacks,
	max_visible: usize,
	master_switch_provider: Option<String>,
	search: Input,
	items: Vec<ListItem>,
	visible: Vec<ListItem>,
	query: String,
	selected_index: usize,
	scroll_offset: usize,
	focused: bool,
	hovered_index: Option<usize>,
	visible_count: usize,
}

impl ExtensionList {
	pub fn new(extensions: Vec<Extension>, callbacks: ExtensionListCallbacks, max_visible: usize) -> Self {
		let mut list = Self {
			extensions,
			callbacks,
			max_visible: max_visible.max(1),
			master_switch_provider: None,
			search: Input::new(),
			items: Vec::new(),
			visible: Vec::new(),
			query: String::new(),
			selected_index: 0,
			scroll_offset: 0,
			focused: false,
			hovered_index: None,
			visible_count: 0,
		};
		list.rebuild_items();
		list
	}

	pub fn set_max_visible(&mut self, max_visible: usize) {
		self.max_visible = max_visible.max(1);
		self.sync_scroll();
	}

	pub fn set_extensions(&mut self, extensions: Vec<Extension>) {
		self.extensions = extensions;
		let keep = self.selected_index;
		self.rebuild_items();
		if !self.visible.is_empty() {
			self.selected_index = keep.min(self.visible.len() - 1);
		} else {
			self.selected_index = 0;
		}
		self.sync_scroll();
	}

	pub fn set_focused(&mut self, focused: bool) {
		self.focused = focused;
	}

	pub fn set_master_switch_provider(&mut self, provider_id: Option<String>) {
		self.master_switch_provider = provider_id;
		self.rebuild_items();
	}

	pub fn search_query(&self) -> &str {
		&self.query
	}

	pub fn set_search_query(&mut self, query: &str) {
		if self.search.value() != query {
			self.search.set_value(query);
		}
		self.query = query.to_string();
		self.rebuild_items();
		self.scroll_offset = 0;
		self.selected_index = 0;
		self.notify_selection_change();
	}

	pub fn clear_search(&mut self) {
		self.set_search_query("");
	}

	pub fn reset_selection(&mut self) {
		self.selected_index = 0;
		self.scroll_offset = 0;
		self.notify_selection_change();
	}

	pub fn get_selected_extension(&self) -> Option<Extension> {
		match self.selected_item() {
			Some(ListItem::Extension { item }) => Some(item.clone()),
			_ => None,
		}
	}

	pub fn selected_switch(&self) -> Option<ExtensionListSwitch> {
		match self.selected_item()? {
			ListItem::KindHeader { .. } => None,
			ListItem::Master { enabled, .. } => Some(ExtensionListSwitch { on: *enabled, blocked: None }),
			ListItem::UserSource { provider_id, enabled, .. } => {
				let provider_off = self.provider_enabled(provider_id).is_some_and(|on| !on);
				if provider_off {
					Some(ExtensionListSwitch {
						on: *enabled,
						blocked: Some("Provider disabled".to_string()),
					})
				} else {
					Some(ExtensionListSwitch { on: *enabled, blocked: None })
				}
			}
			ListItem::Extension { item } => {
				let on = item.state != ExtensionState::Disabled;
				if is_shadowed_extension(item) {
					return Some(ExtensionListSwitch {
						on,
						blocked: Some("Shadowed by another source".to_string()),
					});
				}
				if self.is_master_disabled() {
					return Some(ExtensionListSwitch {
						on,
						blocked: Some("Provider disabled".to_string()),
					});
				}
				Some(ExtensionListSwitch { on, blocked: None })
			}
		}
	}

	pub fn picker_view(&self) -> ExtensionListPickerView {
		let mut order_ids = Vec::new();
		let mut hits = HashMap::new();
		let mut grouped = false;
		for item in &self.visible {
			if matches!(item, ListItem::KindHeader { .. }) {
				grouped = true;
				continue;
			}
			let id = item.native_id();
			if !self.query.is_empty() {
				if let Some(indices) = fuzzy_indices(&item.search_text(), &self.query) {
					hits.insert(id.clone(), ranges_from_indices(&indices));
				}
			}
			order_ids.push(id);
		}
		let selected = self.selected_item().and_then(|item| {
			if matches!(item, ListItem::KindHeader { .. }) {
				None
			} else {
				Some(item.native_id())
			}
		});
		ExtensionListPickerView {
			order_ids,
			selected,
			hits,
			grouped,
		}
	}

	pub fn hit_test(&self, line: usize) -> Option<usize> {
		let row_line = line.checked_sub(2)?; // search + separator
		if row_line >= self.visible_count {
			return None;
		}
		let index = self.scroll_offset + row_line;
		(index < self.visible.len()).then_some(index)
	}

	pub fn handle_click(&mut self, line: usize) -> bool {
		let Some(index) = self.hit_test(line) else {
			return false;
		};
		if index == self.selected_index {
			self.activate_selected();
			return true;
		}
		self.selected_index = index;
		self.sync_scroll();
		self.notify_selection_change();
		true
	}

	pub fn handle_wheel(&mut self, delta: i32) {
		if delta < 0 {
			let _ = self.move_selection_up();
		} else if delta > 0 {
			let _ = self.move_selection_down();
		}
	}

	pub fn activate_selected(&mut self) {
		let Some(item) = self.selected_item().cloned() else {
			return;
		};
		match item {
			ListItem::Master { provider_id, .. } => {
				if let Some(cb) = &self.callbacks.on_master_toggle {
					cb(&provider_id);
				}
			}
			ListItem::UserSource { provider_id, .. } => {
				if self.provider_enabled(&provider_id).unwrap_or(true) {
					if let Some(cb) = &self.callbacks.on_user_source_toggle {
						cb(&provider_id);
					}
				}
			}
			ListItem::Extension { item } => {
				if is_shadowed_extension(&item) || self.is_master_disabled() {
					return;
				}
				let new_enabled = item.state == ExtensionState::Disabled;
				if let Some(cb) = &self.callbacks.on_toggle {
					cb(&item.id, new_enabled);
				}
			}
			ListItem::KindHeader { .. } => {}
		}
	}

	fn selected_item(&self) -> Option<&ListItem> {
		self.visible.get(self.selected_index)
	}

	fn providers(&self) -> Vec<ExtensionProvider> {
		self.callbacks
			.get_providers
			.as_ref()
			.map(|f| f())
			.unwrap_or_default()
	}

	fn provider_enabled(&self, provider_id: &str) -> Option<bool> {
		self.providers()
			.into_iter()
			.find(|p| p.id == provider_id)
			.map(|p| p.enabled)
	}

	fn is_master_disabled(&self) -> bool {
		self.master_switch_provider
			.as_deref()
			.and_then(|id| self.provider_enabled(id))
			.is_some_and(|enabled| !enabled)
	}

	fn rebuild_items(&mut self) {
		self.items = self.build_list_items();
		if self.query.is_empty() {
			self.visible = self.items.clone();
		} else {
			self.visible = self
				.items
				.iter()
				.filter(|item| fuzzy_contains(&item.search_text(), &self.query))
				.cloned()
				.collect();
		}
		if self.selected_index >= self.visible.len() {
			self.selected_index = self.visible.len().saturating_sub(1);
		}
		self.sync_scroll();
	}

	fn build_list_items(&self) -> Vec<ListItem> {
		let mut items = Vec::new();
		if !self.query.is_empty() {
			for ext in &self.extensions {
				if fuzzy_contains(&ext_search_text(ext), &self.query) {
					items.push(ListItem::Extension { item: ext.clone() });
				}
			}
			return items;
		}

		if let Some(provider_id) = &self.master_switch_provider {
			let provider_name = self
				.extensions
				.first()
				.map(|e| e.source.provider_name.clone())
				.or_else(|| {
					self.providers()
						.into_iter()
						.find(|p| &p.id == provider_id)
						.map(|p| p.display_name)
				})
				.unwrap_or_else(|| provider_id.clone());
			let provider = self.providers().into_iter().find(|p| &p.id == provider_id);
			let enabled = provider.as_ref().map(|p| p.enabled).unwrap_or(true);
			items.push(ListItem::Master {
				provider_id: provider_id.clone(),
				provider_name: provider_name.clone(),
				enabled,
			});
			if let Some(p) = provider {
				if p.foreign_user_source {
					items.push(ListItem::UserSource {
						provider_id: provider_id.clone(),
						provider_name,
						enabled: p.user_source_enabled,
					});
				}
			}
			for ext in &self.extensions {
				items.push(ListItem::Extension { item: ext.clone() });
			}
			return items;
		}

		let mut by_kind: HashMap<ExtensionKind, Vec<Extension>> = HashMap::new();
		for ext in &self.extensions {
			by_kind.entry(ext.kind).or_default().push(ext.clone());
		}
		for kind in [
			ExtensionKind::ExtensionModule,
			ExtensionKind::Skill,
			ExtensionKind::Tool,
			ExtensionKind::SlashCommand,
			ExtensionKind::Rule,
			ExtensionKind::Mcp,
			ExtensionKind::Hook,
			ExtensionKind::Prompt,
			ExtensionKind::ContextFile,
			ExtensionKind::Instruction,
		] {
			let Some(kind_items) = by_kind.remove(&kind) else {
				continue;
			};
			if kind_items.is_empty() {
				continue;
			}
			items.push(ListItem::KindHeader {
				kind,
				label: kind_label(kind).to_string(),
				icon: kind_icon(kind).to_string(),
				count: kind_items.len(),
			});
			for ext in kind_items {
				items.push(ListItem::Extension { item: ext });
			}
		}
		items
	}

	fn sync_scroll(&mut self) {
		if self.visible.is_empty() {
			self.scroll_offset = 0;
			return;
		}
		if self.selected_index < self.scroll_offset {
			self.scroll_offset = self.selected_index;
		} else if self.selected_index >= self.scroll_offset + self.max_visible {
			self.scroll_offset = self.selected_index + 1 - self.max_visible;
		}
		let max_off = self.visible.len().saturating_sub(self.max_visible);
		self.scroll_offset = self.scroll_offset.min(max_off);
	}

	fn notify_selection_change(&self) {
		if let Some(cb) = &self.callbacks.on_selection_change {
			cb(self.get_selected_extension());
		}
	}

	fn move_selection_up(&mut self) -> bool {
		if self.selected_index == 0 {
			return false;
		}
		self.selected_index -= 1;
		self.sync_scroll();
		self.notify_selection_change();
		true
	}

	fn move_selection_down(&mut self) -> bool {
		if self.visible.is_empty() || self.selected_index + 1 >= self.visible.len() {
			return false;
		}
		self.selected_index += 1;
		self.sync_scroll();
		self.notify_selection_change();
		true
	}

	fn render_master_switch(&self, provider_name: &str, enabled: bool, is_selected: bool, width: u16) -> String {
		let checkbox = if enabled { "[x]" } else { "[ ]" };
		let mut line = format!(" {checkbox}  Enable {provider_name}  (Master Switch)");
		if is_selected {
			line = format!(">{}", &line[1..]);
		}
		truncate_to_width(&line, width as usize, Ellipsis::Unicode)
	}

	fn render_user_source_switch(
		&self,
		provider_name: &str,
		enabled: bool,
		is_selected: bool,
		_master_disabled: bool,
		width: u16,
	) -> String {
		let checkbox = if enabled { "[x]" } else { "[ ]" };
		let mut line = format!(" {checkbox}  Load ~/ {provider_name} config  (opt-in; project config always loads)");
		if is_selected {
			line = format!(">{}", &line[1..]);
		}
		truncate_to_width(&line, width as usize, Ellipsis::Unicode)
	}

	fn render_kind_header(&self, label: &str, icon: &str, count: usize, is_selected: bool, width: u16) -> String {
		let mut line = format!("  {icon} {label} ({count})");
		if is_selected {
			line = format!(">{}", &line[1..]);
		}
		truncate_to_width(&line, width as usize, Ellipsis::Unicode)
	}

	/// Runtime/project hints shown after the row trigger.
	fn row_hint(&self, ext: &Extension) -> Option<String> {
		join_list_hints(&[self.runtime_hint(ext), project_list_hint(ext)])
	}

	fn runtime_hint(&self, ext: &Extension) -> Option<String> {
		match ext.kind {
			ExtensionKind::Tool => {
				let tool_source = self
					.callbacks
					.tool_source
					.as_deref()
					.map(|source| source as &dyn super::inspector_model::ToolRuntimeSource);
				let lives = live_tools_for_extension(ext, tool_source);
				if lives.is_empty() {
					return None;
				}
				if lives.iter().any(|live| live.hidden == Some(true)) {
					return Some("hidden".to_string());
				}
				(lives.len() > 1).then(|| format!("{} tools", lives.len()))
			}
			ExtensionKind::Mcp => {
				if is_shadowed_extension(ext) {
					return None;
				}
				let server = McpServerDisplay::from_value(&ext.raw)?;
				let snap = snapshot_mcp_runtime(
					&server,
					self.callbacks
						.mcp_source
						.as_deref()
						.map(|source| source as &dyn McpRuntimeSource),
					Some(McpRuntimeOpts {
						enabled: Some(ext.state != ExtensionState::Disabled),
						shadowed: false,
					}),
				);
				match snap.health {
					McpConnectionHealth::Connected => {
						let mut parts = Vec::new();
						if !snap.tools.is_empty() {
							parts.push(format!("{} tools", snap.tools.len()));
						}
						if !snap.resources.is_empty() {
							parts.push(format!("{} resource", snap.resources.len()));
						}
						(!parts.is_empty()).then(|| parts.join(" · "))
					}
					McpConnectionHealth::Disconnected => Some("unavailable".to_string()),
					_ => None,
				}
			}
			ExtensionKind::Skill => {
				let hidden = ext
					.raw
					.get("frontmatter")
					.and_then(|v| v.get("hide"))
					.and_then(|v| v.as_bool())
					== Some(true)
					|| ext.raw.get("hidden").and_then(|v| v.as_bool()) == Some(true);
				hidden.then(|| "hidden".to_string())
			}
			_ => None,
		}
	}

	fn render_extension_row(&self, ext: &Extension, is_selected: bool, master_disabled: bool, width: u16) -> String {
		let shadowed = is_shadowed_extension(ext);
		let effectively_disabled = master_disabled || ext.state == ExtensionState::Disabled;
		let icon = if effectively_disabled {
			"○"
		} else if shadowed {
			"◐"
		} else if ext.state == ExtensionState::Active {
			"●"
		} else {
			"○"
		};
		let mut line = format!("   {icon} {}", sanitize_display_line(&ext.display_name));
		if let Some(trigger) = ext
			.trigger
			.as_deref()
			.and_then(|t| super::display_text::sanitize_display_line_field(Some(t)))
		{
			line.push_str("  ");
			line.push_str(&trigger);
		}
		if shadowed {
			line.push_str("  (shadowed)");
		}
		if let Some(hint) = self.row_hint(ext) {
			line.push_str("  ");
			line.push_str(&hint);
		}
		if is_selected {
			line = format!(">{}", &line[1..]);
		}
		truncate_to_width(&line, width as usize, Ellipsis::Unicode)
	}
}

impl Component for ExtensionList {
	fn render(&mut self, width: u16) -> Vec<String> {
		let mut lines = Vec::new();
		self.visible_count = 0;
		let search_prefix = "Search: ";
		let query = self.search.value();
		let search_line = if query.is_empty() {
			format!("{search_prefix}type to filter")
		} else {
			format!("{search_prefix}{query}")
		};
		lines.push(truncate_to_width(&search_line, width as usize, Ellipsis::Unicode));
		lines.push(String::new());

		if self.visible.is_empty() {
			lines.push(truncate_to_width("  No extensions found for this provider.", width as usize, Ellipsis::Unicode));
			return lines;
		}

		let master_disabled = self.is_master_disabled();
		let end = (self.scroll_offset + self.max_visible).min(self.visible.len());
		for (idx, item) in self.visible.iter().enumerate().skip(self.scroll_offset).take(end.saturating_sub(self.scroll_offset)) {
			let is_selected = self.focused && idx == self.selected_index;
			let row = match item {
				ListItem::Master { provider_name, enabled, .. } => {
					self.render_master_switch(provider_name, *enabled, is_selected, width)
				}
				ListItem::UserSource { provider_name, enabled, .. } => {
					self.render_user_source_switch(provider_name, *enabled, is_selected, master_disabled, width)
				}
				ListItem::KindHeader { label, icon, count, .. } => {
					self.render_kind_header(label, icon, *count, is_selected, width)
				}
				ListItem::Extension { item } => self.render_extension_row(item, is_selected, master_disabled, width),
			};
			lines.push(row);
		}
		self.visible_count = lines.len().saturating_sub(2);
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		if is_arrow_up(key) {
			let _ = self.move_selection_up();
			return;
		}
		if is_arrow_down(key) {
			let _ = self.move_selection_down();
			return;
		}
		if key == &Key::Char(' ') || is_enter(key) {
			self.activate_selected();
			return;
		}

		let before = self.search.value().to_string();
		self.search.handle_input(key);
		let after = self.search.value().to_string();
		if after != before {
			self.query = after;
			self.rebuild_items();
			self.selected_index = 0;
			self.scroll_offset = 0;
			self.notify_selection_change();
		}
	}

	fn invalidate(&mut self) {
		self.visible_count = 0;
	}
}

fn ext_search_text(ext: &Extension) -> String {
	let mut text = ext.display_name.clone();
	if let Some(trigger) = &ext.trigger {
		text.push(' ');
		text.push_str(trigger);
	}
	text.push(' ');
	text.push_str(&ext.id);
	text.push(' ');
	text.push_str(kind_label(ext.kind));
	text
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::extensions::types::{Extension, ExtensionKind, ExtensionSource, ExtensionState};

	fn ext(id: &str, name: &str, kind: ExtensionKind) -> Extension {
		Extension {
			id: id.to_string(),
			name: name.to_string(),
			display_name: name.to_string(),
			kind,
			state: ExtensionState::Active,
			description: None,
			trigger: None,
			path: "/tmp/x".to_string(),
			source: ExtensionSource {
				provider: "skill".to_string(),
				provider_name: "skills".to_string(),
				level: "user".to_string(),
			},
			shadowed_by: None,
			..Extension::default()
		}
	}

	#[test]
	fn builds_kind_grouped_list_in_all_view() {
		let list = ExtensionList::new(
			vec![
				ext("a", "alpha", ExtensionKind::Skill),
				ext("b", "beta", ExtensionKind::Tool),
			],
			ExtensionListCallbacks::default(),
			10,
		);
		let view = list.picker_view();
		assert!(view.grouped);
		assert!(!view.order_ids.is_empty());
	}

	#[test]
	fn search_filters_visible_items() {
		let mut list = ExtensionList::new(
			vec![
				ext("a", "alpha", ExtensionKind::Skill),
				ext("b", "beta", ExtensionKind::Tool),
			],
			ExtensionListCallbacks::default(),
			10,
		);
		list.set_search_query("alp");
		let view = list.picker_view();
		assert_eq!(view.order_ids.len(), 1);
		assert!(!view.grouped);
	}

	#[test]
	fn activate_selected_toggles_extension() {
		use std::sync::{Arc, Mutex};
		let toggled = Arc::new(Mutex::new(None::<(String, bool)>));
		let toggled2 = toggled.clone();
		let callbacks = ExtensionListCallbacks {
			on_toggle: Some(Arc::new(move |id, on| {
				*toggled2.lock().unwrap() = Some((id.to_string(), on));
			})),
			..Default::default()
		};
		let mut list = ExtensionList::new(vec![ext("a", "alpha", ExtensionKind::Skill)], callbacks, 10);
		list.activate_selected();
		assert_eq!(*toggled.lock().unwrap(), Some(("a".to_string(), false)));
	}
}
