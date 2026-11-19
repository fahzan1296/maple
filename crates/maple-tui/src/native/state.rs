//! Native rendering state, settling, memoization and the bounded reconciler.
//! Reference: `l/src/native/state.ts`, `l/src/native/reconcile.ts`, `l/src/native/settle.ts`, `l/src/native/memo.ts`, `l/src/native/apply.ts`.
//!
//! The TypeScript original threads a process-global state object, a settle
//! flag per component, a per-describe memo, and a reconciler that diffs
//! described component trees into frame ops. Rust has no trait upcasting from
//! `dyn Component`, so this port works on resolved [`NativeNode`] trees: a
//! surface is `main`/`dock`/`layer` node lists, and the reconciler assigns
//! stable ids from the node key (or child index) and diffs by id.
//!
//! Simplifications, all bounded:
//!
//! - Children are matched by wire id with a longest-increasing-subsequence
//!   stable mask; a child outside the mask is deleted and re-added in place.
//!   There is no component parking, portal hoisting or overlay stack.
//! - A text change is sent as `text replace`; `append` and `splice` are not
//!   produced (the document applier still accepts them).
//! - Each frame is capped at `max_nodes` added nodes and `max_depth` levels.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use serde_json::{Map, Value};

use crate::native::node::{
    DescribeContext, NativeNode, NativeSurface, NodeKind, Props, Reveal, RevealAt, ScrollBy,
};

/// Element-wise equality of two dependency arrays.
pub fn same_items<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    a == b
}

/// Shallow equality of two flat prop bags.
pub fn same_props(a: &Props, b: &Props) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|(key, value)| b.get(key) == Some(value))
}

/// Keeps the last built value while every dependency is unchanged.
#[derive(Debug, Default)]
pub struct Memo<T> {
    deps: Option<Vec<Value>>,
    value: Option<T>,
}

impl<T> Memo<T> {
    /// An empty memo.
    pub fn new() -> Self {
        Memo {
            deps: None,
            value: None,
        }
    }

    /// The cached value, rebuilt when `deps` changed.
    pub fn get<F: FnOnce() -> T>(&mut self, deps: &[Value], build: F) -> &T {
        let fresh = match &self.deps {
            Some(cached) => !same_items(cached, deps),
            None => true,
        };
        if fresh {
            self.value = Some(build());
            self.deps = Some(deps.to_vec());
        }
        self.value.as_ref().unwrap_or_else(|| unreachable!())
    }

    /// Forget the cached value (theme, glyph preset or keybindings changed).
    pub fn clear(&mut self) {
        self.deps = None;
        self.value = None;
    }

    /// Whether a value is cached.
    pub fn is_empty(&self) -> bool {
        self.value.is_none()
    }
}

/// Process-wide native rendering state, set only by the native backend.
pub struct NativeState {
    rendering: AtomicBool,
    buttons: AtomicBool,
    pacing: AtomicBool,
    listeners: Mutex<Vec<(u64, Box<dyn Fn(bool) + Send>)>>,
    next_listener: AtomicU64,
}

impl Default for NativeState {
    fn default() -> Self {
        NativeState {
            rendering: AtomicBool::new(false),
            buttons: AtomicBool::new(false),
            pacing: AtomicBool::new(false),
            listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU64::new(1),
        }
    }
}

impl NativeState {
    /// Whether a TSP surface is live in this process.
    pub fn is_native_rendering(&self) -> bool {
        self.rendering.load(Ordering::Relaxed)
    }

    /// Called by the native backend on surface open and close.
    pub fn set_native_rendering(&self, on: bool) {
        if self.rendering.swap(on, Ordering::Relaxed) == on {
            return;
        }
        let listeners = std::mem::take(
            &mut *self.listeners.lock().unwrap_or_else(|e| e.into_inner()),
        );
        for (_, listener) in listeners.iter() {
            listener(on);
        }
        *self.listeners.lock().unwrap_or_else(|e| e.into_inner()) = listeners;
    }

    /// Whether the live terminal draws the `button` kind.
    pub fn native_buttons(&self) -> bool {
        self.buttons.load(Ordering::Relaxed)
    }

    /// Called by the native backend when it adopts a `hello` reply and stops.
    pub fn set_native_buttons(&self, on: bool) {
        self.buttons.store(on, Ordering::Relaxed);
    }

    /// Whether a live surface's terminal paces streamed `md`.
    pub fn native_pacing(&self) -> bool {
        self.rendering.load(Ordering::Relaxed) && self.pacing.load(Ordering::Relaxed)
    }

    /// Called by the native backend when it adopts a `hello` reply and stops.
    pub fn set_native_pacing(&self, on: bool) {
        self.pacing.store(on, Ordering::Relaxed);
    }

    /// Call `listener` whenever native rendering starts or stops; the returned
    /// id unregisters it.
    pub fn on_native_rendering_change(
        &self,
        listener: impl Fn(bool) + Send + 'static,
    ) -> u64 {
        let id = self.next_listener.fetch_add(1, Ordering::Relaxed);
        self.listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, Box::new(listener)));
        id
    }

    /// Remove a listener returned by [`NativeState::on_native_rendering_change`].
    pub fn remove_listener(&self, id: u64) {
        self.listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(listener, _)| *listener != id);
    }
}
static NATIVE_STATE: LazyLock<NativeState> = LazyLock::new(NativeState::default);

/// The process-wide native state.
pub fn native_state() -> &'static NativeState {
	&NATIVE_STATE
}

/// Whether a TSP surface is live in this process.
pub fn is_native_rendering() -> bool {
	native_state().is_native_rendering()
}

/// Set process-wide native rendering; the backend calls this on open and close.
pub fn set_native_rendering(on: bool) {
	native_state().set_native_rendering(on);
}

/// Whether the live terminal draws the `button` kind.
pub fn native_buttons() -> bool {
	native_state().native_buttons()
}

/// Set whether the live terminal draws the `button` kind.
pub fn set_native_buttons(on: bool) {
	native_state().set_native_buttons(on);
}

/// Whether a live surface's terminal paces streamed `md`.
pub fn native_pacing() -> bool {
	native_state().native_pacing()
}

/// Set whether a live surface's terminal paces streamed `md`.
pub fn set_native_pacing(on: bool) {
	native_state().set_native_pacing(on);
}

/// Call `listener` whenever native rendering starts or stops; returns an
/// unsubscribe closure.
pub fn on_native_rendering_change(
	listener: impl Fn(bool) + Send + 'static,
) -> impl FnOnce() + Send {
	let id = native_state().on_native_rendering_change(listener);
	move || native_state().remove_listener(id)
}
/// The memo key: theme epoch plus surface geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleKey {
    /// Theme epoch, bumped whenever the active theme changes.
    pub theme_epoch: u64,
    /// Surface width in cells.
    pub cols: u16,
    /// Surface height in cells.
    pub rows: u16,
}

/// Settled ids, memoized on theme epoch plus geometry.
///
/// A settled block stays editable: a later change is sent as targeted ops by
/// id, followed by a fresh `settle` hint.
#[derive(Debug, Default)]
pub struct NativeSettle {
    key: Option<SettleKey>,
    settled: BTreeSet<String>,
}

impl NativeSettle {
    /// An empty settle set.
    pub fn new() -> Self {
        NativeSettle {
            key: None,
            settled: BTreeSet::new(),
        }
    }

    /// The key the memo is currently bound to.
    pub fn key(&self) -> Option<&SettleKey> {
        self.key.as_ref()
    }

    /// Rebind the memo; a changed theme epoch or geometry clears it.
    pub fn rebind(&mut self, key: SettleKey) -> bool {
        if self.key.as_ref() == Some(&key) {
            return false;
        }
        self.key = Some(key);
        self.settled.clear();
        true
    }

    /// Mark `id` finalized. Idempotent; returns whether it was newly settled.
    pub fn settle(&mut self, id: &str) -> bool {
        self.settled.insert(id.to_string())
    }

    /// Whether `id` is settled.
    pub fn is_settled(&self, id: &str) -> bool {
        self.settled.contains(id)
    }

    /// Forget one id.
    pub fn forget(&mut self, id: &str) {
        self.settled.remove(id);
    }

    /// Forget every id.
    pub fn clear(&mut self) {
        self.settled.clear();
    }

    /// Number of settled ids.
    pub fn len(&self) -> usize {
        self.settled.len()
    }

    /// Whether nothing is settled.
    pub fn is_empty(&self) -> bool {
        self.settled.is_empty()
    }
}

/// Frame counters: pending and fallback rows are what the backend reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeCounters {
    /// Frames reconciled.
    pub frames: u64,
    /// Ops emitted.
    pub ops: u64,
    /// Nodes in the document.
    pub nodes: u64,
    /// Described nodes not yet settled.
    pub pending: u64,
    /// `rows` fallback nodes in the last frame.
    pub fallback_rows: u64,
}

impl NativeCounters {
    /// Record one reconciled frame.
    pub fn record_frame(
        &mut self,
        ops: usize,
        nodes: usize,
        pending: usize,
        fallback_rows: usize,
    ) {
        self.frames += 1;
        self.ops += ops as u64;
        self.nodes = nodes as u64;
        self.pending = pending as u64;
        self.fallback_rows = fallback_rows as u64;
    }
}

/// The text mode of a `text` op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMode {
    /// Append to the existing text.
    Append,
    /// Replace the existing text.
    Replace,
}

impl TextMode {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            TextMode::Append => "append",
            TextMode::Replace => "replace",
        }
    }
}

/// One frame op, in the wire tuple shape `apply.ts` validates.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameOp {
    /// Add a node under `parent`, before `before`.
    Add {
        /// Node id.
        id: String,
        /// Parent id.
        parent: String,
        /// Insert-before sibling id; `None` appends.
        before: Option<String>,
        /// The node.
        node: WireNode,
    },
    /// Set props; a `Null` value removes the prop.
    Set {
        /// Node id.
        id: String,
        /// Changed props in key order.
        props: Vec<(String, Value)>,
    },
    /// Append or replace a text kind's `text` prop.
    Text {
        /// Node id.
        id: String,
        /// Append or replace.
        mode: TextMode,
        /// The text.
        text: String,
    },
    /// Splice into a text kind's `text` prop.
    Splice {
        /// Node id.
        id: String,
        /// Start offset.
        at: usize,
        /// Number of characters to delete.
        del: usize,
        /// Inserted text.
        text: String,
    },
    /// Move a node under `parent`, before `before`.
    Move {
        /// Node id.
        id: String,
        /// Parent id.
        parent: String,
        /// Insert-before sibling id; `None` appends.
        before: Option<String>,
    },
    /// Delete a node and its subtree.
    Del {
        /// Node id.
        id: String,
    },
    /// Mark a node finalized.
    Settle {
        /// Node id.
        id: String,
    },
    /// Focus a node, or clear focus with `None`.
    Focus {
        /// Node id.
        id: Option<String>,
    },
    /// Reveal a node in its scroller.
    Reveal {
        /// Node id.
        id: String,
        /// Placement.
        at: RevealAt,
    },
    /// Scroll the scroller holding a node.
    Scroll {
        /// Node id.
        id: String,
        /// Direction.
        by: ScrollBy,
    },
    /// Suspend the document.
    Suspend,
    /// Resume the document.
    Resume,
}

impl FrameOp {
    /// Wire name.
    pub fn op_name(&self) -> &'static str {
        match self {
            FrameOp::Add { .. } => "add",
            FrameOp::Set { .. } => "set",
            FrameOp::Text { .. } => "text",
            FrameOp::Splice { .. } => "splice",
            FrameOp::Move { .. } => "move",
            FrameOp::Del { .. } => "del",
            FrameOp::Settle { .. } => "settle",
            FrameOp::Focus { .. } => "focus",
            FrameOp::Reveal { .. } => "reveal",
            FrameOp::Scroll { .. } => "scroll",
            FrameOp::Suspend => "suspend",
            FrameOp::Resume => "resume",
        }
    }

    /// Serialize to the wire op array.
    pub fn to_value(&self) -> Value {
        match self {
            FrameOp::Add {
                id,
                parent,
                before,
                node,
            } => Value::Array(vec![
                Value::String("add".to_string()),
                Value::String(id.clone()),
                Value::String(parent.clone()),
                before.clone().map(Value::String).unwrap_or(Value::Null),
                node.to_value(),
            ]),
            FrameOp::Set { id, props } => {
                let mut out = Map::new();
                for (key, value) in props {
                    out.insert(key.clone(), value.clone());
                }
                Value::Array(vec![
                    Value::String("set".to_string()),
                    Value::String(id.clone()),
                    Value::Object(out),
                ])
            }
            FrameOp::Text { id, mode, text } => Value::Array(vec![
                Value::String("text".to_string()),
                Value::String(id.clone()),
                Value::String(mode.as_str().to_string()),
                Value::String(text.clone()),
            ]),
            FrameOp::Splice { id, at, del, text } => Value::Array(vec![
                Value::String("splice".to_string()),
                Value::String(id.clone()),
                Value::Number((*at as u64).into()),
                Value::Number((*del as u64).into()),
                Value::String(text.clone()),
            ]),
            FrameOp::Move {
                id,
                parent,
                before,
            } => Value::Array(vec![
                Value::String("move".to_string()),
                Value::String(id.clone()),
                Value::String(parent.clone()),
                before.clone().map(Value::String).unwrap_or(Value::Null),
            ]),
            FrameOp::Del { id } => Value::Array(vec![
                Value::String("del".to_string()),
                Value::String(id.clone()),
            ]),
            FrameOp::Settle { id } => Value::Array(vec![
                Value::String("settle".to_string()),
                Value::String(id.clone()),
            ]),
            FrameOp::Focus { id } => Value::Array(vec![
                Value::String("focus".to_string()),
                id.clone().map(Value::String).unwrap_or(Value::Null),
            ]),
            FrameOp::Reveal { id, at } => Value::Array(vec![
                Value::String("reveal".to_string()),
                Value::String(id.clone()),
                Value::String(at.as_str().to_string()),
            ]),
            FrameOp::Scroll { id, by } => Value::Array(vec![
                Value::String("scroll".to_string()),
                Value::String(id.clone()),
                Value::String(by.as_str().to_string()),
            ]),
            FrameOp::Suspend => Value::Array(vec![Value::String("suspend".to_string())]),
            FrameOp::Resume => Value::Array(vec![Value::String("resume".to_string())]),
        }
    }
}

/// A node in the wire document: id plus kind, props and children.
#[derive(Clone, Debug, PartialEq)]
pub struct WireNode {
    /// Node id.
    pub id: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Node props.
    pub props: Props,
    /// Children in order.
    pub children: Vec<WireNode>,
}

impl WireNode {
    /// A node with `id`, `kind` and no children.
    pub fn new(id: impl Into<String>, kind: NodeKind) -> Self {
        WireNode {
            id: id.into(),
            kind,
            props: Props::new(),
            children: Vec::new(),
        }
    }

    /// Serialize to `{id, k, p?, c?}`.
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".to_string(), Value::String(self.id.clone()));
        out.insert(
            "k".to_string(),
            Value::String(self.kind.as_str().to_string()),
        );
        if !self.props.is_empty() {
            out.insert("p".to_string(), self.props.to_value());
        }
        if !self.children.is_empty() {
            out.insert(
                "c".to_string(),
                Value::Array(self.children.iter().map(WireNode::to_value).collect()),
            );
        }
        Value::Object(out)
    }
}

/// Materialize a described node subtree under `id`.
fn wire_node(node: &NativeNode, id: &str) -> WireNode {
    let children: Vec<WireNode> = node
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let key = child.key.clone().unwrap_or_else(|| index.to_string());
            wire_node(child, &child_id(id, &key))
        })
        .collect();
    WireNode {
        id: id.to_string(),
        kind: node.kind,
        props: node.props.clone(),
        children,
    }
}

/// Escape a keypath segment the way `reconcile.ts` does.
fn escape_key(key: &str) -> String {
    key.replace('%', "%25").replace('/', "%2F").replace('^', "%5E")
}

/// Reverse [`escape_key`] in one pass.
fn unescape_key(segment: &str) -> String {
    let segment = segment.strip_prefix('^').unwrap_or(segment);
    let mut out = String::new();
    let mut index = 0;
    while index < segment.len() {
        let rest = &segment[index..];
        if rest.starts_with('%') && rest.len() >= 3 && rest.is_char_boundary(3) {
            if let Ok(byte) = u8::from_str_radix(&rest[1..3], 16) {
                out.push(byte as char);
                index += 3;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap_or(' ');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}
/// Join a parent id and keypath segment.
fn child_id(parent: &str, key: &str) -> String {
    format!("{parent}.{}", escape_key(key))
}

/// Longest increasing subsequence over non-negative `sources`; marks members.
fn stable_mask(sources: &[i64]) -> Vec<bool> {
    let n = sources.len();
    let mut mask = vec![false; n];
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![-1i64; n];
    for (index, value) in sources.iter().enumerate() {
        if *value < 0 {
            continue;
        }
        let mut lo = 0usize;
        let mut hi = tails.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if sources[tails[mid]] < *value {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo > 0 {
            prev[index] = tails[lo - 1] as i64;
        }
        if lo == tails.len() {
            tails.push(index);
        } else {
            tails[lo] = index;
        }
    }
    let mut at = tails.last().copied().map(|i| i as i64).unwrap_or(-1);
    while at >= 0 {
        mask[at as usize] = true;
        at = prev[at as usize];
    }
    mask
}

/// One node in the reference document.
#[derive(Clone, Debug, PartialEq)]
pub struct DocNode {
    /// Node id.
    pub id: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Node props.
    pub props: Props,
    /// Child ids in order.
    pub children: Vec<String>,
    /// Parent id; `None` for the root.
    pub parent: Option<String>,
}

/// One rejected op.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyError {
    /// Frame sequence number.
    pub s: u64,
    /// Op index inside the frame.
    pub op: usize,
    /// Rejection message.
    pub msg: String,
}

/// A frame: sequence number plus ops.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Sequence number.
    pub s: u64,
    /// Ops in application order.
    pub ops: Vec<FrameOp>,
}

/// A surface document: the root (id = surface id) plus an id index.
#[derive(Default)]
pub struct TspDocument {
    surface: String,
    nodes: HashMap<String, DocNode>,
    focus: Option<String>,
    suspended: bool,
    settled: BTreeSet<String>,
}

impl TspDocument {
    /// A document whose root is `surface`.
    pub fn new(surface: impl Into<String>) -> Self {
        let surface = surface.into();
        let mut nodes = HashMap::new();
        nodes.insert(
            surface.clone(),
            DocNode {
                id: surface.clone(),
                kind: NodeKind::Col,
                props: Props::new(),
                children: Vec::new(),
                parent: None,
            },
        );
        TspDocument {
            surface,
            nodes,
            focus: None,
            suspended: false,
            settled: BTreeSet::new(),
        }
    }

    /// The surface id.
    pub fn surface(&self) -> &str {
        &self.surface
    }

    /// Currently focused node id.
    pub fn focus(&self) -> Option<&str> {
        self.focus.as_deref()
    }

    /// Whether the document is suspended.
    pub fn suspended(&self) -> bool {
        self.suspended
    }

    /// Ids that received a `settle` hint and no later op.
    pub fn settled(&self) -> &BTreeSet<String> {
        &self.settled
    }

    /// Number of nodes, including the root.
    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// Whether `id` is in the document.
    pub fn has(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// A node by id.
    pub fn get(&self, id: &str) -> Option<DocNode> {
        self.nodes.get(id).cloned()
    }

    /// `x keep:true`: drop the live-only regions (`dock`, `layer`) and focus;
    /// `main` stays.
    pub fn close(&mut self) {
        for region in ["dock", "layer"] {
            let children = match self.nodes.get(region) {
                Some(node) if node.parent.as_deref() == Some(self.surface.as_str()) => {
                    node.children.clone()
                }
                _ => continue,
            };
            for child in children {
                self.unregister(&child);
            }
            if let Some(root) = self.nodes.get_mut(&self.surface) {
                root.children.retain(|id| id != region);
            }
            self.nodes.remove(region);
        }
        self.focus = None;
    }

    /// Apply a frame op by op; returns the rejected ops.
    pub fn apply_frame(&mut self, frame: &Frame) -> Vec<ApplyError> {
        let mut errors = Vec::new();
        for (index, op) in frame.ops.iter().enumerate() {
            if let Err(msg) = self.apply_op(op) {
                errors.push(ApplyError {
                    s: frame.s,
                    op: index,
                    msg,
                });
            }
        }
        errors
    }

    fn node(&self, id: &str) -> Result<DocNode, String> {
        self.nodes
            .get(id)
            .cloned()
            .ok_or_else(|| format!("unknown id {id}"))
    }

    fn unsettle(&mut self, id: &str) {
        let mut at = id.to_string();
        loop {
            self.settled.remove(&at);
            match self.nodes.get(&at).and_then(|node| node.parent.clone()) {
                Some(parent) => at = parent,
                None => return,
            }
        }
    }

    fn insert_index(&self, parent: &DocNode, before: Option<&str>) -> Result<usize, String> {
        match before {
            None => Ok(parent.children.len()),
            Some(before) => match parent.children.iter().position(|id| id == before) {
                Some(index) => Ok(index),
                None => Err(format!("{before} is not a child of {}", parent.id)),
            },
        }
    }

    /// Build a flat node list (children first) from a wire subtree.
    fn build(&self, wire: &WireNode, parent: Option<&str>) -> Result<Vec<DocNode>, String> {
        let mut out = Vec::new();
        let mut children = Vec::new();
        for child in &wire.children {
            let mut built = self.build(child, Some(wire.id.as_str()))?;
            if let Some(root) = built.last() {
                children.push(root.id.clone());
            }
            out.extend(built);
        }
        out.push(DocNode {
            id: wire.id.clone(),
            kind: wire.kind,
            props: wire.props.clone(),
            children,
            parent: parent.map(str::to_string),
        });
        Ok(out)
    }

    fn apply_op(&mut self, op: &FrameOp) -> Result<(), String> {
        match op {
            FrameOp::Add {
                id,
                parent,
                before,
                node,
            } => {
                if node.id != *id {
                    return Err(format!("add id {id} does not match node id {}", node.id));
                }
                let parent_node = self.node(parent)?;

                let index = self.insert_index(&parent_node, before.as_deref())?;
                let built = self.build(node, Some(parent.as_str()))?;
                if built.iter().any(|entry| self.nodes.contains_key(&entry.id)) {
                    return Err(format!("add id {id} already exists"));
                }
                if built.last().map(|entry| entry.id.as_str()) != Some(id.as_str()) {
                    return Err(format!("add id {id} does not match node id {}", node.id));
                }
                if let Some(parent_node) = self.nodes.get_mut(parent) {
                    parent_node.children.insert(index, id.clone());
                }
                for entry in built {
                    self.nodes.insert(entry.id.clone(), entry);
                }
                self.unsettle(parent);
                Ok(())
            }
            FrameOp::Set { id, props } => {
                self.node(id)?;
                let node = self
                    .nodes
                    .get_mut(id)
                    .ok_or_else(|| format!("unknown id {id}"))?;
                for (key, value) in props {
                    if *value == Value::Null {
                        node.props.remove(key);
                    } else {
                        node.props.insert(key.clone(), value.clone());
                    }
                }
                self.unsettle(id);
                Ok(())
            }
            FrameOp::Text { id, mode, text } => {
                let node = self.node(id)?;
                if !node.kind.is_text_kind() {
                    return Err(format!("{id} ({}) has no primary text", node.kind));
                }
                let mut current = node.props.text().unwrap_or_default().to_string();
                match mode {
                    TextMode::Append => current.push_str(text),
                    TextMode::Replace => current = text.clone(),
                }
                let node = self
                    .nodes
                    .get_mut(id)
                    .ok_or_else(|| format!("unknown id {id}"))?;
                node.props.set_text(current);
                self.unsettle(id);
                Ok(())
            }
            FrameOp::Splice { id, at, del, text } => {
                let node = self.node(id)?;
                if !node.kind.is_text_kind() {
                    return Err(format!("{id} ({}) has no primary text", node.kind));
                }
                let current = node.props.text().unwrap_or_default().to_string();
                let chars: Vec<char> = current.chars().collect();
                if at + del > chars.len() {
                    return Err(format!(
                        "splice range {at}+{del} outside text of length {}",
                        chars.len()
                    ));
                }
                let mut next: String = chars[..*at].iter().collect();
                next.push_str(text);
                next.extend(chars[*at + *del..].iter());
                let node = self
                    .nodes
                    .get_mut(id)
                    .ok_or_else(|| format!("unknown id {id}"))?;
                node.props.set_text(next);
                self.unsettle(id);
                Ok(())
            }
            FrameOp::Move {
                id,
                parent,
                before,
            } => {
                let node = self.node(id)?;
                if node.parent.is_none() {
                    return Err("cannot move the root".to_string());
                }

                if contains(&self.nodes, parent, id) {
                    return Err(format!("cannot move {id} into its own subtree"));
                }
                if before.as_deref() == Some(id.as_str()) {
                    return Err("move before itself".to_string());
                }
                let old_parent_id = node.parent.clone().unwrap_or_default();
                if let Some(old_parent) = self.nodes.get_mut(&old_parent_id) {
                    old_parent.children.retain(|child| child != id);
                }
                let index = {
                    let target = self
                        .nodes
                        .get(parent)
                        .ok_or_else(|| format!("unknown id {parent}"))?;
                    self.insert_index(target, before.as_deref())?
                };
                if let Some(target) = self.nodes.get_mut(parent) {
                    target.children.insert(index, id.clone());
                }
                if let Some(node) = self.nodes.get_mut(id) {
                    node.parent = Some(parent.clone());
                }
                self.unsettle(&old_parent_id);
                self.unsettle(id);
                Ok(())
            }
            FrameOp::Del { id } => {
                let node = self.node(id)?;
                if node.parent.is_none() {
                    return Err("cannot delete the root".to_string());
                }
                let parent_id = node.parent.clone().unwrap_or_default();
                if let Some(parent) = self.nodes.get_mut(&parent_id) {
                    parent.children.retain(|child| child != id);
                }
                self.unregister(id);
                self.unsettle(&parent_id);
                Ok(())
            }
            FrameOp::Settle { id } => {
                self.node(id)?;
                self.settled.insert(id.clone());
                Ok(())
            }
            FrameOp::Focus { id } => {
                if let Some(id) = id {
                    self.node(id)?;
                }
                self.focus = id.clone();
                Ok(())
            }
            FrameOp::Reveal { id, .. } => {
                self.node(id)?;
                Ok(())
            }
            FrameOp::Scroll { id, .. } => {
                self.node(id)?;
                Ok(())
            }
            FrameOp::Suspend => {
                self.suspended = true;
                Ok(())
            }
            FrameOp::Resume => {
                self.suspended = false;
                Ok(())
            }
        }
    }

    fn unregister(&mut self, id: &str) {
        let mut stack = vec![id.to_string()];
        while let Some(current) = stack.pop() {
            if let Some(node) = self.nodes.remove(&current) {
                self.settled.remove(&current);
                stack.extend(node.children);
            }
        }
    }

    /// Snapshot of the whole document (root included) as wire nodes.
    pub fn snapshot(&self) -> WireNode {
        self.export(&self.surface)
    }

    /// Snapshot of one subtree, or `None` for an unknown id.
    pub fn snapshot_at(&self, id: &str) -> Option<WireNode> {
        if !self.nodes.contains_key(id) {
            return None;
        }
        Some(self.export(id))
    }

    fn export(&self, id: &str) -> WireNode {
        let node = match self.nodes.get(id) {
            Some(node) => node,
            None => {
                return WireNode {
                    id: id.to_string(),
                    kind: NodeKind::Col,
                    props: Props::new(),
                    children: Vec::new(),
                }
            }
        };
        WireNode {
            id: node.id.clone(),
            kind: node.kind,
            props: node.props.clone(),
            children: node
                .children
                .iter()
                .map(|child| self.export(child))
                .collect(),
        }
    }
}

/// Whether `ancestor` is `node` or one of its ancestors.
fn contains(nodes: &HashMap<String, DocNode>, node: &str, ancestor: &str) -> bool {
    let mut at = node.to_string();
    loop {
        if at == ancestor {
            return true;
        }
        match nodes.get(&at).and_then(|entry| entry.parent.clone()) {
            Some(parent) => at = parent,
            None => return false,
        }
    }
}

/// One surface's reconciler: stable ids, bounded diff, frame ops.
pub struct NativeReconciler {
    surface: String,
    document: TspDocument,
    frame: u64,
    max_nodes: usize,
    max_depth: usize,
    budget: usize,
    counters: NativeCounters,
    settle: NativeSettle,
    pending_settle: BTreeSet<String>,
    reveal_n: HashMap<String, u64>,
    scroll_n: HashMap<String, u64>,
    last_errors: Vec<ApplyError>,
}

impl Default for NativeReconciler {
    fn default() -> Self {
        NativeReconciler::new("surface")
    }
}

impl NativeReconciler {
    /// A reconciler for `surface`.
    pub fn new(surface: impl Into<String>) -> Self {
        let surface = surface.into();
        NativeReconciler {
            document: TspDocument::new(surface.clone()),
            surface,
            frame: 0,
            max_nodes: 4096,
            max_depth: 24,
            budget: 0,
            counters: NativeCounters::default(),
            settle: NativeSettle::new(),
            pending_settle: BTreeSet::new(),
            reveal_n: HashMap::new(),
            scroll_n: HashMap::new(),
            last_errors: Vec::new(),
        }
    }

    /// Set the per-frame add budget and depth bound.
    pub fn with_limits(mut self, max_nodes: usize, max_depth: usize) -> Self {
        self.max_nodes = max_nodes.max(1);
        self.max_depth = max_depth.max(1);
        self
    }

    /// The document the reconciler maintains.
    pub fn document(&self) -> &TspDocument {
        &self.document
    }

    /// Frame counters.
    pub fn counters(&self) -> &NativeCounters {
        &self.counters
    }

    /// The settle memo.
    pub fn settle(&self) -> &NativeSettle {
        &self.settle
    }

    /// Rebind the settle memo (theme epoch plus geometry).
    pub fn rebind_settle(&mut self, key: SettleKey) -> bool {
        self.settle.rebind(key)
    }

    /// Mark `id` finalized; the next frame emits a `settle` op for it.
    pub fn mark_settled(&mut self, id: &str) -> bool {
        self.pending_settle.insert(id.to_string())
    }

    /// Ops rejected while applying the last generated frame.
    pub fn last_errors(&self) -> &[ApplyError] {
        &self.last_errors
    }

    /// Diff the regions against the previous frame; the first call adds
    /// everything.
    pub fn reconcile(&mut self, regions: &NativeSurface, cx: &DescribeContext) -> Vec<FrameOp> {
        self.frame += 1;
        self.budget = self.max_nodes;
        let mut ops = Vec::new();
        let mut dels = Vec::new();
        let mut settles = Vec::new();
        let mut reveals = Vec::new();
        let mut scrolls = Vec::new();
        let mut fallback_rows = 0usize;

        for (region, nodes) in regions.regions() {
            if !self.document.has(region) {
                ops.push(FrameOp::Add {
                    id: region.to_string(),
                    parent: self.surface.clone(),
                    before: None,
                    node: WireNode::new(region, NodeKind::Col),
                });
            }
            let old_children = self
                .document
                .get(region)
                .map(|node| node.children)
                .unwrap_or_default();
            self.diff_children(
                region,
                &old_children,
                nodes,
                cx,
                &mut ops,
                &mut dels,
                &mut settles,
                &mut reveals,
                &mut scrolls,
                &mut fallback_rows,
                0,
            );
        }

        for id in dels {
            ops.push(FrameOp::Del { id });
        }
        for id in settles {
            ops.push(FrameOp::Settle { id });
        }
        for (id, at) in reveals {
            ops.push(FrameOp::Reveal { id, at });
        }
        if cx.feature("scroll") {
            for (id, by) in scrolls {
                ops.push(FrameOp::Scroll { id, by });
            }
        }

        let frame = Frame {
            s: self.frame,
            ops: ops.clone(),
        };
        self.last_errors = self.document.apply_frame(&frame);
        let pending = self
            .document
            .size()
            .saturating_sub(self.settle.len())
            .saturating_sub(1);
        self.counters
            .record_frame(ops.len(), self.document.size(), pending, fallback_rows);
        ops
    }

    #[allow(clippy::too_many_arguments)]
    fn diff_children(
        &mut self,
        parent: &str,
        old_children: &[String],
        new_children: &[NativeNode],
        cx: &DescribeContext,
        ops: &mut Vec<FrameOp>,
        dels: &mut Vec<String>,
        settles: &mut Vec<String>,
        reveals: &mut Vec<(String, RevealAt)>,
        scrolls: &mut Vec<(String, ScrollBy)>,
        fallback_rows: &mut usize,
        depth: usize,
    ) {
        if depth >= self.max_depth {
            return;
        }
        let new_ids: Vec<String> = new_children
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let key = node.key.clone().unwrap_or_else(|| index.to_string());
                child_id(parent, &key)
            })
            .collect();
        let sources: Vec<i64> = new_ids
            .iter()
            .map(|id| {
                old_children
                    .iter()
                    .position(|old| old == id)
                    .map(|index| index as i64)
                    .unwrap_or(-1)
            })
            .collect();
        let keep = stable_mask(&sources);
        let existing: Vec<Option<DocNode>> =
            new_ids.iter().map(|id| self.document.get(id)).collect();
        let stays: Vec<bool> = new_children
            .iter()
            .enumerate()
            .map(|(index, node)| {
                sources[index] >= 0
                    && keep[index]
                    && existing[index]
                        .as_ref()
                        .is_some_and(|old| old.kind == node.kind)
            })
            .collect();
        let mut index = 0;
        while index < new_children.len() {
            if stays[index] {
                let id = &new_ids[index];
                if let Some(old) = &existing[index] {
                    self.diff_node(
                        id,
                        &new_children[index],
                        old,
                        cx,
                        ops,
                        dels,
                        settles,
                        reveals,
                        scrolls,
                        fallback_rows,
                        depth,
                    );
                }
                index += 1;
                continue;
            }
            // A run of children that are all being added (new, moved or
            // kind-changed). Emit it in reverse so every add's `before`
            // anchor already exists when the frame is applied.
            let mut run_end = index;
            while run_end < new_children.len() && !stays[run_end] {
                run_end += 1;
            }
            let anchor = if run_end < new_children.len() {
                Some(new_ids[run_end].clone())
            } else {
                None
            };
            let mut cursor = run_end;
            while cursor > index {
                cursor -= 1;
                let id = &new_ids[cursor];
                if sources[cursor] >= 0 {
                    ops.push(FrameOp::Del {
                        id: id.clone(),
                    });
                }
                if new_children[cursor].kind == NodeKind::Rows {
                    *fallback_rows += 1;
                }
                if self.budget == 0 {
                    continue;
                }
                self.budget -= 1;
                ops.push(FrameOp::Add {
                    id: id.clone(),
                    parent: parent.to_string(),
                    before: anchor.clone(),
                    node: wire_node(&new_children[cursor], id),
                });
                self.diff_requests(id, &new_children[cursor], reveals, scrolls);
                if self.pending_settle.remove(id) {
                    self.settle.settle(id);
                    settles.push(id.clone());
                }
            }
            index = run_end + 1;
        }
        for old in old_children {
            if !new_ids.iter().any(|id| id == old) {
                dels.push(old.clone());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn diff_node(
        &mut self,
        id: &str,
        node: &NativeNode,
        old: &DocNode,
        cx: &DescribeContext,
        ops: &mut Vec<FrameOp>,
        dels: &mut Vec<String>,
        settles: &mut Vec<String>,
        reveals: &mut Vec<(String, RevealAt)>,
        scrolls: &mut Vec<(String, ScrollBy)>,
        fallback_rows: &mut usize,
        depth: usize,
    ) {
        if node.kind == NodeKind::Rows {
            *fallback_rows += 1;
        }
        let mut props = Vec::new();
        for (key, value) in node.props.iter() {
            if old.props.get(key) != Some(value) {
                props.push((key.clone(), value.clone()));
            }
        }
        for key in old.props.iter().map(|(key, _)| key) {
            if !node.props.contains_key(key) {
                props.push((key.clone(), Value::Null));
            }
        }
        if !props.is_empty() {
            ops.push(FrameOp::Set {
                id: id.to_string(),
                props,
            });
        }
        if node.kind.is_text_kind() {
            let next = node.text().unwrap_or_default();
            let previous = old.props.text().unwrap_or_default();
            if next != previous {
                ops.push(FrameOp::Text {
                    id: id.to_string(),
                    mode: TextMode::Replace,
                    text: next.to_string(),
                });
            }
        }
        self.diff_requests(id, node, reveals, scrolls);
        if self.pending_settle.remove(id) {
            self.settle.settle(id);
            settles.push(id.to_string());
        }
        let _ = cx;
        self.diff_children(
            id,
            &old.children,
            &node.children,
            cx,
            ops,
            dels,
            settles,
            reveals,
            scrolls,
            fallback_rows,
            depth + 1,
        );
    }

    fn diff_requests(
        &mut self,
        id: &str,
        node: &NativeNode,
        reveals: &mut Vec<(String, RevealAt)>,
        scrolls: &mut Vec<(String, ScrollBy)>,
    ) {
        if let Some(reveal) = &node.reveal {
            let (at, counter) = match reveal {
                Reveal::At(at) => (*at, 0),
                Reveal::Repeat { at, n } => (*at, *n),
            };
            let changed = !self.reveal_n.contains_key(id) || self.reveal_n[id] != counter;
            if changed {
                self.reveal_n.insert(id.to_string(), counter);
                reveals.push((id.to_string(), at));
            }
        }
        if let Some(scroll) = &node.scroll {
            let changed = !self.scroll_n.contains_key(id) || self.scroll_n[id] != scroll.n;
            if changed {
                self.scroll_n.insert(id.to_string(), scroll.n);
                scrolls.push((id.to_string(), scroll.by));
            }
        }
    }
}

/// The regions a frame fills.
pub fn region_ids() -> [&'static str; 3] {
    ["main", "dock", "layer"]
}

/// Escape a keypath segment for event routing.
pub fn escape_segment(segment: &str) -> String {
    escape_key(segment)
}

/// Unescape a keypath segment reported by an event.
pub fn unescape_segment(segment: &str) -> String {
    unescape_key(segment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::describe::{card, col, keyed, text as text_node, with_hidden};
    use crate::native::node::{NodeKind as Kind, Reveal, Scroll, Text};

    fn plain(text: &str) -> NativeNode {
        text_node(Text::Plain(text.to_string()), Props::new())
    }

    #[test]
    fn memo_rebuilds_only_when_deps_change() {
        let mut memo: Memo<u64> = Memo::new();
        let mut builds = 0;
        let first = {
            let builds = &mut builds;
            *memo.get(&[Value::Number(1.into()), Value::String("a".into())], || {
                *builds += 1;
                7
            })
        };
        assert_eq!(first, 7);
        let second = *memo.get(&[Value::Number(1.into()), Value::String("a".into())], || 9);
        assert_eq!(second, 7);
        assert_eq!(builds, 1);
        memo.clear();
        let third = *memo.get(&[Value::Number(1.into()), Value::String("a".into())], || 9);
        assert_eq!(third, 9);
        assert_eq!(builds, 1);
    }

    #[test]
    fn settle_memo_binds_to_theme_and_geometry() {
        let mut settle = NativeSettle::new();
        let key = SettleKey {
            theme_epoch: 1,
            cols: 80,
            rows: 24,
        };
        assert!(settle.rebind(key.clone()));
        assert!(!settle.rebind(key.clone()));
        assert!(settle.settle("main.0"));
        assert!(settle.is_settled("main.0"));
        let moved = SettleKey {
            theme_epoch: 1,
            cols: 100,
            rows: 24,
        };
        assert!(settle.rebind(moved));
        assert!(!settle.is_settled("main.0"));
    }

    #[test]
    fn native_state_notifies_listeners() {
        let state = NativeState::default();
        let hits = std::sync::Arc::new(AtomicU64::new(0));
        let counter = hits.clone();
        let id = state.on_native_rendering_change(move |on| {
            if on {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
        state.set_native_rendering(true);
        state.set_native_rendering(true);
        state.set_native_rendering(false);
        assert_eq!(hits.load(Ordering::Relaxed), 1);
        state.remove_listener(id);
        state.set_native_rendering(true);
        assert_eq!(hits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stable_mask_keeps_longest_increasing_subsequence() {
        assert_eq!(stable_mask(&[0, 1, 2]), vec![true, true, true]);
        assert_eq!(stable_mask(&[2, 0, 1]), vec![false, true, true]);
        assert_eq!(stable_mask(&[-1, 0, -1, 1]), vec![false, true, false, true]);
        assert_eq!(stable_mask(&[]), Vec::<bool>::new());
    }

    #[test]
    fn key_escape_round_trips() {
        for key in ["a", "a/b", "100%", "a^b", "^hoist"] {
            assert_eq!(unescape_segment(&escape_segment(key)), key);
        }
    }

    #[test]
    fn first_frame_adds_everything() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let surface = NativeSurface {
            main: vec![card(
                Props::new(),
                vec![plain("hello"), keyed(plain("world"), "b")],
            )],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        let ops = reconciler.reconcile(&surface, &cx);
        assert_eq!(reconciler.document().size(), 7);
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Add { id, .. } if id == "main")));
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Add { id, .. } if id == "main.0")));
        // Plain children ride inside the card's add node: `reconcile.ts`
        // materializes whole subtrees into one op.
        let card = ops
            .iter()
            .find_map(|op| match op {
                FrameOp::Add { id, node, .. } if id.as_str() == "main.0" => Some(node),
                _ => None,
            })
            .expect("card add op");
        assert_eq!(
            card.children
                .iter()
                .map(|child| child.id.as_str())
                .collect::<Vec<_>>(),
            vec!["main.0.0", "main.0.b"]
        );
        assert!(reconciler.document().has("main.0.0"));
        assert!(reconciler.document().has("main.0.b"));
        assert!(reconciler.last_errors().is_empty());
        assert_eq!(reconciler.counters().nodes, 7);
    }

    #[test]
    fn unchanged_frame_is_empty() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let surface = NativeSurface {
            main: vec![col(vec![plain("hello")], Props::new())],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        reconciler.reconcile(&surface, &cx);
        let ops = reconciler.reconcile(&surface, &cx);
        assert!(ops.is_empty());
        assert!(reconciler.last_errors().is_empty());
    }

    #[test]
    fn text_change_is_a_replace_op() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let first = NativeSurface {
            main: vec![plain("hello")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        let second = NativeSurface {
            main: vec![plain("hello world")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        reconciler.reconcile(&first, &cx);
        let ops = reconciler.reconcile(&second, &cx);
        assert!(ops.iter().any(|op| matches!(
            op,
            FrameOp::Text { id, mode: TextMode::Replace, text }
                if id == "main.0" && text == "hello world"
        )));
        assert!(reconciler.last_errors().is_empty());
    }

    #[test]
    fn removed_child_is_deleted_and_added_child_is_appended() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let first = NativeSurface {
            main: vec![keyed(plain("a"), "a"), keyed(plain("b"), "b")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        let second = NativeSurface {
            main: vec![keyed(plain("b"), "b"), keyed(plain("c"), "c")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        reconciler.reconcile(&first, &cx);
        let ops = reconciler.reconcile(&second, &cx);
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Del { id } if id == "main.a")));
        assert!(!ops.iter().any(|op| matches!(op, FrameOp::Add { id, .. } if id == "main.b")));
        assert_eq!(
            reconciler.document().get("main").unwrap().children,
            vec!["main.b".to_string(), "main.c".to_string()]
        );
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Add { id, .. } if id == "main.c")));
        assert!(reconciler.last_errors().is_empty());
        assert!(reconciler.document().has("main.b"));
        assert!(reconciler.document().has("main.c"));
        assert!(!reconciler.document().has("main.a"));
    }

    #[test]
    fn kind_change_replaces_the_node() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let first = NativeSurface {
            main: vec![keyed(plain("a"), "a")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        let second = NativeSurface {
            main: vec![keyed(card(Props::new(), vec![plain("a")]), "a")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        reconciler.reconcile(&first, &cx);
        let ops = reconciler.reconcile(&second, &cx);
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Del { id } if id == "main.a")));
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Add { id, .. } if id == "main.a")));
        assert!(reconciler.last_errors().is_empty());
        assert_eq!(
            reconciler.document().get("main.a").map(|node| node.kind),
            Some(Kind::Card)
        );
    }

    #[test]
    fn reveal_and_scroll_fire_once_per_counter() {
        let mut reconciler = NativeReconciler::new("s");
        let mut cx = DescribeContext::new(40);
        cx.enable_feature("scroll");
        let mut node = NativeNode::new(Kind::Text)
            .with_reveal(Reveal::Repeat {
                at: RevealAt::End,
                n: 1,
            })
            .with_scroll(Scroll {
                by: ScrollBy::LineDown,
                n: 1,
            });
        node.props.set_text("x");
        let surface = NativeSurface {
            main: vec![node],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        let ops = reconciler.reconcile(&surface, &cx);
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Reveal { .. })));
        assert!(ops.iter().any(|op| matches!(op, FrameOp::Scroll { .. })));
        let ops = reconciler.reconcile(&surface, &cx);
        assert!(!ops.iter().any(|op| matches!(op, FrameOp::Reveal { .. })));
        assert!(!ops.iter().any(|op| matches!(op, FrameOp::Scroll { .. })));
    }

    #[test]
    fn settle_hint_is_emitted_once() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let surface = NativeSurface {
            main: vec![plain("hello")],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        reconciler.reconcile(&surface, &cx);
        assert!(reconciler.mark_settled("main.0"));
        assert!(!reconciler.mark_settled("main.0"));
        let ops = reconciler.reconcile(&surface, &cx);
        assert!(ops
            .iter()
            .any(|op| matches!(op, FrameOp::Settle { id } if id == "main.0")));
        let ops = reconciler.reconcile(&surface, &cx);
        assert!(!ops.iter().any(|op| matches!(op, FrameOp::Settle { .. })));
    }

    #[test]
    fn document_applies_and_validates_ops() {
        let mut document = TspDocument::new("s");
        let mut node = WireNode::new("main.0", Kind::Text);
        node.props.set_text("hello");
        let frame = Frame {
            s: 1,
            ops: vec![
                FrameOp::Add {
                    id: "main.0".to_string(),
                    parent: "s".to_string(),
                    before: None,
                    node,
                },
                FrameOp::Set {
                    id: "main.0".to_string(),
                    props: vec![("tone".to_string(), Value::String("accent".into()))],
                },
                FrameOp::Text {
                    id: "main.0".to_string(),
                    mode: TextMode::Append,
                    text: "!".to_string(),
                },
                FrameOp::Settle {
                    id: "main.0".to_string(),
                },
            ],
        };
        let errors = document.apply_frame(&frame);
        assert!(errors.is_empty());
        let node = document.get("main.0").unwrap();
        assert_eq!(node.props.text(), Some("hello!"));
        assert_eq!(
            node.props.tone(),
            Some(crate::native::node::Tone::Accent)
        );
        assert!(document.settled().contains("main.0"));

        let bad = Frame {
            s: 2,
            ops: vec![
                FrameOp::Text {
                    id: "nope".to_string(),
                    mode: TextMode::Replace,
                    text: "x".to_string(),
                },
                FrameOp::Del {
                    id: "s".to_string(),
                },
                FrameOp::Splice {
                    id: "main.0".to_string(),
                    at: 99,
                    del: 1,
                    text: String::new(),
                },
            ],
        };
        let errors = document.apply_frame(&bad);
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0].op, 0);
        assert_eq!(errors[1].op, 1);
        assert_eq!(errors[2].op, 2);
    }

    #[test]
    fn document_close_drops_live_regions_and_keeps_main() {
        let mut document = TspDocument::new("s");
        let frame = Frame {
            s: 1,
            ops: vec![
                FrameOp::Add {
                    id: "main".to_string(),
                    parent: "s".to_string(),
                    before: None,
                    node: WireNode::new("main", Kind::Col),
                },
                FrameOp::Add {
                    id: "dock".to_string(),
                    parent: "s".to_string(),
                    before: None,
                    node: WireNode::new("dock", Kind::Col),
                },
                FrameOp::Focus {
                    id: Some("main".to_string()),
                },
            ],
        };
        document.apply_frame(&frame);
        document.close();
        assert!(document.has("main"));
        assert!(!document.has("dock"));
        assert_eq!(document.focus(), None);
    }

    #[test]
    fn wire_ops_serialize_in_apply_shape() {
        let op = FrameOp::Splice {
            id: "main.0".to_string(),
            at: 1,
            del: 2,
            text: "x".to_string(),
        };
        let value = op.to_value();
        assert_eq!(value[0], "splice");
        assert_eq!(value[1], "main.0");
        assert_eq!(value[3], 2);
    }

    #[test]
    fn counters_track_pending_and_fallback() {
        let mut reconciler = NativeReconciler::new("s");
        let cx = DescribeContext::new(40);
        let surface = NativeSurface {
            main: vec![with_hidden(
                crate::native::describe::describe_child(
                    &mut crate::components::Panel::new(0, 0),
                    &cx,
                ),
                false,
            )],
            dock: Vec::new(),
            layer: Vec::new(),
        };
        reconciler.reconcile(&surface, &cx);
        assert_eq!(reconciler.counters().fallback_rows, 1);
        assert!(reconciler.counters().pending >= 1);
    }
}
