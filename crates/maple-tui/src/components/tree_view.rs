//! Keyed, expandable tree with selection, search, and a rendered gutter.

use std::collections::{HashMap, HashSet};

use crate::component::Component;
use crate::keys::{
    is_arrow_down, is_arrow_left, is_arrow_right, is_arrow_up, is_end, is_enter, is_home, Key,
};
use crate::mouse::{MouseButtonKind, MouseKind, MouseEvent};
use crate::style::{Color, Style};
use crate::text_shape::{truncate_to_width, Ellipsis};

/// One node in a tree hierarchy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeNode {
    /// Stable identity, unique within one hierarchy.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Nested children.
    pub children: Vec<TreeNode>,
    /// Optional free-form metadata (searchable, not rendered).
    pub metadata: Option<String>,
    /// Overrides whether the children can be expanded.
    pub expandable: Option<bool>,
}

impl TreeNode {
    /// Create a leaf node.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        TreeNode {
            id: id.into(),
            label: label.into(),
            children: Vec::new(),
            metadata: None,
            expandable: None,
        }
    }

    /// Attach children and return the node.
    pub fn with_children(mut self, children: Vec<TreeNode>) -> Self {
        self.children = children;
        self
    }

    /// Attach metadata and return the node.
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Override expandability and return the node.
    pub fn with_expandable(mut self, expandable: bool) -> Self {
        self.expandable = Some(expandable);
        self
    }

    /// Whether the node can expand.
    pub fn is_expandable(&self) -> bool {
        self.expandable.unwrap_or(!self.children.is_empty())
    }
}

/// Glyphs and styles used to render tree rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeViewTheme {
    /// Glyph drawn for ancestor levels that still have following siblings.
    pub indent_guide: String,
    /// Glyph for a sibling that has following siblings.
    pub branch_glyph: String,
    /// Glyph for the last sibling.
    pub last_glyph: String,
    /// Glyph for an expanded branch.
    pub expanded_glyph: String,
    /// Glyph for a collapsed branch.
    pub collapsed_glyph: String,
    /// Glyph for a leaf row.
    pub leaf_glyph: String,
    /// Style applied to the selected row.
    pub selected: Style,
}

impl Default for TreeViewTheme {
    fn default() -> Self {
        TreeViewTheme {
            indent_guide: "│".to_string(),
            branch_glyph: "├".to_string(),
            last_glyph: "└".to_string(),
            expanded_glyph: "▾".to_string(),
            collapsed_glyph: "▸".to_string(),
            leaf_glyph: "•".to_string(),
            selected: Style {
                fg: Some(Color::Black),
                bg: Some(Color::Gray),
                ..Style::default()
            },
        }
    }
}

/// Structural metadata for one item in depth-first display order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// Node identity.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Depth from the root (roots are zero).
    pub depth: usize,
    /// Index in the flattened visible rows.
    pub index: usize,
    /// Whether this node is the last sibling.
    pub is_last: bool,
    /// Parent identity, when the node is not a root.
    pub parent_id: Option<String>,
    /// Whether the node can expand.
    pub expandable: bool,
    /// Whether children are currently shown.
    pub expanded: bool,
    /// Per-ancestor `is_last` flags used to draw the gutter.
    pub ancestor_lasts: Vec<bool>,
    /// Searchable metadata.
    pub metadata: Option<String>,
}

/// Reusable keyed hierarchy component.
pub struct TreeView {
    roots: Vec<TreeNode>,
    expanded: HashSet<String>,
    rows: Vec<TreeRow>,
    index_by_id: HashMap<String, usize>,
    cursor: usize,
    search: Option<String>,
    theme: TreeViewTheme,
    on_select: Option<Box<dyn FnMut(&TreeNode) + 'static>>,
    on_expand: Option<Box<dyn FnMut(&TreeNode) + 'static>>,
    on_collapse: Option<Box<dyn FnMut(&TreeNode) + 'static>>,
}

impl Default for TreeView {
    fn default() -> Self {
        TreeView::new()
    }
}

impl TreeView {
    /// Create an empty tree.
    pub fn new() -> Self {
        TreeView {
            roots: Vec::new(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            index_by_id: HashMap::new(),
            cursor: 0,
            search: None,
            theme: TreeViewTheme::default(),
            on_select: None,
            on_expand: None,
            on_collapse: None,
        }
    }

    /// Replace the root nodes, clearing expansion and selection.
    pub fn set_nodes(&mut self, nodes: Vec<TreeNode>) {
        self.roots = nodes;
        self.expanded.clear();
        self.cursor = 0;
        self.rebuild();
    }

    /// Append a root node.
    pub fn add_node(&mut self, node: TreeNode) {
        self.roots.push(node);
        self.rebuild();
    }

    /// Current root nodes.
    pub fn nodes(&self) -> &[TreeNode] {
        &self.roots
    }

    /// Replace the gutter and selection theme.
    pub fn set_theme(&mut self, theme: TreeViewTheme) {
        self.theme = theme;
    }

    /// Current theme.
    pub fn theme(&self) -> &TreeViewTheme {
        &self.theme
    }

    /// Flattened visible rows in navigation order.
    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    /// Index of the selected row, or `None` when the tree is empty.
    pub fn selected_index(&self) -> Option<usize> {
        if self.rows.is_empty() {
            None
        } else {
            Some(self.cursor)
        }
    }

    /// Identity of the selected node, if any.
    pub fn selected_id(&self) -> Option<&str> {
        self.rows.get(self.cursor).map(|row| row.id.as_str())
    }

    /// The selected node, if any.
    pub fn selected_node(&self) -> Option<&TreeNode> {
        let id = self.selected_id()?;
        find_node(&self.roots, id)
    }

    /// Find a node by identity anywhere in the hierarchy.
    pub fn find_node(&self, id: &str) -> Option<&TreeNode> {
        find_node(&self.roots, id)
    }

    /// Move the selection down by one visible row.
    pub fn select_next(&mut self) {
        let next = self.cursor.saturating_add(1);
        self.set_cursor(next);
    }

    /// Move the selection up by one visible row.
    pub fn select_previous(&mut self) {
        let previous = self.cursor.saturating_sub(1);
        self.set_cursor(previous);
    }

    /// Select a visible row by index, clamped to range.
    pub fn select_index(&mut self, index: usize) {
        self.set_cursor(index);
    }

    /// Select the first visible row.
    pub fn select_first(&mut self) {
        self.set_cursor(0);
    }

    /// Select the last visible row.
    pub fn select_last(&mut self) {
        let last = self.rows.len().saturating_sub(1);
        self.set_cursor(last);
    }

    /// Expand a node by identity, firing `on_expand` when it changes.
    pub fn expand(&mut self, id: &str) {
        let expandable = self
            .find_node(id)
            .is_some_and(|node| node.is_expandable());
        if !expandable || !self.expanded.insert(id.to_string()) {
            return;
        }
        self.rebuild();
        let node = find_node(&self.roots, id);
        if let (Some(callback), Some(node)) = (self.on_expand.as_mut(), node) {
            callback(node);
        }
    }

    /// Collapse a node by identity, firing `on_collapse` when it changes.
    pub fn collapse(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            return;
        }
        self.rebuild();
        let node = find_node(&self.roots, id);
        if let (Some(callback), Some(node)) = (self.on_collapse.as_mut(), node) {
            callback(node);
        }
    }

    /// Toggle a node's expansion state.
    pub fn toggle_node(&mut self, id: &str) {
        if self.expanded.contains(id) {
            self.collapse(id);
        } else {
            self.expand(id);
        }
    }

    /// Toggle the selected node's expansion state.
    pub fn toggle_selected(&mut self) {
        if let Some(id) = self.selected_id().map(str::to_string) {
            self.toggle_node(&id);
        }
    }

    /// Expand every expandable node.
    pub fn expand_all(&mut self) {
        let mut ids = Vec::new();
        collect_expandable(&self.roots, &mut ids);
        self.expanded.extend(ids);
        self.rebuild();
    }

    /// Collapse every node.
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.rebuild();
    }

    /// Restrict visible rows to nodes matching `query` (case-insensitive).
    pub fn search_nodes(&mut self, query: &str) {
        self.search = if query.is_empty() {
            None
        } else {
            Some(query.to_lowercase())
        };
        self.rebuild();
    }

    /// Clear the active search filter.
    pub fn clear_search(&mut self) {
        self.search = None;
        self.rebuild();
    }

    /// Active search query, if any.
    pub fn search(&self) -> Option<&str> {
        self.search.as_deref()
    }

    /// Register a callback fired with the selected node after selection changes.
    pub fn set_on_select<F: FnMut(&TreeNode) + 'static>(&mut self, callback: F) {
        self.on_select = Some(Box::new(callback));
    }

    /// Register a callback fired with a node when it expands.
    pub fn set_on_expand<F: FnMut(&TreeNode) + 'static>(&mut self, callback: F) {
        self.on_expand = Some(Box::new(callback));
    }

    /// Register a callback fired with a node when it collapses.
    pub fn set_on_collapse<F: FnMut(&TreeNode) + 'static>(&mut self, callback: F) {
        self.on_collapse = Some(Box::new(callback));
    }

    /// Apply navigation and expansion keys.
    pub fn handle_key(&mut self, key: &Key) {
        if is_arrow_down(key) {
            self.select_next();
        } else if is_arrow_up(key) {
            self.select_previous();
        } else if is_home(key) {
            self.select_first();
        } else if is_end(key) {
            self.select_last();
        } else if is_arrow_right(key) {
            self.expand_or_descend();
        } else if is_arrow_left(key) {
            self.collapse_or_ascend();
        } else if is_enter(key) {
            self.activate();
        }
    }

    fn set_cursor(&mut self, index: usize) {
        if self.rows.is_empty() {
            self.cursor = 0;
            return;
        }
        let next = index.min(self.rows.len() - 1);
        if next == self.cursor {
            return;
        }
        self.cursor = next;
        self.fire_select();
    }

    fn activate(&mut self) {
        let (id, expandable) = match self.rows.get(self.cursor) {
            Some(row) => (row.id.clone(), row.expandable),
            None => return,
        };
        if expandable {
            self.toggle_node(&id);
        } else {
            self.fire_select();
        }
    }

    fn expand_or_descend(&mut self) {
        let (id, expandable, expanded) = match self.rows.get(self.cursor) {
            Some(row) => (row.id.clone(), row.expandable, row.expanded),
            None => return,
        };
        if expandable && !expanded {
            self.expand(&id);
        } else if expanded && self.cursor + 1 < self.rows.len() {
            self.set_cursor(self.cursor + 1);
        }
    }

    fn collapse_or_ascend(&mut self) {
        let (id, expandable, expanded, parent) = match self.rows.get(self.cursor) {
            Some(row) => (
                row.id.clone(),
                row.expandable,
                row.expanded,
                row.parent_id.clone(),
            ),
            None => return,
        };
        if expandable && expanded {
            self.collapse(&id);
        } else if let Some(parent) = parent.as_deref() {
            if let Some(index) = self.index_by_id.get(parent).copied() {
                self.set_cursor(index);
            }
        }
    }

    fn fire_select(&mut self) {
        let id = match self.rows.get(self.cursor) {
            Some(row) => row.id.as_str(),
            None => return,
        };
        let node = find_node(&self.roots, id);
        if let (Some(callback), Some(node)) = (self.on_select.as_mut(), node) {
            callback(node);
        }
    }

    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        for root in &self.roots {
            self.walk(root, 0, true, None, &mut Vec::new(), &mut rows);
        }
        if let Some(query) = self.search.clone() {
            rows.retain(|row| {
                row.label.to_lowercase().contains(query.as_str())
                    || row
                        .metadata
                        .as_deref()
                        .map(|metadata| metadata.to_lowercase().contains(query.as_str()))
                        .unwrap_or(false)
            });
        }
        for (index, row) in rows.iter_mut().enumerate() {
            row.index = index;
        }
        let mut index_by_id = HashMap::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            index_by_id.insert(row.id.clone(), index);
        }
        self.rows = rows;
        self.index_by_id = index_by_id;
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
    }

    fn walk(
        &self,
        node: &TreeNode,
        depth: usize,
        is_last: bool,
        parent_id: Option<String>,
        ancestor_lasts: &mut Vec<bool>,
        rows: &mut Vec<TreeRow>,
    ) {
        let expandable = node.is_expandable();
        let expanded = expandable && self.expanded.contains(&node.id);
        rows.push(TreeRow {
            id: node.id.clone(),
            label: node.label.clone(),
            depth,
            index: rows.len(),
            is_last,
            parent_id,
            expandable,
            expanded,
            ancestor_lasts: ancestor_lasts.clone(),
            metadata: node.metadata.clone(),
        });
        if !expanded {
            return;
        }
        ancestor_lasts.push(is_last);
        let count = node.children.len();
        for (index, child) in node.children.iter().enumerate() {
            let child_last = index + 1 == count;
            self.walk(
                child,
                depth + 1,
                child_last,
                Some(node.id.clone()),
                ancestor_lasts,
                rows,
            );
        }
        ancestor_lasts.pop();
    }

    fn gutter(&self, row: &TreeRow) -> String {
        let mut out = String::new();
        for &ancestor_last in &row.ancestor_lasts {
            if ancestor_last {
                out.push_str("   ");
            } else {
                out.push_str(&self.theme.indent_guide);
                out.push_str("  ");
            }
        }
        out.push_str(if row.is_last {
            &self.theme.last_glyph
        } else {
            &self.theme.branch_glyph
        });
        out.push(' ');
        out
    }
}

/// Depth-first node lookup by identity.
fn find_node<'a>(nodes: &'a [TreeNode], id: &str) -> Option<&'a TreeNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Collect the identities of every expandable node.
fn collect_expandable(nodes: &[TreeNode], out: &mut Vec<String>) {
    for node in nodes {
        if node.is_expandable() {
            out.push(node.id.clone());
        }
        collect_expandable(&node.children, out);
    }
}

impl Component for TreeView {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(self.rows.len());
        for (index, row) in self.rows.iter().enumerate() {
            let marker = if row.expandable {
                if row.expanded {
                    self.theme.expanded_glyph.as_str()
                } else {
                    self.theme.collapsed_glyph.as_str()
                }
            } else {
                self.theme.leaf_glyph.as_str()
            };
            let line = format!("{}{marker} {}", self.gutter(row), row.label);
            let clipped = truncate_to_width(&line, width, Ellipsis::Omit);
            if index == self.cursor {
                out.push(self.theme.selected.paint(&clipped));
            } else {
                out.push(clipped);
            }
        }
        out
    }

    fn handle_input(&mut self, key: &Key) {
        self.handle_key(key);
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if event.kind == MouseKind::Down(MouseButtonKind::Left) {
            let index = event.row as usize;
            if index < self.rows.len() {
                self.set_cursor(index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;
    use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn plain_theme() -> TreeViewTheme {
        TreeViewTheme {
            selected: Style::default(),
            ..TreeViewTheme::default()
        }
    }

    fn sample_tree() -> TreeView {
        let mut tree = TreeView::new();
        tree.set_nodes(vec![TreeNode::new("root", "root").with_children(vec![
            TreeNode::new("a", "a").with_children(vec![TreeNode::new("a1", "a1")]),
            TreeNode::new("b", "b"),
        ])]);
        tree.set_theme(plain_theme());
        tree
    }

    #[test]
    fn collapsed_tree_shows_only_roots() {
        let mut tree = sample_tree();
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.render(20), vec!["└ ▸ root"]);
    }

    #[test]
    fn expansion_flattens_depth_first() {
        let mut tree = sample_tree();
        tree.expand("root");
        assert_eq!(
            tree.render(24),
            vec!["└ ▾ root", "   ├ ▸ a", "   └ • b"]
        );
        tree.expand("a");
        assert_eq!(
            tree.render(24),
            vec!["└ ▾ root", "   ├ ▾ a", "   │  └ • a1", "   └ • b"]
        );
        tree.collapse("root");
        assert_eq!(tree.render(24), vec!["└ ▸ root"]);
    }

    #[test]
    fn collapse_all_and_expand_all() {
        let mut tree = sample_tree();
        tree.expand_all();
        assert_eq!(tree.rows().len(), 4);
        tree.collapse_all();
        assert_eq!(tree.rows().len(), 1);
    }

    #[test]
    fn selection_navigation_and_lookup() {
        let mut tree = sample_tree();
        tree.expand("root");
        assert_eq!(tree.selected_index(), Some(0));
        assert_eq!(tree.selected_id(), Some("root"));
        assert_eq!(tree.selected_node().map(|node| node.label.as_str()), Some("root"));
        tree.select_next();
        assert_eq!(tree.selected_index(), Some(1));
        assert_eq!(tree.selected_id(), Some("a"));
        tree.select_next();
        assert_eq!(tree.selected_id(), Some("b"));
        tree.select_next();
        assert_eq!(tree.selected_id(), Some("b"));
        tree.select_previous();
        assert_eq!(tree.selected_id(), Some("a"));
        tree.select_last();
        assert_eq!(tree.selected_id(), Some("b"));
        tree.select_first();
        assert_eq!(tree.selected_id(), Some("root"));
    }

    #[test]
    fn keyboard_navigates_and_toggles() {
        let mut tree = sample_tree();
        tree.handle_input(&Key::Right);
        assert!(tree.rows()[0].expanded);
        tree.handle_input(&Key::Down);
        assert_eq!(tree.selected_id(), Some("a"));
        tree.handle_input(&Key::Enter);
        assert!(tree.rows()[1].expanded);
        tree.handle_input(&Key::Left);
        assert!(!tree.rows()[1].expanded);
        tree.handle_input(&Key::Left);
        assert_eq!(tree.selected_id(), Some("root"));
        tree.handle_input(&Key::End);
        assert_eq!(tree.selected_id(), Some("b"));
        tree.handle_input(&Key::Home);
        assert_eq!(tree.selected_id(), Some("root"));
    }

    #[test]
    fn callbacks_fire_for_selection_expand_and_collapse() {
        let mut tree = sample_tree();
        let selected = Rc::new(RefCell::new(Vec::new()));
        let expanded = Rc::new(RefCell::new(Vec::new()));
        let collapsed = Rc::new(RefCell::new(Vec::new()));
        let select_sink = selected.clone();
        let expand_sink = expanded.clone();
        let collapse_sink = collapsed.clone();
        tree.set_on_select(move |node| select_sink.borrow_mut().push(node.id.clone()));
        tree.set_on_expand(move |node| expand_sink.borrow_mut().push(node.id.clone()));
        tree.set_on_collapse(move |node| collapse_sink.borrow_mut().push(node.id.clone()));

        tree.handle_input(&Key::Right);
        assert_eq!(*expanded.borrow(), vec!["root".to_string()]);
        tree.handle_input(&Key::Down);
        assert_eq!(*selected.borrow(), vec!["a".to_string()]);
        tree.handle_input(&Key::Enter);
        assert_eq!(*expanded.borrow(), vec!["root".to_string(), "a".to_string()]);
        tree.handle_input(&Key::Left);
        assert_eq!(*collapsed.borrow(), vec!["a".to_string()]);
    }

    #[test]
    fn search_filters_by_label_and_metadata() {
        let mut tree = sample_tree();
        tree.expand_all();
        tree.search_nodes("a1");
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.rows()[0].id, "a1");
        tree.clear_search();
        assert_eq!(tree.rows().len(), 4);

        let mut metadata_tree = TreeView::new();
        metadata_tree.set_nodes(vec![TreeNode::new("x", "plain")
            .with_metadata("secret note")
            .with_children(vec![TreeNode::new("y", "other")])]);
        metadata_tree.set_theme(plain_theme());
        metadata_tree.expand_all();
        metadata_tree.search_nodes("SEC");
        assert_eq!(metadata_tree.rows().len(), 1);
        assert_eq!(metadata_tree.rows()[0].id, "x");
    }

    #[test]
    fn mouse_click_selects_row() {
        let mut tree = sample_tree();
        tree.expand("root");
        tree.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 0,
            row: 2,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(tree.selected_id(), Some("b"));
    }

    #[test]
    fn empty_tree_and_zero_width_are_safe() {
        let mut tree = TreeView::new();
        assert_eq!(tree.render(10), Vec::<String>::new());
        assert_eq!(tree.selected_index(), None);
        assert!(tree.selected_node().is_none());
        tree.handle_input(&Key::Down);
        tree.handle_input(&Key::Enter);
        assert_eq!(tree.render(0), Vec::<String>::new());
    }

    #[test]
    fn non_expandable_node_hides_children() {
        let mut tree = TreeView::new();
        tree.set_nodes(vec![TreeNode::new("root", "root")
            .with_children(vec![TreeNode::new("child", "child")])
            .with_expandable(false)]);
        tree.set_theme(plain_theme());
        tree.expand_all();
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.render(20), vec!["└ • root"]);
    }
}
