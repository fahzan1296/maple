//! Overlay chrome for `describe()`: root cards, key-hint footers, buttons and
//! segmented controls, selectable lists and annotation callouts.
//!
//! The overlay frame itself (glass, ring, placement) is the terminal's: the
//! engine wraps every overlay-stack component in an `overlay` node. An overlay
//! component's root is a `card` whose `head` carries the title.
//!
//! Reference: `l/src/native/overlay.ts` (builders), with `formatTooltipKey`
//! from `l/src/key-hint-format.ts`, the keybinding registry from
//! `l/src/keybindings.ts`, and the `nativeButtons` switch from
//! `l/src/native/state.ts`.
//!
//! Deviations:
//! - The reference reads the process-global keybinding registry
//!   (`getKeybindings()`); maple-tui has no global UI state, so
//!   [`action_hint`] takes the [`KeybindingsManager`] explicitly.
//! - `formatTooltipKey` resolves the host platform from
//!   `std::env::consts::OS` (`"macos"` for the reference's `process.platform`
//!   `"darwin"`). The pure [`format_key_with`] carries the flag so tests are
//!   deterministic.
//! - The button builders read the live terminal's `nativeButtons` switch
//!   through [`crate::native::state::native_state`]; the node construction is
//!   factored into `*_with(..., native: bool)` helpers so both branches are
//!   testable without mutating process state.
//! - `itemIndex`'s JavaScript `Number()` accepts hex and other non-decimal
//!   spellings Rust's parsers reject; this port parses decimal integers and
//!   finite decimals only (the empty string is `0`, as in JavaScript).

use serde_json::{Map, Value};

use crate::keybindings::KeybindingsManager;
use crate::native::describe::{card, kbd, key_names, list, node, row, span, text};
use crate::native::node::{NativeNode, NodeKind, Props, Span, Text as NodeText};
use crate::native::state::native_state;
use crate::theme::symbols::{preset_symbols, SymbolMap, SymbolPreset};

/// Below this many columns a centered sheet with tall content fills the pane
/// instead of floating with margins.
pub const NARROW_SHEET_COLS: u16 = 100;

/// One footer hint: the keys that trigger it and what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeHint {
    /// Key ids (`"ctrl+o"`, `"escape"`); alternatives render side by side.
    pub keys: Vec<String>,
    /// What the keys do.
    pub label: String,
}

/// The keybinding actions [`action_hint`] may read: one id or several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actions<'a> {
    /// One action id.
    One(&'a str),
    /// Several action ids; each contributes its primary key.
    Many(&'a [&'a str]),
}

impl<'a> From<&'a str> for Actions<'a> {
    fn from(action: &'a str) -> Self {
        Actions::One(action)
    }
}

impl<'a> From<&'a [&'a str]> for Actions<'a> {
    fn from(actions: &'a [&'a str]) -> Self {
        Actions::Many(actions)
    }
}

impl<'a, const N: usize> From<&'a [&'a str; N]> for Actions<'a> {
    fn from(actions: &'a [&'a str; N]) -> Self {
        Actions::Many(actions)
    }
}

impl<'a> Actions<'a> {
    fn iter(self) -> ActionsIter<'a> {
        match self {
            Actions::One(action) => ActionsIter {
                one: Some(action),
                many: &[],
            },
            Actions::Many(actions) => ActionsIter {
                one: None,
                many: actions,
            },
        }
    }
}

/// Iterator over [`Actions`].
struct ActionsIter<'a> {
    one: Option<&'a str>,
    many: &'a [&'a str],
}

impl<'a> Iterator for ActionsIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(action) = self.one.take() {
            return Some(action);
        }
        let (first, rest) = self.many.split_first()?;
        self.many = rest;
        Some(first)
    }
}

/// A hint for the primary keys of one or more keybinding actions
/// (`action_hint(["tui.select.up", "tui.select.down"], "navigate")`);
/// `None` when every action is unbound.
pub fn action_hint<'a>(
	bindings: &KeybindingsManager,
	actions: impl Into<Actions<'a>>,
	label: &str,
) -> Option<NativeHint> {
	let mut keys: Vec<String> = Vec::new();
	for action in actions.into().iter() {
        if let Some(key) = bindings.get_keys(action).first() {
            keys.push(key.clone());
        }
    }
    if keys.is_empty() {
        None
    } else {
        Some(NativeHint {
            keys,
            label: label.to_string(),
        })
    }
}

/// A wrapping row of `kbd` keycaps with muted labels: the footer hint strip of
/// a selector. `None` entries (unbound actions) are skipped.
pub fn hints_row(hints: &[Option<NativeHint>], key: &str) -> NativeNode {
    let mut children: Vec<NativeNode> = Vec::new();
    for hint in hints.iter().flatten() {
        let mut group: Vec<NativeNode> = hint.keys.iter().map(|k| kbd(k, None)).collect();
        group.push(text(
            NodeText::Spans(vec![Span::styled(hint.label.clone(), "muted")]),
            Props::new(),
        ));
        let mut props = Props::new();
        props.set_str("gap", "xs");
        props.set_str("align", "center");
        children.push(row(group, props));
    }
    let mut props = Props::new();
    props.set_str("gap", "md");
    props.set_bool("wrap", true);
    props.set_role("omp.overlay.hints");
    node(NodeKind::Row, props, children, Some(key))
}

/// A wrapping footer: a status line (`✓ Clean`, `⚠ Cancelled`) followed by key hints.
pub fn status_hints_row(status: NodeText, hints: &[Option<NativeHint>]) -> NativeNode {
    let mut props = Props::new();
    props.set_str("gap", "md");
    props.set_bool("wrap", true);
    props.set_str("align", "center");
    row(vec![text(status, Props::new()), hints_row(hints, "hints")], props)
}

/// Root card of an overlay. `head` is the overlay title (whitespace runs
/// collapse; empty means no head); `role` is `omp.overlay.<name>`.
pub fn overlay_card(
    role: &str,
    head: Option<NodeText>,
    children: Vec<NativeNode>,
    props: Option<Props>,
) -> NativeNode {
    let title = match head {
        Some(NodeText::Plain(text)) => {
            let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if collapsed.is_empty() {
                None
            } else {
                Some(NodeText::Plain(collapsed))
            }
        }
        other => other,
    };
    let mut card_props = props.unwrap_or_default();
    card_props.set_role(role);
    if let Some(title) = title {
        card_props.set_head(title);
    }
    card(card_props, children)
}

/// A selectable list: click selects, double-click activates.
pub fn select_list(key: &str, items: Vec<NativeNode>, mut props: Props) -> NativeNode {
    props.insert("actions", list_actions("select", "activate"));
    list(items, props, Some(key))
}

/// The `actions` prop of a selectable list.
fn list_actions(click: &str, dblclick: &str) -> Value {
    let mut actions = Map::new();
    actions.insert("click".to_string(), Value::String(click.to_string()));
    actions.insert("dblclick".to_string(), Value::String(dblclick.to_string()));
    Value::Object(actions)
}

/// The index encoded in an item key built as `<prefix><index>`, or -1 when it
/// isn't one.
pub fn item_index(item: &str, prefix: &str) -> i64 {
    let Some(rest) = item.strip_prefix(prefix) else {
        return -1;
    };
    match javascript_integer(rest) {
        Some(index) if index >= 0 => index,
        _ => -1,
    }
}

/// `Number(text)` as an integer, for [`item_index`]. The empty string is `0`.
fn javascript_integer(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(0);
    }
    if let Ok(index) = trimmed.parse::<i64>() {
        return Some(index);
    }
    let number: f64 = trimmed.parse().ok()?;
    if number.is_finite() && number.fract() == 0.0 {
        Some(number as i64)
    } else {
        None
    }
}

/// The tone an [`action_button`] opts into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionButtonTone {
    /// The view's hero action.
    Accent,
    /// Destructive.
    Error,
}

impl ActionButtonTone {
    fn as_str(self) -> &'static str {
        match self {
            ActionButtonTone::Accent => "accent",
            ActionButtonTone::Error => "error",
        }
    }
}

/// Options of an [`action_button`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionButtonOptions {
    /// Sibling identity key (defaults to the action id).
    pub key: Option<String>,
    /// Key id of the keyboard path the button mirrors, drawn as a keycap
    /// inside it (`"r"`, `"ctrl+g"`).
    pub keys: Option<String>,
    /// `accent` = the view's hero action, `error` = destructive.
    pub tone: Option<ActionButtonTone>,
    /// Tooltip; defaults to the label plus the keycap.
    pub title: Option<String>,
    /// Target of an `open` action (URL or `file://` path).
    pub href: Option<String>,
    /// Why the action can't run now: the button dims and a click sends nothing.
    pub disabled: Option<String>,
    /// The button's work is running: a spinner leads the label.
    pub busy: bool,
    /// Icon beside the label (before it on a `button` node, after it on a row).
    pub icon: Option<String>,
}

/// A native button (role `omp.btn`): a label plus an optional keycap.
pub fn action_button(label: &str, act: &str, options: ActionButtonOptions) -> NativeNode {
    action_button_with(label, act, options, native_state().native_buttons())
}

/// [`action_button`] with the terminal's `button` support made explicit.
fn action_button_with(label: &str, act: &str, options: ActionButtonOptions, native: bool) -> NativeNode {
    let title = options
        .title
        .clone()
        .or_else(|| {
            options
                .keys
                .as_ref()
                .map(|keys| format!("{}  {}", label, format_tooltip_key(keys)))
        })
        .unwrap_or_else(|| label.to_string());
    let tone = if options.disabled.is_some() {
        Some("muted")
    } else {
        options.tone.map(ActionButtonTone::as_str)
    };
    let key = options.key.clone().unwrap_or_else(|| act.to_string());
    if native {
        let mut props = Props::new();
        props.set_role("omp.btn");
        props.set_str("text", label);
        props.insert("actions", click_action(act));
        props.set_str("title", title.as_str());
        if let Some(keys) = &options.keys {
            props.insert("keys", strings_value(key_names(keys)));
        }
        if let Some(icon) = &options.icon {
            props.set_str("icon", icon.as_str());
        }
        match tone {
            Some("accent") => {
                props.set_str("variant", "primary");
            }
            Some("error") => {
                props.set_str("variant", "danger");
            }
            _ => {}
        }
        if let Some(tone) = tone {
            props.set_str("tone", tone);
        }
        if let Some(reason) = &options.disabled {
            if reason.is_empty() {
                props.set_bool("disabled", true);
            } else {
                props.set_str("disabled", reason.as_str());
            }
        }
        if options.busy {
            props.set_bool("busy", true);
        }
        if let Some(href) = &options.href {
            props.set_str("href", href.as_str());
        }
        node(NodeKind::Button, props, Vec::new(), Some(key.as_str()))
    } else {
        let mut children: Vec<NativeNode> = Vec::new();
        if options.busy {
            let mut spinner = Props::new();
            spinner.set_str("style", "dots");
            children.push(node(NodeKind::Spinner, spinner, Vec::new(), None));
        }
        children.push(text(NodeText::Plain(label.to_string()), Props::new()));
        if let Some(icon) = &options.icon {
            let mut icon_props = Props::new();
            icon_props.set_str("name", icon.as_str());
            children.push(node(NodeKind::Icon, icon_props, Vec::new(), Some("icon")));
        }
        if let Some(keys) = &options.keys {
            children.push(kbd(keys, None));
        }
        let mut props = Props::new();
        props.set_role("omp.btn");
        props.set_str("gap", "xs");
        props.set_str("align", "center");
        if options.disabled.is_none() {
            props.insert("actions", click_action(act));
        }
        props.set_str("title", title.as_str());
        if let Some(tone) = tone {
            props.set_str("tone", tone);
        }
        if let Some(href) = &options.href {
            props.set_str("href", href.as_str());
        }
        node(NodeKind::Row, props, children, Some(key.as_str()))
    }
}

/// A bare `esc` keycap button for a view's top-right corner: a click sends
/// `act` (default `close`), which the component routes to its Esc path.
pub fn esc_close_button(act: &str, key: &str) -> NativeNode {
    esc_close_button_with(act, key, native_state().native_buttons())
}

/// [`esc_close_button`] with the terminal's `button` support made explicit.
fn esc_close_button_with(act: &str, key: &str, native: bool) -> NativeNode {
    let title = format!("Close  {}", format_tooltip_key("escape"));
    if native {
        let mut props = Props::new();
        props.set_role("omp.btn");
        props.insert("keys", strings_value(vec!["escape".to_string()]));
        props.insert("actions", click_action(act));
        props.set_str("title", title.as_str());
        props.set_str("aria", "Close");
        node(NodeKind::Button, props, Vec::new(), Some(key))
    } else {
        let mut props = Props::new();
        props.set_role("omp.btn");
        props.set_str("align", "center");
        props.insert("actions", click_action(act));
        props.set_str("title", title.as_str());
        node(NodeKind::Row, props, vec![kbd("escape", None)], Some(key))
    }
}

/// Options of an [`icon_button`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconButtonOptions {
    /// Sibling identity key; unkeyed buttons take their position.
    pub key: Option<String>,
    /// Accessible name; defaults to the title.
    pub aria: Option<String>,
    /// A toggle that is on (role `omp.app.ibtn.set`).
    pub on: bool,
}

/// A quiet icon-only button (role `omp.app.ibtn`, the kit's icon button) for an
/// app head or toolbar.
pub fn icon_button(name: &str, act: &str, title: &str, options: IconButtonOptions) -> NativeNode {
    icon_button_with(name, act, title, options, native_state().native_buttons())
}

/// [`icon_button`] with the terminal's `button` support made explicit.
fn icon_button_with(name: &str, act: &str, title: &str, options: IconButtonOptions, native: bool) -> NativeNode {
    let role = if options.on {
        "omp.app.ibtn.set"
    } else {
        "omp.app.ibtn"
    };
    let aria = options.aria.clone().unwrap_or_else(|| title.to_string());
    if native {
        let mut props = Props::new();
        props.set_role(role);
        props.set_str("variant", "quiet");
        props.set_str("icon", name);
        props.set_str("title", title);
        props.set_str("aria", aria.as_str());
        props.insert("actions", click_action(act));
        if options.on {
            props.set_bool("on", true);
        }
        node(NodeKind::Button, props, Vec::new(), options.key.as_deref())
    } else {
        let mut props = Props::new();
        props.set_str("name", name);
        props.set_role(role);
        props.set_str("title", title);
        props.set_str("aria", aria.as_str());
        props.insert("actions", click_action(act));
        node(NodeKind::Icon, props, Vec::new(), options.key.as_deref())
    }
}

/// A quiet text button (role `omp.app.qbtn`, the kit's quiet button) for a
/// section head or toolbar, keyed by its action.
pub fn quiet_button(label: &str, act: &str, title: &str) -> NativeNode {
    quiet_button_with(label, act, title, native_state().native_buttons())
}

/// [`quiet_button`] with the terminal's `button` support made explicit.
fn quiet_button_with(label: &str, act: &str, title: &str, native: bool) -> NativeNode {
    if native {
        let mut props = Props::new();
        props.set_role("omp.app.qbtn");
        props.set_str("variant", "quiet");
        props.set_str("size", "sm");
        props.set_str("text", label);
        props.insert("actions", click_action(act));
        props.set_str("title", title);
        node(NodeKind::Button, props, Vec::new(), Some(act))
    } else {
        let mut props = Props::new();
        props.set_role("omp.app.qbtn");
        props.insert("actions", click_action(act));
        props.set_str("title", title);
        node(
            NodeKind::Row,
            props,
            vec![text(NodeText::Plain(label.to_string()), Props::new())],
            Some(act),
        )
    }
}

/// A segmented control: `tabs` drawn as a well with a raised thumb on the
/// active tab.
pub fn segmented_tabs(props: Props, key: &str) -> NativeNode {
    segmented_tabs_with(props, key, native_state().native_buttons())
}

/// [`segmented_tabs`] with the terminal's `button` support made explicit.
fn segmented_tabs_with(mut props: Props, key: &str, native: bool) -> NativeNode {
    if native {
        props.set_str("variant", "segmented");
    }
    node(NodeKind::Tabs, props, Vec::new(), Some(key))
}

/// A row of [`action_button`]s (role `omp.actions`); `None` entries become the
/// spacer that end-aligns what follows.
pub fn action_bar(buttons: &[Option<NativeNode>], key: &str) -> NativeNode {
    let children = buttons
        .iter()
        .map(|button| {
            button.clone().unwrap_or_else(|| {
                let mut props = Props::new();
                props.set_i64("grow", 1);
                node(NodeKind::Spacer, props, Vec::new(), None)
            })
        })
        .collect();
    let mut props = Props::new();
    props.set_role("omp.actions");
    props.set_str("gap", "sm");
    props.set_str("align", "center");
    node(NodeKind::Row, props, children, Some(key))
}

/// Spans of an annotation callout: a dim `label:` then the note.
pub fn note_spans(label: &str, note: &str) -> Vec<Span> {
    vec![
        span(format!("{}: ", label), Some("dim")),
        span(note, Some("accent")),
    ]
}

/// Format one key for a native tooltip (a TSP `title`): unicode keycap glyphs
/// whatever the theme's symbol preset.
pub fn format_tooltip_key(key: &str) -> String {
    format_key_with(key, std::env::consts::OS == "macos")
}

/// [`format_tooltip_key`] with the platform made explicit.
fn format_key_with(key: &str, mac: bool) -> String {
    if key == "escape" {
        return "esc".to_string();
    }
    let symbols = preset_symbols(SymbolPreset::Unicode);
    // A trailing `+` is the plus key itself (`+`, `ctrl++`), not a separator.
    let mut parts: Vec<&str> = if let Some(prefix) = key.strip_suffix('+') {
        let mut parts: Vec<&str> = prefix.split('+').collect();
        parts.pop();
        parts.push("+");
        parts
    } else {
        key.split('+').collect()
    };
    let base = parts.pop().unwrap_or("");
    let mut out = String::new();
    for modifier in ["ctrl", "alt", "shift", "super"] {
        if !parts.contains(&modifier) {
            continue;
        }
        let label = modifier_symbol(&symbols, modifier, mac).unwrap_or("");
        if has_letter(label) {
            out.push_str(label);
            out.push('+');
        } else {
            out.push_str(label);
            out.push_str(symbols.get("key.joiner").unwrap_or(""));
        }
    }
    let lower = base.to_lowercase();
    if let Some(label) = modifier_symbol(&symbols, &lower, mac) {
        out.push_str(label);
    } else if let Some(glyph) = key_symbol(&symbols, &lower) {
        out.push_str(glyph);
    } else if let Some(word) = key_word(&lower) {
        out.push_str(word);
    } else if base.chars().count() == 1 {
        if parts.is_empty() {
            out.push_str(base);
        } else {
            out.push_str(&base.to_uppercase());
        }
    } else {
        let mut chars = base.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// The glyph for a modifier, if `key` names one.
fn modifier_symbol(symbols: &SymbolMap, key: &str, mac: bool) -> Option<&'static str> {
    let generic = match key {
        "ctrl" => "key.ctrl",
        "alt" => "key.alt",
        "shift" => "key.shift",
        "super" => "key.super",
        _ => return None,
    };
    let mac_key = match key {
        "ctrl" => "key.ctrlMac",
        "alt" => "key.altMac",
        "shift" => "key.shift",
        "super" => "key.superMac",
        _ => return None,
    };
    symbols.get(if mac { mac_key } else { generic })
}

/// The glyph for a named key, if `key` is one.
fn key_symbol(symbols: &SymbolMap, key: &str) -> Option<&'static str> {
    let token = match key {
        "esc" | "escape" => "key.esc",
        "enter" | "return" => "key.enter",
        "space" => "key.space",
        "tab" => "key.tab",
        "backspace" => "key.backspace",
        "delete" => "key.delete",
        "up" => "key.up",
        "down" => "key.down",
        "left" => "key.left",
        "right" => "key.right",
        _ => return None,
    };
    symbols.get(token)
}

/// The spelled-out label for a key with no keycap glyph.
fn key_word(key: &str) -> Option<&'static str> {
    match key {
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("PgUp"),
        "pagedown" => Some("PgDn"),
        "insert" => Some("Ins"),
        _ => None,
    }
}

/// Whether `label` contains an ASCII letter or digit.
fn has_letter(label: &str) -> bool {
    label.chars().any(|ch| ch.is_ascii_alphanumeric())
}

/// An `actions` prop with one `click` target.
fn click_action(act: &str) -> Value {
    let mut actions = Map::new();
    actions.insert("click".to_string(), Value::String(act.to_string()));
    Value::Object(actions)
}

/// A JSON array of strings.
fn strings_value(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(keys: &[&str], label: &str) -> Option<NativeHint> {
        Some(NativeHint {
            keys: keys.iter().map(|k| k.to_string()).collect(),
            label: label.to_string(),
        })
    }

    fn keys_prop(node: &NativeNode) -> Vec<String> {
        node.props
            .get("keys")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn action_hint_collects_primary_keys() {
        let bindings = KeybindingsManager::new();
        let table = [
            (
                Actions::Many(&["tui.select.up", "tui.select.down"]),
                "navigate",
                vec!["up", "down"],
            ),
            (Actions::One("tui.input.submit"), "save", vec!["enter"]),
            (Actions::One("tui.select.cancel"), "close", vec!["escape"]),
        ];
        for (actions, label, expected) in table {
            let result = action_hint(&bindings, actions, label).expect("bound hint");
            assert_eq!(result.keys, expected);
            assert_eq!(result.label, label);
        }
    }

    #[test]
    fn action_hint_skips_unbound_actions() {
        let bindings = KeybindingsManager::new();
        assert_eq!(action_hint(&bindings, Actions::One("tui.select.confirm"), "select"), None);
        let mixed = action_hint(
            &bindings,
            Actions::Many(&["tui.select.confirm", "tui.select.up"]),
            "mixed",
        )
        .expect("one bound action");
        assert_eq!(mixed.keys, vec!["up".to_string()]);
    }

    #[test]
    fn hints_row_skips_absent_hints() {
        let node = hints_row(
            &[
                hint(&["ctrl+o"], "open"),
                None,
                hint(&["escape", "ctrl+c"], "close"),
            ],
            "footer",
        );
        assert_eq!(node.kind, NodeKind::Row);
        assert_eq!(node.key.as_deref(), Some("footer"));
        assert_eq!(node.props.role().as_deref(), Some("omp.overlay.hints"));
        assert_eq!(node.props.get_str("gap"), Some("md"));
        assert_eq!(node.props.get_bool("wrap"), Some(true));
        assert_eq!(node.children.len(), 2);
        let first = &node.children[0];
        assert_eq!(first.kind, NodeKind::Row);
        assert_eq!(first.props.get_str("gap"), Some("xs"));
        assert_eq!(first.children.len(), 2);
        assert_eq!(first.children[0].kind, NodeKind::Kbd);
        assert_eq!(keys_prop(&first.children[0]), vec!["ctrl", "o"]);
        let label = &first.children[1];
        assert_eq!(label.kind, NodeKind::Text);
        let spans = label.props.spans().expect("spans");
        assert_eq!(spans[0].t, "open");
        assert_eq!(spans[0].s.as_deref(), Some("muted"));
        let second = &node.children[1];
        assert_eq!(second.children.len(), 3);
        assert_eq!(keys_prop(&second.children[0]), vec!["escape"]);
        assert_eq!(keys_prop(&second.children[1]), vec!["ctrl", "c"]);
    }

    #[test]
    fn status_hints_row_pairs_status_with_hints() {
        let node = status_hints_row(
            NodeText::Plain("✓ Clean".to_string()),
            &[hint(&["escape"], "close")],
        );
        assert_eq!(node.kind, NodeKind::Row);
        assert_eq!(node.props.get_str("gap"), Some("md"));
        assert_eq!(node.props.get_bool("wrap"), Some(true));
        assert_eq!(node.props.get_str("align"), Some("center"));
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0].plain_text(), "✓ Clean");
        assert_eq!(node.children[1].props.role().as_deref(), Some("omp.overlay.hints"));
    }

    #[test]
    fn overlay_card_collapses_and_defaults_head() {
        let card = overlay_card("omp.overlay.demo", Some(NodeText::Plain("  a   b  ".to_string())), vec![], None);
        assert_eq!(card.kind, NodeKind::Card);
        assert_eq!(card.props.role().as_deref(), Some("omp.overlay.demo"));
        assert_eq!(card.props.head(), Some(NodeText::Plain("a b".to_string())));

        let blank = overlay_card("omp.overlay.demo", Some(NodeText::Plain("   ".to_string())), vec![], None);
        assert_eq!(blank.props.head(), None);

        let none = overlay_card("omp.overlay.demo", None, vec![], None);
        assert_eq!(none.props.head(), None);

        let spans = overlay_card(
            "omp.overlay.demo",
            Some(NodeText::Spans(vec![Span::styled("title", "accent")])),
            vec![],
            None,
        );
        assert_eq!(
            spans.props.head(),
            Some(NodeText::Spans(vec![Span::styled("title", "accent")]))
        );
    }

    #[test]
    fn select_list_maps_click_and_double_click() {
        let mut props = Props::new();
        props.set_str("selected", "a");
        let node = select_list("list", vec![], props);
        assert_eq!(node.kind, NodeKind::List);
        assert_eq!(node.key.as_deref(), Some("list"));
        assert_eq!(
            node.props.get("actions").and_then(Value::as_object).map(|a| a.len()),
            Some(2)
        );
        assert_eq!(
            node.props
                .get("actions")
                .and_then(Value::as_object)
                .and_then(|a| a.get("click"))
                .and_then(Value::as_str),
            Some("select")
        );
        assert_eq!(
            node.props
                .get("actions")
                .and_then(Value::as_object)
                .and_then(|a| a.get("dblclick"))
                .and_then(Value::as_str),
            Some("activate")
        );
    }

    #[test]
    fn item_index_parses_prefixed_indices() {
        let table = [
            ("t3", "t", 3),
            ("t", "t", 0),
            ("t 3", "t", 3),
            ("t1e2", "t", 100),
            ("x3", "t", -1),
            ("t3.5", "t", -1),
            ("t-1", "t", -1),
            ("t+2", "t", 2),
        ];
        for (item, prefix, expected) in table {
            assert_eq!(item_index(item, prefix), expected, "{item:?}");
        }
    }

    #[test]
    fn action_button_native_draws_button_kind() {
        let options = ActionButtonOptions {
            keys: Some("r".to_string()),
            tone: Some(ActionButtonTone::Accent),
            ..ActionButtonOptions::default()
        };
        let node = action_button_with("Refresh", "refresh", options, true);
        assert_eq!(node.kind, NodeKind::Button);
        assert_eq!(node.key.as_deref(), Some("refresh"));
        assert_eq!(node.props.role().as_deref(), Some("omp.btn"));
        assert_eq!(node.props.text(), Some("Refresh"));
        assert_eq!(node.props.get_str("variant"), Some("primary"));
        assert_eq!(node.props.get_str("tone"), Some("accent"));
        assert_eq!(node.props.get_str("title"), Some("Refresh  r"));
        assert_eq!(keys_prop(&node), vec!["r"]);
        assert_eq!(
            node.props
                .get("actions")
                .and_then(Value::as_object)
                .and_then(|a| a.get("click"))
                .and_then(Value::as_str),
            Some("refresh")
        );
    }

    #[test]
    fn action_button_disabled_mutes_and_drops_click() {
        let native = action_button_with(
            "Kill",
            "kill",
            ActionButtonOptions {
                keys: Some("x".to_string()),
                disabled: Some("no selection".to_string()),
                ..ActionButtonOptions::default()
            },
            true,
        );
        assert_eq!(native.props.get_str("tone"), Some("muted"));
        assert_eq!(native.props.get_str("variant"), None);
        assert_eq!(native.props.get_str("disabled"), Some("no selection"));
        assert_eq!(
            native
                .props
                .get("actions")
                .and_then(Value::as_object)
                .and_then(|a| a.get("click"))
                .and_then(Value::as_str),
            Some("kill")
        );

        let row = action_button_with(
            "Kill",
            "kill",
            ActionButtonOptions {
                disabled: Some("no selection".to_string()),
                ..ActionButtonOptions::default()
            },
            false,
        );
        assert_eq!(row.kind, NodeKind::Row);
        assert_eq!(row.props.get_str("tone"), Some("muted"));
        assert_eq!(
            row.props.get("actions").and_then(Value::as_object).map(|a| a.len()),
            None
        );
    }

    #[test]
    fn action_button_row_orders_spinner_label_icon_keycap() {
        let node = action_button_with(
            "Working",
            "work",
            ActionButtonOptions {
                keys: Some("ctrl+g".to_string()),
                busy: true,
                icon: Some("gear".to_string()),
                ..ActionButtonOptions::default()
            },
            false,
        );
        assert_eq!(node.kind, NodeKind::Row);
        assert_eq!(node.props.get_str("gap"), Some("xs"));
        assert_eq!(node.children.len(), 4);
        assert_eq!(node.children[0].kind, NodeKind::Spinner);
        assert_eq!(node.children[0].props.get_str("style"), Some("dots"));
        assert_eq!(node.children[1].plain_text(), "Working");
        assert_eq!(node.children[2].kind, NodeKind::Icon);
        assert_eq!(node.children[2].key.as_deref(), Some("icon"));
        assert_eq!(node.children[3].kind, NodeKind::Kbd);
        assert_eq!(keys_prop(&node.children[3]), vec!["ctrl", "g"]);
    }

    #[test]
    fn action_button_key_defaults_to_action_and_title_to_label() {
        let node = action_button_with("Close", "close", ActionButtonOptions::default(), false);
        assert_eq!(node.key.as_deref(), Some("close"));
        assert_eq!(node.props.get_str("title"), Some("Close"));
        let keyed = action_button_with(
            "Close",
            "close",
            ActionButtonOptions {
                key: Some("bye".to_string()),
                title: Some("Done".to_string()),
                ..ActionButtonOptions::default()
            },
            false,
        );
        assert_eq!(keyed.key.as_deref(), Some("bye"));
        assert_eq!(keyed.props.get_str("title"), Some("Done"));
    }

    #[test]
    fn esc_close_button_both_branches() {
        let native = esc_close_button_with("close", "esc-close", true);
        assert_eq!(native.kind, NodeKind::Button);
        assert_eq!(native.key.as_deref(), Some("esc-close"));
        assert_eq!(native.props.get_str("aria"), Some("Close"));
        assert_eq!(native.props.get_str("title"), Some("Close  esc"));
        assert_eq!(keys_prop(&native), vec!["escape"]);

        let row = esc_close_button_with("close", "esc-close", false);
        assert_eq!(row.kind, NodeKind::Row);
        assert_eq!(row.props.get_str("title"), Some("Close  esc"));
        assert_eq!(row.children.len(), 1);
        assert_eq!(row.children[0].kind, NodeKind::Kbd);
        assert_eq!(keys_prop(&row.children[0]), vec!["escape"]);
    }

    #[test]
    fn icon_button_both_branches() {
        let native = icon_button_with(
            "back",
            "back",
            "Back",
            IconButtonOptions {
                aria: Some("Go back".to_string()),
                on: true,
                ..IconButtonOptions::default()
            },
            true,
        );
        assert_eq!(native.kind, NodeKind::Button);
        assert_eq!(native.props.role().as_deref(), Some("omp.app.ibtn.set"));
        assert_eq!(native.props.get_str("variant"), Some("quiet"));
        assert_eq!(native.props.get_str("icon"), Some("back"));
        assert_eq!(native.props.get_str("aria"), Some("Go back"));
        assert_eq!(native.props.get_bool("on"), Some(true));

        let icon = icon_button_with("back", "back", "Back", IconButtonOptions::default(), false);
        assert_eq!(icon.kind, NodeKind::Icon);
        assert_eq!(icon.props.role().as_deref(), Some("omp.app.ibtn"));
        assert_eq!(icon.props.get_str("name"), Some("back"));
        assert_eq!(icon.props.get_str("aria"), Some("Back"));
        assert_eq!(icon.props.get_bool("on"), None);
    }

    #[test]
    fn quiet_button_both_branches() {
        let native = quiet_button_with("Stage all", "stage-all", "Stage every change", true);
        assert_eq!(native.kind, NodeKind::Button);
        assert_eq!(native.key.as_deref(), Some("stage-all"));
        assert_eq!(native.props.role().as_deref(), Some("omp.app.qbtn"));
        assert_eq!(native.props.get_str("variant"), Some("quiet"));
        assert_eq!(native.props.get_str("size"), Some("sm"));
        assert_eq!(native.props.text(), Some("Stage all"));

        let row = quiet_button_with("Stage all", "stage-all", "Stage every change", false);
        assert_eq!(row.kind, NodeKind::Row);
        assert_eq!(row.key.as_deref(), Some("stage-all"));
        assert_eq!(row.children.len(), 1);
        assert_eq!(row.children[0].plain_text(), "Stage all");
    }

    #[test]
    fn segmented_tabs_variant_follows_terminal() {
        let mut props = Props::new();
        props.set_str("tab", "one");
        let native = segmented_tabs_with(props.clone(), "tabs", true);
        assert_eq!(native.kind, NodeKind::Tabs);
        assert_eq!(native.props.get_str("variant"), Some("segmented"));
        let plain = segmented_tabs_with(props, "tabs", false);
        assert_eq!(plain.props.get_str("variant"), None);
        assert_eq!(plain.props.get_str("tab"), Some("one"));
    }

    #[test]
    fn action_bar_replaces_null_with_spacers() {
        let keep = action_button_with("Close", "close", ActionButtonOptions::default(), false);
        let bar = action_bar(&[None, Some(keep.clone()), None], "actions");
        assert_eq!(bar.kind, NodeKind::Row);
        assert_eq!(bar.props.role().as_deref(), Some("omp.actions"));
        assert_eq!(bar.props.get_str("gap"), Some("sm"));
        assert_eq!(bar.children.len(), 3);
        assert_eq!(bar.children[0].kind, NodeKind::Spacer);
        assert_eq!(bar.children[0].props.get_i64("grow"), Some(1));
        assert_eq!(bar.children[1], keep);
        assert_eq!(bar.children[2].kind, NodeKind::Spacer);
    }

    #[test]
    fn note_spans_dim_label_and_accent_note() {
        let spans = note_spans("Note", "everything is fine");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].t, "Note: ");
        assert_eq!(spans[0].s.as_deref(), Some("dim"));
        assert_eq!(spans[1].t, "everything is fine");
        assert_eq!(spans[1].s.as_deref(), Some("accent"));
    }

    #[test]
    fn tooltip_keys_table() {
        let table = [
            ("escape", false, "esc"),
            ("shift+tab", false, "⇧⇥"),
            ("alt+enter", false, "Alt+⏎"),
            ("ctrl+shift+c", false, "Ctrl+⇧C"),
            ("ctrl+shift+c", true, "⌃⇧C"),
            ("up", false, "↑"),
            ("f5", false, "F5"),
            ("q", false, "q"),
            ("shift", false, "⇧"),
            ("home", false, "Home"),
            ("+", false, "+"),
            ("ctrl++", false, "Ctrl++"),
            ("shift+escape", false, "⇧⎋"),
        ];
        for (key, mac, expected) in table {
            assert_eq!(format_key_with(key, mac), expected, "{key:?} mac={mac}");
        }
        assert_eq!(format_tooltip_key("escape"), "esc");
    }
}
