//! The describe contract: the [`NativeNode`] model, the TSP kind vocabulary,
//! Reference: `l/src/native/node.ts`.
//! and the context components consult while describing themselves.
//!
//! A node is the wire node minus its id: the reconciler derives ids from the
//! description (region + keypath). Component children are resolved before a
//! tree reaches the reconciler, so children here are plain nodes; a component
//! without a native description becomes a `rows` fallback (see
//! [`crate::native::describe::describe_child`]).

use serde_json::{Map, Value};

/// One component kind in the v1 TSP vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// Vertical stack.
    Col,
    /// Horizontal flex row.
    Row,
    /// Card with optional header, status and collapsible body.
    Card,
    /// Headed group.
    Section,
    /// Horizontal rule.
    Rule,
    /// Whitespace.
    Spacer,
    /// Plain or styled text.
    Text,
    /// Markdown source.
    Md,
    /// Highlighted code.
    Code,
    /// Unified diff.
    Diff,
    /// Raw terminal output.
    Ansi,
    /// Math source.
    Math,
    /// Blob-backed image.
    Image,
    /// File the terminal loads by path.
    Media,
    /// Label/value grid.
    Kv,
    /// Data table.
    Table,
    /// Tree view.
    Tree,
    /// Raw JSON drawn as a tree.
    Json,
    /// Small label chip.
    Badge,
    /// Keycap glyphs.
    Kbd,
    /// Named icon glyph.
    Icon,
    /// Indeterminate/activity spinner.
    Spinner,
    /// Terminal-clocked shimmer sweep.
    Shimmer,
    /// Terminal-clocked elapsed counter.
    Elapsed,
    /// Determinate progress bar.
    Progress,
    /// Rate value with unit.
    Rate,
    /// Selectable list.
    List,
    /// One list row.
    Item,
    /// Tab strip.
    Tabs,
    /// Multi-line editor.
    Editor,
    /// Single-line input.
    Input,
    /// Push button.
    Button,
    /// Status bar segment container.
    Status,
    /// One status segment.
    Seg,
    /// Floating overlay.
    Overlay,
    /// Transient toast.
    Toast,
    /// Pre-rendered ANSI rows (migration fallback).
    Rows,
    /// Data-first picker sheet.
    Picker,
    /// Data-first settings page set.
    Prefs,
    /// Tool call card.
    Tool,
    /// Todo list.
    Checklist,
    /// Subagent row.
    Agent,
    /// Chart.
    Chart,
    /// Bar/ring/block meter.
    Meter,
    /// Thinking-effort glyph.
    Effort,
    /// Engine-produced shell presentation.
    Shell,
}

impl NodeKind {
    /// Wire name of this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Col => "col",
            NodeKind::Row => "row",
            NodeKind::Card => "card",
            NodeKind::Section => "section",
            NodeKind::Rule => "rule",
            NodeKind::Spacer => "spacer",
            NodeKind::Text => "text",
            NodeKind::Md => "md",
            NodeKind::Code => "code",
            NodeKind::Diff => "diff",
            NodeKind::Ansi => "ansi",
            NodeKind::Math => "math",
            NodeKind::Image => "image",
            NodeKind::Media => "media",
            NodeKind::Kv => "kv",
            NodeKind::Table => "table",
            NodeKind::Tree => "tree",
            NodeKind::Json => "json",
            NodeKind::Badge => "badge",
            NodeKind::Kbd => "kbd",
            NodeKind::Icon => "icon",
            NodeKind::Spinner => "spinner",
            NodeKind::Shimmer => "shimmer",
            NodeKind::Elapsed => "elapsed",
            NodeKind::Progress => "progress",
            NodeKind::Rate => "rate",
            NodeKind::List => "list",
            NodeKind::Item => "item",
            NodeKind::Tabs => "tabs",
            NodeKind::Editor => "editor",
            NodeKind::Input => "input",
            NodeKind::Button => "button",
            NodeKind::Status => "status",
            NodeKind::Seg => "seg",
            NodeKind::Overlay => "overlay",
            NodeKind::Toast => "toast",
            NodeKind::Rows => "rows",
            NodeKind::Picker => "picker",
            NodeKind::Prefs => "prefs",
            NodeKind::Tool => "tool",
            NodeKind::Checklist => "checklist",
            NodeKind::Agent => "agent",
            NodeKind::Chart => "chart",
            NodeKind::Meter => "meter",
            NodeKind::Effort => "effort",
            NodeKind::Shell => "shell",
        }
    }

    /// Parse a wire kind name; `None` for an unknown kind.
    pub fn from_str(name: &str) -> Option<Self> {
        let kind = match name {
            "col" => NodeKind::Col,
            "row" => NodeKind::Row,
            "card" => NodeKind::Card,
            "section" => NodeKind::Section,
            "rule" => NodeKind::Rule,
            "spacer" => NodeKind::Spacer,
            "text" => NodeKind::Text,
            "md" => NodeKind::Md,
            "code" => NodeKind::Code,
            "diff" => NodeKind::Diff,
            "ansi" => NodeKind::Ansi,
            "math" => NodeKind::Math,
            "image" => NodeKind::Image,
            "media" => NodeKind::Media,
            "kv" => NodeKind::Kv,
            "table" => NodeKind::Table,
            "tree" => NodeKind::Tree,
            "json" => NodeKind::Json,
            "badge" => NodeKind::Badge,
            "kbd" => NodeKind::Kbd,
            "icon" => NodeKind::Icon,
            "spinner" => NodeKind::Spinner,
            "shimmer" => NodeKind::Shimmer,
            "elapsed" => NodeKind::Elapsed,
            "progress" => NodeKind::Progress,
            "rate" => NodeKind::Rate,
            "list" => NodeKind::List,
            "item" => NodeKind::Item,
            "tabs" => NodeKind::Tabs,
            "editor" => NodeKind::Editor,
            "input" => NodeKind::Input,
            "button" => NodeKind::Button,
            "status" => NodeKind::Status,
            "seg" => NodeKind::Seg,
            "overlay" => NodeKind::Overlay,
            "toast" => NodeKind::Toast,
            "rows" => NodeKind::Rows,
            "picker" => NodeKind::Picker,
            "prefs" => NodeKind::Prefs,
            "tool" => NodeKind::Tool,
            "checklist" => NodeKind::Checklist,
            "agent" => NodeKind::Agent,
            "chart" => NodeKind::Chart,
            "meter" => NodeKind::Meter,
            "effort" => NodeKind::Effort,
            "shell" => NodeKind::Shell,
            _ => return None,
        };
        Some(kind)
    }

    /// Every kind in the v1 vocabulary, in wire order.
    pub const ALL: &'static [NodeKind] = &[
        NodeKind::Col,
        NodeKind::Row,
        NodeKind::Card,
        NodeKind::Section,
        NodeKind::Rule,
        NodeKind::Spacer,
        NodeKind::Text,
        NodeKind::Md,
        NodeKind::Code,
        NodeKind::Diff,
        NodeKind::Ansi,
        NodeKind::Math,
        NodeKind::Image,
        NodeKind::Media,
        NodeKind::Kv,
        NodeKind::Table,
        NodeKind::Tree,
        NodeKind::Json,
        NodeKind::Badge,
        NodeKind::Kbd,
        NodeKind::Icon,
        NodeKind::Spinner,
        NodeKind::Shimmer,
        NodeKind::Elapsed,
        NodeKind::Progress,
        NodeKind::Rate,
        NodeKind::List,
        NodeKind::Item,
        NodeKind::Tabs,
        NodeKind::Editor,
        NodeKind::Input,
        NodeKind::Button,
        NodeKind::Status,
        NodeKind::Seg,
        NodeKind::Overlay,
        NodeKind::Toast,
        NodeKind::Rows,
        NodeKind::Picker,
        NodeKind::Prefs,
        NodeKind::Tool,
        NodeKind::Checklist,
        NodeKind::Agent,
        NodeKind::Chart,
        NodeKind::Meter,
        NodeKind::Effort,
        NodeKind::Shell,
    ];

    /// Whether the `text`/`splice` ops address this kind's `text` prop.
    pub fn is_text_kind(self) -> bool {
        matches!(
            self,
            NodeKind::Text
                | NodeKind::Md
                | NodeKind::Code
                | NodeKind::Ansi
                | NodeKind::Math
                | NodeKind::Editor
                | NodeKind::Input
                | NodeKind::Shimmer
        )
    }
}

impl Default for NodeKind {
    fn default() -> Self {
        NodeKind::Col
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Semantic colour of a node's chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Default chrome.
    Neutral,
    /// Accent chrome.
    Accent,
    /// Informational chrome.
    Info,
    /// Success chrome.
    Success,
    /// Warning chrome.
    Warning,
    /// Error chrome.
    Error,
    /// Pending/in-flight chrome.
    Pending,
    /// Muted chrome.
    Muted,
    /// User-authored chrome.
    User,
}

impl Tone {
    /// Wire name of this tone.
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Accent => "accent",
            Tone::Info => "info",
            Tone::Success => "success",
            Tone::Warning => "warning",
            Tone::Error => "error",
            Tone::Pending => "pending",
            Tone::Muted => "muted",
            Tone::User => "user",
        }
    }

    /// Parse a wire tone; `None` for an unknown tone.
    pub fn from_str(name: &str) -> Option<Self> {
        let tone = match name {
            "neutral" => Tone::Neutral,
            "accent" => Tone::Accent,
            "info" => Tone::Info,
            "success" => Tone::Success,
            "warning" => Tone::Warning,
            "error" => Tone::Error,
            "pending" => Tone::Pending,
            "muted" => Tone::Muted,
            "user" => Tone::User,
            _ => return None,
        };
        Some(tone)
    }
}

impl Default for Tone {
    fn default() -> Self {
        Tone::Neutral
    }
}

/// One styled run of text; `s` holds space-separated semantic tokens.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// The run's text.
    pub t: String,
    /// Space-separated semantic or theme tokens.
    pub s: Option<String>,
    /// Per-span visual effect.
    pub fx: Option<String>,
    /// Link target.
    pub href: Option<String>,
}

impl Span {
    /// A bare run with no style.
    pub fn new(t: impl Into<String>) -> Self {
        Span {
            t: t.into(),
            s: None,
            fx: None,
            href: None,
        }
    }

    /// A run carrying space-separated style tokens.
    pub fn styled(t: impl Into<String>, s: impl Into<String>) -> Self {
        Span {
            t: t.into(),
            s: Some(s.into()),
            fx: None,
            href: None,
        }
    }

    /// Serialize to the wire span object.
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("t".to_string(), Value::String(self.t.clone()));
        if let Some(s) = &self.s {
            out.insert("s".to_string(), Value::String(s.clone()));
        }
        if let Some(fx) = &self.fx {
            out.insert("fx".to_string(), Value::String(fx.clone()));
        }
        if let Some(href) = &self.href {
            out.insert("href".to_string(), Value::String(href.clone()));
        }
        Value::Object(out)
    }

    /// Parse a wire span object; `None` when `t` is missing or not a string.
    pub fn from_value(value: &Value) -> Option<Self> {
        let obj = value.as_object()?;
        let t = obj.get("t")?.as_str()?.to_string();
        let s = obj.get("s").and_then(Value::as_str).map(str::to_string);
        let fx = obj.get("fx").and_then(Value::as_str).map(str::to_string);
        let href = obj.get("href").and_then(Value::as_str).map(str::to_string);
        Some(Span { t, s, fx, href })
    }
}

impl Default for Span {
    fn default() -> Self {
        Span::new(String::new())
    }
}

/// Text given either as one plain string or as styled spans.
#[derive(Clone, Debug, PartialEq)]
pub enum Text {
    /// One plain string.
    Plain(String),
    /// Styled runs.
    Spans(Vec<Span>),
}

impl Text {
    /// Plain text.
    pub fn plain(text: impl Into<String>) -> Self {
        Text::Plain(text.into())
    }

    /// Styled spans.
    pub fn spans(spans: Vec<Span>) -> Self {
        Text::Spans(spans)
    }

    /// The plain string, when this is [`Text::Plain`].
    pub fn as_plain(&self) -> Option<&str> {
        match self {
            Text::Plain(text) => Some(text),
            Text::Spans(_) => None,
        }
    }

    /// Concatenated text of every span, ignoring styling.
    pub fn plain_text(&self) -> String {
        match self {
            Text::Plain(text) => text.clone(),
            Text::Spans(spans) => spans.iter().map(|span| span.t.as_str()).collect(),
        }
    }

    /// Whether there is no visible text.
    pub fn is_empty(&self) -> bool {
        match self {
            Text::Plain(text) => text.is_empty(),
            Text::Spans(spans) => spans.iter().all(|span| span.t.is_empty()),
        }
    }

    /// Serialize to the wire text value (string or span array).
    pub fn to_value(&self) -> Value {
        match self {
            Text::Plain(text) => Value::String(text.clone()),
            Text::Spans(spans) => Value::Array(spans.iter().map(Span::to_value).collect()),
        }
    }
}

impl Default for Text {
    fn default() -> Self {
        Text::Plain(String::new())
    }
}

/// Node props: a JSON object with typed accessors for the common fields.
///
/// The wire format is an open object (unknown fields are ignored in both
/// directions), so props travel as a map; the accessors keep describe builders
/// and the reconciler readable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Props {
    values: Map<String, Value>,
}

impl Props {
    /// An empty prop set.
    pub fn new() -> Self {
        Props { values: Map::new() }
    }

    /// Wrap an existing JSON object.
    pub fn from_map(values: Map<String, Value>) -> Self {
        Props { values }
    }

    /// Consume the props into their JSON object.
    pub fn into_map(self) -> Map<String, Value> {
        self.values
    }

    /// Borrow a prop.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Borrow a string prop.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(Value::as_str)
    }

    /// Borrow a bool prop.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(Value::as_bool)
    }

    /// Borrow an integer prop.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.values.get(key).and_then(Value::as_i64)
    }

    /// Borrow a numeric prop as `f64`.
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(Value::as_f64)
    }

    /// Insert a JSON prop.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.values.insert(key.into(), value);
    }

    /// Set a string prop.
    pub fn set_str(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.to_string(), Value::String(value.into()));
    }

    /// Set a bool prop.
    pub fn set_bool(&mut self, key: &str, value: bool) {
        self.values.insert(key.to_string(), Value::Bool(value));
    }

    /// Set an integer prop.
    pub fn set_i64(&mut self, key: &str, value: i64) {
        self.values.insert(key.to_string(), Value::Number(value.into()));
    }

    /// Set a numeric prop.
    pub fn set_f64(&mut self, key: &str, value: f64) {
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.values.insert(key.to_string(), Value::Number(number));
        }
    }

    /// Remove a prop, returning its value when present.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.values.remove(key)
    }

    /// Whether a prop is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// Whether there are no props.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Number of props.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Iterate props in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.values.iter()
    }

    /// The accessible role.
    pub fn role(&self) -> Option<&str> {
        self.get_str("role")
    }

    /// Set the accessible role.
    pub fn set_role(&mut self, role: impl Into<String>) {
        self.set_str("role", role);
    }

    /// The semantic tone.
    pub fn tone(&self) -> Option<Tone> {
        self.get_str("tone").and_then(Tone::from_str)
    }

    /// Set the semantic tone.
    pub fn set_tone(&mut self, tone: Tone) {
        self.set_str("tone", tone.as_str());
    }

    /// Whether the node is mounted but not laid out.
    pub fn hidden(&self) -> Option<bool> {
        self.get_bool("hidden")
    }

    /// Set the hidden flag.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.set_bool("hidden", hidden);
    }

    /// Whether the node is selected.
    pub fn selected(&self) -> Option<bool> {
        self.get_bool("selected")
    }

    /// Set the selected flag.
    pub fn set_selected(&mut self, selected: bool) {
        self.set_bool("selected", selected);
    }

    /// Whether the node is inset.
    pub fn inset(&self) -> Option<bool> {
        self.get_bool("inset")
    }

    /// Set the inset flag.
    pub fn set_inset(&mut self, inset: bool) {
        self.set_bool("inset", inset);
    }

    /// The primary text prop.
    pub fn text(&self) -> Option<&str> {
        self.get_str("text")
    }

    /// Set the primary text prop.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.set_str("text", text);
    }

    /// The styled spans prop.
    pub fn spans(&self) -> Option<Vec<Span>> {
        let array = self.values.get("spans")?.as_array()?;
        Some(array.iter().filter_map(Span::from_value).collect())
    }

    /// Set the styled spans prop.
    pub fn set_spans(&mut self, spans: Vec<Span>) {
        let array: Vec<Value> = spans.iter().map(Span::to_value).collect();
        self.values.insert("spans".to_string(), Value::Array(array));
    }

    /// The header text prop.
    pub fn head(&self) -> Option<Text> {
        match self.values.get("head")? {
            Value::String(text) => Some(Text::Plain(text.clone())),
            Value::Array(spans) => {
                Some(Text::Spans(spans.iter().filter_map(Span::from_value).collect()))
            }
            _ => None,
        }
    }

    /// Set the header text prop.
    pub fn set_head(&mut self, head: Text) {
        self.values.insert("head".to_string(), head.to_value());
    }

    /// Serialize to a JSON object.
    pub fn to_value(&self) -> Value {
        Value::Object(self.values.clone())
    }
}

/// Where a revealed node lands in its scroller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealAt {
    /// Top of the scroller.
    Start,
    /// Bottom of the scroller.
    End,
    /// Nearest edge.
    Nearest,
}

impl RevealAt {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            RevealAt::Start => "start",
            RevealAt::End => "end",
            RevealAt::Nearest => "nearest",
        }
    }

    /// Parse a wire placement.
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "start" => Some(RevealAt::Start),
            "end" => Some(RevealAt::End),
            "nearest" => Some(RevealAt::Nearest),
            _ => None,
        }
    }
}

/// A reveal request: a one-shot placement, or a repeatable one keyed by `n`.
#[derive(Clone, Debug, PartialEq)]
pub enum Reveal {
    /// Reveal once, placed at `at`.
    At(RevealAt),
    /// Reveal whenever `n` differs from the previous description.
    Repeat {
        /// Placement.
        at: RevealAt,
        /// Monotonic counter bumped per request.
        n: u64,
    },
}

/// How far a `scroll` op moves the scroller holding a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBy {
    /// One line up.
    LineUp,
    /// One line down.
    LineDown,
    /// One page up.
    PageUp,
    /// One page down.
    PageDown,
    /// To the start.
    Start,
    /// To the end.
    End,
}

impl ScrollBy {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ScrollBy::LineUp => "line-up",
            ScrollBy::LineDown => "line-down",
            ScrollBy::PageUp => "page-up",
            ScrollBy::PageDown => "page-down",
            ScrollBy::Start => "start",
            ScrollBy::End => "end",
        }
    }

    /// Parse a wire direction.
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "line-up" => Some(ScrollBy::LineUp),
            "line-down" => Some(ScrollBy::LineDown),
            "page-up" => Some(ScrollBy::PageUp),
            "page-down" => Some(ScrollBy::PageDown),
            "start" => Some(ScrollBy::Start),
            "end" => Some(ScrollBy::End),
            _ => None,
        }
    }
}

/// A keyboard scroll request, repeated while `n` moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Scroll {
    /// Direction.
    pub by: ScrollBy,
    /// Monotonic counter bumped per key press.
    pub n: u64,
}

/// A described node: kind, props, children, sibling identity and view requests.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeNode {
    /// The node's kind.
    pub kind: NodeKind,
    /// The node's props.
    pub props: Props,
    /// Child slots, in order.
    pub children: Vec<NativeNode>,
    /// Identity among siblings (defaults to the child index).
    pub key: Option<String>,
    /// Scroll-into-view request.
    pub reveal: Option<Reveal>,
    /// Keyboard scroll request.
    pub scroll: Option<Scroll>,
}

impl Default for NativeNode {
    fn default() -> Self {
        NativeNode::new(NodeKind::Col)
    }
}

impl NativeNode {
    /// An empty node of `kind`.
    pub fn new(kind: NodeKind) -> Self {
        NativeNode {
            kind,
            props: Props::new(),
            children: Vec::new(),
            key: None,
            reveal: None,
            scroll: None,
        }
    }

    /// A node of `kind` carrying `props`.
    pub fn with_props(kind: NodeKind, props: Props) -> Self {
        NativeNode {
            kind,
            props,
            children: Vec::new(),
            key: None,
            reveal: None,
            scroll: None,
        }
    }

    /// A node of `kind` carrying `props` and `children`.
    pub fn with_children(kind: NodeKind, props: Props, children: Vec<NativeNode>) -> Self {
        NativeNode {
            kind,
            props,
            children,
            key: None,
            reveal: None,
            scroll: None,
        }
    }

    /// Append a child, consuming the node.
    pub fn child(mut self, child: NativeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Set the sibling identity, consuming the node.
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Set the reveal request, consuming the node.
    pub fn with_reveal(mut self, reveal: Reveal) -> Self {
        self.reveal = Some(reveal);
        self
    }

    /// Set the scroll request, consuming the node.
    pub fn with_scroll(mut self, scroll: Scroll) -> Self {
        self.scroll = Some(scroll);
        self
    }

    /// The primary text prop, when this kind has one.
    pub fn text(&self) -> Option<&str> {
        if self.kind.is_text_kind() {
            self.props.text()
        } else {
            None
        }
    }

    /// The node's plain text: the `text` prop, else concatenated spans.
    pub fn plain_text(&self) -> String {
        if let Some(text) = self.text() {
            return text.to_string();
        }
        self.props.spans().map(|spans| {
            spans
                .iter()
                .map(|span| span.t.as_str())
                .collect::<String>()
        }).unwrap_or_default()
    }

    /// Borrow a prop.
    pub fn prop(&self, key: &str) -> Option<&Value> {
        self.props.get(key)
    }

    /// Set a prop.
    pub fn set_prop(&mut self, key: &str, value: Value) {
        self.props.insert(key, value);
    }

    /// The wire node (`{id, k, p?, c?}`) for `id`.
    pub fn to_wire(&self, id: &str) -> Value {
        let mut out = Map::new();
        out.insert("id".to_string(), Value::String(id.to_string()));
        out.insert("k".to_string(), Value::String(self.kind.as_str().to_string()));
        self.write_body(&mut out);
        Value::Object(out)
    }

    /// The prop-node form (`{k, p?, c?}`) carried inside a prop.
    pub fn to_prop_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("k".to_string(), Value::String(self.kind.as_str().to_string()));
        self.write_body(&mut out);
        Value::Object(out)
    }

    fn write_body(&self, out: &mut Map<String, Value>) {
        if !self.props.is_empty() {
            out.insert("p".to_string(), self.props.to_value());
        }
        if !self.children.is_empty() {
            let children: Vec<Value> = self
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let id = child
                        .key
                        .clone()
                        .unwrap_or_else(|| index.to_string());
                    child.to_wire(&id)
                })
                .collect();
            out.insert("c".to_string(), Value::Array(children));
        }
    }
}

/// What a component may consult while describing itself. Never a clock:
/// motion is terminal-clocked.
#[derive(Clone, Debug)]
pub struct DescribeContext {
    /// Surface width in cells; only for `rows` fallback and wrap hints.
    pub cols: u16,
    /// The terminal asked for reduced motion.
    pub reduce_motion: bool,
    /// The terminal's appearance is dark.
    pub dark: bool,
    /// The user's system reads a 12-hour clock; `None` when unknown.
    pub hour12: Option<bool>,
    supported: Vec<NodeKind>,
    features: Vec<String>,
}

impl Default for DescribeContext {
    fn default() -> Self {
        DescribeContext::new(80)
    }
}

impl DescribeContext {
    /// A context for a `cols`-wide dark surface that supports every kind.
    pub fn new(cols: u16) -> Self {
        DescribeContext {
            cols,
            reduce_motion: false,
            dark: true,
            hour12: None,
            supported: NodeKind::ALL.to_vec(),
            features: Vec::new(),
        }
    }

    /// A context with an explicit kind vocabulary and feature list.
    pub fn with_capabilities(
        cols: u16,
        supported: &[NodeKind],
        features: &[&str],
    ) -> Self {
        DescribeContext {
            cols,
            reduce_motion: false,
            dark: true,
            hour12: None,
            supported: supported.to_vec(),
            features: features.iter().map(|feature| feature.to_string()).collect(),
        }
    }

    /// Whether the terminal renders `kind` natively.
    pub fn supports(&self, kind: NodeKind) -> bool {
        self.supported.contains(&kind)
    }

    /// Whether the terminal advertised protocol feature `name`.
    pub fn feature(&self, name: &str) -> bool {
        self.features.iter().any(|feature| feature == name)
    }

    /// Set the reduced-motion flag.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        self.reduce_motion = reduce;
    }

    /// Set the appearance.
    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
    }

    /// Set the 12-hour clock preference.
    pub fn set_hour12(&mut self, hour12: Option<bool>) {
        self.hour12 = hour12;
    }

    /// Advertise a protocol feature for this context.
    pub fn enable_feature(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.features.iter().any(|feature| feature == &name) {
            self.features.push(name);
        }
    }
}

/// The regions a frame provider fills when it describes a whole surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeSurface {
    /// Primary content.
    pub main: Vec<NativeNode>,
    /// Docked bands.
    pub dock: Vec<NativeNode>,
    /// Floating layer.
    pub layer: Vec<NativeNode>,
}

impl NativeSurface {
    /// An empty surface.
    pub fn new() -> Self {
        NativeSurface {
            main: Vec::new(),
            dock: Vec::new(),
            layer: Vec::new(),
        }
    }

    /// Whether every region is empty.
    pub fn is_empty(&self) -> bool {
        self.main.is_empty() && self.dock.is_empty() && self.layer.is_empty()
    }

    /// Iterate `(region, node)` pairs in region order.
    pub fn regions(&self) -> [(&str, &Vec<NativeNode>); 3] {
        [("main", &self.main), ("dock", &self.dock), ("layer", &self.layer)]
    }
}

/// The screen surface a fullscreen overlay fills as a page of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeScreen {
    /// Primary content.
    pub main: Vec<NativeNode>,
    /// Docked bands.
    pub dock: Vec<NativeNode>,
    /// Floating layer.
    pub layer: Vec<NativeNode>,
    /// Surface role exposed to styling.
    pub role: String,
}

impl NativeScreen {
    /// A screen with `role` and no content.
    pub fn new(role: impl Into<String>) -> Self {
        NativeScreen {
            main: Vec::new(),
            dock: Vec::new(),
            layer: Vec::new(),
            role: role.into(),
        }
    }

    /// The regions as a surface.
    pub fn surface(&self) -> NativeSurface {
        NativeSurface {
            main: self.main.clone(),
            dock: self.dock.clone(),
            layer: self.layer.clone(),
        }
    }
}

/// One tree row: id, label and optional nested children.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    /// Stable row id.
    pub id: String,
    /// Row label.
    pub label: Text,
    /// Leading icon name.
    pub icon: Option<String>,
    /// Whether the row is expanded.
    pub open: Option<bool>,
    /// Nested rows.
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// A row with a plain label.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        TreeNode {
            id: id.into(),
            label: Text::Plain(label.into()),
            icon: None,
            open: None,
            children: Vec::new(),
        }
    }

    /// Set the leading icon.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set the expanded flag.
    pub fn with_open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Append a child row.
    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Serialize to the wire tree node.
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".to_string(), Value::String(self.id.clone()));
        out.insert("label".to_string(), self.label.to_value());
        if let Some(icon) = &self.icon {
            out.insert("icon".to_string(), Value::String(icon.clone()));
        }
        if let Some(open) = self.open {
            out.insert("open".to_string(), Value::Bool(open));
        }
        if !self.children.is_empty() {
            out.insert(
                "children".to_string(),
                Value::Array(self.children.iter().map(TreeNode::to_value).collect()),
            );
        }
        Value::Object(out)
    }
}

/// One table column.
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    /// Column id used as the cell key.
    pub id: String,
    /// Header text.
    pub head: Option<Text>,
    /// Cell alignment.
    pub align: Option<String>,
    /// Overflow truncation.
    pub truncate: Option<String>,
    /// Lower priorities hide first when narrow.
    pub priority: Option<i64>,
    /// Grow weight.
    pub grow: Option<f64>,
}

impl TableColumn {
    /// A column with `id` and no header.
    pub fn new(id: impl Into<String>) -> Self {
        TableColumn {
            id: id.into(),
            head: None,
            align: None,
            truncate: None,
            priority: None,
            grow: None,
        }
    }

    /// Set the header.
    pub fn with_head(mut self, head: impl Into<String>) -> Self {
        self.head = Some(Text::Plain(head.into()));
        self
    }

    /// Set alignment (`start`, `center`, `end`).
    pub fn with_align(mut self, align: impl Into<String>) -> Self {
        self.align = Some(align.into());
        self
    }

    /// Set overflow truncation.
    pub fn with_truncate(mut self, truncate: impl Into<String>) -> Self {
        self.truncate = Some(truncate.into());
        self
    }

    /// Set the hiding priority.
    pub fn with_priority(mut self, priority: i64) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Serialize to the wire column.
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".to_string(), Value::String(self.id.clone()));
        if let Some(head) = &self.head {
            out.insert("head".to_string(), head.to_value());
        }
        if let Some(align) = &self.align {
            out.insert("align".to_string(), Value::String(align.clone()));
        }
        if let Some(truncate) = &self.truncate {
            out.insert("truncate".to_string(), Value::String(truncate.clone()));
        }
        if let Some(priority) = self.priority {
            out.insert("priority".to_string(), Value::Number(priority.into()));
        }
        if let Some(grow) = self.grow {
            if let Some(number) = serde_json::Number::from_f64(grow) {
                out.insert("grow".to_string(), Value::Number(number));
            }
        }
        Value::Object(out)
    }
}

/// One table row: id plus cells keyed by column id.
#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    /// Row id.
    pub id: String,
    /// Cells in column order.
    pub cells: Vec<(String, Text)>,
}

impl TableRow {
    /// A row with `id`.
    pub fn new(id: impl Into<String>) -> Self {
        TableRow {
            id: id.into(),
            cells: Vec::new(),
        }
    }

    /// Append a cell.
    pub fn cell(mut self, column: impl Into<String>, value: impl Into<String>) -> Self {
        self.cells.push((column.into(), Text::Plain(value.into())));
        self
    }

    /// Serialize to the wire row.
    pub fn to_value(&self) -> Value {
        let mut cells = Map::new();
        for (column, value) in &self.cells {
            cells.insert(column.clone(), value.to_value());
        }
        let mut out = Map::new();
        out.insert("id".to_string(), Value::String(self.id.clone()));
        out.insert("cells".to_string(), Value::Object(cells));
        Value::Object(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trips_through_wire_names() {
        for kind in NodeKind::ALL {
            assert_eq!(NodeKind::from_str(kind.as_str()), Some(*kind));
        }
        assert_eq!(NodeKind::from_str("nope"), None);
        assert!(NodeKind::Text.is_text_kind());
        assert!(!NodeKind::Card.is_text_kind());
    }

    #[test]
    fn tone_round_trips() {
        for tone in [
            Tone::Neutral,
            Tone::Accent,
            Tone::Info,
            Tone::Success,
            Tone::Warning,
            Tone::Error,
            Tone::Pending,
            Tone::Muted,
            Tone::User,
        ] {
            assert_eq!(Tone::from_str(tone.as_str()), Some(tone));
        }
        assert_eq!(Tone::from_str("loud"), None);
    }

    #[test]
    fn props_typed_accessors_round_trip() {
        let mut props = Props::new();
        props.set_role("omp.tool");
        props.set_tone(Tone::Accent);
        props.set_selected(true);
        props.set_inset(true);
        props.set_hidden(true);
        props.set_text("hello");
        assert_eq!(props.role(), Some("omp.tool"));
        assert_eq!(props.tone(), Some(Tone::Accent));
        assert_eq!(props.selected(), Some(true));
        assert_eq!(props.inset(), Some(true));
        assert_eq!(props.hidden(), Some(true));
        assert_eq!(props.text(), Some("hello"));
        assert_eq!(props.len(), 6);
        props.remove("tone");
        assert_eq!(props.tone(), None);
    }

    #[test]
    fn node_builder_sets_fields() {
        let node = NativeNode::new(NodeKind::Card)
            .with_key("body")
            .child(NativeNode::new(NodeKind::Text));
        assert_eq!(node.kind, NodeKind::Card);
        assert_eq!(node.key.as_deref(), Some("body"));
        assert_eq!(node.children.len(), 1);
        let wire = node.to_wire("base.body");
        assert_eq!(wire["id"], "base.body");
        assert_eq!(wire["k"], "card");
        assert_eq!(wire["c"][0]["id"], "0");
    }

    #[test]
    fn text_kinds_expose_plain_text() {
        let mut node = NativeNode::new(NodeKind::Text);
        node.props.set_text("hi");
        assert_eq!(node.text(), Some("hi"));
        assert_eq!(node.plain_text(), "hi");
        let mut spans = NativeNode::new(NodeKind::Text);
        spans.props.set_spans(vec![Span::styled("a", "muted"), Span::new("b")]);
        assert_eq!(spans.plain_text(), "ab");
        assert!(spans.text().is_none());
    }

    #[test]
    fn describe_context_reports_capabilities() {
        let cx = DescribeContext::with_capabilities(40, &[NodeKind::Col], &["scroll"]);
        assert!(cx.supports(NodeKind::Col));
        assert!(!cx.supports(NodeKind::Card));
        assert!(cx.feature("scroll"));
        assert!(!cx.feature("blobs"));
        assert_eq!(cx.cols, 40);
    }

    #[test]
    fn tree_and_table_serialize() {
        let tree = TreeNode::new("a", "Root")
            .with_open(true)
            .child(TreeNode::new("b", "Leaf"));
        let value = tree.to_value();
        assert_eq!(value["children"][0]["id"], "b");
        let row = TableRow::new("r0").cell("c0", "x");
        let value = row.to_value();
        assert_eq!(value["cells"]["c0"], "x");
    }
}
