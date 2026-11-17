//! Extension dashboard (fullscreen control center).
//!
//! Reference: `l/src/overlays/extensions/extension-dashboard.ts`.

use std::sync::Arc;

use crate::component::Component;
use crate::components::tab_bar::{Tab, TabBar, TabBarTheme};
use crate::keys::{is_arrow_left, is_arrow_right, is_ctrl_c, is_enter, is_escape, is_shift_tab, is_tab, Key};
use crate::keybinding_matchers::{matches_select_page_down, matches_select_page_up};
use crate::keybindings::{key_id, KeybindingsManager};
use crate::render::render_utils::replace_tabs;
use crate::text_shape::{truncate_to_width, Ellipsis};

use super::extension_list::{ExtensionList, ExtensionListCallbacks};
use super::inspector_panel::InspectorPanel;
use super::state_manager::{
	apply_disabled_extensions_to_state, apply_filter, create_initial_state, is_tab_switch_key,
	switch_to_tab, ALL_TAB_ID,
};
use super::types::{DashboardState, Extension, ExtensionProvider, ProviderTab};

/// Runtime operations owned by the embedding application.
pub trait ExtensionDashboardRuntime: Send {
	fn load_extensions(&self, disabled_ids: &[String]) -> Vec<Extension>;
	fn get_providers(&self) -> Vec<ExtensionProvider>;
	fn get_disabled_extensions(&self) -> Vec<String>;
	fn toggle_provider(&mut self, provider_id: &str) -> Option<bool>;
	fn set_provider_user_source_enabled(&mut self, provider_id: &str, enabled: bool) -> Option<bool>;
	fn toggle_extension(&mut self, extension_id: &str, enabled: bool) -> Option<bool>;
	fn keybindings(&self) -> Option<&KeybindingsManager> {
		None
	}
}

/// Dashboard construction options (dependencies injected).
pub struct ExtensionDashboardOptions<R: ExtensionDashboardRuntime> {
	pub runtime: R,
	pub terminal_height: Option<usize>,
}

const DASHBOARD_TITLE: &str = "Extension Control Center";

fn ext_footer() -> String {
	"Tab/←→ switch scope  Space/Enter toggle  Ctrl+O expand  Esc close".to_string()
}

fn provider_initials(label: &str) -> String {
	let letters: String = label.chars().filter(|c| c.is_alphanumeric()).take(2).collect();
	if letters.is_empty() {
		label.chars().take(2).collect()
	} else if letters.chars().count() == 1 {
		let mut out = letters;
		if let Some(ch) = label.chars().nth(1) {
			out.push(ch);
		}
		out
	} else {
		letters
	}
}

/// Map dashboard provider tabs to tab-bar tabs.
pub fn build_tab_bar_tabs(tabs: &[ProviderTab]) -> Vec<Tab> {
	let all = tabs.iter().find(|t| t.id == ALL_TAB_ID);
	let mut out = Vec::with_capacity(tabs.len());
	if let Some(tab) = all {
		out.push(Tab::new(tab.id.clone(), format!("{} ({})", tab.label, tab.count)));
	}
	for tab in tabs.iter().filter(|t| t.id != ALL_TAB_ID) {
		let mark = provider_initials(&tab.label);
		let label = format!("[{}] {} ({})", mark, tab.label, tab.count);
		let short = format!("[{}] {}", mark, tab.label);
		let mut built = Tab::new(tab.id.clone(), label).with_short(short);
		if tab.enabled && tab.count == 0 {
			built = built.muted();
		}
		out.push(built);
	}
	out
}

fn is_expand_key(key: &Key, kb: Option<&KeybindingsManager>) -> bool {
	if let Some(kb) = kb {
		crate::keybinding_matchers::matches_app_tools_expand(key, kb)
	} else {
		crate::keys::is_ctrl_o(key)
	}
}

fn is_interrupt_key(key: &Key, kb: Option<&KeybindingsManager>) -> bool {
	if let Some(kb) = kb {
		crate::keybinding_matchers::matches_app_interrupt(key, kb)
	} else {
		is_escape(key)
	}
}

fn key_str(key: &Key) -> String {
	key_id(key).unwrap_or_default()
}

/// Fullscreen extension control center.
pub struct ExtensionDashboard<R: ExtensionDashboardRuntime> {
	runtime: R,
	state: DashboardState,
	tab_bar: TabBar,
	main_list: ExtensionList,
	inspector: InspectorPanel,
	terminal_height: usize,
	closed: bool,
}

impl<R: ExtensionDashboardRuntime> ExtensionDashboard<R> {
	pub fn new(options: ExtensionDashboardOptions<R>) -> Self {
		let terminal_height = options.terminal_height.unwrap_or(24);
		let runtime = options.runtime;
		let disabled_ids = runtime.get_disabled_extensions();
		let providers = runtime.get_providers();
		let extensions = runtime.load_extensions(&disabled_ids);
		let state = create_initial_state(extensions, &providers);
		let mut tab_bar = TabBar::new(String::new(), build_tab_bar_tabs(&state.tabs), TabBarTheme::default());
		tab_bar.set_show_hint(false);
		if let Some(active_id) = state.tabs.get(state.active_tab_index).map(|t| t.id.clone()) {
			tab_bar.set_selected_by_id(&active_id);
		}

		let runtime_providers = providers.clone();
		let callbacks = ExtensionListCallbacks {
			get_providers: Some(Arc::new(move || runtime_providers.clone())),
			..Default::default()
		};
		let mut main_list = ExtensionList::new(state.search_filtered.clone(), callbacks, 12);
		main_list.set_focused(true);
		let mut inspector = InspectorPanel::new(None);
		if let Some(selected) = state.selected.clone() {
			inspector.set_extension(Some(selected));
		}

		let mut dashboard = Self {
			runtime,
			state,
			tab_bar,
			main_list,
			inspector,
			terminal_height,
			closed: false,
		};
		dashboard.sync_active_provider();
		dashboard
	}

	pub fn is_closed(&self) -> bool {
		self.closed
	}

	fn close(&mut self) {
		self.closed = true;
	}

	fn terminal_rows(&self) -> usize {
		self.terminal_height.max(14)
	}

	fn active_provider_id(&self) -> Option<String> {
		self.state
			.tabs
			.get(self.state.active_tab_index)
			.and_then(|t| if t.id == ALL_TAB_ID { None } else { Some(t.id.clone()) })
	}

	fn sync_active_provider(&mut self) {
		self.main_list.set_master_switch_provider(self.active_provider_id());
	}

	fn select_provider_by_id(&mut self, tab_id: &str) {
		if let Some(index) = self.state.tabs.iter().position(|t| t.id == tab_id) {
			self.state.active_tab_index = index;
			self.state = switch_to_tab(self.state.clone(), tab_id.to_string());
			self.main_list.set_extensions(self.state.search_filtered.clone());
			self.tab_bar.set_selected_by_id(tab_id);
			self.sync_active_provider();
			self.inspector.set_extension(self.state.selected.clone());
		}
	}

	fn refresh_from_runtime(&mut self) {
		let current_tab_id = self
			.state
			.tabs
			.get(self.state.active_tab_index)
			.map(|t| t.id.clone());
		let disabled = self.runtime.get_disabled_extensions();
		let providers = self.runtime.get_providers();
		let extensions = self.runtime.load_extensions(&disabled);
		self.state = create_initial_state(extensions, &providers);
		if let Some(id) = current_tab_id {
			if let Some(index) = self.state.tabs.iter().position(|t| t.id == id) {
				self.state.active_tab_index = index;
				self.state = switch_to_tab(self.state.clone(), id);
			}
		}
		self.tab_bar = TabBar::new(String::new(), build_tab_bar_tabs(&self.state.tabs), TabBarTheme::default());
		self.tab_bar.set_show_hint(false);
		if let Some(active_id) = self.state.tabs.get(self.state.active_tab_index).map(|t| t.id.clone()) {
			self.tab_bar.set_selected_by_id(&active_id);
		}
		self.main_list.set_extensions(self.state.search_filtered.clone());
		self.sync_active_provider();
		self.inspector.set_extension(self.state.selected.clone());
	}

	fn handle_toggle_extension(&mut self, extension_id: &str, enabled: bool) {
		let _ = self.runtime.toggle_extension(extension_id, enabled);
		let disabled = self.runtime.get_disabled_extensions();
		let providers = self.runtime.get_providers();
		self.state = apply_disabled_extensions_to_state(self.state.clone(), &disabled, &providers);
		self.main_list.set_extensions(self.state.search_filtered.clone());
		self.inspector.set_extension(self.state.selected.clone());
	}

	fn handle_provider_toggle(&mut self, provider_id: &str) {
		let _ = self.runtime.toggle_provider(provider_id);
		self.refresh_from_runtime();
	}

	fn handle_user_source_toggle(&mut self, provider_id: &str) {
		let current = self
			.runtime
			.get_providers()
			.into_iter()
			.find(|p| p.id == provider_id)
			.map(|p| p.user_source_enabled)
			.unwrap_or(false);
		let _ = self.runtime.set_provider_user_source_enabled(provider_id, !current);
		self.refresh_from_runtime();
	}

	fn clear_search(&mut self) {
		self.state.search_query.clear();
		self.state.search_filtered = self.state.tab_filtered.clone();
		self.main_list.set_extensions(self.state.search_filtered.clone());
		self.main_list.clear_search();
	}

	fn render_body(&mut self, width: u16, height: usize) -> Vec<String> {
		let left_w = ((width as usize).saturating_sub(1) * 45 / 100).max(24) as u16;
		let right_w = (width as usize).saturating_sub(left_w as usize + 1).max(10) as u16;
		let left = self.main_list.render(left_w);
		let right = self.inspector.render(right_w, height);
		let mut lines = Vec::with_capacity(height);
		for i in 0..height {
			let l = left.get(i).cloned().unwrap_or_default();
			let r = right.get(i).cloned().unwrap_or_default();
			lines.push(format!(
				"{}│{}",
				truncate_to_width(&l, left_w as usize, Ellipsis::Unicode),
				truncate_to_width(&r, right_w as usize, Ellipsis::Unicode)
			));
		}
		lines
	}

	fn sync_search_from_list(&mut self) {
		let query = self.main_list.search_query().to_string();
		if query != self.state.search_query {
			self.state.search_query = query.clone();
			self.state.search_filtered = apply_filter(&self.state.tab_filtered, &query);
			self.main_list.set_extensions(self.state.search_filtered.clone());
		}
		self.state.selected = self.main_list.get_selected_extension();
		self.inspector.set_extension(self.state.selected.clone());
	}
}

impl<R: ExtensionDashboardRuntime> Component for ExtensionDashboard<R> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let height = self.terminal_rows();
		let inner_w = width.saturating_sub(4).max(1);
		let tabs = self.tab_bar.render(inner_w);
		let fixed_rows = 1 + tabs.len() + 1 + 1 + 1 + 1; // top + tabs + div + div + footer + bottom
		let content_rows = height.saturating_sub(fixed_rows).max(5);
		self.main_list.set_max_visible(content_rows.saturating_sub(2).max(3));

		let mut out = Vec::with_capacity(height);
		let title = truncate_to_width(DASHBOARD_TITLE, width.saturating_sub(5) as usize, Ellipsis::Unicode);
		out.push(format!("╭─ {} ", title));
		for line in tabs {
			let clean = replace_tabs(&line);
			out.push(format!(
				"│ {} │",
				truncate_to_width(&clean, inner_w as usize, Ellipsis::Unicode)
			));
		}
		out.push(format!("├{}┤", "─".repeat(width.saturating_sub(2) as usize)));
		for line in self.render_body(inner_w, content_rows) {
			out.push(format!(
				"│ {} │",
				truncate_to_width(&line, inner_w as usize, Ellipsis::Unicode)
			));
		}
		out.push(format!("├{}┤", "─".repeat(width.saturating_sub(2) as usize)));
		let footer = ext_footer();
		out.push(format!(
			"│ {} │",
			truncate_to_width(&footer, inner_w as usize, Ellipsis::Unicode)
		));
		out.push(format!("╰{}╯", "─".repeat(width.saturating_sub(2) as usize)));
		out.truncate(height.max(1));
		out
	}

	fn handle_input(&mut self, key: &Key) {
		let kb = self.runtime.keybindings();

		if is_ctrl_c(key) {
			self.close();
			return;
		}
		if is_interrupt_key(key, kb) {
			if !self.state.search_query.is_empty() {
				self.clear_search();
				return;
			}
			self.close();
			return;
		}
		if is_expand_key(key, kb) {
			self.inspector.toggle_expanded();
			return;
		}
		if let Some(kb) = kb {
			if matches_select_page_up(key, kb) {
				self.inspector.scroll_up(10);
				return;
			}
			if matches_select_page_down(key, kb) {
				self.inspector.scroll_down(10);
				return;
			}
		}

		let k = key_str(key);
		if is_tab(key) || is_shift_tab(key) || is_arrow_left(key) || is_arrow_right(key) || is_tab_switch_key(&k) {
			self.tab_bar.handle_input(key);
			if let Some(active) = self.tab_bar.active_tab() {
				let id = active.id.clone();
				self.select_provider_by_id(&id);
			}
			return;
		}

		// Space/Enter activates toggle on selected row (master/user/ext) like TS.
		if key == &Key::Char(' ') || is_enter(key) {
			if let Some(sel) = self.main_list.get_selected_extension() {
				if let Some(sw) = self.main_list.selected_switch() {
					if sw.blocked.is_none() {
						self.handle_toggle_extension(&sel.id, !sw.on);
						self.sync_search_from_list();
						return;
					}
				}
			} else if let Some(active) = self.active_provider_id() {
				self.handle_provider_toggle(&active);
				return;
			}
		}

		self.main_list.handle_input(key);
		self.sync_search_from_list();
	}

	fn invalidate(&mut self) {
		self.main_list.invalidate();
		self.inspector.invalidate();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::extensions::types::{Extension, ExtensionKind, ExtensionSource, ExtensionState};

	struct MockRuntime {
		extensions: Vec<Extension>,
		providers: Vec<ExtensionProvider>,
		disabled: Vec<String>,
		toggled: Vec<(String, bool)>,
	}

	impl ExtensionDashboardRuntime for MockRuntime {
		fn load_extensions(&self, _disabled_ids: &[String]) -> Vec<Extension> {
			self.extensions.clone()
		}
		fn get_providers(&self) -> Vec<ExtensionProvider> {
			self.providers.clone()
		}
		fn get_disabled_extensions(&self) -> Vec<String> {
			self.disabled.clone()
		}
		fn toggle_provider(&mut self, provider_id: &str) -> Option<bool> {
			for p in &mut self.providers {
				if p.id == provider_id {
					p.enabled = !p.enabled;
					return Some(p.enabled);
				}
			}
			None
		}
		fn set_provider_user_source_enabled(&mut self, provider_id: &str, enabled: bool) -> Option<bool> {
			for p in &mut self.providers {
				if p.id == provider_id {
					p.user_source_enabled = enabled;
					return Some(enabled);
				}
			}
			None
		}
		fn toggle_extension(&mut self, extension_id: &str, enabled: bool) -> Option<bool> {
			self.toggled.push((extension_id.to_string(), enabled));
			Some(enabled)
		}
	}

	fn ext(id: &str) -> Extension {
		Extension {
			id: id.to_string(),
			name: id.to_string(),
			display_name: id.to_string(),
			kind: ExtensionKind::Skill,
			state: ExtensionState::Active,
			path: "/tmp/x".to_string(),
			source: ExtensionSource {
				provider: "skill".to_string(),
				provider_name: "Skills".to_string(),
				level: "user".to_string(),
			},
			..Extension::default()
		}
	}

	#[test]
	fn builds_tabs_with_all_first() {
		let tabs = vec![
			ProviderTab { id: "skill".into(), label: "Skills".into(), count: 1, enabled: true },
			ProviderTab { id: "all".into(), label: "All".into(), count: 2, enabled: true },
		];
		let built = build_tab_bar_tabs(&tabs);
		assert_eq!(built[0].id, "all");
		assert!(built[1].label.contains("Skills"));
	}

	#[test]
	fn dashboard_renders_title_and_footer() {
		let rt = MockRuntime {
			extensions: vec![ext("a")],
			providers: vec![ExtensionProvider {
				id: "skill".into(),
				display_name: "Skills".into(),
				enabled: true,
				foreign_user_source: false,
				user_source_enabled: false,
			}],
			disabled: vec![],
			toggled: vec![],
		};
		let mut dash = ExtensionDashboard::new(ExtensionDashboardOptions {
			runtime: rt,
			terminal_height: Some(24),
		});
		let out = dash.render(100).join("\n");
		assert!(out.contains(DASHBOARD_TITLE));
		assert!(out.contains("switch scope"));
	}

	#[test]
	fn escape_closes_when_no_search_query() {
		let rt = MockRuntime {
			extensions: vec![ext("a")],
			providers: vec![ExtensionProvider {
				id: "skill".into(),
				display_name: "Skills".into(),
				enabled: true,
				foreign_user_source: false,
				user_source_enabled: false,
			}],
			disabled: vec![],
			toggled: vec![],
		};
		let mut dash = ExtensionDashboard::new(ExtensionDashboardOptions {
			runtime: rt,
			terminal_height: Some(24),
		});
		assert!(!dash.is_closed());
		dash.handle_input(&Key::Esc);
		assert!(dash.is_closed());
	}
}
