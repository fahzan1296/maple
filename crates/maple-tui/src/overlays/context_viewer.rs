//! Real `ContextViewer` overlay port.
//!
//! Reference: `l/src/overlays/context-viewer.ts` (`ContextViewer`, overview →
//! category previews → entry detail states, selection/scroll/navigation, native
//! describe tree).
//!
//! Deviations: TUI widgets (`OverlayPanel`, `MenuSelection`, `ScrollView`,
//! `PanelRows`), ANSI/native theme spans, and host viewport/clock services are
//! abstracted. This port owns view-state navigation, overview/category/entry
//! selection, scroll offsets, entry body/preview caches, and bordered render +
//! native rows describe. `context_viewer_format`/`context_viewer_overview`
//! helpers supply the data/model primitives.

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::mouse::{MouseEvent, MouseKind};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::overlays::context_viewer_format::safe_text;
use std::collections::HashMap;

/// Minimal context-view data model used by the viewer.
///
/// Kept local to avoid coupling to a host-specific model type; callers can map
/// from their concrete snapshot into this structure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextViewCategory {
	pub id: String,
	pub label: String,
	pub tokens: usize,
	pub color: Option<String>,
	pub entries: Vec<ContextViewEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextViewEntry {
	pub id: String,
	pub label: String,
	pub content: String,
	pub thinking_group: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextViewData {
	pub model_label: Option<String>,
	pub context_window: usize,
	pub used_tokens: usize,
	pub categories: Vec<ContextViewCategory>,
	pub notices: Vec<String>,
	pub snapcompact_lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextViewerState {
	Overview,
	Category,
	Entry,
}

/// Host callbacks for viewer lifecycle.
pub trait ContextViewerCallbacks {
	fn on_close(&self);
	fn request_render(&self) {}
}

/// Read-only context snapshot viewer.
pub struct ContextViewer<C: ContextViewerCallbacks> {
	callbacks: C,
	pub data: ContextViewData,
	pub state: ContextViewerState,
	pub overview_index: usize,
	pub category_index: Option<usize>,
	pub entry_index: usize,
	pub entry_parent: ContextViewerState,
	pub overview_scroll: usize,
	pub entry_scroll: usize,
	pub body_rows: usize,
	pub body_width: usize,
	entry_body_cache: HashMap<String, String>,
	theme: OverlayTheme,
}

impl<C: ContextViewerCallbacks> std::fmt::Debug for ContextViewer<C> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ContextViewer")
			.field("state", &self.state)
			.field("overview_index", &self.overview_index)
			.field("category_index", &self.category_index)
			.field("entry_index", &self.entry_index)
			.finish_non_exhaustive()
	}
}

impl<C: ContextViewerCallbacks> ContextViewer<C> {
	#[must_use]
	pub fn new(data: ContextViewData, callbacks: C) -> Self {
		Self {
			callbacks,
			data,
			state: ContextViewerState::Overview,
			overview_index: 0,
			category_index: None,
			entry_index: 0,
			entry_parent: ContextViewerState::Overview,
			overview_scroll: 0,
			entry_scroll: 0,
			body_rows: 12,
			body_width: 80,
			entry_body_cache: HashMap::new(),
			theme: OverlayTheme::default(),
		}
	}
	/// The host callbacks this viewer was built with.
	pub fn callbacks(&self) -> &C {
		&self.callbacks
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	fn overview_rows(&self) -> Vec<String> {
		self.data
			.categories
			.iter()
			.map(|c| format!("{} ({} tokens)", c.label, c.tokens))
			.collect()
	}

	fn selected_category(&self) -> Option<&ContextViewCategory> {
		self.category_index.and_then(|i| self.data.categories.get(i)).or_else(|| {
			// overview selection maps by index into categories when aligned
			self.data.categories.get(self.overview_index)
		})
	}

	fn selected_entry(&self) -> Option<&ContextViewEntry> {
		let cat = self.selected_category()?;
		cat.entries.get(self.entry_index)
	}

	fn open_selected_overview(&mut self) {
		if self.data.categories.is_empty() {
			return;
		}
		self.category_index = Some(self.overview_index.min(self.data.categories.len() - 1));
		self.entry_index = 0;
		self.entry_scroll = 0;
		let cat = self.selected_category();
		if cat.map(|c| c.entries.len() == 1).unwrap_or(false) {
			self.entry_parent = ContextViewerState::Overview;
			self.state = ContextViewerState::Entry;
		} else {
			self.state = ContextViewerState::Category;
		}
	}

	fn open_selected_entry(&mut self) {
		if self.selected_entry().is_some() {
			self.entry_parent = ContextViewerState::Category;
			self.entry_scroll = 0;
			self.state = ContextViewerState::Entry;
		}
	}

	fn back(&mut self) -> bool {
		match self.state {
			ContextViewerState::Overview => false,
			ContextViewerState::Category => {
				self.state = ContextViewerState::Overview;
				self.category_index = None;
				true
			}
			ContextViewerState::Entry => {
				self.state = self.entry_parent;
				if self.entry_parent == ContextViewerState::Overview {
					self.category_index = None;
				}
				true
			}
		}
	}

	fn render_overview(&self, width: usize) -> Vec<String> {
		let mut lines = Vec::new();
		let window = self.data.context_window;
		let used = self.data.used_tokens;
		let percent = if window == 0 { 0.0 } else { used as f64 / window as f64 * 100.0 };
		lines.push(format!("{used} of {window} tokens · {percent:.1}%"));
		lines.push(format!("{} free", window.saturating_sub(used)));
		if let Some(model) = &self.data.model_label {
			lines.push(format!("Model: {}", safe_text(model)));
		}
		lines.push(String::new());
		let rows = self.overview_rows();
		for (i, r) in rows.iter().enumerate() {
			let cursor = if i == self.overview_index { "›" } else { " " };
			lines.push(format!("{cursor} {r}"));
		}
		for n in &self.data.notices {
			lines.push(format!("note: {n}"));
		}
		for n in &self.data.snapcompact_lines {
			lines.push(n.clone());
		}
		let maxw = width.max(10);
		lines.into_iter().map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l }).collect()
	}

	fn render_category(&self, width: usize) -> Vec<String> {
		let Some(cat) = self.selected_category() else {
			return vec!["No category".to_string()];
		};
		let mut lines = vec![format!("{}  ({} entries)", cat.label, cat.entries.len())];
		for (i, e) in cat.entries.iter().enumerate() {
			let cursor = if i == self.entry_index { "›" } else { " " };
			lines.push(format!("{cursor} {}", e.label));
			if let Some(p) = e.content.lines().next() {
				lines.push(format!("  {}", safe_text(p)));
			}
		}
		let maxw = width.max(10);
		lines.into_iter().map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l }).collect()
	}

	fn render_entry(&self, width: usize) -> Vec<String> {
		let Some(cat) = self.selected_category() else {
			return vec!["No category".to_string()];
		};
		let Some(entry) = self.selected_entry() else {
			return vec!["No entry".to_string()];
		};
		let mut lines = vec![format!("[{}]", entry.label)];
		if let Some(group) = entry.thinking_group.as_deref() {
			lines.push(format!("  · {group}"));
		}
		lines.push(String::new());
		let body = self
			.entry_body_cache
			.get(&entry.id)
			.cloned()
			.unwrap_or_else(|| entry.content.trim_end().to_string());
		lines.extend(body.lines().skip(self.entry_scroll).take(self.body_rows.max(6)).map(str::to_string));
		let maxw = width.max(10);
		lines.push(format!("— {} —", cat.label));
		lines.into_iter().map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l }).collect()
	}

	fn render_body(&self, width: usize) -> Vec<String> {
		match self.state {
			ContextViewerState::Overview => self.render_overview(width),
			ContextViewerState::Category => self.render_category(width),
			ContextViewerState::Entry => self.render_entry(width),
		}
	}
}

impl<C: ContextViewerCallbacks> Component for ContextViewer<C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.body_width = width as usize;
		let inner_w = width.saturating_sub(4) as usize;
		let title = match self.state {
			ContextViewerState::Overview => "Context",
			ContextViewerState::Category => self.selected_category().map(|c| c.label.as_str()).unwrap_or("Context"),
			ContextViewerState::Entry => "Context entry",
		};
		let mut out = vec![top_border(width, title, &self.theme)];
		for l in self.render_body(inner_w.max(10)).into_iter().take(self.body_rows.max(8) + 6) {
			out.push(row(&l, width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		let hint = match self.state {
			ContextViewerState::Overview => "↑/↓ select · enter open · esc close",
			ContextViewerState::Category => "↑/↓ select · enter open · esc back",
			ContextViewerState::Entry => "↑/↓ scroll · esc back",
		};
		out.push(row(hint, width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}

	fn handle_input(&mut self, key: &Key) {
		if keys::is_escape(key) {
			if !self.back() {
				self.callbacks.on_close();
			}
			self.callbacks.request_render();
			return;
		}
		match self.state {
			ContextViewerState::Overview => match key {
				Key::Up | Key::Char('k') => {
					self.overview_index = self.overview_index.saturating_sub(1);
				}
				Key::Down | Key::Char('j') => {
					if !self.data.categories.is_empty() {
						self.overview_index = (self.overview_index + 1).min(self.data.categories.len() - 1);
					}
				}
				Key::Enter => self.open_selected_overview(),
				_ => {}
			},
			ContextViewerState::Category => match key {
				Key::Up | Key::Char('k') => {
					self.entry_index = self.entry_index.saturating_sub(1);
				}
				Key::Down | Key::Char('j') => {
					if let Some(cat) = self.selected_category() {
						if !cat.entries.is_empty() {
							self.entry_index = (self.entry_index + 1).min(cat.entries.len() - 1);
						}
					}
				}
				Key::Enter => self.open_selected_entry(),
				_ => {}
			},
			ContextViewerState::Entry => match key {
				Key::Up | Key::Char('k') => self.entry_scroll = self.entry_scroll.saturating_sub(1),
				Key::Down | Key::Char('j') => self.entry_scroll = self.entry_scroll.saturating_add(1),
				Key::PageUp => self.entry_scroll = self.entry_scroll.saturating_sub(self.body_rows.max(1)),
				Key::PageDown => self.entry_scroll = self.entry_scroll.saturating_add(self.body_rows.max(1)),
				Key::Home | Key::Char('g') => self.entry_scroll = 0,
				Key::End | Key::Char('G') => self.entry_scroll = self.entry_scroll.saturating_add(10_000),
				_ => {}
			},
		}
		self.callbacks.request_render();
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		match event.kind {
			MouseKind::ScrollUp => self.handle_input(&Key::Up),
			MouseKind::ScrollDown => self.handle_input(&Key::Down),
			_ => {}
		}
	}
}

impl<C: ContextViewerCallbacks> NativeDescribe for ContextViewer<C> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let inner_w = cx.cols.saturating_sub(4) as usize;
		let lines = self.render_body(inner_w.max(10));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("state", serde_json::Value::String(format!("{:?}", self.state)));
		props.insert("overviewIndex", serde_json::Value::from(self.overview_index as i64));
		props.insert("entryIndex", serde_json::Value::from(self.entry_index as i64));
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

	struct Cb { closed: RefCell<bool> }
	impl ContextViewerCallbacks for Cb {
		fn on_close(&self) { *self.closed.borrow_mut() = true; }
		fn request_render(&self) {}
	}

	fn sample_data() -> ContextViewData {
		ContextViewData {
			model_label: Some("m".into()),
			context_window: 1000,
			used_tokens: 200,
			categories: vec![
				ContextViewCategory {
					id: "sys".into(),
					label: "System".into(),
					tokens: 100,
					color: None,
					entries: vec![ContextViewEntry { id: "e1".into(), label: "prompt".into(), content: "hello\nworld".into(), thinking_group: None }],
				},
				ContextViewCategory {
					id: "msgs".into(),
					label: "Messages".into(),
					tokens: 100,
					color: None,
					entries: vec![
						ContextViewEntry { id: "m1".into(), label: "u1".into(), content: "a".into(), thinking_group: None },
						ContextViewEntry { id: "m2".into(), label: "a1".into(), content: "b".into(), thinking_group: None },
					],
				},
			],
			notices: vec![],
			snapcompact_lines: vec![],
		}
	}

	#[test]
	fn context_viewer_navigates_overview_category_entry() {
		let mut v = ContextViewer::new(sample_data(), Cb { closed: RefCell::new(false) });
		let lines = v.render(80);
		assert!(lines.iter().any(|l| l.contains("Context")));
		v.handle_input(&Key::Down); // messages
		v.handle_input(&Key::Enter); // category
		assert_eq!(v.state, ContextViewerState::Category);
		v.handle_input(&Key::Enter); // entry
		assert_eq!(v.state, ContextViewerState::Entry);
		v.handle_input(&Key::Down);
		assert!(v.entry_scroll >= 1);
		let node = v.describe_native(&DescribeContext { cols: 60, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
		v.handle_input(&Key::Esc); // back to category
		assert_eq!(v.state, ContextViewerState::Category);
	}

	#[test]
	fn context_viewer_close_from_overview() {
		let vcb = Cb { closed: RefCell::new(false) };
		let mut v = ContextViewer::new(sample_data(), vcb);
		v.handle_input(&Key::Esc);
		assert!(*v.callbacks.closed.borrow());
	}
}
