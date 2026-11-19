// Port target: l/test/tree-view-deep.test.ts
// Adapted to maple_tui::components::tree_view API (TreeNode/TreeView/TreeRow,
// ancestor_lasts Vec<bool>, depth/id fields). Verifies deep projection does not
// collapse and ancestry metadata is intact.

use maple_tui::components::tree_view::{TreeNode, TreeView, TreeViewTheme};

fn chain(depth: usize) -> TreeNode {
    let mut root = TreeNode::new("n0", "n0");
    // build bottom-up
    if depth <= 1 {
        return root;
    }
    let mut next = TreeNode::new(format!("n{}", depth - 1), format!("n{}", depth - 1));
    for i in (1..depth - 1).rev() {
        next = TreeNode::new(format!("n{i}"), format!("n{i}")).with_children(vec![next]);
    }
    root.with_children(vec![next])
}

#[test]
fn projects_deep_chain_and_reports_full_ancestry() {
    let mut view = TreeView::new();
    view.set_nodes(vec![chain(50_000)]);
    view.set_theme(TreeViewTheme {
        selected: maple_tui::style::Style::default(),
        ..TreeViewTheme::default()
    });
    view.expand_all();

    assert_eq!(view.rows().len(), 50_000);
    let last = &view.rows()[view.rows().len() - 1];
    assert_eq!(last.id, "n49999");
    assert_eq!(last.depth, 49_999);
    assert_eq!(last.ancestor_lasts.len(), 49_999);
    // all ancestors in a pure chain are last siblings in this construction
    assert!(last.ancestor_lasts.iter().all(|is_last| *is_last));
}
