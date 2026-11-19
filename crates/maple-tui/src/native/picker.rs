//! Builders for the data-first `picker` kind: the model hub, session selector,
//! rewind, tree, copy, history search and the small selectors send their data
//! and the terminal draws the sheet (head, scopes, tabs, fact columns, rows,
//! preview, action bar).
//!
//! A picker is its own sheet: the component returns `nativeSheet(cx)` true
//! (usually `cx.supports("picker")`) so the backend puts it directly in
//! `layer`, and `describe()` returns [`picker`]. When the terminal lacks the
//! kind, components keep describing their generic composition.
//!
//! Reference: `l/src/native/picker.ts`, with `SelectList`/`SelectItem` from
//! `l/src/components/select-list.ts`, `Input` from `l/src/components/input.ts`
//! and the keybinding registry from `l/src/keybindings.ts`.
//!
//! Deviations:
//! - The reference folds a process-global `NativeUiEvent` union; maple-tui has
//!   no backend event router yet, so [`PickerUiEvent`] is the picker-facing
//!   subset of `l/src/native/node.ts`'s `NativeUiEvent`.
//! - `bindingKeys` reads the process-global registry; maple-tui has no global
//!   UI state, so [`binding_keys`] takes the [`KeybindingsManager`] explicitly.
//! - `SelectList.pickerView()` and its caret/pending accessors do not exist on
//!   the crate's `SelectList` port, so a picker reads a [`PickerView`]
//!   snapshot through the [`PickerList`] trait instead of the list itself.
//!   `SelectListSheet` is generic over that trait.
//! - The reference's `SelectItem.value`/`label` are the crate's
//!   `SelectItem.key`/`title`; the picker maps them accordingly.
//! - `Input` exposes `value()` but not its caret, so [`picker_query`] takes a
//!   [`PickerInput`] (text plus caret) rather than the component; the caret
//!   stays a UTF-16 offset, as on the wire.
//! - `pickerAge`/`pickerDate` take epoch milliseconds instead of `Date`; the
//!   reference's `Date.now()` default is the caller's `now`.
//! - `CLOSE_ACTION` is a `LazyLock` rather than a `const` (Rust `const`s
//!   cannot own a `String`).

use std::sync::{Arc, LazyLock};

use serde_json::Value;

use crate::components::select_list::SelectItem;
use crate::keybindings::KeybindingsManager;
use crate::native::describe::{col, node};
use crate::native::node::{NativeNode, NodeKind, Props};

/// Key of the picker child a dock/editor-mounted selector describes; its
/// events arrive at this keypath.
pub const PICKER_KEY: &str = "picker";

/// The picker sheet; `preview` children fill its preview pane.
pub fn picker(props: Props, preview: Option<Vec<NativeNode>>) -> NativeNode {
    node(NodeKind::Picker, props, preview.unwrap_or_default(), None)
}

/// The root a selector mounted in the editor slot describes: a column holding
/// the picker as child [`PICKER_KEY`], which the reconciler hoists into
/// `layer`.
pub fn docked_picker(props: Props, preview: Option<Vec<NativeNode>>) -> NativeNode {
    col(
        vec![node(
            NodeKind::Picker,
            props,
            preview.unwrap_or_default(),
            Some(PICKER_KEY),
        )],
        Props::new(),
    )
}

/// The text and caret of the `Input` editing a picker's search field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerInput {
    /// The field's text.
    pub text: String,
    /// Caret offset into `text` (a UTF-16 offset, as on the wire).
    pub cursor: usize,
}

impl PickerInput {
    /// A field with `text` and its caret.
    pub fn new(text: impl Into<String>, cursor: usize) -> Self {
        PickerInput {
            text: text.into(),
            cursor,
        }
    }
}

/// A picker's search field: its text and caret. `None` hides the field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerQuery {
    /// The field's text; `None` hides the field.
    pub query: Option<String>,
    /// Caret offset into `query`.
    pub cursor: Option<usize>,
}

/// A picker's search field from the `Input` that edits it: the text and its
/// caret. `None` hides the field.
pub fn picker_query(input: Option<&PickerInput>) -> PickerQuery {
    match input {
        Some(input) => PickerQuery {
            query: Some(input.text.clone()),
            cursor: Some(input.cursor),
        },
        None => PickerQuery {
            query: None,
            cursor: None,
        },
    }
}

/// Keycap names for one key id (`"alt+enter"` → `["alt", "enter"]`,
/// `"escape"` → `["esc"]`).
pub fn picker_keys(key: &str) -> Vec<String> {
    crate::native::describe::key_names(key)
        .into_iter()
        .map(|part| if part == "escape" { "esc".to_string() } else { part })
        .collect()
}

/// The keycaps of a keybinding's primary key, empty when unbound.
pub fn binding_keys(bindings: &KeybindingsManager, binding: &str) -> Vec<String> {
    bindings
        .get_keys(binding)
        .first()
        .map(|key| picker_keys(key))
        .unwrap_or_default()
}

/// The keycaps [`picker_action`] draws: a key id or pre-split caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKeys<'a> {
    /// A key id (`"alt+enter"`).
    Id(&'a str),
    /// Already-split keycaps.
    Caps(&'a [&'a str]),
}

impl<'a> From<&'a str> for PickerKeys<'a> {
    fn from(key: &'a str) -> Self {
        PickerKeys::Id(key)
    }
}

impl<'a> From<&'a [&'a str]> for PickerKeys<'a> {
    fn from(caps: &'a [&'a str]) -> Self {
        PickerKeys::Caps(caps)
    }
}

impl<'a, const N: usize> From<&'a [&'a str; N]> for PickerKeys<'a> {
    fn from(caps: &'a [&'a str; N]) -> Self {
        PickerKeys::Caps(caps)
    }
}

/// Why an action cannot run: a flag, or a reason string.
#[derive(Clone, Debug, PartialEq)]
pub enum PickerActionDisabled {
    /// The action is disabled.
    Flag(bool),
    /// Why it is disabled.
    Reason(String),
}

/// An action-bar button.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerAction {
    /// Action id events report back.
    pub id: String,
    /// Button label.
    pub label: String,
    /// Keycaps drawn inside the button.
    pub keys: Option<Vec<String>>,
    /// The sheet's primary action.
    pub primary: bool,
    /// End-align the action (the right-aligned `Close`).
    pub end: bool,
    /// Destructive action.
    pub danger: bool,
    /// Why the action can't run now.
    pub disabled: Option<PickerActionDisabled>,
    /// A toggle that is on.
    pub on: bool,
    /// Value the action carries (scope/tab/strip id).
    pub value: Option<String>,
    /// Remaining wire fields.
    pub rest: Props,
}

impl PickerAction {
    /// An action with `id` and `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        PickerAction {
            id: id.into(),
            label: label.into(),
            ..PickerAction::default()
        }
    }

    /// The wire props of this action.
    pub fn to_props(&self) -> Props {
        let mut props = Props::new();
        props.set_str("id", self.id.as_str());
        props.set_str("label", self.label.as_str());
        if let Some(keys) = &self.keys {
            props.insert("keys", strings_value(keys));
        }
        if self.primary {
            props.set_bool("primary", true);
        }
        if self.end {
            props.set_bool("end", true);
        }
        if self.danger {
            props.set_bool("danger", true);
        }
        match &self.disabled {
            Some(PickerActionDisabled::Flag(flag)) => {
                props.set_bool("disabled", *flag);
            }
            Some(PickerActionDisabled::Reason(reason)) => {
                props.set_str("disabled", reason.as_str());
            }
            None => {}
        }
        if self.on {
            props.set_bool("on", true);
        }
        if let Some(value) = &self.value {
            props.set_str("value", value.as_str());
        }
        for (key, value) in self.rest.iter() {
            props.insert(key.clone(), value.clone());
        }
        props
    }
}

/// Optional fields of a [`PickerAction`] ([`picker_action`]'s `opts`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerActionOptions {
    /// The sheet's primary action.
    pub primary: bool,
    /// End-align the action.
    pub end: bool,
    /// Destructive action.
    pub danger: bool,
    /// Why the action can't run now.
    pub disabled: Option<PickerActionDisabled>,
    /// A toggle that is on.
    pub on: bool,
    /// Value the action carries.
    pub value: Option<String>,
    /// Remaining wire fields.
    pub rest: Props,
}

/// An action-bar button; `keys` is a key id (`"alt+enter"`), a keybinding's
/// keycaps, or none.
pub fn picker_action(
    id: &str,
    label: &str,
    keys: Option<PickerKeys<'_>>,
    opts: PickerActionOptions,
) -> PickerAction {
    let caps = match keys {
        None => None,
        Some(PickerKeys::Id(key)) => {
            let caps = picker_keys(key);
            if caps.is_empty() {
                None
            } else {
                Some(caps)
            }
        }
        Some(PickerKeys::Caps(caps)) => {
            let caps: Vec<String> = caps.iter().map(|cap| (*cap).to_string()).collect();
            if caps.is_empty() {
                None
            } else {
                Some(caps)
            }
        }
    };
    PickerAction {
        id: id.to_string(),
        label: label.to_string(),
        keys: caps,
        primary: opts.primary,
        end: opts.end,
        danger: opts.danger,
        disabled: opts.disabled,
        on: opts.on,
        value: opts.value,
        rest: opts.rest,
    }
}

/// The right-aligned `Close esc` every picker ends with.
pub static CLOSE_ACTION: LazyLock<PickerAction> = LazyLock::new(|| PickerAction {
    id: "close".to_string(),
    label: "Close".to_string(),
    keys: Some(vec!["esc".to_string()]),
    end: true,
    ..PickerAction::default()
});

/// A `time` fact: the compact age of `at` (`now`, `5m`, `2h`, `3d`, `2w`,
/// `6mo`, `1y`). `at` and `now` are epoch milliseconds.
pub fn picker_age(at: i64, now: i64) -> String {
    let seconds = ((now - at) / 1000).max(0);
    if seconds < 60 {
        return "now".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    if days < 30 {
        return format!("{}w", days / 7);
    }
    if days < 365 {
        return format!("{}mo", days / 30);
    }
    format!("{}y", days / 365)
}

/// Search `hits` for a label: disjoint, ordered UTF-16 ranges of every
/// case-insensitive occurrence of any of the (lowercase) `tokens`.
pub fn picker_hits(label: &str, tokens: &[&str]) -> Vec<(usize, usize)> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let lower = label.to_lowercase();
    let map = utf16_map(&lower);
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        let mut from = 0usize;
        while let Some(at) = lower[from..].find(token) {
            let start = from + at;
            let end = start + token.len();
            ranges.push((map[start], map[end]));
            from = end;
        }
    }
    ranges.sort_by_key(|range| range.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

/// Map every byte offset of `text` to its UTF-16 code-unit offset.
fn utf16_map(text: &str) -> Vec<usize> {
    let mut map = vec![0usize; text.len() + 1];
    let mut units = 0usize;
    let mut byte = 0usize;
    for ch in text.chars() {
        let end = byte + ch.len_utf8();
        for slot in &mut map[byte..end] {
            *slot = units;
        }
        units += ch.len_utf16();
        byte = end;
    }
    map[text.len()] = units;
    map
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The absolute local time behind a `time` fact, for its tooltip
/// (`28 Sep 2026, 18:02`). `at` is epoch milliseconds.
pub fn picker_date(at: i64) -> String {
    let (year, month, day, hour, minute) = local_parts(at).unwrap_or((1970, 1, 1, 0, 0));
    format_picker_date(year, month, day, hour, minute)
}

/// [`picker_date`] from already-resolved local parts.
fn format_picker_date(year: i64, month: u32, day: u32, hour: u32, minute: u32) -> String {
    let month_name = MONTHS
        .get((month.saturating_sub(1) as usize).min(MONTHS.len() - 1))
        .copied()
        .unwrap_or("Jan");
    format!("{day} {month_name} {year}, {hour:02}:{minute:02}")
}

/// Local calendar parts of epoch milliseconds.
#[cfg(unix)]
fn local_parts(at_ms: i64) -> Option<(i64, u32, u32, u32, u32)> {
    let secs: libc::time_t = at_ms.div_euclid(1000) as libc::time_t;
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::localtime_r(&secs, &mut parts) };
    if result.is_null() {
        return None;
    }
    Some((
        i64::from(parts.tm_year) + 1900,
        parts.tm_mon as u32 + 1,
        parts.tm_mday as u32,
        parts.tm_hour as u32,
        parts.tm_min as u32,
    ))
}

/// UTC calendar parts of epoch milliseconds (non-Unix fallback).
#[cfg(not(unix))]
fn local_parts(at_ms: i64) -> Option<(i64, u32, u32, u32, u32)> {
    let secs = at_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let remainder = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Some((
        year,
        month,
        day,
        (remainder / 3_600) as u32,
        ((remainder % 3_600) / 60) as u32,
    ))
}

/// Days since the Unix epoch to a proleptic Gregorian date.
#[cfg(not(unix))]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

/// A user action on a described node: the picker-facing subset of
/// `NativeUiEvent` (`l/src/native/node.ts`).
#[derive(Clone, Debug, PartialEq)]
pub enum PickerUiEvent {
    /// A row was clicked.
    Select {
        /// The node's keypath inside the component.
        key: String,
        /// The item id.
        item: String,
    },
    /// A row was activated.
    Activate {
        /// The node's keypath inside the component.
        key: String,
        /// The item id.
        item: String,
    },
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
    /// Any other event; [`picker_event`] ignores it.
    Other {
        /// The node's keypath inside the component.
        key: String,
    },
}

/// A picker pointer event, folded: which item, or which action with its value.
#[derive(Clone, Debug, PartialEq)]
pub enum PickerEvent {
    /// A row was clicked.
    Select {
        /// The item id.
        item: String,
    },
    /// A row was activated.
    Activate {
        /// The item id.
        item: String,
    },
    /// An action was triggered.
    Action {
        /// The action id.
        act: String,
        /// The value it carries, if any.
        value: Option<String>,
    },
}

/// The picker event `event` carries, or `None` when it is not for the picker at
/// keypath `key`: `""` for a root picker, else the picker child's key (a
/// hoisted child's keypath carries a `^` marker, ignored here).
pub fn picker_event(event: &PickerUiEvent, key: &str) -> Option<PickerEvent> {
    let event_key = match event {
        PickerUiEvent::Select { key, .. }
        | PickerUiEvent::Activate { key, .. }
        | PickerUiEvent::Action { key, .. }
        | PickerUiEvent::Other { key } => key,
    };
    if event_key.replace('^', "") != key {
        return None;
    }
    match event {
        PickerUiEvent::Select { item, .. } => Some(PickerEvent::Select { item: item.clone() }),
        PickerUiEvent::Activate { item, .. } => Some(PickerEvent::Activate { item: item.clone() }),
        PickerUiEvent::Action { act, value, .. } => Some(PickerEvent::Action {
            act: act.clone(),
            value: value.clone(),
        }),
        PickerUiEvent::Other { .. } => None,
    }
}

/// [`picker_hits`] for fuzzy-filtered lists: each whitespace token of `query`
/// marks its first case-insensitive substring, else its characters as a greedy
/// in-order subsequence. UTF-16 ranges; `None` when nothing matches.
pub fn picker_fuzzy_hits(label: &str, query: &str) -> Option<Vec<(usize, usize)>> {
    let lower_query = query.to_lowercase();
    let tokens: Vec<&str> = lower_query.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let lower = label.to_lowercase();
    let map = utf16_map(&lower);
    let mut marked = vec![false; lower.len()];
    for token in tokens {
        if let Some(at) = lower.find(token) {
            for slot in &mut marked[at..at + token.len()] {
                *slot = true;
            }
            continue;
        }
        let mut from = 0usize;
        for ch in token.chars() {
            let Some(relative) = lower[from..].find(ch) else {
                break;
            };
            let index = from + relative;
            for slot in &mut marked[index..index + ch.len_utf8()] {
                *slot = true;
            }
            from = index + ch.len_utf8();
        }
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut index = 0usize;
    while index < marked.len() {
        if !marked[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < marked.len() && marked[index] {
            index += 1;
        }
        ranges.push((map[start], map[index]));
    }
    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}

/// One picker fact column.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickerColumn {
	/// Column id.
	pub id: String,
	/// Column heading.
	pub head: Option<String>,
	/// Value format (`"dim"`, …).
	pub format: Option<String>,
	/// Alignment (`"start"`, `"center"`, `"end"`).
	pub align: Option<String>,
	/// Whether the column grows to fill remaining space.
	pub grow: Option<bool>,
	/// Sort priority.
	pub priority: Option<i64>,
}

impl PickerColumn {
	/// A column with `id`.
	pub fn new(id: impl Into<String>) -> Self {
		PickerColumn {
			id: id.into(),
			..PickerColumn::default()
		}
	}

	/// The wire props of this column.
	pub fn to_props(&self) -> Props {
		let mut props = Props::new();
		props.set_str("id", self.id.as_str());
		if let Some(head) = &self.head {
			props.set_str("head", head.as_str());
		}
		if let Some(format) = &self.format {
			props.set_str("format", format.as_str());
		}
		if let Some(align) = &self.align {
			props.set_str("align", align.as_str());
		}
		if let Some(grow) = self.grow {
			props.set_bool("grow", grow);
		}
		if let Some(priority) = self.priority {
			props.set_i64("priority", priority);
		}
		props
	}
}

/// One picker row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerItem {
    /// Stable id events report back.
    pub id: String,
    /// Row label.
    pub label: String,
    /// Remaining row fields (`detail`, `disabled`, `facts`, `badges`, …).
    pub rest: Props,
}

impl PickerItem {
    /// A row with `id` and `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        PickerItem {
            id: id.into(),
            label: label.into(),
            rest: Props::new(),
        }
    }

    /// The row's detail text, if any.
    pub fn detail(&self) -> Option<&str> {
        self.rest.get_str("detail")
    }

    /// Whether the row is disabled.
    pub fn disabled(&self) -> bool {
        self.rest.get_bool("disabled").unwrap_or(false)
    }

    /// The wire props of this row.
    pub fn to_props(&self) -> Props {
        let mut props = Props::new();
        props.set_str("id", self.id.as_str());
        props.set_str("label", self.label.as_str());
        for (key, value) in self.rest.iter() {
            props.insert(key.clone(), value.clone());
        }
        props
    }
}

/// The pending-confirmation strip of a picker.
#[derive(Clone, Debug, PartialEq)]
pub struct PickerConfirm {
    /// The question asked over the row.
    pub text: String,
    /// The action a confirm sends.
    pub act: String,
    /// The confirm button's label.
    pub label: String,
}

impl PickerConfirm {
    /// The wire props of this strip.
    pub fn to_props(&self) -> Props {
        let mut props = Props::new();
        props.set_str("text", self.text.as_str());
        props.set_str("act", self.act.as_str());
        props.set_str("label", self.label.as_str());
        props
    }
}

/// The data-first picker sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerProps {
    /// Sheet title.
    pub title: String,
    /// Sheet subtitle.
    pub subtitle: Option<String>,
    /// Sheet icon.
    pub icon: Option<String>,
    /// What the rows are (`"options"`, `"agents"`, …).
    pub noun: String,
    /// Sheet size (`"md"`, `"lg"`, …).
    pub size: String,
    /// Row layout (`"rows"`, `"cards"`, `"tree"`, …).
    pub layout: String,
    /// Preview pane placement (`"none"`, `"side"`, …).
    pub preview: String,
    /// Search field text; `None` serializes as `null`.
    pub query: Option<String>,
    /// Search field caret.
    pub cursor: Option<usize>,
    /// Rows in order.
    pub items: Vec<PickerItem>,
    /// Selected row id; `None` serializes as `null`.
    pub selected: Option<String>,
    /// Values in use now (the current theme, level, …).
    pub current: Option<Vec<String>>,
    /// Fact columns.
    pub columns: Option<Vec<PickerColumn>>,
    /// Action bar.
    pub actions: Vec<PickerAction>,
    /// Pending-confirmation strip; `None` serializes as `null`.
    pub confirm: Option<PickerConfirm>,
}

impl PickerProps {
    /// The wire props of this sheet.
    pub fn to_props(&self) -> Props {
        let mut props = Props::new();
        props.set_str("title", self.title.as_str());
        if let Some(subtitle) = &self.subtitle {
            props.set_str("subtitle", subtitle.as_str());
        }
        if let Some(icon) = &self.icon {
            props.set_str("icon", icon.as_str());
        }
        props.set_str("noun", self.noun.as_str());
        props.set_str("size", self.size.as_str());
        props.set_str("layout", self.layout.as_str());
        props.set_str("preview", self.preview.as_str());
        match &self.query {
            Some(query) => props.set_str("query", query.as_str()),
            None => {
                props.insert("query", Value::Null);
            }
        }
        if let Some(cursor) = self.cursor {
            props.set_i64("cursor", cursor as i64);
        }
        props.insert(
            "items",
            Value::Array(self.items.iter().map(|item| item.to_props().to_value()).collect()),
        );
        match &self.selected {
            Some(selected) => props.set_str("selected", selected.as_str()),
            None => {
                props.insert("selected", Value::Null);
            }
        }
        if let Some(current) = &self.current {
            props.insert("current", strings_value(current));
        }
        if let Some(columns) = &self.columns {
            props.insert(
                "columns",
                Value::Array(columns.iter().map(|column| column.to_props().to_value()).collect()),
            );
        }
        props.insert(
            "actions",
            Value::Array(self.actions.iter().map(|action| action.to_props().to_value()).collect()),
        );
        match &self.confirm {
            Some(confirm) => {
                props.insert("confirm", confirm.to_props().to_value());
            }
            None => {
                props.insert("confirm", Value::Null);
            }
        }
        props
    }
}

/// What a native picker shows of a selection list: the reference's
/// `SelectList.pickerView()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerView {
    /// Filtered items in order.
    pub items: Vec<SelectItem>,
    /// Selected item's value, if any.
    pub selected: Option<String>,
    /// Pending-confirmation item's value, if any.
    pub pending: Option<String>,
    /// Search text.
    pub query: String,
    /// Search caret (a UTF-16 offset, as on the wire).
    pub cursor: usize,
}

/// The picker-facing surface of a selection list.
///
/// The crate's `SelectList` port does not expose `pickerView`, its caret, its
/// pending key, or its cancel/selection callbacks, so a host adapts it here.
/// [`select_index`](PickerList::select_index) selects the row and fires the
/// selection-change callback, matching the reference's `setSelectedIndex`
/// plus `onSelectionChange` pair; [`activate_index`](PickerList::activate_index)
/// selects and activates, as `clickItem` does.
pub trait PickerList {
    /// The filtered items and selection state a picker shows.
    fn picker_view(&self) -> PickerView;
    /// Select the visible row at `index` and fire the selection-change callback.
    fn select_index(&mut self, index: usize);
    /// Select and activate the visible row at `index`.
    fn activate_index(&mut self, index: usize);
    /// Cancel the selection.
    fn cancel(&mut self);
}

/// The `md` sheet a short selector becomes (thinking level, hook options,
/// queue mode, …).
#[derive(Clone, Default)]
pub struct SelectPickerOptions {
    /// Sheet title.
    pub title: String,
    /// Sheet subtitle.
    pub subtitle: Option<String>,
    /// Sheet icon.
    pub icon: Option<String>,
    /// What the rows are; defaults to `"options"`.
    pub noun: Option<String>,
    /// Show the filter text as the search field.
    pub searchable: bool,
    /// Per-item extras (a thinking-level dot, a swatch mark, a budget fact).
    pub decorate: Option<Arc<dyn Fn(&SelectItem) -> Option<Props>>>,
    /// Fact columns.
    pub columns: Option<Vec<PickerColumn>>,
    /// Values in use now (the current theme, the current level).
    pub current: Option<Vec<String>>,
    /// The primary action's label (`Apply`, `Select`).
    pub confirm: Option<String>,
}

/// Where [`SelectListSheet`] gets its options: once, or per build.
#[derive(Clone)]
pub enum SelectPickerOptionsSource {
    /// Fixed options.
    Once(SelectPickerOptions),
    /// Options rebuilt every describe.
    Each(Arc<dyn Fn() -> SelectPickerOptions>),
}

impl SelectPickerOptionsSource {
    fn get(&self) -> SelectPickerOptions {
        match self {
            SelectPickerOptionsSource::Once(options) => options.clone(),
            SelectPickerOptionsSource::Each(build) => build(),
        }
    }
}

/// A selection list described as a picker, memoized.
pub struct SelectListSheet<L: PickerList> {
    list: L,
    options: SelectPickerOptionsSource,
    docked: bool,
    memo: Option<SelectListSheetMemo>,
}

/// The memoized description of a [`SelectListSheet`].
struct SelectListSheetMemo {
    items: Vec<SelectItem>,
    selected: Option<String>,
    pending: Option<String>,
    query: String,
    cursor: usize,
    node: NativeNode,
}

impl<L: PickerList> SelectListSheet<L> {
    /// Describe `list` as a docked picker with fixed `options`.
    pub fn new(list: L, options: SelectPickerOptions) -> Self {
        SelectListSheet {
            list,
            options: SelectPickerOptionsSource::Once(options),
            docked: true,
            memo: None,
        }
    }

    /// Describe `list` with options rebuilt by `options` on every change.
    /// `docked` is `true` for editor-slot selectors.
    pub fn with_options(
        list: L,
        options: impl Fn() -> SelectPickerOptions + 'static,
        docked: bool,
    ) -> Self {
        SelectListSheet {
            list,
            options: SelectPickerOptionsSource::Each(Arc::new(options)),
            docked,
            memo: None,
        }
    }

    /// The described list.
    pub fn list(&self) -> &L {
        &self.list
    }

    /// Drop the memo (decorations or options changed outside the list).
    pub fn invalidate(&mut self) {
        self.memo = None;
    }

    /// The memoized description; rebuilt only when the list's filtered items,
    /// selection, pending confirmation or query change.
    pub fn describe(&mut self) -> NativeNode {
        let view = self.list.picker_view();
        if let Some(memo) = &self.memo {
            if memo.items == view.items
                && memo.selected == view.selected
                && memo.pending == view.pending
                && memo.query == view.query
                && memo.cursor == view.cursor
            {
                return memo.node.clone();
            }
        }
        let props = select_list_picker(&self.list, &self.options.get());
        let result = if self.docked {
            docked_picker(props.to_props(), None)
        } else {
            picker(props.to_props(), None)
        };
        self.memo = Some(SelectListSheetMemo {
            items: view.items,
            selected: view.selected,
            pending: view.pending,
            query: view.query,
            cursor: view.cursor,
            node: result.clone(),
        });
        result
    }

    /// Route a picker event onto the list.
    pub fn handle(&mut self, event: &PickerUiEvent) -> bool {
        let key = if self.docked { PICKER_KEY } else { "" };
        route_select_list_picker(&mut self.list, event, key)
    }
}

/// Picker props for a selection list: its filtered items in order, the
/// selection, the pending-confirmation strip, and `Select ⏎` / `Close esc`.
pub fn select_list_picker(list: &dyn PickerList, o: &SelectPickerOptions) -> PickerProps {
    let view = list.picker_view();
    let mut items: Vec<PickerItem> = Vec::with_capacity(view.items.len());
    for item in &view.items {
        let mut rest = Props::new();
        if let Some(detail) = &item.description {
            rest.set_str("detail", detail.as_str());
        }
        if item.disabled {
            rest.set_bool("disabled", true);
        }
        if let Some(decorate) = &o.decorate {
            if let Some(extra) = decorate(item) {
                for (key, value) in extra.iter() {
                    rest.insert(key.clone(), value.clone());
                }
            }
        }
        items.push(PickerItem {
            id: item.key.clone(),
            label: item.title.clone(),
            rest,
        });
    }
    let pending = view
        .pending
        .as_ref()
        .and_then(|pending| view.items.iter().find(|item| &item.key == pending));
    let confirm = pending.and_then(|item| item.confirmation.clone()).map(|text| PickerConfirm {
        text,
        act: "confirm".to_string(),
        label: "Confirm".to_string(),
    });
    PickerProps {
        title: o.title.clone(),
        subtitle: o.subtitle.clone(),
        icon: o.icon.clone(),
        noun: o.noun.clone().unwrap_or_else(|| "options".to_string()),
        size: "md".to_string(),
        layout: "rows".to_string(),
        preview: "none".to_string(),
        query: if o.searchable {
            Some(view.query.clone())
        } else {
            None
        },
        cursor: if o.searchable {
            Some(view.cursor)
        } else {
            None
        },
        items,
        selected: view.selected.clone(),
        current: o.current.clone(),
        columns: o.columns.clone(),
        actions: vec![
            picker_action(
                "confirm",
                o.confirm.as_deref().unwrap_or("Select"),
                Some(PickerKeys::Id("enter")),
                PickerActionOptions {
                    primary: true,
                    ..PickerActionOptions::default()
                },
            ),
            (*CLOSE_ACTION).clone(),
        ],
        confirm,
    }
}

/// Routes a picker event to a selection list: a row click selects (and
/// previews, through the selection-change callback), a second click or
/// `confirm` activates, `close`/`cancel` cancels. Returns whether it handled
/// the event.
pub fn route_select_list_picker(list: &mut dyn PickerList, event: &PickerUiEvent, key: &str) -> bool {
    let Some(ev) = picker_event(event, key) else {
        return false;
    };
    let view = list.picker_view();
    match ev {
        PickerEvent::Action { act, .. } => {
            if act == "close" || act == "cancel" {
                list.cancel();
                return true;
            }
            if act == "confirm" {
                if let Some(selected) = &view.selected {
                    if let Some(index) = view.items.iter().position(|item| &item.key == selected) {
                        list.activate_index(index);
                    }
                }
                return true;
            }
            false
        }
        PickerEvent::Select { item } => {
            let Some(index) = view.items.iter().position(|entry| entry.key == item) else {
                return false;
            };
            list.select_index(index);
            true
        }
        PickerEvent::Activate { item } => {
            let Some(index) = view.items.iter().position(|entry| entry.key == item) else {
                return false;
            };
            list.activate_index(index);
            true
        }
    }
}

/// A JSON array of strings.
fn strings_value(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn select(key: &str, title: &str) -> SelectItem {
        SelectItem::new(key, title)
    }

    struct FakeList {
        view: Arc<Mutex<PickerView>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl FakeList {
        fn new(view: PickerView) -> Self {
            FakeList {
                view: Arc::new(Mutex::new(view)),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn view(&self) -> Arc<Mutex<PickerView>> {
            self.view.clone()
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl PickerList for FakeList {
        fn picker_view(&self) -> PickerView {
            self.view.lock().unwrap().clone()
        }

        fn select_index(&mut self, index: usize) {
            self.calls.lock().unwrap().push(format!("select:{index}"));
        }

        fn activate_index(&mut self, index: usize) {
            self.calls.lock().unwrap().push(format!("activate:{index}"));
        }

        fn cancel(&mut self) {
            self.calls.lock().unwrap().push("cancel".to_string());
        }
    }

    fn event(event: PickerUiEvent) -> PickerUiEvent {
        event
    }

    #[test]
    fn picker_and_docked_picker_shape() {
        let plain = picker(Props::new(), None);
        assert_eq!(plain.kind, NodeKind::Picker);
        assert_eq!(plain.key, None);
        assert!(plain.children.is_empty());

        let preview = picker(
            Props::new(),
            Some(vec![node(NodeKind::Text, Props::new(), Vec::new(), None)]),
        );
        assert_eq!(preview.children.len(), 1);

        let docked = docked_picker(Props::new(), None);
        assert_eq!(docked.kind, NodeKind::Col);
        assert_eq!(docked.children.len(), 1);
        assert_eq!(docked.children[0].kind, NodeKind::Picker);
        assert_eq!(docked.children[0].key.as_deref(), Some(PICKER_KEY));
    }

    #[test]
    fn picker_query_hides_or_reports_field() {
        assert_eq!(
            picker_query(None),
            PickerQuery {
                query: None,
                cursor: None
            }
        );
        let query = picker_query(Some(&PickerInput::new("foo", 2)));
        assert_eq!(query.query.as_deref(), Some("foo"));
        assert_eq!(query.cursor, Some(2));
    }

    #[test]
    fn picker_keys_table() {
        let table = [
            ("alt+enter", vec!["alt", "enter"]),
            ("escape", vec!["esc"]),
            ("+", vec!["+"]),
            ("ctrl++", vec!["ctrl", "+"]),
            ("shift+escape", vec!["shift", "esc"]),
            ("", Vec::<&str>::new()),
        ];
        for (key, expected) in table {
            assert_eq!(picker_keys(key), expected, "{key:?}");
        }
    }

    #[test]
    fn binding_keys_reads_the_primary_key() {
        let bindings = KeybindingsManager::new();
        assert_eq!(binding_keys(&bindings, "tui.select.up"), vec!["up".to_string()]);
        assert_eq!(binding_keys(&bindings, "tui.select.pageUp"), vec!["pageUp".to_string()]);
        assert!(binding_keys(&bindings, "tui.select.confirm").is_empty());
    }

    #[test]
    fn picker_action_reads_ids_caps_and_options() {
        let from_id = picker_action(
            "confirm",
            "Select",
            Some(PickerKeys::Id("alt+enter")),
            PickerActionOptions::default(),
        );
        assert_eq!(from_id.id, "confirm");
        assert_eq!(from_id.label, "Select");
        assert_eq!(from_id.keys, Some(vec!["alt".to_string(), "enter".to_string()]));

        let from_caps = picker_action(
            "close",
            "Close",
            Some(PickerKeys::Caps(&["esc"])),
            PickerActionOptions::default(),
        );
        assert_eq!(from_caps.keys, Some(vec!["esc".to_string()]));

        let empty = picker_action("x", "X", Some(PickerKeys::Caps(&[])), PickerActionOptions::default());
        assert_eq!(empty.keys, None);

        let opts = picker_action(
            "kill",
            "Kill",
            None,
            PickerActionOptions {
                danger: true,
                disabled: Some(PickerActionDisabled::Reason("no selection".to_string())),
                ..PickerActionOptions::default()
            },
        );
        assert_eq!(
            opts.disabled,
            Some(PickerActionDisabled::Reason("no selection".to_string()))
        );
        assert!(opts.danger);
        assert!(!opts.primary);
    }

    #[test]
    fn close_action_is_the_right_aligned_esc() {
        let close = &*CLOSE_ACTION;
        assert_eq!(close.id, "close");
        assert_eq!(close.label, "Close");
        assert_eq!(close.keys, Some(vec!["esc".to_string()]));
        assert!(close.end);
        let wire = close.to_props();
        assert_eq!(wire.get_str("id"), Some("close"));
        assert_eq!(wire.get_bool("end"), Some(true));
    }

    #[test]
    fn picker_age_buckets() {
        let now = 1_000_000_000_000i64;
        let sec = 1_000i64;
        let table = [
            (now, "now"),
            (now - 59 * sec, "now"),
            (now - 60 * sec, "1m"),
            (now - 59 * 60 * sec, "59m"),
            (now - 60 * 60 * sec, "1h"),
            (now - 23 * 3_600 * sec, "23h"),
            (now - 24 * 3_600 * sec, "1d"),
            (now - 6 * 86_400 * sec, "6d"),
            (now - 7 * 86_400 * sec, "1w"),
            (now - 29 * 86_400 * sec, "4w"),
            (now - 30 * 86_400 * sec, "1mo"),
            (now - 364 * 86_400 * sec, "12mo"),
            (now - 365 * 86_400 * sec, "1y"),
            (now + 10 * sec, "now"),
        ];
        for (at, expected) in table {
            assert_eq!(picker_age(at, now), expected);
        }
    }

    #[test]
    fn picker_hits_merges_and_orders_ranges() {
        let table = [
            ("Hello World", vec!["world"], vec![(6, 11)]),
            ("abcabc", vec!["abc"], vec![(0, 6)]),
            ("aXbxc", vec!["x"], vec![(1, 2), (3, 4)]),
            ("abcdef", vec!["bcd", "cde"], vec![(1, 5)]),
            ("ABC", vec!["bc"], vec![(1, 3)]),
            ("héllo", vec!["llo"], vec![(2, 5)]),
            ("a😀b", vec!["b"], vec![(3, 4)]),
            ("foo", vec![], Vec::new()),
        ];
        for (label, tokens, expected) in table {
            assert_eq!(picker_hits(label, &tokens), expected, "{label:?}");
        }
    }

    #[test]
    fn picker_fuzzy_hits_marks_substrings_and_subsequences() {
        assert_eq!(
            picker_fuzzy_hits("Hello World", "hello"),
            Some(vec![(0, 5)])
        );
        assert_eq!(
            picker_fuzzy_hits("Hello World", "hlo"),
            Some(vec![(0, 1), (2, 3), (4, 5)])
        );
        assert_eq!(
            picker_fuzzy_hits("abcdef", "ace"),
            Some(vec![(0, 1), (2, 3), (4, 5)])
        );
        assert_eq!(
            picker_fuzzy_hits("foo bar", "foo baz"),
            Some(vec![(0, 3), (4, 6)])
        );
        assert_eq!(picker_fuzzy_hits("abc", "xyz"), None);
        assert_eq!(picker_fuzzy_hits("abc", ""), None);
        assert_eq!(picker_fuzzy_hits("abc", "   "), None);
        assert_eq!(picker_fuzzy_hits("héllo", "llo"), Some(vec![(2, 5)]));
        assert_eq!(picker_fuzzy_hits("a😀b", "b"), Some(vec![(3, 4)]));
    }

    #[test]
    fn picker_date_formats_local_parts() {
        assert_eq!(format_picker_date(2026, 9, 28, 18, 2), "28 Sep 2026, 18:02");
        assert_eq!(format_picker_date(2026, 1, 3, 0, 5), "3 Jan 2026, 00:05");
        assert_eq!(format_picker_date(2026, 12, 31, 23, 59), "31 Dec 2026, 23:59");
        // The host-timezone formatter stays a valid date string.
        let date = picker_date(0);
        assert!(date.contains(','), "{date:?}");
        assert!(date.contains(':'), "{date:?}");
    }

    #[test]
    fn picker_event_folds_and_filters() {
        let table = vec![
            (
                event(PickerUiEvent::Select {
                    key: "picker".to_string(),
                    item: "a".to_string(),
                }),
                "picker",
                Some(PickerEvent::Select {
                    item: "a".to_string(),
                }),
            ),
            (
                event(PickerUiEvent::Activate {
                    key: "picker".to_string(),
                    item: "b".to_string(),
                }),
                "picker",
                Some(PickerEvent::Activate {
                    item: "b".to_string(),
                }),
            ),
            (
                event(PickerUiEvent::Action {
                    key: "picker".to_string(),
                    act: "close".to_string(),
                    value: None,
                    mods: vec![],
                }),
                "picker",
                Some(PickerEvent::Action {
                    act: "close".to_string(),
                    value: None,
                }),
            ),
            (
                event(PickerUiEvent::Action {
                    key: "picker".to_string(),
                    act: "tab".to_string(),
                    value: Some("all".to_string()),
                    mods: vec!["shift".to_string()],
                }),
                "picker",
                Some(PickerEvent::Action {
                    act: "tab".to_string(),
                    value: Some("all".to_string()),
                }),
            ),
            (
                event(PickerUiEvent::Other {
                    key: "picker".to_string(),
                }),
                "picker",
                None,
            ),
            (
                event(PickerUiEvent::Select {
                    key: "other".to_string(),
                    item: "a".to_string(),
                }),
                "picker",
                None,
            ),
            (
                event(PickerUiEvent::Select {
                    key: "picker".to_string(),
                    item: "a".to_string(),
                }),
                "",
                None,
            ),
        ];
        for (event, key, expected) in table {
            assert_eq!(picker_event(&event, key), expected, "{key:?}");
        }
        let hoisted = PickerUiEvent::Select {
            key: "picker^^body".to_string(),
            item: "a".to_string(),
        };
        assert_eq!(
            picker_event(&hoisted, "pickerbody"),
            Some(PickerEvent::Select {
                item: "a".to_string()
            })
        );
    }

    #[test]
    fn select_list_picker_maps_the_view() {
        let view = PickerView {
            items: vec![
                select("a", "Alpha").with_description("first"),
                select("b", "Beta").disabled().with_confirmation("Sure?"),
            ],
            selected: Some("a".to_string()),
            pending: Some("b".to_string()),
            query: "al".to_string(),
            cursor: 1,
        };
        let list = FakeList::new(view);
        let options = SelectPickerOptions {
            title: "Pick".to_string(),
            ..SelectPickerOptions::default()
        };
        let props = select_list_picker(&list, &options);
        assert_eq!(props.title, "Pick");
        assert_eq!(props.noun, "options");
        assert_eq!(props.size, "md");
        assert_eq!(props.layout, "rows");
        assert_eq!(props.preview, "none");
        assert_eq!(props.query, None);
        assert_eq!(props.cursor, None);
        assert_eq!(props.items.len(), 2);
        assert_eq!(props.items[0].id, "a");
        assert_eq!(props.items[0].label, "Alpha");
        assert_eq!(props.items[0].detail(), Some("first"));
        assert!(!props.items[0].disabled());
        assert_eq!(props.items[1].id, "b");
        assert_eq!(props.items[1].label, "Beta");
        assert!(props.items[1].disabled());
        assert_eq!(props.selected.as_deref(), Some("a"));
        assert_eq!(props.actions.len(), 2);
        assert_eq!(props.actions[0].id, "confirm");
        assert_eq!(props.actions[0].label, "Select");
        assert_eq!(props.actions[0].keys, Some(vec!["enter".to_string()]));
        assert!(props.actions[0].primary);
        assert_eq!(props.actions[1].id, "close");
        assert!(props.actions[1].end);
        let confirm = props.confirm.expect("pending confirmation");
        assert_eq!(confirm.text, "Sure?");
        assert_eq!(confirm.act, "confirm");
        assert_eq!(confirm.label, "Confirm");
    }

    #[test]
    fn select_list_picker_searchable_and_decorated() {
        let view = PickerView {
            items: vec![select("a", "Alpha")],
            selected: Some("a".to_string()),
            pending: None,
            query: "al".to_string(),
            cursor: 1,
        };
        let list = FakeList::new(view);
        let options = SelectPickerOptions {
            title: "Pick".to_string(),
            searchable: true,
            noun: Some("levels".to_string()),
            current: Some(vec!["a".to_string()]),
            confirm: Some("Apply".to_string()),
            columns: Some(vec![PickerColumn {
                id: "dot".to_string(),
                head: None,
                format: Some("dim".to_string()),
                priority: Some(1),
            }]),
            decorate: Some(Arc::new(|item: &SelectItem| {
                if item.key == "a" {
                    let mut extra = Props::new();
                    extra.set_str("dot", "accent");
                    Some(extra)
                } else {
                    None
                }
            })),
            ..SelectPickerOptions::default()
        };
        let props = select_list_picker(&list, &options);
        assert_eq!(props.query.as_deref(), Some("al"));
        assert_eq!(props.cursor, Some(1));
        assert_eq!(props.noun, "levels");
        assert_eq!(props.current, Some(vec!["a".to_string()]));
        assert_eq!(props.actions[0].label, "Apply");
        assert_eq!(props.columns.as_ref().map(|c| c.len()), Some(1));
        assert_eq!(props.items[0].rest.get_str("dot"), Some("accent"));
        assert_eq!(props.confirm, None);
    }

    #[test]
    fn select_list_picker_wire_defaults() {
        let list = FakeList::new(PickerView::default());
        let options = SelectPickerOptions {
            title: "Pick".to_string(),
            ..SelectPickerOptions::default()
        };
        let props = select_list_picker(&list, &options);
        let wire = props.to_props();
        assert_eq!(wire.get_str("title"), Some("Pick"));
        assert_eq!(wire.get_str("noun"), Some("options"));
        assert_eq!(wire.get_str("size"), Some("md"));
        assert_eq!(wire.get_str("layout"), Some("rows"));
        assert_eq!(wire.get_str("preview"), Some("none"));
        assert!(wire.get("query").expect("query").is_null());
        assert!(wire.get("selected").expect("selected").is_null());
        assert!(wire.get("confirm").expect("confirm").is_null());
        assert_eq!(
            wire.get("items").and_then(Value::as_array).map(|items| items.len()),
            Some(0)
        );
        assert_eq!(
            wire.get("actions")
                .and_then(Value::as_array)
                .map(|actions| actions.len()),
            Some(2)
        );
    }

    #[test]
    fn route_select_list_picker_routes_rows_and_actions() {
        let view = PickerView {
            items: vec![select("a", "Alpha"), select("b", "Beta")],
            selected: Some("a".to_string()),
            pending: None,
            query: String::new(),
            cursor: 0,
        };
        let table: Vec<(PickerUiEvent, bool, Vec<String>)> = vec![
            (
                PickerUiEvent::Select {
                    key: String::new(),
                    item: "a".to_string(),
                },
                true,
                vec!["select:0".to_string()],
            ),
            (
                PickerUiEvent::Activate {
                    key: String::new(),
                    item: "b".to_string(),
                },
                true,
                vec!["activate:1".to_string()],
            ),
            (
                PickerUiEvent::Action {
                    key: String::new(),
                    act: "close".to_string(),
                    value: None,
                    mods: vec![],
                },
                true,
                vec!["cancel".to_string()],
            ),
            (
                PickerUiEvent::Action {
                    key: String::new(),
                    act: "cancel".to_string(),
                    value: None,
                    mods: vec![],
                },
                true,
                vec!["cancel".to_string()],
            ),
            (
                PickerUiEvent::Action {
                    key: String::new(),
                    act: "confirm".to_string(),
                    value: None,
                    mods: vec![],
                },
                true,
                vec!["activate:0".to_string()],
            ),
            (
                PickerUiEvent::Action {
                    key: String::new(),
                    act: "other".to_string(),
                    value: None,
                    mods: vec![],
                },
                false,
                vec![],
            ),
            (
                PickerUiEvent::Select {
                    key: String::new(),
                    item: "missing".to_string(),
                },
                false,
                vec![],
            ),
            (
                PickerUiEvent::Select {
                    key: "elsewhere".to_string(),
                    item: "a".to_string(),
                },
                false,
                vec![],
            ),
        ];
        for (event, expected, calls) in table {
            let mut list = FakeList::new(view.clone());
            assert_eq!(route_select_list_picker(&mut list, &event, ""), expected);
            assert_eq!(list.calls(), calls);
        }
    }

    #[test]
    fn route_confirm_without_selection_is_handled() {
        let view = PickerView {
            items: vec![select("a", "Alpha")],
            selected: None,
            pending: None,
            query: String::new(),
            cursor: 0,
        };
        let mut list = FakeList::new(view);
        let handled = route_select_list_picker(
            &mut list,
            &PickerUiEvent::Action {
                key: String::new(),
                act: "confirm".to_string(),
                value: None,
                mods: vec![],
            },
            "",
        );
        assert!(handled);
        assert!(list.calls().is_empty());
    }

    #[test]
    fn sheet_memoizes_rebuilds_and_routes() {
        let builds = Arc::new(AtomicUsize::new(0));
        let factory = {
            let builds = builds.clone();
            move || {
                builds.fetch_add(1, Ordering::SeqCst);
                SelectPickerOptions {
                    title: "Pick".to_string(),
                    searchable: true,
                    ..SelectPickerOptions::default()
                }
            }
        };
        let view = Arc::new(Mutex::new(PickerView {
            items: vec![select("a", "Alpha")],
            selected: Some("a".to_string()),
            pending: None,
            query: String::new(),
            cursor: 0,
        }));
        let list = FakeList {
            view: view.clone(),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let mut sheet = SelectListSheet::with_options(list, factory, true);
        let first = sheet.describe();
        assert_eq!(first.kind, NodeKind::Col);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        let second = sheet.describe();
        assert_eq!(first, second);
        assert_eq!(builds.load(Ordering::SeqCst), 1);

        view.lock().unwrap().query = "al".to_string();
        let third = sheet.describe();
        assert_ne!(first, third);
        assert_eq!(builds.load(Ordering::SeqCst), 2);

        sheet.invalidate();
        let fourth = sheet.describe();
        assert_eq!(third, fourth);
        assert_eq!(builds.load(Ordering::SeqCst), 3);

        let routed = sheet.handle(&PickerUiEvent::Select {
            key: PICKER_KEY.to_string(),
            item: "a".to_string(),
        });
        assert!(routed);
        assert_eq!(sheet.list().calls(), vec!["select:0".to_string()]);
    }

    #[test]
    fn sheet_root_variant_routes_at_empty_key() {
        let list = FakeList::new(PickerView {
            items: vec![select("a", "Alpha")],
            selected: None,
            pending: None,
            query: String::new(),
            cursor: 0,
        });
        let options = SelectPickerOptions {
            title: "Pick".to_string(),
            ..SelectPickerOptions::default()
        };
        let mut sheet = SelectListSheet::with_options(list, move || options.clone(), false);
        assert_eq!(sheet.describe().kind, NodeKind::Picker);
        assert!(sheet.handle(&PickerUiEvent::Activate {
            key: String::new(),
            item: "a".to_string(),
        }));
        assert_eq!(sheet.list().calls(), vec!["activate:0".to_string()]);
        assert!(!sheet.handle(&PickerUiEvent::Activate {
            key: PICKER_KEY.to_string(),
            item: "a".to_string(),
        }));
    }
}
