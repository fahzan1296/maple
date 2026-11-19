//! Icon glyph normalization for native frames.
//!
//! Components style icons as Private Use Area codepoints (Nerd Font glyphs)
//! baked into text with a padding space, e.g. `{t:"<glyph> Model", s:"statusLineModel"}`.
//! The terminal draws icons and spaces them itself, so every PUA run becomes
//! its own span carrying the `icon` token next to the original tokens (colour
//! survives), and the single space that padded it from the neighbouring text
//! is dropped. Applied once, centrally, to every text field of a node's props
//! before they go on the wire.
//!
//! Reference: l/src/native/icons.ts
//!
//! Deviations:
//! - The reference's `TspSpan`/`TspText`/`TspKind` wire types are the crate's
//!   [`Span`]/[`Text`]/[`NodeKind`], and props are a [`Props`] map rather than
//!   a JS object.
//! - "The same array/object when nothing changed" becomes `None`:
//!   [`normalize_icon_spans`] returns `Option<Vec<Span>>` and
//!   [`normalize_icon_props`] returns `Option<Props>`, both `None` when the
//!   input carries no icon glyph.
//! - [`normalize_icon_props`] takes `&Props` (the crate always has a prop map)
//!   instead of `Record | undefined`.
//! - The reference treats any array as text and would throw on an array of
//!   non-span values; this port leaves such a value untouched.
//! - Span objects round-trip through [`Span`], so unknown span fields are
//!   dropped (the crate ignores unknown wire fields in both directions).
//! - The reference reads no theme, so every function here is pure; language
//!   icon lookup (`Theme::get_lang_icon`) is unrelated to this module.

use serde_json::{Map, Value};

use crate::native::node::{NodeKind, Props, Span, Text};

/// One split piece: the span and whether it is an icon glyph run.
struct Piece {
    span: Span,
    icon: bool,
}

/// True when `ch` is a Private Use Area codepoint (a Nerd Font glyph).
fn is_pua(ch: char) -> bool {
    matches!(
        ch as u32,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}

/// True when `text` carries at least one icon glyph.
fn contains_pua(text: &str) -> bool {
    text.chars().any(is_pua)
}

/// Byte ranges of every run of icon glyphs in `text`.
fn pua_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (index, ch) in text.char_indices() {
        if is_pua(ch) {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(begin) = start.take() {
            runs.push((begin, index));
        }
    }
    if let Some(begin) = start {
        runs.push((begin, text.len()));
    }
    runs
}

/// The style tokens for an icon span: the existing tokens plus `icon`, or
/// `icon` alone when there were none.
fn with_icon(tokens: Option<&str>) -> Option<String> {
    match tokens {
        None | Some("") => Some("icon".to_string()),
        Some(tokens) if tokens.split(' ').any(|token| token == "icon") => Some(tokens.to_string()),
        Some(tokens) => Some(format!("{tokens} icon")),
    }
}

/// Spans with every PUA run split into an `icon` span; `None` when no span
/// carries one (the reference returns the same array then).
pub fn normalize_icon_spans(spans: &[Span]) -> Option<Vec<Span>> {
    if !spans.iter().any(|span| contains_pua(&span.t)) {
        return None;
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for span in spans {
        let mut at = 0;
        for (start, end) in pua_runs(&span.t) {
            if start > at {
                pieces.push(Piece {
                    span: Span {
                        t: span.t[at..start].to_string(),
                        ..span.clone()
                    },
                    icon: false,
                });
            }
            pieces.push(Piece {
                span: Span {
                    t: span.t[start..end].to_string(),
                    s: with_icon(span.s.as_deref()),
                    ..span.clone()
                },
                icon: true,
            });
            at = end;
        }
        if at < span.t.len() {
            pieces.push(Piece {
                span: if at == 0 {
                    span.clone()
                } else {
                    Span {
                        t: span.t[at..].to_string(),
                        ..span.clone()
                    }
                },
                icon: false,
            });
        }
    }
    // The terminal spaces icons itself: drop the one space on each side that
    // padded them.
    for index in 0..pieces.len() {
        if !pieces[index].icon {
            continue;
        }
        let drop_prev = index > 0
            && !pieces[index - 1].icon
            && pieces[index - 1].span.t.ends_with(' ');
        let drop_next = index + 1 < pieces.len()
            && !pieces[index + 1].icon
            && pieces[index + 1].span.t.starts_with(' ');
        if drop_prev {
            pieces[index - 1].span.t.pop();
        }
        if drop_next {
            pieces[index + 1].span.t.remove(0);
        }
    }
    let mut out = Vec::with_capacity(pieces.len());
    for piece in pieces {
        if !piece.span.t.is_empty() {
            out.push(piece.span);
        }
    }
    Some(out)
}

/// A [`Text`] with icons normalized; plain strings with icons become spans.
pub fn normalize_icon_text(text: Text) -> Text {
    match text {
        Text::Plain(plain) => {
            if contains_pua(&plain) {
                let spans = normalize_icon_spans(&[Span::new(plain)]);
                Text::Spans(spans.unwrap_or_default())
            } else {
                Text::Plain(plain)
            }
        }
        Text::Spans(spans) => match normalize_icon_spans(&spans) {
            Some(next) => Text::Spans(next),
            None => Text::Spans(spans),
        },
    }
}

/// Normalize a JSON text value (a string or a span array); `None` when it is
/// not text or carries no icon glyph.
fn normalize_text_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(plain) if contains_pua(plain) => {
            let spans = normalize_icon_spans(&[Span::new(plain.as_str())]);
            Some(match spans {
                Some(spans) => Value::Array(spans.iter().map(Span::to_value).collect()),
                None => value.clone(),
            })
        }
        Value::String(_) => None,
        Value::Array(items) => {
            let spans: Option<Vec<Span>> = items.iter().map(Span::from_value).collect();
            let spans = spans?;
            let normalized = normalize_icon_spans(&spans)?;
            Some(Value::Array(
                normalized.iter().map(Span::to_value).collect(),
            ))
        }
        _ => None,
    }
}

/// Map `f` over `items`, returning `None` when nothing changed. `f` returns
/// `Some` only for a replacement (the reference's "same array" contract).
fn map_same<T: PartialEq + Clone>(items: &[T], mut f: impl FnMut(&T) -> Option<T>) -> Option<Vec<T>> {
    let mut out: Option<Vec<T>> = None;
    for (index, item) in items.iter().enumerate() {
        match f(item) {
            Some(next) => {
                out.get_or_insert_with(|| items[..index].to_vec()).push(next);
            }
            None => {
                if let Some(out) = out.as_mut() {
                    out.push(item.clone());
                }
            }
        }
    }
    out
}

/// Copy `props` into `out` and set `key`, unless `value` already equals the
/// original prop (the reference's `put`).
fn put_prop(out: &mut Option<Props>, props: &Props, key: &str, value: Value) {
    if props.get(key) == Some(&value) {
        return;
    }
    out.get_or_insert_with(|| props.clone()).insert(key, value);
}

/// `TspText` props per kind (besides the structured ones handled below).
fn text_fields(kind: NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Text | NodeKind::Shimmer | NodeKind::Seg => &["spans"],
        NodeKind::Card | NodeKind::Section | NodeKind::Overlay => &["head"],
        NodeKind::Rule | NodeKind::Spinner | NodeKind::Progress => &["label"],
        NodeKind::List => &["empty"],
        NodeKind::Item => &["label", "detail", "value"],
        NodeKind::Editor | NodeKind::Input => &["prompt"],
        _ => &[],
    }
}

/// Normalize the text fields of one `tabs`/`kv` item; `None` when unchanged.
fn normalize_item_fields(item: &Map<String, Value>, fields: &[&str]) -> Option<Map<String, Value>> {
    let mut next: Option<Map<String, Value>> = None;
    for field in fields {
        let Some(value) = item.get(*field) else {
            continue;
        };
        let Some(normalized) = normalize_text_value(value) else {
            continue;
        };
        next.get_or_insert_with(|| item.clone())
            .insert((*field).to_string(), normalized);
    }
    next
}

/// Normalize one table column header; `None` when unchanged.
fn normalize_column(column: &Map<String, Value>) -> Option<Map<String, Value>> {
    let normalized = normalize_text_value(column.get("head")?)?;
    let mut next = column.clone();
    next.insert("head".to_string(), normalized);
    Some(next)
}

/// Normalize one table row's cells; `None` when unchanged.
fn normalize_row(row: &Map<String, Value>) -> Option<Map<String, Value>> {
    let cells = row.get("cells")?.as_object()?;
    let mut next_cells: Option<Map<String, Value>> = None;
    for (column, value) in cells {
        let Some(normalized) = normalize_text_value(value) else {
            continue;
        };
        next_cells
            .get_or_insert_with(|| cells.clone())
            .insert(column.clone(), normalized);
    }
    let cells = next_cells?;
    let mut next = row.clone();
    next.insert("cells".to_string(), Value::Object(cells));
    Some(next)
}

/// Normalize one tree node's label and children; `None` when unchanged.
fn normalize_tree_value(value: &Value) -> Option<Value> {
    let node = value.as_object()?;
    let label = match node.get("label") {
        Some(label) => normalize_text_value(label),
        None => None,
    };
    let children = match node.get("children").filter(|children| !children.is_null()) {
        Some(Value::Array(children)) => map_same(children, normalize_tree_value).map(Value::Array),
        _ => None,
    };
    if label.is_none() && children.is_none() {
        return None;
    }
    let mut next = node.clone();
    if let Some(label) = label {
        next.insert("label".to_string(), label);
    }
    if let Some(children) = children {
        next.insert("children".to_string(), children);
    }
    Some(Value::Object(next))
}

/// Props with every text field's icons normalized; `None` when nothing
/// changed (the reference returns the same object then).
pub fn normalize_icon_props(kind: NodeKind, props: &Props) -> Option<Props> {
    let mut out: Option<Props> = None;

    // A plain `text` label holding an icon becomes spans.
    if matches!(kind, NodeKind::Text | NodeKind::Shimmer) {
        if let Some(text) = props.get_str("text") {
            if contains_pua(text) {
                let normalized = out.get_or_insert_with(|| props.clone());
                normalized.remove("text");
                let spans = normalize_icon_spans(&[Span::new(text)]);
                normalized.set_spans(spans.unwrap_or_default());
            }
        }
    }

    for field in text_fields(kind) {
        let normalized = out
            .as_ref()
            .unwrap_or(props)
            .get(field)
            .and_then(normalize_text_value);
        if let Some(normalized) = normalized {
            put_prop(&mut out, props, field, normalized);
        }
    }

    match kind {
        NodeKind::Tabs | NodeKind::Kv => {
            let fields: &[&str] = if kind == NodeKind::Tabs {
                &["label"]
            } else {
                &["k", "v"]
            };
            if let Some(items) = props.get("items").and_then(Value::as_array) {
                let normalized = map_same(items, |item| {
                    let map = item.as_object()?;
                    normalize_item_fields(map, fields).map(Value::Object)
                });
                if let Some(items) = normalized {
                    put_prop(&mut out, props, "items", Value::Array(items));
                }
            }
        }
        NodeKind::Table => {
            if let Some(cols) = props.get("cols").and_then(Value::as_array) {
                let normalized = map_same(cols, |col| {
                    let map = col.as_object()?;
                    normalize_column(map).map(Value::Object)
                });
                if let Some(cols) = normalized {
                    put_prop(&mut out, props, "cols", Value::Array(cols));
                }
            }
            if let Some(rows) = props.get("rows").and_then(Value::as_array) {
                let normalized = map_same(rows, |row| {
                    let map = row.as_object()?;
                    normalize_row(map).map(Value::Object)
                });
                if let Some(rows) = normalized {
                    put_prop(&mut out, props, "rows", Value::Array(rows));
                }
            }
        }
        NodeKind::Tree => {
            if let Some(nodes) = props.get("nodes").and_then(Value::as_array) {
                if let Some(nodes) = map_same(nodes, normalize_tree_value) {
                    put_prop(&mut out, props, "nodes", Value::Array(nodes));
                }
            }
        }
        _ => {}
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::describe::span;

    #[test]
    fn spans_split_icon_runs_and_drop_padding() {
        let spans = vec![Span::styled("a \u{E000} b", "muted")];
        let normalized = normalize_icon_spans(&spans).expect("icon run");
        let texts: Vec<&str> = normalized.iter().map(|span| span.t.as_str()).collect();
        assert_eq!(texts, ["a", "\u{E000}", "b"]);
        assert_eq!(normalized[0].s.as_deref(), Some("muted"));
        assert_eq!(normalized[1].s.as_deref(), Some("muted icon"));
    }

    #[test]
    fn spans_without_icons_are_unchanged() {
        assert!(normalize_icon_spans(&[Span::new("hello"), Span::styled("world", "dim")]).is_none());
        assert!(normalize_icon_spans(&[]).is_none());
    }

    #[test]
    fn separate_icon_runs_split_separately() {
        let spans = vec![Span::new("\u{E000}a\u{E001}")];
        let normalized = normalize_icon_spans(&spans).expect("icon run");
        let texts: Vec<&str> = normalized.iter().map(|span| span.t.as_str()).collect();
        assert_eq!(texts, ["\u{E000}", "a", "\u{E001}"]);
    }

    #[test]
    fn icon_token_is_added_once() {
        for (tokens, expected) in [
            (None, "icon"),
            (Some(""), "icon"),
            (Some("icon dim"), "icon dim"),
            (Some("muted"), "muted icon"),
        ] {
            let span = match tokens {
                None => Span::new("\u{E000}"),
                Some(tokens) => Span::styled("\u{E000}", tokens),
            };
            let normalized = normalize_icon_spans(&[span]).expect("icon run");
            assert_eq!(normalized[0].s.as_deref(), Some(expected));
        }
    }

    #[test]
    fn icon_span_keeps_other_span_fields() {
        let mut span = Span::styled(" \u{E000} ", "muted");
        span.fx = Some("bold".to_string());
        span.href = Some("https://example.invalid".to_string());
        let normalized = normalize_icon_spans(&[span]).expect("icon run");
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].t, "\u{E000}");
        assert_eq!(normalized[0].fx.as_deref(), Some("bold"));
        assert_eq!(normalized[0].href.as_deref(), Some("https://example.invalid"));
    }

    #[test]
    fn plain_text_with_icons_becomes_spans() {
        let text = normalize_icon_text(Text::plain("\u{E000} Model"));
        let Text::Spans(spans) = text else {
            panic!("expected spans");
        };
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].t, "\u{E000}");
        assert_eq!(spans[0].s.as_deref(), Some("icon"));
        assert_eq!(spans[1].t, "Model");
        assert!(normalize_icon_text(Text::plain("plain")).as_plain().is_some());
    }

    #[test]
    fn text_props_label_becomes_spans() {
        let mut props = Props::new();
        props.set_text("\u{E000} Model");
        let out = normalize_icon_props(NodeKind::Text, &props).expect("changed");
        assert!(out.text().is_none());
        let spans = out.spans().expect("spans");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].t, "\u{E000}");
        assert_eq!(spans[0].s.as_deref(), Some("icon"));
    }

    #[test]
    fn props_without_icons_are_unchanged() {
        let mut props = Props::new();
        props.set_text("hello");
        assert!(normalize_icon_props(NodeKind::Text, &props).is_none());
        assert!(normalize_icon_props(NodeKind::Item, &Props::new()).is_none());
    }

    #[test]
    fn text_fields_follow_the_kind() {
        let mut props = Props::new();
        props.set_str("label", "\u{E000} ignored");
        assert!(normalize_icon_props(NodeKind::Text, &props).is_none());
        let out = normalize_icon_props(NodeKind::Rule, &props).expect("changed");
        assert!(out.get("label").and_then(Value::as_array).is_some());
    }

    #[test]
    fn item_label_detail_value_normalize() {
        let mut props = Props::new();
        props.set_str("label", "\u{E000} Run");
        props.set_str("detail", "plain");
        props.set_str("value", "\u{F0000} 3");
        let out = normalize_icon_props(NodeKind::Item, &props).expect("changed");
        let label = out.get("label").and_then(Value::as_array).expect("spans");
        assert_eq!(label[0]["t"], "\u{E000}");
        assert_eq!(out.get_str("detail"), Some("plain"));
        let value = out.get("value").and_then(Value::as_array).expect("spans");
        assert_eq!(value[0]["t"], "\u{F0000}");
    }

    #[test]
    fn tabs_and_kv_items_normalize() {
        let mut item = Map::new();
        item.insert("label".to_string(), Value::String("\u{E000} Tab".to_string()));
        let mut props = Props::new();
        props.insert("items", Value::Array(vec![Value::Object(item)]));
        let out = normalize_icon_props(NodeKind::Tabs, &props).expect("changed");
        let items = out.get("items").and_then(Value::as_array).expect("items");
        assert_eq!(items[0]["label"][0]["t"], "\u{E000}");

        let mut item = Map::new();
        item.insert("k".to_string(), Value::String("\u{E000} Key".to_string()));
        let mut props = Props::new();
        props.insert("items", Value::Array(vec![Value::Object(item)]));
        let out = normalize_icon_props(NodeKind::Kv, &props).expect("changed");
        let items = out.get("items").and_then(Value::as_array).expect("items");
        assert_eq!(items[0]["k"][0]["t"], "\u{E000}");
    }

    #[test]
    fn table_columns_and_rows_normalize() {
        let mut col = Map::new();
        col.insert("id".to_string(), Value::String("c0".to_string()));
        col.insert("head".to_string(), Value::String("\u{E000} Name".to_string()));
        let mut cells = Map::new();
        cells.insert("c0".to_string(), Value::String("\u{E000} Cell".to_string()));
        let mut row = Map::new();
        row.insert("id".to_string(), Value::String("r0".to_string()));
        row.insert("cells".to_string(), Value::Object(cells));
        let mut props = Props::new();
        props.insert("cols", Value::Array(vec![Value::Object(col)]));
        props.insert("rows", Value::Array(vec![Value::Object(row)]));
        let out = normalize_icon_props(NodeKind::Table, &props).expect("changed");
        let cols = out.get("cols").and_then(Value::as_array).expect("cols");
        assert_eq!(cols[0]["head"][0]["t"], "\u{E000}");
        let rows = out.get("rows").and_then(Value::as_array).expect("rows");
        assert_eq!(rows[0]["cells"]["c0"][0]["t"], "\u{E000}");
    }

    #[test]
    fn tree_labels_normalize_recursively() {
        let mut child = Map::new();
        child.insert("id".to_string(), Value::String("b".to_string()));
        child.insert("label".to_string(), Value::String("\u{E000} Leaf".to_string()));
        let mut root = Map::new();
        root.insert("id".to_string(), Value::String("a".to_string()));
        root.insert("label".to_string(), Value::String("\u{E000} Root".to_string()));
        root.insert("children".to_string(), Value::Array(vec![Value::Object(child)]));
        let mut props = Props::new();
        props.insert("nodes", Value::Array(vec![Value::Object(root)]));
        let out = normalize_icon_props(NodeKind::Tree, &props).expect("changed");
        let nodes = out.get("nodes").and_then(Value::as_array).expect("nodes");
        assert_eq!(nodes[0]["label"][0]["t"], "\u{E000}");
        assert_eq!(nodes[0]["children"][0]["label"][0]["t"], "\u{E000}");
    }

    #[test]
    fn span_arrays_normalize_in_props() {
        let mut props = Props::new();
        props.set_spans(vec![span("a \u{E000} b", Some("muted"))]);
        let out = normalize_icon_props(NodeKind::Text, &props).expect("changed");
        let spans = out.spans().expect("spans");
        let texts: Vec<&str> = spans.iter().map(|span| span.t.as_str()).collect();
        assert_eq!(texts, ["a", "\u{E000}", "b"]);
    }
}
