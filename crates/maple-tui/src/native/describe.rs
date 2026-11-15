//! Builders for [`NativeNode`] trees, so `describe()` implementations read as
//! node expressions, plus the [`NativeDescribe`] contract for built-in
//! components.
//!
//! The TypeScript original lets a component appear as a child and describes
//! itself per frame. Rust has no trait upcasting from `dyn Component`, so a
//! child slot is always a [`NativeNode`]; a component without a native
//! description is rendered to a `rows` fallback by [`describe_child`].

use sha2::{Digest, Sha256};

use crate::component::Component;
use crate::components::{Input, Spacer, Text, TruncatedText};
use crate::native::node::{
    DescribeContext, NativeNode, NodeKind, Props, Span, TableColumn as NodeTableColumn,
    TableRow as NodeTableRow, Text as NodeText, TreeNode,
};

/// Cursor marker stripped from `rows` fallback lines (see `l/src/tui.ts`).
pub const CURSOR_MARKER: &str = "\x1b_pi:c\x07";

/// A node of any kind.
pub fn node(
    kind: NodeKind,
    props: Props,
    children: Vec<NativeNode>,
    key: Option<&str>,
) -> NativeNode {
    let mut node = NativeNode::with_children(kind, props, children);
    node.key = key.map(str::to_string);
    node
}

/// One styled span; `s` is a space-separated list of semantic or theme tokens.
pub fn span(t: impl Into<String>, s: Option<&str>) -> Span {
    match s {
        Some(tokens) => Span::styled(t, tokens),
        None => Span::new(t),
    }
}

/// Plain or styled text.
pub fn text(content: NodeText, props: Props) -> NativeNode {
    let mut node = NativeNode::with_props(NodeKind::Text, props);
    match content {
        NodeText::Plain(text) => node.props.set_text(text),
        NodeText::Spans(spans) => node.props.set_spans(spans),
    };
    node
}

/// Vertical stack.
pub fn col(children: Vec<NativeNode>, props: Props) -> NativeNode {
    NativeNode::with_children(NodeKind::Col, props, children)
}

/// Horizontal flex row.
pub fn row(children: Vec<NativeNode>, props: Props) -> NativeNode {
    NativeNode::with_children(NodeKind::Row, props, children)
}

/// Card with an optional header, status and collapsible body.
pub fn card(props: Props, children: Vec<NativeNode>) -> NativeNode {
    NativeNode::with_children(NodeKind::Card, props, children)
}

/// Markdown source.
pub fn md(source: impl Into<String>, mut props: Props) -> NativeNode {
    props.set_text(source);
    NativeNode::with_props(NodeKind::Md, props)
}

/// Highlighted code.
pub fn code(source: impl Into<String>, mut props: Props) -> NativeNode {
    props.set_text(source);
    NativeNode::with_props(NodeKind::Code, props)
}

/// Raw terminal output rendered as a mini terminal.
pub fn ansi(output: impl Into<String>, mut props: Props) -> NativeNode {
    props.set_text(output);
    NativeNode::with_props(NodeKind::Ansi, props)
}

/// Unified diff.
pub fn diff(unified: impl Into<String>, mut props: Props) -> NativeNode {
    props.set_text(unified);
    NativeNode::with_props(NodeKind::Diff, props)
}

/// Palette tiers for a shimmer sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct ShimmerPalette {
    /// Dim tier token.
    pub low: Option<String>,
    /// Mid tier token.
    pub mid: Option<String>,
    /// Bright tier token.
    pub high: Option<String>,
}

impl Default for ShimmerPalette {
    fn default() -> Self {
        ShimmerPalette {
            low: Some("dim".to_string()),
            mid: Some("muted".to_string()),
            high: Some("accent".to_string()),
        }
    }
}

/// Terminal-clocked shimmer sweep over `content`.
pub fn shimmer(
    content: NodeText,
    mode: &str,
    palette: &ShimmerPalette,
    key: Option<&str>,
) -> NativeNode {
    let mut props = Props::new();
    match content {
        NodeText::Plain(text) => props.set_text(text),
        NodeText::Spans(spans) => props.set_spans(spans),
    };
    props.set_str("mode", mode);
    let mut tiers = serde_json::Map::new();
    for (name, token) in [
        ("low", &palette.low),
        ("mid", &palette.mid),
        ("high", &palette.high),
    ] {
        if let Some(token) = token {
            tiers.insert(name.to_string(), serde_json::Value::String(token.clone()));
        }
    }
    props.insert("palette", serde_json::Value::Object(tiers));
    node(NodeKind::Shimmer, props, Vec::new(), key)
}

/// Tree rows.
pub fn tree(nodes: Vec<TreeNode>, mut props: Props) -> NativeNode {
    props.insert(
        "nodes",
        serde_json::Value::Array(nodes.iter().map(TreeNode::to_value).collect()),
    );
    NativeNode::with_props(NodeKind::Tree, props)
}

/// Data table.
pub fn table(
    columns: Vec<NodeTableColumn>,
    rows: Vec<NodeTableRow>,
    mut props: Props,
) -> NativeNode {
    props.insert(
        "cols",
        serde_json::Value::Array(columns.iter().map(NodeTableColumn::to_value).collect()),
    );
    props.insert(
        "rows",
        serde_json::Value::Array(rows.iter().map(NodeTableRow::to_value).collect()),
    );
    NativeNode::with_props(NodeKind::Table, props)
}

/// A mounted, empty, invisible node for components that describe nothing.
pub fn empty_node() -> NativeNode {
    let mut props = Props::new();
    props.set_hidden(true);
    NativeNode::with_props(NodeKind::Col, props)
}

/// `described` with a sibling identity key.
pub fn keyed(mut described: NativeNode, key: impl Into<String>) -> NativeNode {
    described.key = Some(key.into());
    described
}

/// `described` mounted but not laid out when `hidden`.
pub fn with_hidden(mut described: NativeNode, hidden: bool) -> NativeNode {
    described.props.set_hidden(hidden);
    described
}

/// A described tree as a prop value (a `hovercard`): plain nodes only.
pub fn prop_node(described: &NativeNode) -> serde_json::Value {
    described.to_prop_value()
}

/// Stable node key for an arbitrary identity string.
///
/// The TypeScript original uses `Bun.hash`, whose algorithm is not vendored;
/// this port hashes with SHA-256 and folds to a base-36 integer, which keeps
/// keys short, stable and independent of the standard library's randomized
/// hasher.
pub fn stable_key(identity: &str) -> String {
    let digest = Sha256::digest(identity.as_bytes());
    let mut folded: u64 = 0;
    for (index, byte) in digest.iter().enumerate() {
        folded ^= u64::from(*byte) << ((index % 8) * 8);
    }
    base36(folded)
}

/// Format `value` in base 36 (lowercase, no sign).
fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Drop absent entries from a child list.
pub fn compact(items: Vec<Option<NativeNode>>) -> Vec<NativeNode> {
    items.into_iter().flatten().collect()
}

/// A `kv` grid from label/value pairs with muted labels, skipping empty values;
/// `None` when nothing is left.
pub fn kv(pairs: Vec<(String, NodeText)>) -> Option<NativeNode> {
    let mut items = Vec::new();
    for (label, value) in pairs {
        if value.is_empty() {
            continue;
        }
        let mut item = serde_json::Map::new();
        item.insert(
            "k".to_string(),
            serde_json::Value::Array(vec![span(label, Some("muted")).to_value()]),
        );
        item.insert("v".to_string(), value.to_value());
        items.push(serde_json::Value::Object(item));
    }
    if items.is_empty() {
        return None;
    }
    let mut props = Props::new();
    props.insert("items", serde_json::Value::Array(items));
    Some(NativeNode::with_props(NodeKind::Kv, props))
}

/// One list `item`, keyed by the value select/activate events report back.
pub fn item(key: impl Into<String>, props: Props) -> NativeNode {
    let mut node = NativeNode::with_props(NodeKind::Item, props);
    node.key = Some(key.into());
    node
}

/// A `list` of items.
pub fn list(items: Vec<NativeNode>, props: Props, key: Option<&str>) -> NativeNode {
    node(NodeKind::List, props, items, key)
}

/// Terminal-clocked elapsed counter: `age` ms already elapsed.
pub fn elapsed(age_ms: f64, stopped: bool) -> NativeNode {
    let ms = age_ms.max(0.0).round() as i64;
    let mut props = Props::new();
    props.set_i64("age", ms);
    props.set_str("format", "short");
    if stopped {
        props.set_i64("stopped", ms);
    }
    NativeNode::with_props(NodeKind::Elapsed, props)
}

/// The key names of one key id (`"ctrl+o"` → `["ctrl", "o"]`).
pub fn key_names(key: &str) -> Vec<String> {
    if key == "+" {
        return vec!["+".to_string()];
    }
    let parts = if let Some(prefix) = key.strip_suffix("++") {
        let mut parts: Vec<String> = prefix.split('+').map(str::to_string).collect();
        parts.push("+".to_string());
        parts
    } else {
        key.split('+').map(str::to_string).collect()
    };
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

/// Keycap for one key id; the terminal draws platform glyphs.
pub fn kbd(key: &str, node_key: Option<&str>) -> NativeNode {
    let mut props = Props::new();
    props.insert(
        "keys",
        serde_json::Value::Array(
            key_names(key)
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    node(NodeKind::Kbd, props, Vec::new(), node_key)
}

/// Render a component with no native description to a `rows` fallback.
///
/// This is the migration path the TypeScript reconciler uses for components
/// without `describe`: their `render(cols)` rows are sent verbatim. The cursor
/// marker is internal and stripped.
pub fn describe_child(child: &mut dyn Component, cx: &DescribeContext) -> NativeNode {
    let cols = cx.cols.max(1);
    let lines: Vec<String> = child
        .render(cols)
        .into_iter()
        .map(|line| line.replace(CURSOR_MARKER, ""))
        .collect();
    let mut props = Props::new();
    props.set_i64("cols", i64::from(cols));
    props.insert(
        "lines",
        serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
    );
    NativeNode::with_props(NodeKind::Rows, props)
}

/// Implemented by components that describe themselves as native nodes.
pub trait NativeDescribe {
    /// Describe this component for `cx`.
    fn describe_native(&self, cx: &DescribeContext) -> NativeNode;
}

impl NativeDescribe for Text {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_str("wrap", "word");
        text(NodeText::Plain(self.text().to_string()), props)
    }
}

impl NativeDescribe for TruncatedText {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_str("truncate", "end");
        text(NodeText::Plain(self.text().to_string()), props)
    }
}

impl NativeDescribe for Spacer {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let size = match self.lines() {
            0 => "none",
            1 => "xs",
            2 => "sm",
            3 => "md",
            _ => "lg",
        };
        let mut props = Props::new();
        props.set_str("size", size);
        NativeNode::with_props(NodeKind::Spacer, props)
    }
}

impl NativeDescribe for Input {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_text(self.value());
        if !self.placeholder().is_empty() {
            props.set_str("placeholder", self.placeholder());
        }
        let cursor = self.value().encode_utf16().count() as i64;
        props.set_i64("cursor", cursor);
        // The maple input exposes no password flag; the terminal treats the
        // field as plain text.
        NativeNode::with_props(NodeKind::Input, props)
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Panel;

    #[test]
    fn builders_set_kind_and_props() {
        let mut props = Props::new();
        props.set_tone(crate::native::node::Tone::Accent);
        let node = col(
            vec![text(NodeText::Plain("hi".to_string()), Props::new())],
            props.clone(),
        );
        assert_eq!(node.kind, NodeKind::Col);
        assert_eq!(node.props.tone(), Some(crate::native::node::Tone::Accent));
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.children[0].kind, NodeKind::Text);
        assert_eq!(node.children[0].text(), Some("hi"));
    }

    #[test]
    fn text_builders_accept_plain_and_spans() {
        let plain = md("hello", Props::new());
        assert_eq!(plain.kind, NodeKind::Md);
        assert_eq!(plain.text(), Some("hello"));
        let mut props = Props::new();
        props.set_bool("stream", true);
        let streamed = md("hi", props);
        assert_eq!(streamed.prop("stream"), Some(&serde_json::Value::Bool(true)));
        let spans = text(NodeText::Spans(vec![span("a", Some("muted"))]), Props::new());
        assert_eq!(spans.props.spans().unwrap()[0].s.as_deref(), Some("muted"));
    }

    #[test]
    fn card_and_hidden_helpers() {
        let hidden = with_hidden(empty_node(), true);
        assert_eq!(hidden.kind, NodeKind::Col);
        assert_eq!(hidden.props.hidden(), Some(true));
        let keyed = keyed(hidden, "body");
        assert_eq!(keyed.key.as_deref(), Some("body"));
        let prop = prop_node(&keyed);
        assert!(prop.get("id").is_none());
        assert_eq!(prop["k"], "col");
        assert_eq!(prop["p"]["hidden"], true);
    }

    #[test]
    fn kv_skips_empty_values() {
        assert!(kv(Vec::new()).is_none());
        assert!(kv(vec![("empty".to_string(), NodeText::plain(""))]).is_none());
        let grid = kv(vec![
            ("name".to_string(), NodeText::plain("omp")),
            ("blank".to_string(), NodeText::plain("")),
        ])
        .unwrap();
        let items = grid.prop("items").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["v"], "omp");
        assert_eq!(items[0]["k"][0]["s"], "muted");
    }

    #[test]
    fn list_item_and_elapsed() {
        let mut props = Props::new();
        props.set_str("label", "row");
        let list = list(vec![item("a", props)], Props::new(), Some("rows"));
        assert_eq!(list.key.as_deref(), Some("rows"));
        assert_eq!(list.children[0].key.as_deref(), Some("a"));
        let tick = elapsed(1500.0, false);
        assert_eq!(tick.prop("age"), Some(&serde_json::Value::Number(1500.into())));
        assert_eq!(tick.prop("stopped"), None);
        let stopped = elapsed(1500.0, true);
        assert_eq!(stopped.prop("stopped"), tick.prop("age"));
    }

    #[test]
    fn key_names_split_and_keep_plus() {
        assert_eq!(key_names("ctrl+o"), vec!["ctrl", "o"]);
        assert_eq!(key_names("+"), vec!["+"]);
        assert_eq!(key_names("ctrl++"), vec!["ctrl", "+"]);
        assert_eq!(key_names("alt+enter"), vec!["alt", "enter"]);
        let key = kbd("ctrl+s", Some("save"));
        assert_eq!(key.key.as_deref(), Some("save"));
        assert_eq!(key.prop("keys").unwrap()[0], "ctrl");
    }

    #[test]
    fn stable_key_is_stable_and_base36() {
        assert_eq!(stable_key("omp.tool"), stable_key("omp.tool"));
        assert_ne!(stable_key("omp.tool"), stable_key("omp.other"));
        assert!(stable_key("x").chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn shimmer_carries_palette_tokens() {
        let node = shimmer(
            NodeText::Spans(vec![span("working", Some("muted"))]),
            "classic",
            &ShimmerPalette::default(),
            Some("spin"),
        );
        assert_eq!(node.kind, NodeKind::Shimmer);
        assert_eq!(node.prop("mode"), Some(&serde_json::Value::String("classic".into())));
        assert_eq!(node.prop("palette").unwrap()["mid"], "muted");
        assert_eq!(node.key.as_deref(), Some("spin"));
    }

    #[test]
    fn tree_and_table_builders() {
        let tree = tree(vec![TreeNode::new("a", "Root")], Props::new());
        assert_eq!(tree.prop("nodes").unwrap()[0]["label"], "Root");
        let table = table(
            vec![NodeTableColumn::new("c0").with_head("Name")],
            vec![NodeTableRow::new("r0").cell("c0", "omp")],
            Props::new(),
        );
        assert_eq!(table.prop("cols").unwrap()[0]["head"], "Name");
        assert_eq!(table.prop("rows").unwrap()[0]["cells"]["c0"], "omp");
    }

    #[test]
    fn compact_drops_absent_children() {
        let children = compact(vec![
            Some(NativeNode::new(NodeKind::Text)),
            None,
            Some(NativeNode::new(NodeKind::Rule)),
        ]);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn builtin_components_describe() {
        let cx = DescribeContext::new(40);
        let text = Text::new("hello");
        let described = text.describe_native(&cx);
        assert_eq!(described.kind, NodeKind::Text);
        assert_eq!(described.text(), Some("hello"));

        let clipped = TruncatedText::new("hello");
        assert_eq!(clipped.describe_native(&cx).prop("truncate"), Some(&serde_json::Value::String("end".into())));

        let spacer = Spacer::new(2);
        assert_eq!(spacer.describe_native(&cx).prop("size"), Some(&serde_json::Value::String("sm".into())));

        let mut input = Input::new();
        input.set_value("abc");
        input.set_placeholder("type");
        let node = input.describe_native(&cx);
        assert_eq!(node.kind, NodeKind::Input);
        assert_eq!(node.text(), Some("abc"));
        assert_eq!(node.prop("placeholder"), Some(&serde_json::Value::String("type".into())));

        // ProgressBar and Table have no re-export in `components/mod.rs` yet;
        // their describe impls land with that wiring.
    }

    #[test]
    fn child_without_describe_falls_back_to_rows() {
        let cx = DescribeContext::new(8);
        let mut panel = Panel::new(0, 0);
        panel.add_child(Text::new("row"));
        let node = describe_child(&mut panel, &cx);
        assert_eq!(node.kind, NodeKind::Rows);
        assert_eq!(node.prop("cols"), Some(&serde_json::Value::Number(8.into())));
        let lines = node.prop("lines").unwrap().as_array().unwrap();
        assert_eq!(lines[0].as_str().unwrap_or("").trim_end(), "row");
    }
}
