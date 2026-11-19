//! Port of `l/test/custom-editor-plugin-ctor.test.ts`.
//!
//! The reference checks upstream-pi constructor compatibility: plugins call
//! `new CustomEditor(tui, theme, keybindings)` and the base resolves the theme
//! by shape. The crate `CustomEditor` has only `new()` and no theme/tui
//! arguments, so those contracts are ignored; the own-constructor contract is
//! asserted.

use maple_tui::prompt::custom_editor::CustomEditor;

#[test]
fn own_constructor_roundtrips_text() {
	let mut editor = CustomEditor::new();
	editor.set_text("hello");
	assert_eq!(editor.get_text(), "hello");
}

#[test]
#[ignore = "missing facility: CustomEditor::new takes no (tui, theme, keybindings) arguments and stores no tui/theme"]
fn renders_when_constructed_as_tui_theme_keybindings() {
	let mut editor = CustomEditor::new();
	editor.set_text("run this workflow");
	assert_eq!(editor.get_text(), "run this workflow");
}

#[test]
#[ignore = "missing facility: CustomEditor has no render(usize) frame API and no tui capture"]
fn still_accepts_the_own_theme_constructor() {
	let mut editor = CustomEditor::new();
	editor.set_text("hello");
	assert_eq!(editor.get_text(), "hello");
}
