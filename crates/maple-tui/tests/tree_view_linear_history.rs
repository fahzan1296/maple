// Port target: l/test/tree-view-linear-history.test.ts
// Rust TreeView API differs (no generic key/render callbacks, no getChildDepth/
// compactSameDepthAncestors). This adaptation asserts linear-history structure and
// that sibling gutter ancestry is retained for branch continuation in crate semantics.

use maple_tui::components::tree_view::{TreeNode, TreeView, TreeViewTheme};

fn build_linear_tree(length: usize) -> TreeNode {
    let mut root = TreeNode::new("0", "0");
    let mut current = TreeNode::new(format!("{}", length - 1), format!("{}", length - 1));
    for index in (1..length - 1).rev() {
        let node = TreeNode::new(index.to_string(), index.to_string()).with_children(vec![current]);
        current = node;
    }
    if length > 1 {
        root = root.with_children(vec![current]);
    }
    root
}

#[test]
fn linear_history_projects_full_depth_chain() {
    let mut tree = TreeView::new();
    tree.set_theme(TreeViewTheme {
        selected: maple_tui::style::Style::default(),
        ..TreeViewTheme::default()
    });
    tree.set_nodes(vec![build_linear_tree(10_000)]);
    tree.expand_all();

    assert_eq!(tree.rows().len(), 10_000);
    assert_eq!(tree.rows()[0].id, "0");
    assert_eq!(tree.rows()[0].depth, 0);
    assert_eq!(tree.rows()[9_999].id, "9999");
    assert_eq!(tree.rows()[9_999].depth, 9_999);
}

#[test]
fn retains_branch_ancestor_flags_for_sibling_gutters() {
    let continuation = TreeNode::new("a2", "a2");
    let first_branch = TreeNode::new("a", "a").with_children(vec![continuation]);
    let second_branch = TreeNode::new("b", "b");
    let root = TreeNode::new("root", "root").with_children(vec![first_branch, second_branch]);

    let mut tree = TreeView::new();
    tree.set_theme(TreeViewTheme {
        selected: maple_tui::style::Style::default(),
        ..TreeViewTheme::default()
    });
    tree.set_nodes(vec![root]);
    tree.expand_all();

    let continuation_row = tree.rows().iter().find(|row| row.id == "a2").unwrap();
    // ancestors: sole root is last; branch node `a` is not last because `b` follows.
    assert_eq!(continuation_row.depth, 2);
    assert_eq!(continuation_row.ancestor_lasts, vec![true, false]);
}
