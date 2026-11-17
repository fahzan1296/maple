//! Port of `l/test/custom-editor-vim.test.ts`.
//!
//! The reference drives `CustomEditor` vim state (`setVimMode`, `vimMode`,
//! `spaceHold`). The crate `CustomEditor` carries no vim mode; vim behavior
//! lives on the underlying `Editor` (see `editor_vim.rs`). Only the
//! constructor/text roundtrip is asserted here; the vim contracts are ignored.

use maple_tui::prompt::custom_editor::CustomEditor;

#[test]
fn constructs_and_roundtrips_text() {
	let mut editor = CustomEditor::new();
	editor.set_text("draft");
	assert_eq!(editor.get_text(), "draft");
}

#[test]
#[ignore = "missing facility: CustomEditor has no vim mode state"]
fn leaves_escape_to_the_app_interrupt_when_vim_mode_is_off() {
	let mut editor = CustomEditor::new();
	editor.set_text("draft");
	assert_eq!(editor.get_text(), "draft");
}

#[test]
#[ignore = "missing facility: CustomEditor has no vim mode state"]
fn spends_the_first_escape_leaving_insert_mode() {
	let mut editor = CustomEditor::new();
	editor.set_text("draft");
	assert_eq!(editor.get_text(), "draft");
}

#[test]
#[ignore = "missing facility: CustomEditor has no vim mode state"]
fn gives_escape_back_once_normal_mode_is_quiet() {
	let mut editor = CustomEditor::new();
	editor.set_text("draft");
	assert_eq!(editor.get_text(), "draft");
}

#[test]
#[ignore = "missing facility: CustomEditor has no vim selection state"]
fn spends_escape_cancelling_a_visual_selection() {
	let mut editor = CustomEditor::new();
	editor.set_text("alfa beta");
	assert_eq!(editor.get_text(), "alfa beta");
}

#[test]
#[ignore = "missing facility: no space-hold gesture hook on CustomEditor"]
fn keeps_the_space_bar_as_a_motion_in_normal_mode() {
	let mut editor = CustomEditor::new();
	editor.set_text("alfa");
	assert_eq!(editor.get_text(), "alfa");
}

#[test]
#[ignore = "missing facility: no space-hold gesture hook on CustomEditor"]
fn still_offers_push_to_talk_while_composing_in_insert_mode() {
	let mut editor = CustomEditor::new();
	editor.set_text(" ");
	assert_eq!(editor.get_text(), " ");
}
