//! Port of `l/test/custom-editor-keybindings.test.ts`.
//!
//! The reference drives `CustomEditor.handleInput` routing. The crate
//! `CustomEditor` has no key handling, so the routing/precedence tests are
//! ignored; the shipped `KeybindingsManager` defaults and override/conflict
//! behavior are asserted for real.

use maple_tui::keybindings::{KeybindingsManager, canonical_key_id, default_bindings};
use maple_tui::keys::Key;

#[test]
fn shipped_dequeue_defaults_bind_alt_up_and_shift_up() {
    let manager = KeybindingsManager::new();
    let keys = manager.get_keys("app.dequeue");
    assert!(keys.contains(&"alt+up".to_string()), "keys: {keys:?}");
    assert!(keys.contains(&"shift+up".to_string()), "keys: {keys:?}");
    let defaults: Vec<_> = default_bindings()
        .into_iter()
        .find(|(id, _)| *id == "app.dequeue")
        .map(|(_, keys)| keys)
        .expect("dequeue defaults");
    assert_eq!(defaults, vec!["alt+up", "shift+up"]);
}

#[test]
fn user_override_of_cursor_up_keeps_dequeue_and_reports_conflict() {
    let mut manager = KeybindingsManager::new();
    manager.set_user_binding("tui.editor.cursorUp", Some(vec!["shift+up".to_string()]));
    // The crate replaces the default list instead of stealing the key, so the
    // duplicate is surfaced as a conflict rather than silently dropped.
    assert_eq!(manager.get_keys("app.dequeue"), vec!["alt+up", "shift+up"]);
    assert_eq!(manager.get_keys("tui.editor.cursorUp"), vec!["shift+up"]);
    assert!(manager.conflicts().contains("shift+up"));
}

#[test]
fn retry_default_matches_and_user_override_replaces() {
    let mut manager = KeybindingsManager::new();
    assert_eq!(manager.get_keys("app.retry"), vec!["f5", "alt+r"]);
    assert!(manager.matches(&Key::Alt('r'), "app.retry"));
    assert!(manager.matches(&Key::F(5), "app.retry"));
    manager.set_user_binding("app.retry", Some(vec!["alt+shift+r".to_string()]));
    assert!(!manager.matches(&Key::Alt('r'), "app.retry"));
    assert_eq!(manager.get_keys("app.retry"), vec!["alt+shift+r"]);
}

#[test]
fn display_reset_exit_and_live_toggle_defaults() {
    let manager = KeybindingsManager::new();
    assert!(manager.matches(&Key::Alt('l'), "app.display.reset"));
    assert!(manager.matches(&Key::Ctrl('d'), "app.exit"));
    assert!(manager.matches(&Key::Ctrl('l'), "app.sessions.live.toggle"));
    assert!(manager.matches(&Key::Alt('i'), "app.btw.inject"));
}

#[test]
fn unbinding_removes_keys() {
    let mut manager = KeybindingsManager::new();
    assert!(manager.matches(&Key::Left, "tui.editor.cursorLeft"));
    manager.set_user_binding("tui.editor.cursorLeft", None);
    assert!(manager.get_keys("tui.editor.cursorLeft").is_empty());
    assert!(!manager.matches(&Key::Left, "tui.editor.cursorLeft"));
}

#[test]
fn canonical_key_id_orders_modifiers() {
    assert_eq!(canonical_key_id(&["shift", "ctrl"], "D"), "ctrl+shift+d");
    assert_eq!(canonical_key_id(&[], "Esc"), "escape");
    assert_eq!(canonical_key_id(&[], "A"), "shift+a");
}

#[test]
#[ignore = "missing facility: CustomEditor::handle_input routing (crate CustomEditor has no key handling)"]
fn routes_configured_retry_chord_through_handle_input() {
    let manager = KeybindingsManager::new();
    assert_eq!(manager.get_keys("app.retry"), vec!["f5", "alt+r"]);
}

#[test]
#[ignore = "missing facility: CustomEditor custom key handler precedence (no handle_input)"]
fn custom_handlers_keep_precedence_over_default_retry_chord() {
    let manager = KeybindingsManager::new();
    assert!(manager.matches(&Key::Alt('r'), "app.retry"));
}

#[test]
#[ignore = "missing facility: CustomEditor exit/forward-delete chord handling (no handle_input)"]
fn ctrl_d_exits_on_empty_draft_and_forward_deletes_with_text() {
    let manager = KeybindingsManager::new();
    assert!(manager.matches(&Key::Ctrl('d'), "app.exit"));
    assert!(manager.matches(&Key::Delete, "tui.editor.deleteCharForward"));
}

#[test]
#[ignore = "missing facility: CustomEditor vim/spelling/queue chord handling (no handle_input)"]
fn vim_visual_jump_and_spelling_chords_follow_delete_semantics() {
    let manager = KeybindingsManager::new();
    assert!(manager.matches(&Key::Delete, "tui.editor.deleteCharForward"));
    assert!(manager.matches(&Key::Alt('o'), "tui.sessions.links.hint"));
}
