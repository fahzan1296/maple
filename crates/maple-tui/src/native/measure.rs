//! A rough lower bound on how many terminal rows a described subtree takes,
//! for layout decisions the program must make before the terminal lays the
//! content out (whether a sheet fills a narrow pane). Counts cell lines at the
//! surface width; the terminal's own fonts and padding differ, so callers
//! compare against a fraction of the viewport, not an exact fit.
//!
//! Reference: l/src/native/measure.ts
//!
//! Deviations:
//! - The reference's `NativeChild = NativeNode | Component` becomes
//!   [`MeasureChild`]: the crate's [`NativeNode`] children are already
//!   resolved nodes (see `native/state.rs`), so a component only appears as a
//!   root or when a caller builds one explicitly.
//! - `DescribedOf` resolves a [`NativeDescribe`] instead of a `Component`; the
//!   trait is object-safe, so the alias is a `&dyn Fn`. The closure owns the
//!   [`crate::native::node::DescribeContext`] (the reference's
//!   `Component.describe` reads one from its owner).
//! - `cols` and `limit` are `usize`; the reference's fractional extents
//!   (`"2.5lines"`) truncate, and `cols` is clamped to at least 1.
//! - The reference's `Bun.stringWidth` is the crate's
//!   `text_shape::visible_width`.

use serde_json::Value;

use crate::native::describe::NativeDescribe;
use crate::native::node::{NativeNode, NodeKind};
use crate::text_shape::visible_width;

/// Leaf images, media and charts: a fixed guess, since their height is the
/// terminal's choice.
const GRAPHIC_ROWS: usize = 8;
/// Nesting deeper than this is not measured (cyclic or runaway descriptions).
const MAX_DEPTH: usize = 64;

/// The last description of a component, or `None` when unknown.
pub type DescribedOf<'a> = &'a dyn Fn(&dyn NativeDescribe) -> Option<NativeNode>;

/// A child slot: a described node, or a component resolved through
/// [`DescribedOf`].
#[derive(Clone, Copy)]
pub enum MeasureChild<'a> {
    /// An already-described node.
    Node(&'a NativeNode),
    /// A component resolved through [`DescribedOf`].
    Component(&'a dyn NativeDescribe),
}

/// Estimated rows of `root` at `cols` cells wide, stopping once `limit` is
/// reached (the result never exceeds it). Component children are measured
/// through `described`.
pub fn estimate_rows(
    root: MeasureChild<'_>,
    cols: usize,
    limit: usize,
    described: DescribedOf<'_>,
) -> usize {
    RowEstimator {
        cols: cols.max(1),
        described,
    }
    .measure(root, limit, 0)
}

/// The measuring walk. `cap` is the row budget left; it never goes negative.
struct RowEstimator<'a> {
    cols: usize,
    described: DescribedOf<'a>,
}

impl<'a> RowEstimator<'a> {
    fn measure(&self, child: MeasureChild<'a>, cap: usize, depth: usize) -> usize {
        if cap == 0 || depth > MAX_DEPTH {
            return 0;
        }
        let described: Option<NativeNode>;
        let node: &NativeNode = match child {
            MeasureChild::Node(node) => node,
            MeasureChild::Component(component) => {
                described = (self.described)(component);
                match described.as_ref() {
                    Some(node) => node,
                    None => return 0,
                }
            }
        };
        if node.kind == NodeKind::Chart {
            return cap.min(GRAPHIC_ROWS);
        }
        let props = &node.props;
        if props.hidden() == Some(true) {
            return 0;
        }
        let max = lines_extent(
            props
                .get("max")
                .and_then(Value::as_object)
                .and_then(|max| max.get("h")),
        );
        let own_cap = match max {
            Some(max) => cap.min(max),
            None => cap,
        };
        let own = self.kind_rows(node, own_cap, depth);
        let min = lines_extent(
            props
                .get("min")
                .and_then(Value::as_object)
                .and_then(|min| min.get("h")),
        )
        .unwrap_or(0);
        own.max(min).min(cap)
    }

    /// Stack `children`, stopping once `cap` rows are accounted for.
    fn sum(&self, children: &[NativeNode], cap: usize, depth: usize) -> usize {
        let mut rows = 0;
        for child in children {
            if rows >= cap {
                break;
            }
            rows += self.measure(MeasureChild::Node(child), cap - rows, depth + 1);
        }
        rows
    }

    /// The tallest of `children`.
    fn tallest(&self, children: &[NativeNode], cap: usize, depth: usize) -> usize {
        let mut rows = 0;
        for child in children {
            rows = rows.max(self.measure(MeasureChild::Node(child), cap, depth + 1));
        }
        rows
    }

    /// Rows of `text`, one per line, or wrapped at the surface width.
    fn text_rows(&self, text: Option<&str>, wrap: bool, cap: usize) -> usize {
        let Some(text) = text else {
            return 0;
        };
        let mut rows = 0;
        let mut start = 0;
        while rows < cap {
            let end = text[start..].find('\n').map(|index| start + index);
            let line = match end {
                Some(end) => &text[start..end],
                None => &text[start..],
            };
            rows += if wrap {
                let width = visible_width(line);
                ((width + self.cols - 1) / self.cols).max(1)
            } else {
                1
            };
            match end {
                Some(end) => start = end + 1,
                None => break,
            }
        }
        rows.min(cap)
    }

    fn kind_rows(&self, node: &NativeNode, cap: usize, depth: usize) -> usize {
        let props = &node.props;
        match node.kind {
            // Scrollers of their own: they take whatever height they get.
            NodeKind::Picker | NodeKind::Prefs => cap,
            NodeKind::Row => self.tallest(&node.children, cap, depth),
            // One page shows at a time: the tab strip, then the tallest page.
            NodeKind::Tabs => 1 + self.tallest(&node.children, cap.saturating_sub(1), depth),
            NodeKind::Card | NodeKind::Section => {
                let head = if props.get("head").is_none() { 0 } else { 1 };
                let body = self.sum(&node.children, cap.saturating_sub(head), depth);
                if props.get_bool("collapsed") != Some(true) {
                    head + body
                } else {
                    let preview = if node.kind == NodeKind::Card {
                        preview_lines(props.get("preview")).unwrap_or(0)
                    } else {
                        0
                    };
                    head + preview.min(body)
                }
            }
            NodeKind::Text => {
                let text = props.text().map(str::to_string).or_else(|| {
                    props
                        .spans()
                        .map(|spans| spans.iter().map(|span| span.t.as_str()).collect())
                });
                let wrap = props.get_str("wrap") != Some("none") && !props.contains_key("truncate");
                let limit = match props.get("lines").and_then(Value::as_f64) {
                    Some(lines) => cap.min(lines.max(0.0) as usize),
                    None => cap,
                };
                self.text_rows(text.as_deref(), wrap, limit)
            }
            NodeKind::Md | NodeKind::Editor | NodeKind::Input => {
                self.text_rows(props.text(), true, cap)
            }
            NodeKind::Code | NodeKind::Diff => self.text_rows(props.text(), false, cap),
            NodeKind::Ansi => {
                if props.get_bool("follow") == Some(true) {
                    return cap;
                }
                let preview = if props.get("preview").map_or(false, Value::is_object) {
                    preview_lines(props.get("preview")).unwrap_or(0)
                } else {
                    cap
                };
                self.text_rows(props.text(), false, cap.min(preview))
            }
            NodeKind::Rows => cap.min(
                props
                    .get("lines")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            ),
            NodeKind::Kv => cap.min(
                props
                    .get("items")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            ),
            NodeKind::Table => cap.min(
                1 + props
                    .get("rows")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            ),
            NodeKind::Tree => tree_rows(props.get("nodes").and_then(Value::as_array), cap),
            NodeKind::Json => cap.min(json_rows(props.get("value"), cap)),
            NodeKind::Checklist => {
                let mut rows = 0;
                if let Some(phases) = props.get("phases").and_then(Value::as_array) {
                    for phase in phases {
                        let Some(phase) = phase.as_object() else {
                            continue;
                        };
                        let collapsed = phase.get("collapsed").and_then(Value::as_bool) == Some(true);
                        let items = phase
                            .get("items")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        rows += 1 + if collapsed { 0 } else { items };
                    }
                }
                cap.min(rows)
            }
            NodeKind::List => {
                let limit = match props.get("max") {
                    Some(Value::Number(number)) => {
                        number.as_f64().unwrap_or(0.0).max(0.0) as usize
                    }
                    Some(Value::Object(max)) => max
                        .get("lines")
                        .and_then(Value::as_f64)
                        .unwrap_or(cap as f64)
                        .max(0.0) as usize,
                    _ => cap,
                };
                self.sum(&node.children, cap.min(limit), depth)
            }
            NodeKind::Item => {
                let rows = if has_text(props.get("detail")) { 2 } else { 1 };
                rows + self.sum(&node.children, cap, depth)
            }
            NodeKind::Image | NodeKind::Media => cap.min(GRAPHIC_ROWS),
            NodeKind::Tool | NodeKind::Agent => {
                1 + self.sum(&node.children, cap.saturating_sub(1), depth)
            }
            // Containers stack their children; leaves (badges, keys, buttons,
            // meters) take a row.
            _ => {
                if node.children.is_empty() {
                    1
                } else {
                    self.sum(&node.children, cap, depth)
                }
            }
        }
    }
}

/// True when `text` is a non-empty string or a span array with a non-empty run.
fn has_text(text: Option<&Value>) -> bool {
    match text {
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(spans)) => spans.iter().any(|span| {
            span.get("t")
                .and_then(Value::as_str)
                .map_or(false, |run| !run.is_empty())
        }),
        _ => false,
    }
}

/// A `Nlines` extent as a row count; fractions and widths say nothing about
/// rows.
fn lines_extent(extent: Option<&Value>) -> Option<usize> {
    let text = extent?.as_str()?;
    let number = text.strip_suffix("lines")?;
    let lines = parse_float_prefix(number)?;
    if lines.is_finite() && lines >= 0.0 {
        Some(lines as usize)
    } else {
        None
    }
}

/// `Number.parseFloat` over the leading numeric prefix of `text`.
fn parse_float_prefix(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut index = 0;
    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        index += 1;
    }
    let digits_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    if index == digits_start {
        if text[digits_start..].starts_with("Infinity") {
            return Some(if text.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        return None;
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        let mut lookahead = index + 1;
        if lookahead < bytes.len() && (bytes[lookahead] == b'+' || bytes[lookahead] == b'-') {
            lookahead += 1;
        }
        let exponent_digits = lookahead;
        while lookahead < bytes.len() && bytes[lookahead].is_ascii_digit() {
            lookahead += 1;
        }
        if lookahead > exponent_digits {
            index = lookahead;
        }
    }
    text[..index].parse().ok()
}

/// The `lines` field of a `preview` object.
fn preview_lines(value: Option<&Value>) -> Option<usize> {
    let preview = value?.as_object()?;
    let lines = preview.get("lines")?.as_f64()?;
    Some(lines.max(0.0) as usize)
}

/// Rows of a tree: one per node, plus the children of open nodes.
fn tree_rows(nodes: Option<&Vec<Value>>, cap: usize) -> usize {
    let mut rows = 0;
    let Some(nodes) = nodes else {
        return 0;
    };
    for node in nodes {
        if rows >= cap {
            break;
        }
        rows += 1;
        let open = node.get("open").and_then(Value::as_bool).unwrap_or(false);
        if open {
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                rows += tree_rows(Some(children), cap - rows);
            }
        }
    }
    rows.min(cap)
}

/// Rows of `value` pretty-printed one entry per line.
fn json_rows(value: Option<&Value>, cap: usize) -> usize {
    let Some(value) = value else {
        return 1;
    };
    if value.is_null() || !(value.is_object() || value.is_array()) {
        return 1;
    }
    let mut rows = 2;
    let entries: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    };
    for entry in entries {
        if rows >= cap {
            break;
        }
        rows += json_rows(Some(entry), cap - rows);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text as ComponentText;
    use crate::native::describe::{ansi, card, code, col, item, list, md, row, text};
    use crate::native::node::{DescribeContext, Props, Text as NodeText};
    use serde_json::{Map, Value};

    fn text_node(content: &str) -> NativeNode {
        text(NodeText::Plain(content.to_string()), Props::new())
    }

    /// Measure a node tree with a `described` that resolves every component.
    fn rows_of(root: &NativeNode, cols: usize, limit: usize) -> usize {
        let cx = DescribeContext::new(cols.max(1) as u16);
        let described: DescribedOf<'_> = &|component: &dyn NativeDescribe| {
            Some(component.describe_native(&cx))
        };
        estimate_rows(MeasureChild::Node(root), cols, limit, described)
    }

    fn none_described(_component: &dyn NativeDescribe) -> Option<NativeNode> {
        None
    }

    #[test]
    fn cols_clamp_to_one() {
        let root = text_node("abcdefghij");
        assert_eq!(rows_of(&root, 0, 100), 10);
        assert_eq!(rows_of(&root, 1, 100), 10);
        assert_eq!(rows_of(&root, 2, 100), 5);
    }

    #[test]
    fn limit_is_never_exceeded_and_exits_early() {
        let children: Vec<NativeNode> = (0..1000).map(|_| text_node("x")).collect();
        let root = col(children, Props::new());
        assert_eq!(rows_of(&root, 10, 5), 5);
        assert_eq!(rows_of(&root, 10, 1), 1);
        assert_eq!(rows_of(&root, 10, 0), 0);
    }

    #[test]
    fn depth_cap_stops_runaway_nesting() {
        let mut root = text_node("leaf");
        for _ in 0..80 {
            root = col(vec![root], Props::new());
        }
        assert_eq!(rows_of(&root, 10, 100), 0);
        let mut at_cap = text_node("leaf");
        for _ in 0..64 {
            at_cap = col(vec![at_cap], Props::new());
        }
        assert_eq!(rows_of(&at_cap, 10, 100), 1);
        let mut past_cap = text_node("leaf");
        for _ in 0..65 {
            past_cap = col(vec![past_cap], Props::new());
        }
        assert_eq!(rows_of(&past_cap, 10, 100), 0);
    }

    #[test]
    fn graphic_kinds_take_fixed_rows() {
        for kind in [NodeKind::Image, NodeKind::Media, NodeKind::Chart] {
            let root = NativeNode::new(kind);
            assert_eq!(rows_of(&root, 10, 100), GRAPHIC_ROWS);
            assert_eq!(rows_of(&root, 10, 3), 3);
        }
    }

    #[test]
    fn hidden_and_unresolved_are_zero() {
        let mut root = text_node("x");
        root.props.set_hidden(true);
        assert_eq!(rows_of(&root, 10, 10), 0);

        let described: DescribedOf<'_> = &none_described;
        let component = ComponentText::new("hello");
        assert_eq!(
            estimate_rows(MeasureChild::Component(&component), 10, 10, described),
            0
        );
    }

    #[test]
    fn component_root_is_measured_through_described() {
        let cx = DescribeContext::new(40);
        let described: DescribedOf<'_> = &|component: &dyn NativeDescribe| {
            Some(component.describe_native(&cx))
        };
        let component = ComponentText::new("hello");
        assert_eq!(
            estimate_rows(MeasureChild::Component(&component), 40, 10, described),
            1
        );
    }

    #[test]
    fn text_wraps_or_not_and_honours_lines() {
        let root = text(NodeText::Plain("aaaa".to_string()), Props::new());
        assert_eq!(rows_of(&root, 2, 10), 2);

        let mut props = Props::new();
        props.set_str("wrap", "none");
        let root = text(NodeText::Plain("aaaa".to_string()), props);
        assert_eq!(rows_of(&root, 2, 10), 1);

        let mut props = Props::new();
        props.set_i64("lines", 2);
        let root = text(NodeText::Plain("aaaa".to_string()), props);
        assert_eq!(rows_of(&root, 1, 10), 2);

        let mut props = Props::new();
        props.set_str("truncate", "end");
        let root = text(NodeText::Plain("aaaa".to_string()), props);
        assert_eq!(rows_of(&root, 2, 10), 1);

        let root = text(NodeText::Plain("aa\nbb".to_string()), Props::new());
        assert_eq!(rows_of(&root, 10, 10), 2);
    }

    #[test]
    fn md_wraps_and_code_does_not() {
        assert_eq!(rows_of(&md("aaaa", Props::new()), 2, 10), 2);
        assert_eq!(rows_of(&code("aaaa", Props::new()), 2, 10), 1);
    }

    #[test]
    fn ansi_follow_takes_cap_and_preview_limits() {
        let mut props = Props::new();
        props.set_bool("follow", true);
        let root = ansi("a\nb\nc", props);
        assert_eq!(rows_of(&root, 10, 4), 4);

        let mut props = Props::new();
        props.insert("preview", serde_json::json!({ "lines": 2 }));
        let root = ansi("a\nb\nc\nd", props);
        assert_eq!(rows_of(&root, 10, 10), 2);
    }

    #[test]
    fn rows_kv_and_table_count_entries() {
        let mut props = Props::new();
        props.insert(
            "lines",
            Value::Array(vec![Value::String("a".into()), Value::String("b".into())]),
        );
        let root = NativeNode::with_props(NodeKind::Rows, props);
        assert_eq!(rows_of(&root, 10, 10), 2);
        assert_eq!(rows_of(&root, 10, 1), 1);

        let mut props = Props::new();
        props.insert(
            "items",
            Value::Array(vec![
                Value::Object(Map::new()),
                Value::Object(Map::new()),
                Value::Object(Map::new()),
            ]),
        );
        let root = NativeNode::with_props(NodeKind::Kv, props);
        assert_eq!(rows_of(&root, 10, 10), 3);

        let mut props = Props::new();
        props.insert("rows", Value::Array(vec![Value::Object(Map::new())]));
        let root = NativeNode::with_props(NodeKind::Table, props);
        assert_eq!(rows_of(&root, 10, 10), 2);
    }

    #[test]
    fn tree_counts_open_children_only() {
        let mut child = Map::new();
        child.insert("label".to_string(), Value::String("c".into()));
        let mut open = Map::new();
        open.insert("label".to_string(), Value::String("r".into()));
        open.insert("open".to_string(), Value::Bool(true));
        open.insert("children".to_string(), Value::Array(vec![Value::Object(child)]));
        let mut props = Props::new();
        props.insert("nodes", Value::Array(vec![Value::Object(open)]));
        let root = NativeNode::with_props(NodeKind::Tree, props);
        assert_eq!(rows_of(&root, 10, 10), 2);
        assert_eq!(rows_of(&root, 10, 1), 1);

        let mut closed = Map::new();
        closed.insert("label".to_string(), Value::String("r".into()));
        closed.insert("open".to_string(), Value::Bool(false));
        closed.insert(
            "children".to_string(),
            Value::Array(vec![Value::Object(Map::new())]),
        );
        let mut props = Props::new();
        props.insert("nodes", Value::Array(vec![Value::Object(closed)]));
        let root = NativeNode::with_props(NodeKind::Tree, props);
        assert_eq!(rows_of(&root, 10, 10), 1);
    }

    #[test]
    fn json_counts_entries_one_per_line() {
        let mut props = Props::new();
        props.insert("value", serde_json::json!({ "a": 1, "b": 2 }));
        let root = NativeNode::with_props(NodeKind::Json, props);
        assert_eq!(rows_of(&root, 10, 10), 4);
        assert_eq!(rows_of(&root, 10, 2), 2);

        let mut props = Props::new();
        props.insert("value", Value::Number(5.into()));
        let root = NativeNode::with_props(NodeKind::Json, props);
        assert_eq!(rows_of(&root, 10, 10), 1);
    }

    #[test]
    fn checklist_counts_phases_and_open_items() {
        let mut collapsed = Map::new();
        collapsed.insert("collapsed".to_string(), Value::Bool(true));
        collapsed.insert("items".to_string(), Value::Array(vec![Value::Object(Map::new())]));
        let mut open = Map::new();
        open.insert(
            "items".to_string(),
            Value::Array(vec![Value::Object(Map::new()), Value::Object(Map::new())]),
        );
        let mut props = Props::new();
        props.insert(
            "phases",
            Value::Array(vec![Value::Object(collapsed), Value::Object(open)]),
        );
        let root = NativeNode::with_props(NodeKind::Checklist, props);
        assert_eq!(rows_of(&root, 10, 10), 4);
    }

    #[test]
    fn list_honours_max_then_the_limit() {
        let children: Vec<NativeNode> = (0..5).map(|_| text_node("x")).collect();
        let mut props = Props::new();
        props.set_i64("max", 2);
        let root = list(children, props, None);
        assert_eq!(rows_of(&root, 10, 10), 2);
        assert_eq!(rows_of(&root, 10, 1), 1);

        let children: Vec<NativeNode> = (0..5).map(|_| text_node("x")).collect();
        let mut props = Props::new();
        props.insert("max", serde_json::json!({ "lines": 3 }));
        let root = list(children, props, None);
        assert_eq!(rows_of(&root, 10, 10), 3);
    }

    #[test]
    fn item_detail_adds_a_row() {
        let mut props = Props::new();
        props.set_str("label", "a");
        assert_eq!(rows_of(&item("a", props), 10, 10), 1);

        let mut props = Props::new();
        props.set_str("detail", "d");
        assert_eq!(rows_of(&item("a", props), 10, 10), 2);

        let mut props = Props::new();
        props.insert(
            "detail",
            Value::Array(vec![crate::native::node::Span::new("").to_value()]),
        );
        assert_eq!(rows_of(&item("a", props), 10, 10), 1);

        let mut props = Props::new();
        props.insert(
            "detail",
            Value::Array(vec![crate::native::node::Span::new("d").to_value()]),
        );
        assert_eq!(rows_of(&item("a", props), 10, 10), 2);
    }

    #[test]
    fn tool_and_agent_add_a_header_row() {
        let root = NativeNode::with_children(NodeKind::Tool, Props::new(), vec![text_node("x")]);
        assert_eq!(rows_of(&root, 10, 10), 2);
        let root = NativeNode::with_children(NodeKind::Agent, Props::new(), vec![text_node("x")]);
        assert_eq!(rows_of(&root, 10, 10), 2);
    }

    #[test]
    fn row_takes_the_tallest_child() {
        let root = row(
            vec![
                text_node("x"),
                text(NodeText::Plain("aaaa".to_string()), Props::new()),
            ],
            Props::new(),
        );
        assert_eq!(rows_of(&root, 2, 10), 2);
    }

    #[test]
    fn tabs_take_strip_plus_tallest_page() {
        let root = NativeNode::with_children(
            NodeKind::Tabs,
            Props::new(),
            vec![text_node("x"), text_node("y")],
        );
        assert_eq!(rows_of(&root, 10, 10), 2);
    }

    #[test]
    fn card_and_section_heads_and_collapse() {
        let mut props = Props::new();
        props.set_head(NodeText::Plain("h".to_string()));
        let root = card(props, vec![text_node("x")]);
        assert_eq!(rows_of(&root, 10, 10), 2);
        let root = card(Props::new(), vec![text_node("x")]);
        assert_eq!(rows_of(&root, 10, 10), 1);

        let mut props = Props::new();
        props.set_head(NodeText::Plain("h".to_string()));
        props.set_bool("collapsed", true);
        props.insert("preview", serde_json::json!({ "lines": 1 }));
        let root = card(props, vec![text_node("x"), text_node("y")]);
        assert_eq!(rows_of(&root, 10, 10), 2);

        let mut props = Props::new();
        props.set_bool("collapsed", true);
        let root = card(props, vec![text_node("x"), text_node("y")]);
        assert_eq!(rows_of(&root, 10, 10), 0);
    }

    #[test]
    fn picker_and_prefs_take_the_cap() {
        for kind in [NodeKind::Picker, NodeKind::Prefs] {
            let root = NativeNode::new(kind);
            assert_eq!(rows_of(&root, 10, 4), 4);
        }
    }

    #[test]
    fn containers_sum_and_leaves_take_one_row() {
        let root = NativeNode::with_children(NodeKind::Badge, Props::new(), vec![text_node("x")]);
        assert_eq!(rows_of(&root, 10, 10), 1);
        let root = NativeNode::new(NodeKind::Badge);
        assert_eq!(rows_of(&root, 10, 10), 1);
        let root = col(vec![text_node("x"), text_node("y")], Props::new());
        assert_eq!(rows_of(&root, 10, 10), 2);
    }

    #[test]
    fn min_and_max_extents_clamp_the_estimate() {
        let mut props = Props::new();
        props.insert("max", serde_json::json!({ "h": "3lines" }));
        props.insert("min", serde_json::json!({ "h": "5lines" }));
        let root = text(NodeText::Plain("x".to_string()), props);
        assert_eq!(rows_of(&root, 10, 10), 5);

        let mut props = Props::new();
        props.insert("max", serde_json::json!({ "h": "2 lines" }));
        let root = text(NodeText::Plain("xxxx".to_string()), props);
        assert_eq!(rows_of(&root, 1, 10), 2);

        let mut props = Props::new();
        props.insert("max", serde_json::json!({ "h": "lines" }));
        let root = text(NodeText::Plain("x".to_string()), props);
        assert_eq!(rows_of(&root, 10, 10), 1);
    }
}
