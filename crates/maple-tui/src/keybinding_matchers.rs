//! Effective-key matchers shared by overlays, editors, and native hints.
//!
//! Reference: `l/src/keybinding-matchers.ts`.
//!
//! Deviations forced by Rust:
//!
//! - `getKeybindings()` is a module-level singleton upstream; maple-tui has no
//!   global keybindings, so every matcher takes `&KeybindingsManager`
//!   explicitly.
//! - The reference matches raw terminal `data: string` through `matchesKey`.
//!   The crate decodes input into [`crate::keys::Key`] upstream, so the
//!   matchers take `&Key`; id comparison uses
//!   [`crate::keybindings::key_id`], which is what `matchesKey` canonicalizes
//!   to.
//! - `matchesEffectiveKey`'s modifier-tagged-LF branch (raw `\n` with length
//!   > 1 standing in for Ctrl+Enter) needs raw bytes. [`crate::keys::Key`] has
//!   no Ctrl+Enter variant (crossterm reports Enter without the modifier), so
//!   that branch is upstream decoding's responsibility here; `ctrl+enter`
//!   remains matchable as a bound id string.

use crate::keybindings::{key_id, KeybindingsManager};
use crate::keys::Key;

/// Keys approval and follow-up fall back to when unbound: both must stay
/// reachable by keyboard.
pub const UNBOUND_SUBMIT_KEYS: &[&str] = &["ctrl+enter", "ctrl+q"];

/// Match the coding-agent interrupt key, falling back to Escape when unbound.
pub fn matches_app_interrupt(key: &Key, keybindings: &KeybindingsManager) -> bool {
    if !keybindings.get_keys("app.interrupt").is_empty() {
        return keybindings.matches(key, "app.interrupt");
    }
    matches!(key, Key::Esc)
}

/// Match `app.tools.expand`, falling back to Ctrl+O when the action is unbound.
pub fn matches_app_tools_expand(key: &Key, keybindings: &KeybindingsManager) -> bool {
    if !keybindings.get_keys("app.tools.expand").is_empty() {
        return keybindings.matches(key, "app.tools.expand");
    }
    matches!(key, Key::Ctrl('o'))
}

/// Effective tools-expand key: the first bound key, or Ctrl+O when unbound.
pub fn app_tools_expand_key(keybindings: &KeybindingsManager) -> String {
    keybindings
        .get_keys("app.tools.expand")
        .into_iter()
        .next()
        .unwrap_or_else(|| "ctrl+o".to_string())
}

/// Match the generic selector cancel keybinding.
pub fn matches_select_cancel(key: &Key, keybindings: &KeybindingsManager) -> bool {
    keybindings.matches(key, "tui.select.cancel")
}

/// Match the generic selector up-navigation keybinding.
pub fn matches_select_up(key: &Key, keybindings: &KeybindingsManager) -> bool {
    keybindings.matches(key, "tui.select.up")
}

/// Match the generic selector down-navigation keybinding.
pub fn matches_select_down(key: &Key, keybindings: &KeybindingsManager) -> bool {
    keybindings.matches(key, "tui.select.down")
}

/// Match the generic selector page-up keybinding.
pub fn matches_select_page_up(key: &Key, keybindings: &KeybindingsManager) -> bool {
    keybindings.matches(key, "tui.select.pageUp")
}

/// Match the generic selector page-down keybinding.
pub fn matches_select_page_down(key: &Key, keybindings: &KeybindingsManager) -> bool {
    keybindings.matches(key, "tui.select.pageDown")
}

/// Effective external-editor key: the first bound key, or Ctrl+G when unbound.
pub fn app_external_editor_key(keybindings: &KeybindingsManager) -> String {
    keybindings
        .get_keys("app.editor.external")
        .into_iter()
        .next()
        .unwrap_or_else(|| "ctrl+g".to_string())
}

/// Match the external-editor keybinding, falling back to Ctrl+G when unbound.
pub fn matches_app_external_editor(key: &Key, keybindings: &KeybindingsManager) -> bool {
    if !keybindings.get_keys("app.editor.external").is_empty() {
        return keybindings.matches(key, "app.editor.external");
    }
    matches!(key, Key::Ctrl('g'))
}

fn matches_effective_key(key: &Key, id: &str) -> bool {
    key_id(key).is_some_and(|canonical| canonical == id)
}

fn matches_effective_keys(key: &Key, keys: &[String]) -> bool {
    keys.iter().any(|id| matches_effective_key(key, id))
}

/// Effective follow-up keys: the `app.message.followUp` bindings, or
/// Ctrl+Enter/Ctrl+Q when unbound.
pub fn app_follow_up_keys(keybindings: &KeybindingsManager) -> Vec<String> {
    let keys = keybindings.get_keys("app.message.followUp");
    if keys.is_empty() {
        UNBOUND_SUBMIT_KEYS.iter().map(|key| key.to_string()).collect()
    } else {
        keys
    }
}

/// Effective tool-approval keys: the `app.approval.approve` bindings, or
/// Ctrl+Enter/Ctrl+Q when unbound.
pub fn app_approve_keys(keybindings: &KeybindingsManager) -> Vec<String> {
    let keys = keybindings.get_keys("app.approval.approve");
    if keys.is_empty() {
        UNBOUND_SUBMIT_KEYS.iter().map(|key| key.to_string()).collect()
    } else {
        keys
    }
}

/// Match the "submit multi-line text input" keybinding
/// (`app.message.followUp`).
pub fn matches_app_follow_up(key: &Key, keybindings: &KeybindingsManager) -> bool {
    let keys = app_follow_up_keys(keybindings);
    matches_effective_keys(key, &keys)
}

/// Match the tool-approval key.
pub fn matches_app_approve(key: &Key, keybindings: &KeybindingsManager) -> bool {
    let keys = app_approve_keys(keybindings);
    matches_effective_keys(key, &keys)
}

/// Effective /btw inject key (Alt+I by default). No fallback: it sends text
/// into the main session, so a user who unbinds it gets no key at all.
pub fn app_btw_inject_key(keybindings: &KeybindingsManager) -> Option<String> {
    keybindings.get_keys("app.btw.inject").into_iter().next()
}

/// Match the key that sends a /btw answer's steering prompt to the main
/// session.
pub fn matches_app_btw_inject(key: &Key, keybindings: &KeybindingsManager) -> bool {
    keybindings.matches(key, "app.btw.inject")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_default_bindings() {
        let manager = KeybindingsManager::new();
        assert!(matches_app_interrupt(&Key::Esc, &manager));
        assert!(matches_app_tools_expand(&Key::Ctrl('o'), &manager));
        assert!(matches_app_external_editor(&Key::Ctrl('g'), &manager));
        assert!(matches_app_btw_inject(&Key::Alt('i'), &manager));
        assert_eq!(app_tools_expand_key(&manager), "ctrl+o");
        assert_eq!(app_external_editor_key(&manager), "ctrl+g");
        assert_eq!(app_btw_inject_key(&manager), Some("alt+i".to_string()));
    }

    #[test]
    fn selector_matchers_use_bound_ids() {
        let manager = KeybindingsManager::new();
        assert!(matches_select_cancel(&Key::Esc, &manager));
        assert!(matches_select_up(&Key::Up, &manager));
        assert!(matches_select_up(&Key::Ctrl('p'), &manager));
        assert!(matches_select_down(&Key::Down, &manager));
        assert!(matches_select_page_up(&Key::PageUp, &manager));
        assert!(matches_select_page_down(&Key::PageDown, &manager));
        assert!(!matches_select_up(&Key::Down, &manager));
    }

    #[test]
    fn unbound_actions_fall_back_to_defaults() {
        let mut manager = KeybindingsManager::new();
        manager.set_user_binding("app.interrupt", None);
        manager.set_user_binding("app.tools.expand", None);
        manager.set_user_binding("app.editor.external", None);
        manager.set_user_binding("app.btw.inject", None);
        assert!(matches_app_interrupt(&Key::Esc, &manager));
        assert!(!matches_app_interrupt(&Key::Enter, &manager));
        assert!(matches_app_tools_expand(&Key::Ctrl('o'), &manager));
        assert!(!matches_app_tools_expand(&Key::Ctrl('p'), &manager));
        assert!(matches_app_external_editor(&Key::Ctrl('g'), &manager));
        assert_eq!(app_tools_expand_key(&manager), "ctrl+o");
        assert_eq!(app_external_editor_key(&manager), "ctrl+g");
        assert_eq!(app_btw_inject_key(&manager), None);
        assert!(!matches_app_btw_inject(&Key::Alt('i'), &manager));
    }

    #[test]
    fn user_overrides_win_over_fallbacks() {
        let mut manager = KeybindingsManager::new();
        manager.set_user_binding("app.tools.expand", Some(vec!["ctrl+e".into()]));
        manager.set_user_binding("app.editor.external", Some(vec!["ctrl+u".into()]));
        assert!(matches_app_tools_expand(&Key::Ctrl('e'), &manager));
        assert!(!matches_app_tools_expand(&Key::Ctrl('o'), &manager));
        assert_eq!(app_tools_expand_key(&manager), "ctrl+e");
        assert!(matches_app_external_editor(&Key::Ctrl('u'), &manager));
        assert!(!matches_app_external_editor(&Key::Ctrl('g'), &manager));
        assert_eq!(app_external_editor_key(&manager), "ctrl+u");
    }

    #[test]
    fn follow_up_and_approve_keys_fall_back_and_honor_overrides() {
        let mut manager = KeybindingsManager::new();
        // The crate defaults do not bind `app.message.followUp`, so the
        // reference fallback applies.
        assert_eq!(app_follow_up_keys(&manager), UNBOUND_SUBMIT_KEYS);
        assert!(matches_app_follow_up(&Key::Ctrl('q'), &manager));
        assert!(!matches_app_follow_up(&Key::Enter, &manager));

        manager.set_user_binding("app.message.followUp", Some(vec!["ctrl+f".into()]));
        assert_eq!(app_follow_up_keys(&manager), vec!["ctrl+f".to_string()]);
        assert!(matches_app_follow_up(&Key::Ctrl('f'), &manager));
        assert!(!matches_app_follow_up(&Key::Ctrl('q'), &manager));

        assert_eq!(app_approve_keys(&manager), vec!["ctrl+enter", "ctrl+q"]);
        assert!(matches_app_approve(&Key::Ctrl('q'), &manager));
        assert!(!matches_app_approve(&Key::Enter, &manager));
        manager.set_user_binding("app.approval.approve", Some(vec!["ctrl+a".into()]));
        assert!(matches_app_approve(&Key::Ctrl('a'), &manager));
        assert!(!matches_app_approve(&Key::Ctrl('q'), &manager));
    }
}
