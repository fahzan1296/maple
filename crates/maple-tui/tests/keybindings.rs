//! Port of `l/test/keybindings.test.ts`.
//!
//! The crate's `KeybindingsManager::new()` takes no overrides; user bindings
//! go through `set_user_binding`, conflicts are a key set rather than
//! `{key, keybindings}` records, and `matches` takes a decoded `Key` instead of
//! raw input text. The crate's default table has no `tui.select.confirm`, so
//! the non-eviction check uses `tui.select.cancel`.

use maple_tui::keybindings::{KeybindingsManager, canonical_key_id};
use maple_tui::keys::Key;

fn manager_with(id: &str, keys: &[&str]) -> KeybindingsManager {
	let mut manager = KeybindingsManager::new();
	manager.set_user_binding(id, Some(keys.iter().map(|k| k.to_string()).collect()));
	manager
}

#[test]
fn does_not_evict_another_default_binding_when_input_submit_is_rebound() {
	let manager = manager_with("tui.input.submit", &["enter", "ctrl+enter"]);
	assert_eq!(manager.get_keys("tui.input.submit"), vec!["enter", "ctrl+enter"]);
	assert_eq!(manager.get_keys("tui.select.cancel"), vec!["escape", "ctrl+c"]);
}

#[test]
fn does_not_evict_cursor_bindings_when_another_action_reuses_the_same_key() {
	let manager = manager_with("tui.select.up", &["up", "ctrl+p"]);
	assert_eq!(manager.get_keys("tui.select.up"), vec!["up", "ctrl+p"]);
	assert_eq!(manager.get_keys("tui.editor.cursorUp"), vec!["up"]);
}

#[test]
fn preserves_shift_when_matching_printable_uppercase_letters() {
	let manager = manager_with("tui.input.copy", &["shift+a"]);
	assert!(manager.matches(&Key::Shift('A'), "tui.input.copy"));
	assert!(!manager.matches(&Key::Char('a'), "tui.input.copy"));
}

#[test]
fn still_reports_direct_user_binding_conflicts_without_evicting_defaults() {
	let mut manager = KeybindingsManager::new();
	manager.set_user_binding("tui.input.submit", Some(vec!["ctrl+x".to_string()]));
	manager.set_user_binding("tui.select.cancel", Some(vec!["ctrl+x".to_string()]));
	assert!(manager.conflicts().contains("ctrl+x"));
	assert_eq!(manager.get_keys("tui.editor.cursorLeft"), vec!["left", "ctrl+b"]);
}

#[test]
fn canonical_ids_normalize_aliases_and_uppercase() {
	assert_eq!(canonical_key_id(&[], "A"), "shift+a");
	assert_eq!(canonical_key_id(&["shift"], "?"), "shift+?");
	assert_eq!(canonical_key_id(&[], "esc"), "escape");
	assert_eq!(canonical_key_id(&[], "return"), "enter");

	let manager = manager_with("tui.input.copy", &["escape", "enter", "?", "shift+a"]);
	for key in [Key::Esc, Key::Enter, Key::Char('?'), Key::Shift('A')] {
		assert!(manager.matches(&key, "tui.input.copy"), "{key:?}");
	}
}

#[test]
#[ignore = "missing API: add_key_aliases / parseKey raw-input string matching"]
fn exports_the_canonical_alias_helpers_used_by_matching() {}
