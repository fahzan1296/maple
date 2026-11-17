//! Reference: `l/src/keybindings.ts`, `l/src/keys.ts`.
use crate::keys::Key;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub fn canonical_key_id(modifiers: &[&str], base: &str) -> String {
    let order = ["ctrl", "shift", "alt", "super"];
    let mut mods: Vec<String> = modifiers
        .iter()
        .map(|m| m.to_ascii_lowercase())
        .filter(|m| order.contains(&m.as_str()))
        .collect();
    mods.sort_by_key(|m| order.iter().position(|o| o == m).unwrap_or(usize::MAX));
    let original = base.to_string();
    let mut base = base.to_ascii_lowercase();
    if base == "esc" {
        base = "escape".into();
    }
    if base == "return" {
        base = "enter".into();
    }
    if original.chars().count() == 1
        && original.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && !mods.iter().any(|m| m == "shift")
    {
        mods.push("shift".into());
    }
    let mut parts = mods;
    parts.push(base);
    parts.join("+")
}

/// Canonical id for a decoded key.
pub fn key_id(key: &Key) -> Option<String> {
    Some(match key {
        Key::Char(c) => canonical_key_id(&[], &c.to_string()),
        Key::Ctrl(c) => canonical_key_id(&["ctrl"], &c.to_string()),
        Key::Alt(c) => canonical_key_id(&["alt"], &c.to_string()),
        Key::Shift(c) => canonical_key_id(&["shift"], &c.to_string()),
        Key::Enter => "enter".into(),
        Key::Esc => "escape".into(),
        Key::Tab => "tab".into(),
        Key::BackTab => "shift+tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Delete => "delete".into(),
        Key::Insert => "insert".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PageUp => "pageUp".into(),
        Key::PageDown => "pageDown".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::CtrlUp => "ctrl+up".into(),
        Key::CtrlDown => "ctrl+down".into(),
        Key::CtrlLeft => "ctrl+left".into(),
        Key::CtrlRight => "ctrl+right".into(),
        Key::F(n) => format!("f{n}"),
        Key::Paste(_) | Key::Unknown => return None,
    })
}

/// Default bindings (port of `TUI_KEYBINDINGS` + `KEYBINDINGS`).
pub fn default_bindings() -> Vec<(&'static str, Vec<&'static str>)> {
    let rows: Vec<(&str, Vec<&str>)> = vec![
        // editor
        ("tui.editor.cursorLeft", vec!["left", "ctrl+b"]),
        ("tui.editor.cursorRight", vec!["right", "ctrl+f"]),
        ("tui.editor.cursorUp", vec!["up"]),
        ("tui.editor.cursorDown", vec!["down"]),
        ("tui.editor.cursorLineStart", vec!["home", "ctrl+a"]),
        ("tui.editor.cursorLineEnd", vec!["end", "ctrl+e"]),
        (
            "tui.editor.cursorWordLeft",
            vec!["alt+left", "ctrl+left", "alt+b"],
        ),
        (
            "tui.editor.cursorWordRight",
            vec!["alt+right", "ctrl+right", "alt+f"],
        ),
        ("tui.editor.cursorPageUp", vec!["pageUp"]),
        ("tui.editor.cursorPageDown", vec!["pageDown"]),
        ("tui.editor.deleteCharBackward", vec!["backspace"]),
        ("tui.editor.deleteCharForward", vec!["delete"]),
        (
            "tui.editor.deleteWordBackward",
            vec!["ctrl+w", "alt+backspace", "ctrl+backspace"],
        ),
        (
            "tui.editor.deleteWordForward",
            vec!["alt+delete", "ctrl+delete", "alt+d"],
        ),
        ("tui.editor.deleteToLineStart", vec!["ctrl+u"]),
        ("tui.editor.deleteToLineEnd", vec!["ctrl+k"]),
        ("tui.editor.yank", vec!["ctrl+y"]),
        ("tui.editor.yankPop", vec!["alt+y"]),
        ("tui.editor.newline", vec!["enter"]),
        ("tui.input.newLine", vec!["shift+enter", "ctrl+j"]),
        ("tui.editor.tab", vec!["tab"]),
        ("tui.editor.undo", vec!["ctrl+z"]),
        // input
        ("tui.input.submit", vec!["enter"]),
        ("tui.input.copy", vec!["ctrl+c"]),
        ("tui.select.cancel", vec!["escape", "ctrl+c"]),
        ("tui.select.up", vec!["up", "ctrl+p"]),
        ("tui.select.down", vec!["down", "ctrl+n"]),
        ("tui.select.pageUp", vec!["pageUp"]),
        ("tui.select.pageDown", vec!["pageDown"]),
        // app
        ("app.interrupt", vec!["escape"]),
        ("app.clear", vec!["ctrl+c"]),
        ("app.exit", vec!["ctrl+d"]),
        ("app.suspend", vec!["ctrl+z"]),
        ("app.display.reset", vec!["alt+l"]),
        ("app.thinking.cycle", vec!["shift+tab"]),
        ("app.thinking.toggle", vec!["ctrl+t"]),
        ("app.model.cycleForward", vec!["ctrl+p"]),
        ("app.model.cycleBackward", vec!["shift+ctrl+p"]),
        ("app.select", vec!["alt+m"]),
        ("app.selectTemporary", vec!["alt+p"]),
        ("app.tools.expand", vec!["ctrl+o"]),
        ("app.tools.toggleVisibility", vec!["shift+ctrl+o"]),
        ("app.ask.hide", vec!["alt+h"]),
        ("app.ask.note", vec!["n", "alt+n"]),
        ("app.approval.approve", vec!["ctrl+enter", "ctrl+q"]),
        ("app.editor.external", vec!["ctrl+g"]),
        ("app.local.open", vec!["alt+g"]),
        ("app.followUp", vec!["ctrl+q", "ctrl+enter"]),
        ("app.retry", vec!["f5", "alt+r"]),
        ("app.dequeue", vec!["alt+up", "shift+up"]),
        ("app.btw.inject", vec!["alt+i"]),
        // sessions
        ("app.sessions.observe", vec!["ctrl+s"]),
        ("app.sessions.togglePath", vec!["ctrl+p"]),
        ("app.sessions.toggleSort", vec!["ctrl+s"]),
        ("app.sessions.rename", vec!["ctrl+r"]),
        ("app.sessions.delete", vec!["backspace", "delete"]),
        ("app.sessions.deleteNoninvasive", vec!["ctrl+backspace"]),
        ("app.sessions.pin", vec!["alt+k"]),
        ("app.sessions.unpin", vec!["alt+shift+k"]),
        ("app.sessions.plan.toggle", vec!["alt+shift+p"]),
        ("app.sessions.history.search", vec!["ctrl+r"]),
        ("app.sessions.links.hint", vec!["alt+o"]),
        ("app.sessions.stt.pushToTalk", vec!["space"]),
        ("app.sessions.live.toggle", vec!["ctrl+l"]),
    ];
    rows.into_iter().map(|(id, keys)| (id, keys)).collect()
}

/// Resolved binding set with per-id key lists.
#[derive(Clone, Debug, Default)]
pub struct KeybindingsManager {
    defaults: BTreeMap<String, Vec<String>>,
    user: BTreeMap<String, Option<Vec<String>>>,
    resolved: BTreeMap<String, Vec<String>>,
    conflicts: BTreeSet<String>,
}

impl KeybindingsManager {
    pub fn new() -> Self {
        let mut manager = Self::default();
        for (id, keys) in default_bindings() {
            manager.defaults.insert(id.into(), keys.into_iter().map(String::from).collect());
        }
        manager.rebuild();
        manager
    }

    /// Rebuild the resolved map after changing user bindings.
    pub fn rebuild(&mut self) {
        let mut resolved: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut conflicts: BTreeSet<String> = BTreeSet::new();
        let mut claimed: HashMap<String, String> = HashMap::new();
        for (id, keys) in &self.defaults {
            let effective = match self.user.get(id) {
                Some(Some(keys)) => keys.clone(),
                Some(None) => Vec::new(),
                None => keys.clone(),
            };
            for key in &effective {
                if let Some(other) = claimed.get(key) {
                    if other != id {
                        conflicts.insert(key.clone());
                    }
                } else {
                    claimed.insert(key.clone(), id.clone());
                }
            }
            resolved.insert(id.clone(), effective);
        }
        for (id, binding) in &self.user {
            if let Some(keys) = binding {
                for key in keys {
                    if let Some(other) = claimed.get(key) {
                        if other != id {
                            conflicts.insert(key.clone());
                        }
                    } else {
                        claimed.insert(key.clone(), id.clone());
                    }
                }
                if !resolved.contains_key(id) {
                    resolved.insert(id.clone(), keys.clone());
                }
            }
        }
        self.resolved = resolved;
        self.conflicts = conflicts;
    }

    /// Does `key` match binding `id`?
    pub fn matches(&self, key: &Key, id: &str) -> bool {
        let Some(id_string) = key_id(key) else {
            return false;
        };
        self.resolved
            .get(id)
            .map(|keys| keys.iter().any(|k| *k == id_string))
            .unwrap_or(false)
    }

    /// Resolved keys for `id` (user overrides applied).
    pub fn get_keys(&self, id: &str) -> Vec<String> {
        self.resolved.get(id).cloned().unwrap_or_default()
    }

    /// Keys with conflicting claims.
    pub fn conflicts(&self) -> &BTreeSet<String> {
        &self.conflicts
    }

    /// Set a user binding: `None` or empty list unbinds.
    pub fn set_user_binding(&mut self, id: impl Into<String>, keys: Option<Vec<String>>) {
        self.user.insert(id.into(), keys);
        self.rebuild();
    }

    pub fn user_bindings(&self) -> &BTreeMap<String, Option<Vec<String>>> {
        &self.user
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_modifier_order() {
        assert_eq!(canonical_key_id(&["shift", "ctrl"], "D"), "ctrl+shift+d");
        assert_eq!(canonical_key_id(&[], "Esc"), "escape");
        assert_eq!(canonical_key_id(&[], "A"), "shift+a");
    }

    #[test]
    fn matches_defaults_and_overrides() {
        let mut manager = KeybindingsManager::new();
        assert!(manager.matches(&Key::Left, "tui.editor.cursorLeft"));
        assert!(manager.matches(&Key::Ctrl('b'), "tui.editor.cursorLeft"));
        assert!(!manager.matches(&Key::Right, "tui.editor.cursorLeft"));
        manager.set_user_binding("tui.editor.cursorLeft", Some(vec!["ctrl+g".into()]));
        assert!(manager.matches(&Key::Ctrl('g'), "tui.editor.cursorLeft"));
        assert!(!manager.matches(&Key::Left, "tui.editor.cursorLeft"));
        manager.set_user_binding("tui.editor.cursorLeft", None);
        assert!(manager.get_keys("tui.editor.cursorLeft").is_empty());
    }

    #[test]
    fn reports_conflicts() {
        let mut manager = KeybindingsManager::new();
        manager.set_user_binding("app.custom", Some(vec!["ctrl+b".into()]));
        assert!(manager.conflicts().contains("ctrl+b"));
    }
}
