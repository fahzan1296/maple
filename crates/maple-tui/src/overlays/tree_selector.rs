//! Session tree selector overlay.
//!
//! Reference: `l/src/overlays/tree-selector.ts`.
//!
//! Deviations:
//! - The TS implementation composes TreeView/SearchLine/LabelInput, outline
//!   targets, copy preview, and a rich native picker. This port provides the
//!   public exports and a behavior-focused selector with filtering, search,
//!   keyboard navigation, label editing, and native description; internals are
//!   simplified to a flat visible-list model.
//! - Session entry shapes are modeled as data structs instead of the full
//!   transcript union; unknown/custom entry payloads are preserved as JSON.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use serde_json::Value;

use crate::component::Component;
use crate::keys::Key;
use crate::native::describe::{col, node, row, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};

/// Available session-tree display filters.
pub const TREE_FILTER_MODES: &[&str] = &["default", "no-tools", "user-only", "labeled-only", "all"];
/// Session-tree display filter.
pub type TreeFilterMode = &'static str;

/// Fields consumed when displaying persisted entries in the session tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionTreeEntry {
	pub id: String,
	pub parent_id: Option<String>,
	/// Entry type (`message`, `compaction`, `custom`, ...).
	pub kind: String,
	/// Role for message entries.
	pub role: Option<String>,
	/// Text/content summary.
	pub text: Option<String>,
	/// Tool call names present on assistant messages.
	pub tool_calls: Vec<String>,
	/// Label attached to the node.
	pub label: Option<String>,
	/// Whether this entry is a user request / prompt.
	pub is_user_request: bool,
	/// Arbitrary extra payload.
	pub extra: Option<Value>,
}

/// Session tree shape consumed by the selector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeSelectorNode {
	pub entry: SessionTreeEntry,
	pub children: Vec<TreeSelectorNode>,
	pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VisibleRow {
	id: String,
	depth: usize,
	label: Option<String>,
	text: String,
	kind: String,
	active: bool,
}

/// Component that renders a session tree selector for navigation.
pub struct TreeSelectorComponent {
	roots: Vec<TreeSelectorNode>,
	all: HashMap<String, TreeSelectorNode>,
	visible: Vec<VisibleRow>,
	selected: usize,
	filter_mode: TreeFilterMode,
	search: String,
	searching: bool,
	label_edit: Option<(String, String)>,
	current_leaf_id: Option<String>,
	on_select: Box<dyn FnMut(String, bool)>,
	on_cancel: Box<dyn FnMut()>,
	on_label_change: Option<Box<dyn FnMut(String, Option<String>)>>,
}

impl std::fmt::Debug for TreeSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TreeSelectorComponent")
			.field("filter_mode", &self.filter_mode)
			.field("search", &self.search)
			.field("selected", &self.selected)
			.field("visible", &self.visible.len())
			.finish()
	}
}

impl TreeSelectorComponent {
	/// Create a new selector.
	pub fn new<F, C>(
		tree: Vec<TreeSelectorNode>,
		current_leaf_id: Option<String>,
		_terminal_height: usize,
		on_select: F,
		on_cancel: C,
		on_label_change: Option<Box<dyn FnMut(String, Option<String>)>>,
		initial_filter_mode: TreeFilterMode,
		_session_name: Option<String>,
	) -> Self
	where
		F: FnMut(String, bool) + 'static,
		C: FnMut() + 'static,
	{
		let mut component = Self {
			all: index_nodes(&tree),
			roots: tree,
			visible: Vec::new(),
			selected: 0,
			filter_mode: normalize_filter_mode(initial_filter_mode),
			search: String::new(),
			searching: false,
			label_edit: None,
			current_leaf_id,
			on_select: Box::new(on_select),
			on_cancel: Box::new(on_cancel),
			on_label_change,
		};
		component.rebuild();
		component
	}

	fn rebuild(&mut self) {
		self.visible = flatten_visible(
			&self.roots,
			self.filter_mode,
			&self.search,
			self.current_leaf_id.as_deref(),
		);
		if self.visible.is_empty() {
			self.selected = 0;
			return;
		}
		if self.selected >= self.visible.len() {
			self.selected = self.visible.len() - 1;
		}
	}

	/// Current filter mode.
	#[must_use]
	pub fn filter_mode(&self) -> TreeFilterMode {
		self.filter_mode
	}

	/// Current search query.
	#[must_use]
	pub fn search_query(&self) -> &str {
		&self.search
	}

	/// Selected entry id.
	#[must_use]
	pub fn selected_id(&self) -> Option<&str> {
		self.visible.get(self.selected).map(|row| row.id.as_str())
	}

	/// Visible row count.
	#[must_use]
	pub fn visible_len(&self) -> usize {
		self.visible.len()
	}

	/// Set filter mode.
	pub fn set_filter_mode(&mut self, mode: TreeFilterMode) {
		self.filter_mode = normalize_filter_mode(mode);
		self.rebuild();
	}

	/// Cycle filter mode.
	pub fn cycle_filter(&mut self, delta: isize) {
		let modes = TREE_FILTER_MODES;
		let idx = modes.iter().position(|m| *m == self.filter_mode).unwrap_or(0) as isize;
		let next = (idx + delta).rem_euclid(modes.len() as isize) as usize;
		self.set_filter_mode(modes[next]);
	}

	/// Update the label of a node.
	pub fn update_node_label(&mut self, entry_id: &str, label: Option<String>) {
		if let Some(node) = self.all.get_mut(entry_id) {
			node.label = label.clone();
			node.entry.label = label.clone();
		}
		update_label_in_roots(&mut self.roots, entry_id, label.clone());
		self.rebuild();
		if let Some(cb) = &mut self.on_label_change {
			cb(entry_id.to_string(), label);
		}
	}

	fn select_next(&mut self) {
		if self.visible.is_empty() {
			return;
		}
		if self.selected + 1 < self.visible.len() {
			self.selected += 1;
		}
	}

	fn select_prev(&mut self) {
		if self.selected > 0 {
			self.selected -= 1;
		}
	}

	fn page_down(&mut self, page: usize) {
		if self.visible.is_empty() {
			return;
		}
		self.selected = (self.selected + page.max(1)).min(self.visible.len() - 1);
	}

	fn page_up(&mut self, page: usize) {
		self.selected = self.selected.saturating_sub(page.max(1));
	}

	fn confirm(&mut self, summarize: bool) {
		if let Some(id) = self.selected_id().map(str::to_string) {
			(self.on_select)(id, summarize);
		}
	}

	fn cancel(&mut self) {
		(self.on_cancel)();
	}

	fn start_label_edit(&mut self) {
		let Some(row) = self.visible.get(self.selected) else {
			return;
		};
		self.label_edit = Some((row.id.clone(), row.label.clone().unwrap_or_default()));
	}

	fn commit_label_edit(&mut self) {
		if let Some((id, value)) = self.label_edit.take() {
			let trimmed = value.trim().to_string();
			let label = if trimmed.is_empty() { None } else { Some(trimmed) };
			self.update_node_label(&id, label);
		}
	}

	fn cancel_label_edit(&mut self) {
		self.label_edit = None;
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let mut out = Vec::new();
		let head = format!("Session Tree  [filter: {}]", self.filter_mode);
		out.push(truncate(&head, width as usize));
		let search = if self.search.is_empty() {
			"Search: (type to search)".to_string()
		} else {
			format!("Search: {}", self.search)
		};
		out.push(truncate(&search, width as usize));
		out.push(truncate(&"─".repeat(width.saturating_sub(1) as usize), width as usize));
		if self.visible.is_empty() {
			out.push(truncate("  No matching entries", width as usize));
		} else {
			for (i, row) in self.visible.iter().enumerate() {
				let marker = if i == self.selected { "›" } else { " " };
				let active = if row.active { "*" } else { " " };
				let label = row.label.as_deref().map(|l| format!(" [{l}]")).unwrap_or_default();
				let indent = "  ".repeat(row.depth.min(12));
				let line = format!("{marker}{active} {indent}{}{}{label}", row.kind_prefix(), row.text);
				out.push(truncate(&line, width as usize));
			}
		}
		if let Some((_, value)) = &self.label_edit {
			out.push(truncate(&format!("Label: {value}_"), width as usize));
		} else {
			out.push(truncate("Enter: switch  Shift+Enter: summarize  Shift+L: label  Ctrl+O: filter  Esc: close", width as usize));
		}
		out
	}
}

impl Component for TreeSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if let Some((_, value)) = &mut self.label_edit {
			match key {
				Key::Enter => {
					self.commit_label_edit();
				}
				Key::Esc => {
					self.cancel_label_edit();
				}
				Key::Backspace => {
					value.pop();
				}
				Key::Char(ch) => value.push(*ch),
				Key::Shift(ch) => value.push(*ch),
				_ => {}
			}
			return;
		}
		match key {
			Key::Up => self.select_prev(),
			Key::Down => self.select_next(),
			Key::PageUp | Key::Left => self.page_up(10),
			Key::PageDown | Key::Right => self.page_down(10),
			Key::Home => self.selected = 0,
			Key::End => {
				if !self.visible.is_empty() {
					self.selected = self.visible.len() - 1;
				}
			}
			Key::Enter => self.confirm(false),
			Key::Shift('\n') | Key::Shift('\r') => self.confirm(true),
			Key::Esc => {
				if !self.search.is_empty() {
					self.search.clear();
					self.rebuild();
				} else {
					self.cancel();
				}
			}
			Key::Ctrl('o') => self.cycle_filter(1),
			Key::Char('L') if !self.searching => self.start_label_edit(),
			Key::Backspace => {
				self.search.pop();
				self.rebuild();
			}
			Key::Char(ch) => {
				if *ch == 'L' && self.search.is_empty() {
					// keep plain 'L' as search char; Shift+L handled above by some decoders
				}
				self.search.push(*ch);
				self.rebuild();
			}
			_ => {}
		}
	}
}

impl NativeDescribe for TreeSelectorComponent {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let mut items = Vec::new();
		for row in &self.visible {
			let mut props = Props::new();
			props.set_str("label", &format!("{}{}", row.kind_prefix(), row.text));
			if let Some(label) = &row.label {
				props.set_str("badge", label);
			}
			if row.active {
				props.set_bool("active", true);
			}
			items.push(node(NodeKind::Item, props, Vec::new(), Some(&row.id)));
		}
		let mut list_props = Props::new();
		if let Some(selected) = self.selected_id() {
			list_props.set_str("selected", selected);
		}
		list_props.set_str("empty", "No matching entries");
		let list = node(NodeKind::List, list_props, items, Some("list"));
		let search = text(NodeText::Spans(vec![span(format!("Search: {}", self.search), Some("muted"))]), Props::new());
		let tabs = node(NodeKind::Tabs, {
			let mut p = Props::new();
			p.set_str("active", self.filter_mode);
			p
		}, Vec::new(), Some("filter"));
		let hints = text(
			NodeText::Spans(vec![span(
				"Enter switch · Shift+Enter summarize · Shift+L label · Ctrl+O filter · Esc close",
				Some("muted"),
			)]),
			Props::new(),
		);
		let mut root = Props::new();
		root.set_role("omp.overlay.tree");
		col(vec![tabs, search, list, hints], root)
	}
}

impl VisibleRow {
	fn kind_prefix(&self) -> &'static str {
		match self.kind.as_str() {
			"message" => "msg ",
			"compaction" => "cmp ",
			"branch_summary" => "sum ",
			"custom" => "cst ",
			_ => "· ",
		}
	}
}

fn normalize_filter_mode(mode: &str) -> TreeFilterMode {
	match mode {
		"default" => "default",
		"no-tools" => "no-tools",
		"user-only" => "user-only",
		"labeled-only" => "labeled-only",
		"all" => "all",
		_ => "default",
	}
}

fn truncate(s: &str, width: usize) -> String {
	if s.chars().count() <= width {
		s.to_string()
	} else if width == 0 {
		String::new()
	} else {
		s.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
	}
}

fn index_nodes(roots: &[TreeSelectorNode]) -> HashMap<String, TreeSelectorNode> {
	let mut out = HashMap::new();
	fn walk(node: &TreeSelectorNode, out: &mut HashMap<String, TreeSelectorNode>) {
		out.insert(node.entry.id.clone(), node.clone());
		for child in &node.children {
			walk(child, out);
		}
	}
	for root in roots {
		walk(root, &mut out);
	}
	out
}

fn update_label_in_roots(roots: &mut [TreeSelectorNode], entry_id: &str, label: Option<String>) {
	for node in roots {
		if node.entry.id == entry_id {
			node.label = label.clone();
			node.entry.label = label.clone();
			return;
		}
		update_label_in_roots(&mut node.children, entry_id, label.clone());
	}
}

fn active_path_ids(roots: &[TreeSelectorNode], leaf: Option<&str>) -> HashSet<String> {
	let mut parents = HashMap::new();
	fn walk(node: &TreeSelectorNode, parent: Option<&str>, parents: &mut HashMap<String, Option<String>>) {
		parents.insert(node.entry.id.clone(), parent.map(str::to_string));
		for child in &node.children {
			walk(child, Some(&node.entry.id), parents);
		}
	}
	for root in roots {
		walk(root, None, &mut parents);
	}
	let mut out = HashSet::new();
	let mut cur = leaf.map(str::to_string);
	while let Some(id) = cur {
		out.insert(id.clone());
		cur = parents.get(&id).cloned().flatten();
	}
	out
}

fn flatten_visible(
	roots: &[TreeSelectorNode],
	filter: TreeFilterMode,
	search: &str,
	current_leaf_id: Option<&str>,
) -> Vec<VisibleRow> {
	let active = active_path_ids(roots, current_leaf_id);
	let q = search.trim().to_lowercase();
	let mut out = Vec::new();
	fn walk(
		node: &TreeSelectorNode,
		depth: usize,
		filter: TreeFilterMode,
		q: &str,
		active: &HashSet<String>,
		out: &mut Vec<VisibleRow>,
	) -> bool {
		let own = row_matches(node, filter, q);
		let mut child_any = false;
		let start = out.len();
		for child in &node.children {
			if walk(child, depth + 1, filter, q, active, out) {
				child_any = true;
			}
		}
		if own || child_any {
			let text = node
				.entry
				.text
				.clone()
				.or_else(|| node.entry.role.clone())
				.unwrap_or_else(|| node.entry.kind.clone());
			let label = node.label.clone().or_else(|| node.entry.label.clone());
			out.insert(
				start,
				VisibleRow {
					id: node.entry.id.clone(),
					depth,
					label,
					text,
					kind: node.entry.kind.clone(),
					active: active.contains(&node.entry.id),
				},
			);
			true
		} else {
			false
		}
	}
	for root in roots {
		walk(root, 0, filter, &q, &active, &mut out);
	}
	out
}

fn row_matches(node: &TreeSelectorNode, filter: TreeFilterMode, q: &str) -> bool {
	let entry = &node.entry;
	let type_ok = match filter {
		"all" => true,
		"user-only" => entry.is_user_request || entry.role.as_deref() == Some("user"),
		"no-tools" => !entry.kind.contains("tool") && entry.tool_calls.is_empty(),
		"labeled-only" => node.label.is_some() || entry.label.is_some(),
		_ => !matches!(entry.kind.as_str(), "tool_result"),
	};
	if !type_ok {
		return false;
	}
	if q.is_empty() {
		return true;
	}
	let mut hay = format!("{} {} {}", entry.kind, entry.text.as_deref().unwrap_or(""), entry.id);
	if let Some(label) = node.label.as_ref().or(entry.label.as_ref()) {
		hay.push(' ');
		hay.push_str(label);
	}
	for tool in &entry.tool_calls {
		hay.push(' ');
		hay.push_str(tool);
	}
	let hay = hay.to_lowercase();
	q.split_whitespace().all(|token| hay.contains(token))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn leaf(id: &str, kind: &str, text: &str, user: bool) -> TreeSelectorNode {
		TreeSelectorNode {
			entry: SessionTreeEntry {
				id: id.into(),
				parent_id: None,
				kind: kind.into(),
				role: if user { Some("user".into()) } else { Some("assistant".into()) },
				text: Some(text.into()),
				tool_calls: Vec::new(),
				label: None,
				is_user_request: user,
				extra: None,
			},
			children: Vec::new(),
			label: None,
		}
	}

	fn sample_tree() -> Vec<TreeSelectorNode> {
		let mut root = leaf("1", "message", "hello", true);
		let mut child = leaf("2", "message", "world", false);
		child.entry.tool_calls.push("bash".into());
		root.children.push(child);
		root.children.push(leaf("3", "compaction", "compact", false));
		vec![root]
	}

	#[test]
	fn filter_modes_and_cycle() {
		assert!(TREE_FILTER_MODES.contains(&"default"));
		let mut c = TreeSelectorComponent::new(sample_tree(), Some("2".into()), 24, |_, _| {}, || {}, None, "default", None);
		assert_eq!(c.filter_mode(), "default");
		c.cycle_filter(1);
		assert_eq!(c.filter_mode(), "no-tools");
		c.set_filter_mode("user-only");
		assert_eq!(c.visible_len(), 1);
	}

	#[test]
	fn search_filters_visible_rows() {
		let mut c = TreeSelectorComponent::new(sample_tree(), Some("2".into()), 24, |_, _| {}, || {}, None, "all", None);
		assert!(c.visible_len() >= 2);
		c.handle_input(&Key::Char('w'));
		c.handle_input(&Key::Char('o'));
		assert_eq!(c.search_query(), "wo");
		assert!(c.visible_len() >= 1);
		assert!(c.visible.iter().any(|r| r.id == "2"));
	}

	#[test]
	fn navigation_and_selection_callbacks() {
		let selected = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&selected);
		let mut c = TreeSelectorComponent::new(
			sample_tree(),
			Some("1".into()),
			24,
			move |id, summarize| sink.borrow_mut().push((id, summarize)),
			|| {},
			None,
			"all",
			None,
		);
		c.handle_input(&Key::Down);
		assert!(c.selected_id().is_some());
		c.handle_input(&Key::Enter);
		assert_eq!(selected.borrow().len(), 1);
		assert!(!selected.borrow()[0].1);
	}

	#[test]
	fn label_edit_updates_node() {
		let changed = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&changed);
		let mut c = TreeSelectorComponent::new(
			sample_tree(),
			Some("1".into()),
			24,
			|_, _| {},
			|| {},
			Some(Box::new(move |id, label| sink.borrow_mut().push((id, label)))),
			"all",
			None,
		);
		c.start_label_edit();
		c.handle_input(&Key::Char('x'));
		c.handle_input(&Key::Enter);
		assert_eq!(changed.borrow().len(), 1);
		assert_eq!(changed.borrow()[0].1.as_deref(), Some("x"));
	}

	#[test]
	fn render_and_describe_have_real_content() {
		let mut c = TreeSelectorComponent::new(sample_tree(), Some("1".into()), 24, |_, _| {}, || {}, None, "default", None);
		let rows = c.render(60);
		assert!(rows.iter().any(|r| r.contains("Session Tree")));
		assert!(rows.iter().any(|r| r.contains("Search:")));
		let described = c.describe_native(&DescribeContext::new(80));
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.props.role.as_deref(), Some("omp.overlay.tree"));
		assert!(described.children.len() >= 3);
	}
}
