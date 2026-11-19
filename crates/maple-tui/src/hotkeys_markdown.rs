//! `/hotkeys` reference: Markdown sheet and native overlay.
//!
//! Reference: `l/src/hotkeys-markdown.ts`.
//!
//! Deviations forced by Rust:
//!
//! - `HotkeysMarkdownBindings` is a structural `interface` over manager
//!   methods upstream. The crate's [`KeybindingsManager`] has no
//!   `getDisplayString`/`matchesCanonical`, so the equivalents are free
//!   functions here ([`get_display_string`], [`matches_canonical`]) and the
//!   formatter/platform travel explicitly.
//! - `keyHintPlatform()` and `formatKeyHint` read globals upstream; the
//!   [`KeyHintFormatter`] carries the theme and platform.
//! - `nativeOverlay` (`{ role, size, head }`) is host metadata upstream; the
//!   crate exposes [`HotkeysSheetComponent::native_role`] and
//!   [`HotkeysSheetComponent::head`] instead.
//! - `describe()` caches its node and `invalidate()` clears it.
//!   [`NativeDescribe::describe_native`] takes `&self`, so the tree is rebuilt
//!   per call with identical output.
//! - `getMarkdownTheme()` derives a theme-bound markdown palette; the crate
//!   only offers `MarkdownTheme::default()` (as `render/code-cell.rs` does).
//! - `handleNativeEvent` folds the process-global `NativeUiEvent` union; the
//!   crate uses the local [`HotkeysUiEvent`] subset.

use std::cell::RefCell;

use crate::component::Component;
use crate::components::markdown::{Markdown, MarkdownTheme};
use crate::key_hint_format::{KeyHintFormatter, Platform};
use crate::theme::theme_class::Theme;
use crate::keybinding_matchers::matches_select_cancel;
use crate::keybindings::{canonical_key_id, KeybindingsManager};
use crate::keys::Key;
use crate::native::describe::{col, node, span, NativeDescribe};
use crate::native::node::{
    DescribeContext, NativeNode, NodeKind, Props, Span, TableColumn, TableRow, Text,
};
use crate::native::overlay::{action_bar, action_button, ActionButtonOptions};

/// One piece of a shortcut cell: formatted key hints (alternatives, one keycap
/// each), a literal typed token (`#<number>`, `!!`), or connecting prose.
#[derive(Clone, Debug, PartialEq)]
enum HotkeyPart {
    /// Formatted for the markdown table; `action` names the binding the ids
    /// came from.
    Keys {
        keys: Vec<String>,
        ids: Vec<String>,
        action: Option<String>,
    },
    Token {
        token: String,
    },
    Text {
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct HotkeyRow {
    keys: Vec<HotkeyPart>,
    action: String,
}

#[derive(Clone, Debug, PartialEq)]
struct HotkeyGroup {
    title: String,
    rows: Vec<HotkeyRow>,
}

/// Formatted hints for key ids.
fn hints(formatter: &KeyHintFormatter, keys: &[&str]) -> HotkeyPart {
    HotkeyPart::Keys {
        keys: keys.iter().map(|key| formatter.format_key_hint(key)).collect(),
        ids: keys.iter().map(|key| key.to_string()).collect(),
        action: None,
    }
}

/// The markdown cell text of a part; key alternatives join with `/`.
fn markdown_part(
    keybindings: &KeybindingsManager,
    formatter: &KeyHintFormatter,
    part: &HotkeyPart,
) -> String {
    match part {
        HotkeyPart::Text { text } => text.clone(),
        HotkeyPart::Token { token } => format!("`{token}`"),
        HotkeyPart::Keys { keys, action, .. } => {
            let label = match action {
                Some(action) => {
                    let display = get_display_string(keybindings, formatter, action);
                    if display.is_empty() {
                        "Disabled".to_string()
                    } else {
                        display
                    }
                }
                None => keys.join("/"),
            };
            format!("`{label}`")
        }
    }
}

/// Display string for a keybinding's keys; empty when unbound.
fn get_display_string(
    keybindings: &KeybindingsManager,
    formatter: &KeyHintFormatter,
    action: &str,
) -> String {
    let keys = keybindings.get_keys(action);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    formatter.format_key_hints(&refs)
}

/// Canonicalize a key id string (`Ctrl+D` → `ctrl+d`, `esc` → `escape`).
fn canonicalize_key_id(id: &str) -> String {
    let mut parts: Vec<&str> = id.split('+').collect();
    let base = parts.pop().unwrap_or("");
    canonical_key_id(&parts, base)
}

/// Set-lookup variant of `KeybindingsManager::matches` for an already
/// canonical key id.
fn matches_canonical(keybindings: &KeybindingsManager, canonical: &str, action: &str) -> bool {
    keybindings
        .get_keys(action)
        .iter()
        .any(|key| canonicalize_key_id(key) == canonical)
}

/// An action's effective keys, one keycap each; `Disabled` when unbound.
fn act(keybindings: &KeybindingsManager, formatter: &KeyHintFormatter, action: &str) -> HotkeyPart {
    let keys = keybindings.get_keys(action);
    if !keys.is_empty() {
        let ids: Vec<String> = keys.clone();
        return HotkeyPart::Keys {
            keys: keys.iter().map(|key| formatter.format_key_hint(key.as_str())).collect(),
            ids,
            action: Some(action.to_string()),
        };
    }
    let display = get_display_string(keybindings, formatter, action);
    let label = if display.is_empty() {
        "Disabled".to_string()
    } else {
        display
    };
    HotkeyPart::Keys {
        keys: vec![label],
        ids: Vec::new(),
        action: Some(action.to_string()),
    }
}

fn token(value: &str) -> HotkeyPart {
    HotkeyPart::Token {
        token: value.to_string(),
    }
}

fn text(value: &str) -> HotkeyPart {
    HotkeyPart::Text {
        text: value.to_string(),
    }
}

/// The shortcut groups, built from the effective bindings.
fn hotkey_groups(
    keybindings: &KeybindingsManager,
    formatter: &KeyHintFormatter,
) -> Vec<HotkeyGroup> {
    let is_mac = formatter.platform().is_darwin();
    let start_of_line: &[&str] = if is_mac {
        &["ctrl+a", "home", "super+left"]
    } else {
        &["ctrl+a", "home"]
    };
    let end_of_line: &[&str] = if is_mac {
        &["ctrl+e", "end", "super+right"]
    } else {
        &["ctrl+e", "end"]
    };
    let push_to_talk_keys = keybindings.get_keys("app.stt.pushToTalk");
    // CustomEditor tests the chord that was actually pressed, so exit keys split by role: a key
    // that also carries tui.editor.deleteCharForward (the readline `^D` overlap) forward-deletes
    // while the prompt holds a draft, any other exit key quits immediately.
    let exit_keys = keybindings.get_keys("app.exit");
    let deleting_exit_keys: Vec<String> = exit_keys
        .iter()
        .filter(|key| {
            matches_canonical(
                keybindings,
                &canonicalize_key_id(key.as_str()),
                "tui.editor.deleteCharForward",
            )
        })
        .cloned()
        .collect();
    let quitting_exit_keys: Vec<String> = exit_keys
        .iter()
        .filter(|key| {
            !matches_canonical(
                keybindings,
                &canonicalize_key_id(key.as_str()),
                "tui.editor.deleteCharForward",
            )
        })
        .cloned()
        .collect();
    let mut exit_rows: Vec<HotkeyRow> = Vec::new();
    if !deleting_exit_keys.is_empty() {
        let deleting_refs: Vec<&str> =
            deleting_exit_keys.iter().map(String::as_str).collect();
        let keys = hints(formatter, &deleting_refs);
        exit_rows.push(HotkeyRow {
            keys: vec![keys],
            action: "Delete char forward (with draft) / exit (empty prompt)".to_string(),
        });
    }
    if !quitting_exit_keys.is_empty() || deleting_exit_keys.is_empty() {
        let keys = if !quitting_exit_keys.is_empty() {
            let refs: Vec<&str> = quitting_exit_keys.iter().map(String::as_str).collect();
            hints(formatter, &refs)
        } else {
            HotkeyPart::Keys {
                keys: vec!["Disabled".to_string()],
                ids: Vec::new(),
                action: None,
            }
        };
        exit_rows.push(HotkeyRow {
            keys: vec![keys],
            action: "Exit".to_string(),
        });
    }

    let groups = vec![
        HotkeyGroup {
            title: "Navigation".to_string(),
            rows: vec![
                HotkeyRow {
                    keys: vec![hints(formatter, &["up", "down", "left", "right"])],
                    action: format!(
                        "Move cursor / browse history ({} when empty)",
                        formatter.format_key_hint("up")
                    ),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, &["alt+left", "alt+right"])],
                    action: "Move by word".to_string(),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, start_of_line)],
                    action: "Start of line".to_string(),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, end_of_line)],
                    action: "End of line".to_string(),
                },
            ],
        },
        HotkeyGroup {
            title: "Editing".to_string(),
            rows: vec![
                HotkeyRow {
                    keys: vec![hints(formatter, &["enter"])],
                    action: "Send message".to_string(),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, &["shift+enter", "alt+enter"])],
                    action: "New line".to_string(),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, &["ctrl+w", "alt+backspace"])],
                    action: "Delete word backwards".to_string(),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, &["ctrl+u"])],
                    action: "Delete to start of line".to_string(),
                },
                HotkeyRow {
                    keys: vec![hints(formatter, &["ctrl+k"])],
                    action: "Delete to end of line".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.clipboard.copyLine")],
                    action: "Copy current line".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.clipboard.copyPrompt")],
                    action: "Copy whole prompt".to_string(),
                },
            ],
        },
        HotkeyGroup {
            title: "Other".to_string(),
            rows: vec![
                HotkeyRow {
                    keys: vec![hints(formatter, &["tab"])],
                    action: "Path completion / accept autocomplete".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.interrupt")],
                    action: "Cancel autocomplete / interrupt active work".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.clear")],
                    action: "Clear editor (first) / exit (second)".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.suspend")],
                    action: "Suspend to background".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.display.reset")],
                    action: "Reset terminal display".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.thinking.cycle")],
                    action: "Cycle thinking level".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.model.cycleForward")],
                    action: "Cycle role models (slow/default/smol)".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.model.cycleBackward")],
                    action: "Cycle role models (backward)".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.model.selectTemporary")],
                    action: "Select model (temporary)".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.model.select")],
                    action: "Select model (set roles)".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.plan.toggle")],
                    action: "Toggle plan mode".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.history.search")],
                    action: "Search prompt history".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.tools.expand")],
                    action: "Toggle tool output expansion".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.tools.toggleVisibility")],
                    action: "Toggle tool activity visibility".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.links.hint")],
                    action: "Pick an extension link on screen by its tag; press again to browse the whole transcript".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.ask.hide")],
                    action: "Hide/show a pending ask question".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.ask.note")],
                    action: "Add or edit a note on an ask option".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.approval.approve")],
                    action: "Approve a pending tool call".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.thinking.toggle")],
                    action: "Toggle thinking block visibility".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.editor.external")],
                    action: "Edit message in external editor".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.local.open")],
                    action: "Open a local:// file in external editor".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.retry")],
                    action: "Retry last failed assistant turn".to_string(),
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.clipboard.pasteImage")],
                    action: "Paste image or text from clipboard".to_string(),
                },
                HotkeyRow {
                    keys: if push_to_talk_keys.is_empty() {
                        vec![act(keybindings, formatter, "app.stt.pushToTalk")]
                    } else {
                        vec![
                            text("Hold "),
                            act(keybindings, formatter, "app.stt.pushToTalk"),
                        ]
                    },
                    action: if push_to_talk_keys.is_empty() {
                        "Speech-to-text (push-to-talk) (disabled)".to_string()
                    } else {
                        "Speech-to-text (push-to-talk): hold to record, release to transcribe".to_string()
                    },
                },
                HotkeyRow {
                    keys: vec![act(keybindings, formatter, "app.live.toggle")],
                    action: "Start/stop live voice mode (/live)".to_string(),
                },
                HotkeyRow {
                    keys: vec![
                        act(keybindings, formatter, "app.agents.hub"),
                        text(" / "),
                        act(keybindings, formatter, "app.session.observe"),
                        text(" / double-tap "),
                        hints(formatter, &["left"]),
                        text(" (empty editor)"),
                    ],
                    action: "Open the agent hub".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("#<number>")],
                    action:
                        "GitHub issue/PR reference (e.g. `#3164` → `pr://`/`issue://`)".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("#"), text(" / "), token("#<text>")],
                    action: "Prompt actions (copy / undo / move cursor)".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("/")],
                    action: "Slash commands".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("!")],
                    action: "Run bash command".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("!!")],
                    action: "Run bash command (excluded from context)".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("$")],
                    action: "Run Python in shared kernel".to_string(),
                },
                HotkeyRow {
                    keys: vec![token("$$")],
                    action: "Run Python (excluded from context)".to_string(),
                },
            ],
        },
    ];
    // The exit rows belong to "Other", after app.clear.
    let mut groups = groups;
    let other = &mut groups[2];
    let mut rows = Vec::new();
    rows.extend(other.rows.drain(..3));
    rows.extend(exit_rows);
    rows.extend(other.rows.drain(..));
    groups[2].rows = rows;
    groups
}

/// Build the platform-aware Markdown reference for effective application hotkeys.
pub fn build_hotkeys_markdown(
    keybindings: &KeybindingsManager,
    formatter: &KeyHintFormatter,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (index, group) in hotkey_groups(keybindings, formatter).iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.push(format!("**{}**", group.title));
        lines.push("| Key | Action |".to_string());
        lines.push("|-----|--------|".to_string());
        for row in &group.rows {
            let keys: Vec<String> = row
                .keys
                .iter()
                .map(|part| markdown_part(keybindings, formatter, part))
                .collect();
            lines.push(format!("| {} | {} |", keys.join(""), row.action));
        }
    }
    lines.join("\n")
}

/// A shortcut cell as spans: each key alternative a `key` span holding its key
/// id, literal tokens as code, prose dim.
fn key_spans(parts: &[HotkeyPart]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for part in parts {
        match part {
            HotkeyPart::Text { text } => spans.push(span(text.clone(), Some("dim"))),
            HotkeyPart::Token { token } => spans.push(span(token.clone(), Some("code"))),
            HotkeyPart::Keys { keys, ids, .. } => {
                if ids.is_empty() {
                    spans.push(span(keys.join("/"), Some("dim")));
                } else {
                    for (index, id) in ids.iter().enumerate() {
                        if index > 0 {
                            spans.push(span(" ", Some("dim")));
                        }
                        spans.push(span(id.clone(), Some("key")));
                    }
                }
            }
        }
    }
    spans
}

/// Inline markdown (backticks) of an action description as spans: code runs
/// mono, the rest plain.
fn action_spans(action: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut index = 0;
    let bytes = action.as_bytes();
    let mut current = String::new();
    while index < bytes.len() {
        if bytes[index] == b'`' {
            if !current.is_empty() {
                spans.push(span(current.clone(), None));
                current.clear();
            }
            if let Some(end) = action[index + 1..].find('`') {
                let end = index + 1 + end;
                if end > index + 1 {
                    spans.push(span(action[index + 1..end].to_string(), Some("code")));
                    index = end + 1;
                    continue;
                }
            }
        }
        let ch = action[index..].chars().next().unwrap();
        current.push(ch);
        index += ch.len_utf8();
    }
    if !current.is_empty() {
        spans.push(span(current, None));
    }
    spans
}

/// A user action on a described node: the hotkey-sheet subset of
/// `NativeUiEvent`.
#[derive(Clone, Debug, PartialEq)]
pub enum HotkeysUiEvent {
    /// An action control was clicked.
    Action {
        /// The node's keypath inside the component.
        key: String,
        /// The action id.
        act: String,
        /// Scope/tab/strip id, when the action carries one.
        value: Option<String>,
        /// Modifier keys held.
        mods: Vec<String>,
    },
    /// Any other event; [`HotkeysSheetComponent::handle_native_event`] ignores it.
    Other {
        /// The node's keypath inside the component.
        key: String,
    },
}

/// `/hotkeys` under a native terminal: a large sheet "Keyboard shortcuts" with
/// one section per group, each a two-column table whose key cells carry `key`
/// spans (Tern draws them as keycaps). Esc (or Close) dismisses it.
pub struct HotkeysSheetComponent<'a> {
    keybindings: &'a KeybindingsManager,
    formatter: KeyHintFormatter<'a>,
    on_close: RefCell<Box<dyn FnMut()>>,
    markdown: Option<Markdown>,
}

impl<'a> HotkeysSheetComponent<'a> {
    /// Create the sheet; `platform` pins keycap glyphs (`None` = host).
    pub fn new(
        keybindings: &'a KeybindingsManager,
        theme: &'a Theme,
        platform: Option<Platform>,
        on_close: impl FnMut() + 'static,
    ) -> Self {
        let formatter = match platform {
            Some(platform) => KeyHintFormatter::with_platform(theme, platform),
            None => KeyHintFormatter::new(theme),
        };
        Self {
            keybindings,
            formatter,
            on_close: RefCell::new(Box::new(on_close)),
            markdown: None,
        }
    }

    /// The semantic role reported to the native host.
    pub fn native_role(&self) -> &str {
        "omp.overlay.hotkeys"
    }

    /// The overlay title.
    pub fn head(&self) -> &str {
        "Keyboard shortcuts"
    }

    /// The markdown reference, for a terminal that falls back to rows.
    pub fn markdown(&self) -> Option<&Markdown> {
        self.markdown.as_ref()
    }

    /// Close runs what Esc runs.
    pub fn handle_native_event(&self, event: &HotkeysUiEvent) {
        if let HotkeysUiEvent::Action { act, .. } = event {
            if act == "close" {
                (self.on_close.borrow_mut())();
            }
        }
    }
}

impl Component for HotkeysSheetComponent<'_> {
    fn render(&mut self, width: u16) -> Vec<String> {
        if self.markdown.is_none() {
            let mut markdown = Markdown::new();
            markdown.set_text(build_hotkeys_markdown(self.keybindings, &self.formatter));
            markdown.set_padding(1, 1);
            markdown.set_theme(MarkdownTheme::default());
            self.markdown = Some(markdown);
        }
        self.markdown.as_mut().expect("markdown initialized").render(width)
    }

    fn handle_input(&mut self, key: &Key) {
        if matches_select_cancel(key, self.keybindings) {
            (self.on_close.borrow_mut())();
        }
    }

    fn invalidate(&mut self) {
        self.markdown = None;
    }
}

impl NativeDescribe for HotkeysSheetComponent<'_> {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let sections: Vec<NativeNode> = hotkey_groups(self.keybindings, &self.formatter)
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let mut action_column = TableColumn::new("action");
                action_column.head = Some(Text::Plain("Action".to_string()));
                action_column.grow = Some(1.0);
                let cols = vec![
                    TableColumn::new("keys").with_head("Key").to_value(),
                    action_column.to_value(),
                ];
                let rows: Vec<serde_json::Value> = group
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row_index, row)| {
                        let mut table_row = TableRow::new(row_index.to_string());
                        table_row.cells.push((
                            "keys".to_string(),
                            Text::spans(key_spans(&row.keys)),
                        ));
                        table_row.cells.push((
                            "action".to_string(),
                            Text::spans(action_spans(&row.action)),
                        ));
                        table_row.to_value()
                    })
                    .collect();
                let mut table_props = Props::new();
                table_props.set_role("omp.hotkeys.table");
                table_props.insert("cols", serde_json::Value::Array(cols));
                table_props.insert("rows", serde_json::Value::Array(rows));
                let table = node(NodeKind::Table, table_props, Vec::new(), None);

                let mut section_props = Props::new();
                section_props.set_role("omp.hotkeys.group");
                section_props.set_head(Text::Plain(group.title.clone()));
                let key = format!("g{index}");
                node(NodeKind::Section, section_props, vec![table], Some(key.as_str()))
            })
            .collect();
        let close = action_button(
            "Close",
            "close",
            ActionButtonOptions {
                keys: Some("escape".to_string()),
                ..ActionButtonOptions::default()
            },
        );
        let mut props = Props::new();
        props.set_str("gap", "lg");
        let mut children = sections;
        children.push(action_bar(&[None, Some(close)], "actions"));
        col(children, props)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_hint_format::test_theme;
    use crate::theme::symbols::SymbolPreset;
    use std::cell::Cell;
    use std::rc::Rc;

    fn formatter(theme: &Theme, platform: Platform) -> KeyHintFormatter<'_> {
        KeyHintFormatter::with_platform(theme, platform)
    }

    #[test]
    fn builds_markdown_groups_and_rows() {
        let theme = test_theme(SymbolPreset::Unicode);
        let formatter = formatter(&theme, Platform::Other);
        let manager = KeybindingsManager::new();
        let markdown = build_hotkeys_markdown(&manager, &formatter);
        for expected in [
            "**Navigation**",
            "**Editing**",
            "**Other**",
            "| Key | Action |",
            "|-----|--------|",
            "| `⏎` | Send message |",
            "| `⎋` | Cancel autocomplete / interrupt active work |",
            "| `Ctrl+D` | Exit |",
            "| `Disabled` | Toggle plan mode |",
            "| `⇥` | Path completion / accept autocomplete |",
            "| `#<number>` | GitHub issue/PR reference (e.g. `#3164` → `pr://`/`issue://`) |",
        ] {
            assert!(markdown.contains(expected), "missing {expected:?} in:\n{markdown}");
        }
        assert!(markdown.contains("| `↑↓←→` |"));
    }

    #[test]
    fn mac_adds_super_arrow_keys() {
        let theme = test_theme(SymbolPreset::Unicode);
        let formatter = formatter(&theme, Platform::Darwin);
        let manager = KeybindingsManager::new();
        let markdown = build_hotkeys_markdown(&manager, &formatter);
        assert!(markdown.contains("⌘←"));
        assert!(markdown.contains("⌘→"));
    }

    #[test]
    fn unbound_exit_action_reports_disabled() {
        let theme = test_theme(SymbolPreset::Unicode);
        let formatter = formatter(&theme, Platform::Other);
        let mut manager = KeybindingsManager::new();
        manager.set_user_binding("app.exit", None);
        let markdown = build_hotkeys_markdown(&manager, &formatter);
        assert!(markdown.contains("| `Disabled` | Exit |"));
    }

    #[test]
    fn exit_key_matching_delete_splits_rows() {
        let theme = test_theme(SymbolPreset::Unicode);
        let formatter = formatter(&theme, Platform::Other);
        let mut manager = KeybindingsManager::new();
        manager.set_user_binding("app.exit", Some(vec!["ctrl+d".into(), "ctrl+q".into()]));
        let markdown = build_hotkeys_markdown(&manager, &formatter);
        assert!(markdown.contains("Delete char forward (with draft) / exit (empty prompt)"));
        assert!(markdown.contains("| `Ctrl+D` | Delete char forward"));
        assert!(markdown.contains("| `Ctrl+Q` | Exit |"));
    }

    #[test]
    fn key_spans_style_keys_tokens_and_prose() {
        let parts = vec![
            HotkeyPart::Keys {
                keys: vec!["Ctrl+R".to_string()],
                ids: vec!["ctrl+r".to_string()],
                action: None,
            },
            HotkeyPart::Token {
                token: "#<number>".to_string(),
            },
            HotkeyPart::Text {
                text: " / ".to_string(),
            },
            HotkeyPart::Keys {
                keys: vec!["Disabled".to_string()],
                ids: Vec::new(),
                action: None,
            },
        ];
        let spans = key_spans(&parts);
        let styles: Vec<Option<&str>> = spans.iter().map(|span| span.s.as_deref()).collect();
        assert_eq!(
            styles,
            vec![
                Some("key"),
                Some("code"),
                Some("dim"),
                Some("dim"),
            ]
        );
    }

    #[test]
    fn action_spans_split_inline_code() {
        let spans = action_spans("GitHub issue/PR reference (e.g. `#3164` → `pr://`/`issue://`)");
        let styled: Vec<(String, Option<&str>)> = spans
            .iter()
            .map(|span| (span.t.clone(), span.s.as_deref()))
            .collect();
        assert_eq!(
            styled,
            vec![
                ("GitHub issue/PR reference (e.g. ".to_string(), None),
                ("#3164".to_string(), Some("code")),
                (" → ".to_string(), None),
                ("pr://".to_string(), Some("code")),
                ("/".to_string(), None),
                ("issue://".to_string(), Some("code")),
                (")".to_string(), None),
            ]
        );
        assert!(action_spans("plain")
            .iter()
            .all(|span| span.s.as_deref().is_none()));
    }

    #[test]
    fn sheet_closes_on_escape_and_close_action() {
        let theme = test_theme(SymbolPreset::Unicode);
        let manager = KeybindingsManager::new();
        let closes = Rc::new(Cell::new(0));
        let counter = Rc::clone(&closes);
        let mut sheet = HotkeysSheetComponent::new(&manager, &theme, None, move || {
            counter.set(counter.get() + 1);
        });
        assert_eq!(sheet.native_role(), "omp.overlay.hotkeys");
        assert_eq!(sheet.head(), "Keyboard shortcuts");
        sheet.handle_input(&Key::Esc);
        assert_eq!(closes.get(), 1);
        sheet.handle_input(&Key::Enter);
        assert_eq!(closes.get(), 1);
        sheet.handle_native_event(&HotkeysUiEvent::Action {
            key: "actions".to_string(),
            act: "close".to_string(),
            value: None,
            mods: Vec::new(),
        });
        assert_eq!(closes.get(), 2);
        sheet.handle_native_event(&HotkeysUiEvent::Other {
            key: "g0".to_string(),
        });
        assert_eq!(closes.get(), 2);
    }

    #[test]
    fn sheet_renders_markdown_and_invalidates() {
        let theme = test_theme(SymbolPreset::Unicode);
        let manager = KeybindingsManager::new();
        let mut sheet = HotkeysSheetComponent::new(&manager, &theme, None, || {});
        let rows = sheet.render(80);
        assert!(rows.iter().any(|row| row.contains("Keyboard shortcuts") || row.contains("Key")));
        assert!(sheet.markdown().is_some());
        sheet.invalidate();
        assert!(sheet.markdown().is_none());
        let rows = sheet.render(80);
        assert!(!rows.is_empty());
    }

    #[test]
    fn describe_builds_sections_and_tables() {
        let theme = test_theme(SymbolPreset::Unicode);
        let manager = KeybindingsManager::new();
        let sheet = HotkeysSheetComponent::new(&manager, &theme, None, || {});
        let cx = DescribeContext::default();
        let root = sheet.describe_native(&cx);
        assert_eq!(root.kind, NodeKind::Col);
        let sections: Vec<&NativeNode> = root
            .children
            .iter()
            .filter(|child| child.kind == NodeKind::Section)
            .collect();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].props.role().as_deref(), Some("omp.hotkeys.group"));
        let table = &sections[0].children[0];
        assert_eq!(table.kind, NodeKind::Table);
        assert_eq!(table.props.role().as_deref(), Some("omp.hotkeys.table"));
    }
}
