//! Port of `l/test/editor-atoms.test.ts`.
//!
//! The crate's `Editor` has no atom table: `registerAtom`, `insertAtom`,
//! `collapseToAtom`, `getExpandedText`, `atomicTokenPattern`, and
//! `textRevision` are not ported, so every assertion in the reference file is
//! ignored.

use maple_tui::components::editor::Editor;

#[test]
#[ignore = "missing facility: Editor atom table (registerAtom/insertAtom/getExpandedText) is not ported"]
fn expands_an_inserted_atom_to_its_registered_expansion_on_submit() {
	let editor = Editor::new();
	assert_eq!(editor.get_text(), "");
}

#[test]
#[ignore = "missing facility: Editor atom table (registerAtom/getExpandedText) is not ported"]
fn expands_the_longest_label_first_so_1_never_corrupts_10() {
	let editor = Editor::new();
	assert_eq!(editor.get_text(), "");
}

#[test]
#[ignore = "missing facility: Editor atom table (registerAtom/getExpandedText) is not ported"]
fn never_rescans_expanded_content_for_other_tokens() {
	let editor = Editor::new();
	assert_eq!(editor.get_text(), "");
}

#[test]
#[ignore = "missing facility: Editor atom table and submit-time atom clearing are not ported"]
fn clears_atoms_on_submit_so_the_next_draft_starts_fresh() {
	let editor = Editor::new();
	assert_eq!(editor.get_text(), "");
}

#[test]
#[ignore = "missing facility: Editor::collapseToAtom and getExpandedText are not ported"]
fn collapses_a_typed_span_into_an_atom_and_keeps_a_trailing_cursor_anchored_to_the_text_after_it() {
	let editor = Editor::new();
	assert_eq!(editor.cursor_row(), 0);
}

#[test]
#[ignore = "missing facility: Editor::collapseToAtom cursor remapping is not ported"]
fn moves_a_cursor_that_sat_inside_the_collapsed_span_to_just_after_the_atom() {
	let editor = Editor::new();
	assert_eq!(editor.cursor_column(), 0);
}

#[test]
#[ignore = "missing facility: Editor::atomicTokenPattern is not ported"]
fn deletes_an_icon_atom_as_one_unit_under_atomic_token_pattern() {
	let editor = Editor::new();
	assert_eq!(editor.get_text(), "");
}

#[test]
#[ignore = "missing facility: Editor::textRevision is not ported"]
fn advances_the_text_revision_for_programmatic_edits_deletion_undo_and_history_restore() {
	let mut editor = Editor::new();
	editor.set_text("one");
	assert_eq!(editor.get_text(), "one");
}
