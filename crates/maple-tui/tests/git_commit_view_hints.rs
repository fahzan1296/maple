//! Port of `l/test/git-commit-view-hints.test.ts`.
//!
//! The footer-hint contract ports directly (`git_footer_hints`). The sidebar
//! wording assertions stay ignored: the crate's `apps/git/sidebar.rs` does not
//! render the commit-view strings ("Working tree clean", "showing committed
//! <sha>", "N files in <sha>", "N file changes on <branch>").

use maple_tui::apps::git::git_tui::{git_footer_hints, Focus};

#[test]
fn advertises_stage_and_discard_while_the_worktree_has_changes() {
	for focus in [Focus::Sidebar, Focus::Diff] {
		let hints = git_footer_hints(false, focus);
		assert!(hints.contains("stage"), "{hints}");
		assert!(hints.contains("discard"), "{hints}");
	}
}

#[test]
fn drops_stage_and_discard_in_the_commit_view() {
	for focus in [Focus::Sidebar, Focus::Diff] {
		let hints = git_footer_hints(true, focus);
		assert!(!hints.contains("stage"), "{hints}");
		assert!(!hints.contains("discard"), "{hints}");
	}
}

#[test]
fn keeps_the_keys_that_still_work_in_the_commit_view() {
	assert!(git_footer_hints(true, Focus::Sidebar).contains("quit"));
	assert!(git_footer_hints(true, Focus::Diff).contains("view"));
}

#[test]
fn drops_commit_and_select_in_the_commit_view() {
	for focus in [Focus::Sidebar, Focus::Diff] {
		assert!(!git_footer_hints(true, focus).contains("commit"));
	}
	assert!(!git_footer_hints(true, Focus::Diff).contains("select"));
	assert!(git_footer_hints(false, Focus::Diff).contains("commit"));
	assert!(git_footer_hints(false, Focus::Diff).contains("select"));
}

#[test]
#[ignore = "missing API: sidebar commit-view wording ('Working tree clean', 'showing committed <sha>', 'N files in <sha>')"]
fn commit_view_says_the_tree_is_clean_and_attributes_counts() {
}

#[test]
#[ignore = "missing API: sidebar worktree wording ('N file changes on <branch>')"]
fn worktree_view_describes_changes_on_the_branch() {
}
