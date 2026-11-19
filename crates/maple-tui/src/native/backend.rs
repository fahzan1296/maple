//! Native (Tern Surface Protocol) backend: owns the surfaces a TSP terminal
//! shows, sends reconciled frames under credit-based flow control, uploads
//! blobs, and routes terminal events back to components.
//!
//! Reference: `l/src/native/backend.ts`, `l/src/native/reconcile.ts`.
//!
//! Deviations (JS/runtime and crate-shape differences):
//! - `Bun.file`, `fs.promises`, `Date.now`, `setTimeout(...).unref()` and
//!   `process.env` are modeled with `std::fs`, `std::time::SystemTime` and a
//!   [`RenderScheduler`] abstraction; the default scheduler is synchronous.
//! - `@oh-my-pi/pi-wire` `Tsp*` types are mapped onto this crate's
//!   `encode`/`state`/`node` types and re-exported under the reference names
//!   where possible (`TspHello`, `TspEvent`, `TspFrame`, `TspOp`, `TspNode`,
//!   `TspKind`). Field shapes follow crate wire types, not the vendored JS
//!   package.
//! - The crate reconciler (`native::state::NativeReconciler`) diffs resolved
//!   [`NativeNode`] region trees rather than TS component instances. Component
//!   boundaries are tracked for region children described as
//!   [`NativeChild::Component`] (stable root ids, owner chains rooted at those
//!   boundaries, focus-target/editor search, and event dispatch). Nested
//!   component boundaries inside a component-described node tree are not
//!   representable in the crate node model and are attributed to the nearest
//!   tracked boundary.
//! - `getNativeBlob` from `l/src/native/blobs.ts` has no global registry in
//!   this crate; blob upload can only inline bytes already known to the caller.
//!   Unknown blob ids referenced by ops are tracked as sent placeholders.
//! - Theme/motion symbol hooks (`getCurrentThemeName`, `isLightTheme`,
//!   `setNativeSymbolPreset`, `onMotionEffectsChange`, `onSpinnerIntervalChange`,
//!   `logger`) are not globally wired in this crate; integrations are local
//!   approximations.
//! - `detachLive` preserves the inline mirror document's live `main` region and
//!   resets the reconciler/routes so `dock`/`layer` are re-added on the next
//!   frame. Without a component-aware reconciler, the next frame may fully
//!   re-describe regions; terminal-side ids remain stable because ids derive
//!   from keys.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::native::describe::node as make_node;
use crate::native::encode::{
	encode_json, split_message, Event, Hello, Incoming, Reply, TspParams, TspReader, Verb,
	TSP_DEFAULT_APC_LIMIT, TSP_DEFAULT_CREDITS, TSP_VERSION,
};
use crate::native::node::{DescribeContext, NativeNode, NativeScreen, NativeSurface, NodeKind, Props};
use crate::native::state::{
	escape_segment, set_native_buttons, set_native_pacing, set_native_rendering, unescape_segment,
	Frame, FrameOp, NativeReconciler, TspDocument, WireNode,
};
use crate::terminal::Terminal;

/// Share of the viewport's rows a centered sheet's content must reach to fill a narrow pane.
pub const SHEET_FILL_FRACTION: f64 = 0.5;

/// Frames kept for the debug `tsp` op.
pub const RECENT_FRAMES: usize = 64;

/// An unanswered frame older than this no longer holds rendering back.
pub const STALLED_ACK_MS: u64 = 5000;

/// Role of the session's surfaces; a screen page may name its own.
pub const SESSION_ROLE: &str = "omp.session";

/// A `blobs` query unanswered this long counts its ids as missing: they go inline.
pub const BLOB_REPLY_MS: u64 = 3000;

/// Expired queries kept to pair late replies with their queries, at most.
pub const MAX_BLOB_QUERIES: usize = 16;

/// Kinds newer than some builds that answer `TERM_PROGRAM=tern`; excluded from optimistic hello.
pub const UNASSUMED_KINDS: [&str; 4] = ["media", "json", "button", "shell"];

/// A timer handle returned by [`RenderScheduler::scheduleRender`].
pub trait RenderTimer {
	/// Cancel the scheduled callback when possible.
	fn cancel(&self);
}

impl RenderTimer for () {
	fn cancel(&self) {}
}

/// Clock and timers (stall wake-up); defaults to wall clock and immediate scheduling.
pub trait RenderScheduler {
	/// Current wall-clock milliseconds.
	fn now(&self) -> u64;
	/// Schedule `callback` to run soon.
	fn scheduleImmediate(&self, callback: Box<dyn FnOnce() + Send + 'static>);
	/// Schedule `callback` after `delayMs`.
	fn scheduleRender(
		&self,
		callback: Box<dyn FnOnce() + Send + 'static>,
		delayMs: u64,
	) -> Box<dyn RenderTimer>;
}

/// Real clock and immediate timers (no background thread ownership in this crate).
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultScheduler;

impl RenderScheduler for DefaultScheduler {
	fn now(&self) -> u64 {
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap_or_default()
			.as_millis() as u64
	}

	fn scheduleImmediate(&self, callback: Box<dyn FnOnce() + Send + 'static>) {
		callback();
	}

	fn scheduleRender(
		&self,
		callback: Box<dyn FnOnce() + Send + 'static>,
		_delayMs: u64,
	) -> Box<dyn RenderTimer> {
		callback();
		Box::new(())
	}
}

/// Default scheduler instance.
pub static DEFAULT_SCHEDULER: DefaultScheduler = DefaultScheduler;

/// A visible TUI overlay, bottom to top.
#[derive(Clone, Debug)]
pub struct NativeOverlay<C> {
	/// Overlay component.
	pub component: C,
	/// Overlay options.
	pub options: Option<OverlayOptions>,
	/// Keyboard focus is inside this overlay.
	pub focused: bool,
}

/// Overlay placement options consumed by the native backend.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayOptions {
	/// Anchor position (e.g. `center`, `top-left`, `bottom-right`).
	pub anchor: Option<String>,
	/// Width as columns or percent string (e.g. `"50%"`).
	pub width: Option<OverlayWidth>,
	/// Whether the overlay is fullscreen.
	pub fullscreen: bool,
}

/// Overlay width.
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayWidth {
	/// Fixed columns.
	Cols(u16),
	/// Percent string as provided by the host.
	Percent(String),
}

/// Component hooks the backend consults when describing/routing native UI.
pub trait NativeComponent {
	/// Describe this component as a native subtree.
	fn describe(&self, _cx: &DescribeContext) -> Option<NativeNode> {
		None
	}
	/// Describe a fullscreen page this overlay owns.
	fn describeScreen(&self, _cx: &DescribeContext) -> Option<NativeScreen> {
		None
	}
	/// Whether this component is its own sheet in native mode.
	fn nativeSheet(&self, _cx: &DescribeContext) -> bool {
		false
	}
	/// Overlay chrome this component supplies itself.
	fn nativeOverlay(&self) -> Option<NativeOverlaySpec> {
		None
	}
	/// Handle a native UI event routed to this component.
	fn handleNativeEvent(&self, _event: &NativeUiEvent) {}
}

/// Overlay chrome supplied by a component.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeOverlaySpec {
	/// Sheet role.
	pub role: Option<String>,
	/// Head text (wire text value).
	pub head: Option<Value>,
	/// Size hint.
	pub size: Option<String>,
	/// Anchor hint.
	pub anchor: Option<String>,
}

/// A child in a native region: a described node or a component boundary.
#[derive(Clone, Debug)]
pub enum NativeChild<C> {
	/// An already-described node.
	Node(NativeNode),
	/// A component described on demand.
	Component(C),
}

/// What the backend needs from the TUI.
pub trait NativeHost<C> {
	/// Terminal the backend writes to.
	fn terminal(&mut self) -> &mut dyn Terminal;
	/// `main`/`dock` content: the frame provider's surface, or the TUI children.
	fn describeSurface(&mut self, cx: &DescribeContext) -> NativeRegions<C>;
	/// Visible overlays, bottom to top.
	fn overlays(&mut self) -> Vec<NativeOverlay<C>>;
	/// Component receiving keyboard input.
	fn focused(&self) -> Option<C>;
	/// Move keyboard focus after a pointer event.
	fn focusFromPointer(
		&mut self,
		owners: Vec<C>,
		field: Option<C>,
		sheet: &dyn Fn(&C) -> bool,
	);
	/// Request a render.
	fn requestRender(&mut self);
	/// The terminal switched appearance.
	fn appearanceChanged(&mut self, dark: bool);
	/// Reduce Motion toggled.
	fn motionChanged(&mut self, reduce: bool);
	/// Theme-derived output changed: drop render and describe caches.
	fn invalidate(&mut self);
}

/// Regions a frame fills.
#[derive(Clone, Debug, Default)]
pub struct NativeRegions<C> {
	/// Main region.
	pub main: Vec<NativeChild<C>>,
	/// Dock region.
	pub dock: Vec<NativeChild<C>>,
}

/// Backend options.
#[derive(Clone, Debug, Default)]
pub struct NativeBackendOptions {
	/// Keep the reference document and recent frames for debugging.
	pub mirror: bool,
	/// Append every TSP message to this JSONL file.
	pub recordPath: Option<String>,
	/// Log the `rows` fallback count per frame.
	pub stats: bool,
}

/// UI events routed from the terminal to components.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeUiEvent {
	/// Toggle a collapsible node.
	Toggle {
		/// Target keypath segments.
		key: Vec<String>,
		/// Collapsed state.
		collapsed: bool,
	},
	/// Select an item.
	Select {
		/// Target keypath segments.
		key: Vec<String>,
		/// Selected item id/key.
		item: String,
	},
	/// Activate an item.
	Activate {
		/// Target keypath segments.
		key: Vec<String>,
		/// Activated item id/key.
		item: String,
	},
	/// Run an action.
	Action {
		/// Target keypath segments.
		key: Vec<String>,
		/// Action id.
		act: String,
		/// Optional action value.
		value: Option<Value>,
		/// Modifier keys.
		mods: Vec<String>,
	},
	/// Change a value.
	Change {
		/// Target keypath segments.
		key: Vec<String>,
		/// Changed item id.
		item: String,
		/// New value.
		value: Value,
	},
	/// Edit text.
	Edit {
		/// Target keypath segments.
		key: Vec<String>,
		/// Edit start offset.
		from: usize,
		/// Edit end offset.
		to: usize,
		/// Inserted text.
		text: String,
		/// Cursor offset.
		cursor: usize,
		/// New text length.
		len: usize,
	},
	/// Undo an edit.
	Undo {
		/// Target keypath segments.
		key: Vec<String>,
	},
	/// Send text.
	Send {
		/// Target keypath segments.
		key: Vec<String>,
		/// Sent text.
		text: String,
	},
}

/// Reference-compatible wire aliases.
pub use crate::native::encode::Event as TspEvent;
pub use crate::native::encode::Hello as TspHello;
pub use crate::native::node::NodeKind as TspKind;
pub use crate::native::state::Frame as TspFrame;
pub use crate::native::state::FrameOp as TspOp;
pub use crate::native::state::WireNode as TspNode;

/// Where a wire id came from, for event routing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTarget<C> {
	/// Component that described the node.
	pub component: C,
	/// Keypath inside the component (empty for its root node).
	pub keypath: Vec<String>,
}

#[derive(Clone, Debug)]
struct Boundary<C> {
	component: C,
	root_id: String,
}

#[derive(Clone, Debug)]
struct RouteIndex<C> {
	boundaries: Vec<Boundary<C>>,
	gone: HashSet<String>,
}

impl<C> Default for RouteIndex<C> {
	fn default() -> Self {
		RouteIndex {
			boundaries: Vec::new(),
			gone: HashSet::new(),
		}
	}
}

impl<C: Clone> RouteIndex<C> {
	fn clear(&mut self) {
		self.boundaries.clear();
	}

	fn register(&mut self, root_id: String, component: C) {
		self.boundaries.push(Boundary { component, root_id });
	}

	fn forget(&mut self, ids: &[String]) {
		for id in ids {
			self.gone.insert(id.clone());
		}
	}

	fn owners(&self, id: &str) -> Vec<C> {
		let mut matches: Vec<&Boundary<C>> = self
			.boundaries
			.iter()
			.filter(|b| id == b.root_id || id.starts_with(&(b.root_id.clone() + ".")))
			.collect();
		matches.sort_by_key(|b| std::cmp::Reverse(b.root_id.len()));
		matches.into_iter().map(|b| b.component.clone()).collect()
	}

	fn target(&self, id: &str) -> Option<NativeTarget<C>> {
		let component = self.owners(id).into_iter().next()?;
		let base_len = self
			.boundaries
			.iter()
			.filter(|b| id == b.root_id || id.starts_with(&(b.root_id.clone() + ".")))
			.map(|b| b.root_id.len())
			.max()
			.unwrap_or(0);
		let keypath = if id.len() == base_len {
			Vec::new()
		} else {
			id[base_len + 1..]
				.split('.')
				.map(unescape_segment)
				.collect()
		};
		Some(NativeTarget { component, keypath })
	}
}

#[derive(Clone, Debug)]
struct BlobQuery {
	ids: Vec<String>,
	timer_live: bool,
}

struct Surface<C> {
	id: String,
	mode: &'static str,
	role: String,
	reconciler: NativeReconciler,
	doc: Option<TspDocument>,
	seq: u64,
	acked: u64,
	unacked: Vec<u64>,
	stalled_since: Option<u64>,
	stall_resets: u64,
	focus: Option<String>,
	dirty: bool,
	routes: RouteIndex<C>,
}

impl<C: Clone> Surface<C> {
	fn new(id: String, mode: &'static str, role: String, mirror: bool) -> Self {
		Surface {
			reconciler: NativeReconciler::new(id.clone()),
			doc: if mirror { Some(TspDocument::new(id.clone())) } else { None },
			id,
			mode,
			role,
			seq: 0,
			acked: 0,
			unacked: Vec::new(),
			stalled_since: None,
			stall_resets: 0,
			focus: None,
			dirty: false,
			routes: RouteIndex::default(),
		}
	}

	fn detach_live(&mut self) {
		if let Some(doc) = &mut self.doc {
			doc.close();
		}
		self.reconciler = NativeReconciler::new(self.id.clone());
		self.focus = None;
		self.routes.clear();
	}
}

#[derive(Clone, Debug)]
enum BlobState {
	Tracked,
}

fn overlayAnchor(options: Option<&OverlayOptions>) -> &'static str {
	match options.and_then(|o| o.anchor.as_deref()) {
		Some(anchor) if anchor.starts_with("top") => "top",
		Some(anchor) if anchor.starts_with("bottom") => "bottom",
		_ => "center",
	}
}

fn overlaySize(options: Option<&OverlayOptions>, cols: u16) -> &'static str {
	let Some(options) = options else {
		return "md";
	};
	if options.fullscreen {
		return "full";
	}
	let Some(width) = &options.width else {
		return "md";
	};
	let cols_f = cols.max(1) as f64;
	let fraction = match width {
		OverlayWidth::Cols(value) => (*value as f64) / cols_f,
		OverlayWidth::Percent(text) => text
			.trim_end_matches('%')
			.parse::<f64>()
			.unwrap_or(f64::NAN)
			/ 100.0,
	};
	if !fraction.is_finite() {
		return "md";
	}
	if fraction >= 0.75 {
		"lg"
	} else if fraction >= 0.45 {
		"md"
	} else {
		"sm"
	}
}

fn collectBlobs(op: &FrameOp, out: &mut HashSet<String>) {
	match op {
		FrameOp::Set { props, .. } => {
			for (key, value) in props {
				if key == "blob" {
					if let Some(id) = value.as_str() {
						out.insert(id.to_string());
					}
				}
				if key == "decor" {
					collectDecorBlobs(value, out);
				}
			}
		}
		FrameOp::Add { node, .. } => collectWireBlobs(node, out),
		_ => {}
	}
}

fn collectWireBlobs(node: &WireNode, out: &mut HashSet<String>) {
	if node.kind == NodeKind::Image {
		if let Some(id) = node.props.get_str("blob") {
			out.insert(id.to_string());
		}
	}
	if matches!(node.kind, NodeKind::Editor | NodeKind::Input) {
		if let Some(decor) = node.props.get("decor") {
			collectDecorBlobs(decor, out);
		}
	}
	for child in &node.children {
		collectWireBlobs(child, out);
	}
}

fn collectDecorBlobs(decor: &Value, out: &mut HashSet<String>) {
	let Some(items) = decor.as_array() else {
		return;
	};
	for item in items {
		if let Some(id) = item.get("blob").and_then(Value::as_str) {
			out.insert(id.to_string());
		}
	}
}

fn sameVocabulary(a: &DescribeContext, b: &DescribeContext) -> bool {
	a.dark == b.dark && a.reduce_motion == b.reduce_motion && a.hour12 == b.hour12
}

fn keypath_from_id(id: &str) -> Vec<String> {
	id.split('.').skip(1).map(unescape_segment).collect()
}

fn item_key_from_id(id: &str) -> String {
	let segment = id.rsplit_once(['.', '/']).map(|(_, s)| s).unwrap_or(id);
	unescape_segment(segment)
}

fn event_sf(event: &Event) -> Option<&str> {
	event.get_str("sf")
}

fn segment_for_index_or_key(node: &NativeNode, index: usize) -> String {
	node.key.clone().unwrap_or_else(|| index.to_string())
}

fn described_or_rows<C: NativeComponent>(component: &C, cx: &DescribeContext) -> NativeNode {
	component.describe(cx).unwrap_or_else(|| {
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(1)));
		props.insert("lines", Value::Array(Vec::new()));
		NativeNode::with_props(NodeKind::Rows, props)
	})
}

fn resolve_regions<C: NativeComponent + Clone>(
	regions: &NativeRegions<C>,
	cx: &DescribeContext,
	layer: Vec<NativeNode>,
	routes: &mut RouteIndex<C>,
) -> NativeSurface {
	routes.clear();
	let main = resolve_children("main", &regions.main, cx, routes);
	let dock = resolve_children("dock", &regions.dock, cx, routes);
	NativeSurface { main, dock, layer }
}

fn resolve_children<C: NativeComponent + Clone>(
	parent: &str,
	children: &[NativeChild<C>],
	cx: &DescribeContext,
	routes: &mut RouteIndex<C>,
) -> Vec<NativeNode> {
	children
		.iter()
		.enumerate()
		.map(|(index, child)| match child {
			NativeChild::Node(node) => node.clone(),
			NativeChild::Component(component) => {
				let described = described_or_rows(component, cx);
				let key = segment_for_index_or_key(&described, index);
				let id = format!("{parent}.{}", escape_segment(&key));
				routes.register(id, component.clone());
				described
			}
		})
		.collect()
}

fn find_editor_in_doc(doc: &TspDocument, id: &str) -> Option<String> {
	let node = doc.get(id)?;
	if matches!(node.kind, NodeKind::Editor | NodeKind::Input) {
		return Some(node.id);
	}
	for child in &node.children {
		if let Some(found) = find_editor_in_doc(doc, child) {
			return Some(found);
		}
	}
	None
}

fn component_root_for<C: Clone + PartialEq>(routes: &RouteIndex<C>, component: &C) -> Option<String> {
	routes
		.boundaries
		.iter()
		.find(|b| &b.component == component)
		.map(|b| b.root_id.clone())
}

/// The reply a v1 terminal is assumed to give before its real `hello` arrives.
pub fn assumedTspHello(terminal: &dyn Terminal) -> TspHello {
	let kinds = NodeKind::ALL
		.iter()
		.filter_map(|kind| {
			let name = kind.as_str();
			if UNASSUMED_KINDS.contains(&name) {
				None
			} else {
				Some(name.to_string())
			}
		})
		.collect();
	Hello {
		v: TSP_VERSION as i64,
		term: "tern".to_string(),
		kinds,
		features: vec!["blobs".to_string()],
		apc: Some(TSP_DEFAULT_APC_LIMIT as u32),
		credits: Some(TSP_DEFAULT_CREDITS),
		cols: Some(terminal.columns()),
		dark: Some(true),
		reduce_motion: Some(false),
		hour12: None,
		ver: None,
		extra: serde_json::Map::new(),
	}
}

/// The native backend.
pub struct NativeBackend<C, H, S = DefaultScheduler>
where
	S: RenderScheduler,
{
	host: H,
	hello: TspHello,
	cx: DescribeContext,
	limit: usize,
	credits: u32,
	reader: TspReader,
	inline: Surface<C>,
	screen: Option<Surface<C>>,
	next_surface: u64,
	mirror: bool,
	record_path: Option<String>,
	stats: bool,
	scheduler: S,
	recent: Vec<(String, u64, Vec<Value>)>,
	saw_resize: bool,
	live: bool,
	blobs: HashMap<String, BlobState>,
	blob_queries: Vec<BlobQuery>,
	ask_held: bool,
	_marker: std::marker::PhantomData<C>,
}

impl<C, H> NativeBackend<C, H, DefaultScheduler>
where
	C: NativeComponent + Clone + PartialEq,
	H: NativeHost<C>,
{
	/// Construct a backend with the default scheduler.
	pub fn new(host: H, hello: TspHello, options: NativeBackendOptions) -> Self {
		Self::with_scheduler(host, hello, options, DefaultScheduler)
	}
}

impl<C, H, S> NativeBackend<C, H, S>
where
	C: NativeComponent + Clone + PartialEq,
	H: NativeHost<C>,
	S: RenderScheduler,
{
	/// Construct a backend with an explicit scheduler.
	pub fn with_scheduler(
		host: H,
		hello: TspHello,
		options: NativeBackendOptions,
		scheduler: S,
	) -> Self {
		let mut backend = NativeBackend {
			host,
			hello: hello.clone(),
			cx: DescribeContext::with_capabilities(hello.cols.unwrap_or(80), &[], &[]),
			limit: TSP_DEFAULT_APC_LIMIT,
			credits: TSP_DEFAULT_CREDITS,
			reader: TspReader::new(),
			inline: Surface::new("s:1".to_string(), "inline", SESSION_ROLE.to_string(), options.mirror),
			screen: None,
			next_surface: 2,
			mirror: options.mirror,
			record_path: options
				.recordPath
				.or_else(|| std::env::var("PI_TUI_TSP_RECORD").ok()),
			stats: options.stats || std::env::var("PI_TUI_NATIVE_STATS").ok().as_deref() == Some("1"),
			scheduler,
			recent: Vec::new(),
			saw_resize: false,
			live: false,
			blobs: HashMap::new(),
			blob_queries: Vec::new(),
			ask_held: true,
			_marker: std::marker::PhantomData,
		};
		backend.apply_hello(hello);
		backend
	}

	/// The terminal's hello reply.
	pub fn hello(&self) -> &TspHello {
		&self.hello
	}

	/// Describe context shared by every surface.
	pub fn context(&self) -> &DescribeContext {
		&self.cx
	}

	/// `rows` fallback nodes in the last frame.
	pub fn fallbackCount(&self) -> u64 {
		self.screen
			.as_ref()
			.unwrap_or(&self.inline)
			.reconciler
			.counters()
			.fallback_rows
	}

	/// Open the inline surface and send the first frame.
	pub fn start(&mut self) {
		if self.live {
			return;
		}
		self.live = true;
		self.open_surface(&self.inline.id.clone(), self.inline.mode, &self.inline.role.clone(), false);
		set_native_rendering(true);
		self.render();
	}

	/// Close every surface.
	pub fn stop(&mut self, keep: bool) {
		if !self.live {
			return;
		}
		self.live = false;
		if let Some(screen) = self.screen.take() {
			self.write_close(&screen.id, false);
		}
		let inline_id = self.inline.id.clone();
		self.write_close(&inline_id, keep);
		if let Some(doc) = &mut self.inline.doc {
			doc.close();
		}
		set_native_rendering(false);
		set_native_buttons(false);
		set_native_pacing(false);
		self.reset_blobs();
		self.blob_queries.clear();
	}

	/// Adopt the closed inline surface after a stop/start cycle.
	pub fn resume(&mut self, hello: TspHello) {
		if self.live {
			return;
		}
		self.hello = hello.clone();
		self.apply_hello(hello);
		self.live = true;
		self.reset_blobs();
		self.inline.detach_live();
		self.inline.seq = 0;
		self.inline.acked = 0;
		self.inline.unacked.clear();
		self.inline.stalled_since = None;
		self.inline.stall_resets = 0;
		self.inline.dirty = false;
		self.write_open(&self.inline.id.clone(), "inline", "omp", SESSION_ROLE, true);
		set_native_rendering(true);
		self.render();
	}

	/// The terminal's real `hello` reply after an optimistic start.
	pub fn confirm(&mut self, hello: TspHello) {
		let before = self.cx.clone();
		let saw_resize = self.saw_resize;
		self.hello = hello.clone();
		self.apply_hello(hello);
		if saw_resize {
			self.cx.cols = before.cols;
			self.saw_resize = true;
		}
		if self.cx.dark != before.dark {
			self.host.appearanceChanged(self.cx.dark);
		}
		if self.cx.reduce_motion != before.reduce_motion
			|| self.cx.hour12 != before.hour12
			|| !sameVocabulary(&self.cx, &before)
		{
			self.host.invalidate();
		}
		self.host.requestRender();
	}

	/// Refresh the width used for `rows` fallback until the terminal reports one.
	pub fn noteTerminalColumns(&mut self, columns: u16) {
		if self.saw_resize || columns == self.cx.cols {
			return;
		}
		self.cx.cols = columns;
		self.host.requestRender();
	}

	/// Reconcile and send a frame, unless the target surface is out of credits.
	pub fn render(&mut self) {
		if !self.live {
			return;
		}
		let regions = self.host.describeSurface(&self.cx);
		let layer_nodes = Vec::new();
		let active_screen = self.screen.is_some();
		let surface_id = {
			let credits = self.credits;
			let now = self.scheduler.now();
			let surface = if active_screen {
				self.screen.as_mut().unwrap()
			} else {
				&mut self.inline
			};
			if !Self::has_credit(credits, now, surface) {
				surface.dirty = true;
				return;
			}
			surface.dirty = false;
			surface.id.clone()
		};
		let resolved = if active_screen {
			let surface = self.screen.as_mut().unwrap();
			resolve_regions(&regions, &self.cx, layer_nodes, &mut surface.routes)
		} else {
			resolve_regions(&regions, &self.cx, layer_nodes, &mut self.inline.routes)
		};
		let ops = if active_screen {
			self.screen.as_mut().unwrap().reconciler.reconcile(&resolved, &self.cx)
		} else {
			self.inline.reconciler.reconcile(&resolved, &self.cx)
		};
		if ops.is_empty() {
			return;
		}
		self.upload_blobs(&ops);
		self.send_frame(&surface_id, ops);
	}

	/// Handle one complete `ESC _ tsp;…` input string.
	pub fn handleInput(&mut self, sequence: &str) -> bool {
		let Some(raw) = split_message(sequence) else {
			return false;
		};
		self.record("in", &raw.verb, Some(&raw.params), &Value::String(raw.body.clone()));
		let Some(message) = self.reader.feed(sequence) else {
			return true;
		};
		match message {
			Incoming::Event { event, .. } => self.handle_event(&event),
			Incoming::Reply { reply, .. } => {
				if let Reply::Blobs { have, .. } = reply {
					self.on_blobs_reply(&have);
				}
			}
		}
		true
	}

	/// Reference document of the live surface (mirror mode only).
	pub fn document(&self) -> Option<TspNode> {
		self.screen
			.as_ref()
			.or(Some(&self.inline))
			.and_then(|surface| surface.doc.as_ref())
			.map(|doc| doc.snapshot())
	}

	/// Last sent frames, oldest first (mirror mode only).
	pub fn recentFrames(&self, count: usize) -> Vec<Value> {
		let keep = if count == 0 { RECENT_FRAMES } else { count };
		let start = self.recent.len().saturating_sub(keep);
		self.recent[start..]
			.iter()
			.map(|(sf, s, ops)| {
				let mut frame = serde_json::Map::new();
				frame.insert("sf".to_string(), Value::String(sf.clone()));
				frame.insert("s".to_string(), Value::Number((*s).into()));
				frame.insert("ops".to_string(), Value::Array(ops.clone()));
				Value::Object(frame)
			})
			.collect()
	}

	/// The component and keypath that described wire node `id`.
	pub fn target(&self, surface_id: &str, id: &str) -> Option<NativeTarget<C>> {
		self.surface(surface_id)?.routes.target(id)
	}

	/// The component that described wire node `id`, then each containing owner, innermost first.
	pub fn owners(&self, surface_id: &str, id: &str) -> Vec<C> {
		self.surface(surface_id)
			.map(|s| s.routes.owners(id))
			.unwrap_or_default()
	}

	/// The key a list item node was described with (its key, else its child index/segment).
	pub fn itemKey(&self, _surface_id: &str, id: &str) -> String {
		item_key_from_id(id)
	}

	/// Wire id of the first `editor`/`input` `component` describes (for `focus`), or null.
	pub fn focusTarget(&self, surface_id: &str, component: &C) -> Option<String> {
		let surface = self.surface(surface_id)?;
		let root = component_root_for(&surface.routes, component)?;
		let doc = surface.doc.as_ref()?;
		find_editor_in_doc(doc, &root)
	}

	/// Stop referencing ids the terminal dropped (`gone`).
	pub fn forget(&mut self, surface_id: &str, ids: &[String]) {
		if let Some(surface) = self.surface_mut(surface_id) {
			surface.routes.forget(ids);
		}
	}

	/// The inline surface was closed (`keep:true`) and is being adopted.
	pub fn detachLive(&mut self) {
		self.inline.detach_live();
	}

	fn surface(&self, id: &str) -> Option<&Surface<C>> {
		if self.inline.id == id {
			Some(&self.inline)
		} else {
			self.screen.as_ref().filter(|s| s.id == id)
		}
	}

	fn surface_mut(&mut self, id: &str) -> Option<&mut Surface<C>> {
		if self.inline.id == id {
			Some(&mut self.inline)
		} else {
			self.screen.as_mut().filter(|s| s.id == id)
		}
	}

	fn apply_hello(&mut self, hello: TspHello) {
		self.limit = hello
			.apc
			.map(|v| v as usize)
			.filter(|v| *v > 0)
			.unwrap_or(TSP_DEFAULT_APC_LIMIT);
		self.credits = hello.credits.filter(|v| *v > 0).unwrap_or(TSP_DEFAULT_CREDITS);
		let features: Vec<&str> = hello.features.iter().map(String::as_str).collect();
		self.cx = DescribeContext::with_capabilities(hello.cols.unwrap_or(self.cx.cols), &[], &features);
		self.cx.dark = hello.dark.unwrap_or(true);
		self.cx.reduce_motion = hello.reduce_motion.unwrap_or(false);
		self.cx.hour12 = hello.hour12;
		set_native_buttons(self.cx.feature("button"));
		set_native_pacing(self.cx.feature("pace"));
		self.saw_resize = false;
	}

	fn open_surface(&mut self, id: &str, mode: &str, role: &str, adopt: bool) {
		self.write_open(id, mode, "omp", role, adopt);
	}

	fn write_open(&mut self, id: &str, mode: &str, title: &str, role: &str, adopt: bool) {
		let mut body = serde_json::Map::new();
		body.insert("id".to_string(), Value::String(id.to_string()));
		body.insert("mode".to_string(), Value::String(mode.to_string()));
		body.insert("title".to_string(), Value::String(title.to_string()));
		body.insert("role".to_string(), Value::String(role.to_string()));
		if adopt {
			body.insert("adopt".to_string(), Value::Bool(true));
		}
		self.write_json(Verb::Open, Value::Object(body));
	}

	fn write_close(&mut self, id: &str, keep: bool) {
		let mut body = serde_json::Map::new();
		body.insert("id".to_string(), Value::String(id.to_string()));
		body.insert("keep".to_string(), Value::Bool(keep));
		self.write_json(Verb::Close, Value::Object(body));
	}

	fn has_credit(credits: u32, now: u64, surface: &mut Surface<C>) -> bool {
		if (surface.unacked.len() as u32) < credits {
			return true;
		}
		let Some(oldest) = surface.unacked.first().copied() else {
			return true;
		};
		if now.saturating_sub(oldest) < STALLED_ACK_MS {
			return false;
		}
		if surface.stalled_since.is_none() {
			surface.stalled_since = Some(oldest);
		}
		surface.stall_resets += 1;
		surface.unacked.clear();
		surface.acked = surface.seq;
		true
	}

	fn send_frame(&mut self, surface_id: &str, ops: Vec<FrameOp>) {
		let screen_active = self.screen.as_ref().is_some_and(|s| s.id == surface_id);
		let (seq, wire_ops) = {
			let surface = if screen_active {
				self.screen.as_mut().unwrap()
			} else {
				&mut self.inline
			};
			surface.seq += 1;
			surface.unacked.push(self.scheduler.now());
			let wire_ops: Vec<Value> = ops.iter().map(FrameOp::to_value).collect();
			let doc_ops = ops.clone();
			if let Some(doc) = &mut surface.doc {
				let _ = doc.apply_frame(&Frame {
					s: surface.seq,
					ops: doc_ops,
				});
			}
			(surface.seq, wire_ops)
		};
		self.recent.push((surface_id.to_string(), seq, wire_ops.clone()));
		if self.recent.len() > RECENT_FRAMES {
			let extra = self.recent.len() - RECENT_FRAMES;
			self.recent.drain(0..extra);
		}
		let mut body = serde_json::Map::new();
		body.insert("sf".to_string(), Value::String(surface_id.to_string()));
		body.insert("s".to_string(), Value::Number(seq.into()));
		body.insert("ops".to_string(), Value::Array(wire_ops));
		self.write_json(Verb::Frame, Value::Object(body));
	}

	fn upload_blobs(&mut self, ops: &[FrameOp]) {
		let mut ids = HashSet::new();
		for op in ops {
			collectBlobs(op, &mut ids);
		}
		for id in ids {
			self.blobs.entry(id).or_insert(BlobState::Tracked);
		}
	}

	fn on_blobs_reply(&mut self, have: &[String]) {
		if let Some(query) = self.blob_queries.first_mut() {
			query.timer_live = false;
		}
		if !self.blob_queries.is_empty() {
			self.blob_queries.remove(0);
		}
		for id in have {
			self.blobs.insert(id.clone(), BlobState::Tracked);
		}
	}

	fn reset_blobs(&mut self) {
		self.blobs.clear();
		self.ask_held = true;
		for query in &mut self.blob_queries {
			query.timer_live = false;
		}
	}

	fn write_json(&mut self, verb: Verb, body: Value) {
		self.record("out", &verb.as_char().to_string(), None, &body);
		let message = encode_json(verb, &body, &TspParams::new());
		self.host.terminal().write(&message);
	}

	fn record(&mut self, dir: &str, verb: &str, params: Option<&TspParams>, body: &Value) {
		let Some(path) = self.record_path.clone() else {
			return;
		};
		let payload = if dir == "in" {
			match body {
				Value::String(text) => {
					serde_json::from_str::<Value>(text).unwrap_or_else(|_| body.clone())
				}
				_ => body.clone(),
			}
		} else {
			body.clone()
		};
		let mut entry = serde_json::Map::new();
		entry.insert("t".to_string(), Value::Number(self.scheduler.now().into()));
		entry.insert("dir".to_string(), Value::String(dir.to_string()));
		entry.insert("verb".to_string(), Value::String(verb.to_string()));
		if let Some(params) = params {
			let mut map = serde_json::Map::new();
			for (key, value) in params {
				map.insert(key.clone(), Value::String(value.clone()));
			}
			entry.insert("params".to_string(), Value::Object(map));
		}
		entry.insert("body".to_string(), payload);
		let line = format!("{}\n", Value::Object(entry));
		match OpenOptions::new().create(true).append(true).open(&path) {
			Ok(mut file) => {
				let _ = file.write_all(line.as_bytes());
			}
			Err(_) => {
				self.record_path = None;
			}
		}
	}

	fn handle_event(&mut self, event: &Event) {
		match event.ev() {
			"ack" => {
				let Some(sf) = event_sf(event) else { return; };
				let Some(s) = event.get_i64("s").map(|v| v.max(0) as u64) else { return; };
				let inline = self.inline.id == sf;
				let screen = self.screen.as_ref().is_some_and(|surface| surface.id == sf);
				if !inline && !screen {
					return;
				}
				let surface = if screen { self.screen.as_mut().unwrap() } else { &mut self.inline };
				if s <= surface.acked {
					return;
				}
				let newly = s.min(surface.seq).saturating_sub(surface.acked) as usize;
				surface.acked = s.min(surface.seq);
				let drain = newly.min(surface.unacked.len());
				surface.unacked.drain(0..drain);
				surface.stalled_since = None;
				surface.stall_resets = 0;
				if surface.dirty {
					self.host.requestRender();
				}
			}
			"resize" => {
				self.saw_resize = true;
				if let Some(cols) = event.get_i64("cols").and_then(|v| u16::try_from(v).ok()) {
					if cols != self.cx.cols {
						self.cx.cols = cols;
						self.host.requestRender();
					}
				}
			}
			"theme" => {
				if let Some(dark) = event.get_bool("dark") {
					self.cx.dark = dark;
					self.host.appearanceChanged(dark);
				}
			}
			"motion" => {
				if let Some(reduce) = event.get_bool("reduce") {
					self.cx.reduce_motion = reduce;
					self.host.motionChanged(reduce);
				}
			}
			"gone" => {
				let ids: Vec<String> = event
					.get("ids")
					.and_then(Value::as_array)
					.map(|items| {
						items
							.iter()
							.filter_map(Value::as_str)
							.map(str::to_string)
							.collect()
					})
					.unwrap_or_default();
				if ids.iter().any(|id| id == &self.inline.id) {
					self.reset_blobs();
					let id = format!("s:{}", self.next_surface);
					self.next_surface += 1;
					self.inline = Surface::new(id.clone(), "inline", SESSION_ROLE.to_string(), self.mirror);
					if self.live {
						self.open_surface(&id, "inline", SESSION_ROLE, false);
					}
					self.host.requestRender();
					return;
				}
				if let Some(sf) = event_sf(event) {
					self.forget(sf, &ids);
				}
			}
			"focus" => self.route_focus_event(event),
			"toggle" | "select" | "activate" | "action" | "change" | "edit" | "undo" | "send" => {
				self.route_ui_event(event);
			}
			"visible" | "error" => {}
			_ => {}
		}
	}

	fn route_focus_event(&mut self, event: &Event) {
		let Some(id) = event.get_str("id") else { return; };
		let Some(sf) = event_sf(event) else { return; };
		let owners = self.owners(sf, id);
		if owners.is_empty() {
			return;
		}
		let field = owners
			.iter()
			.rev()
			.find(|owner| self.focusTarget(sf, owner).as_deref() == Some(id))
			.cloned();
		let sheet = |_overlay: &C| false;
		self.host.focusFromPointer(owners, field, &sheet);
		self.host.requestRender();
	}

	fn route_ui_event(&mut self, event: &Event) {
		let Some(id) = event.get_str("id") else { return; };
		let Some(sf) = event_sf(event) else { return; };
		let Some(target) = self.target(sf, id) else {
			self.host.requestRender();
			return;
		};
		if event.ev() == "send" && self.focusTarget(sf, &target.component).as_deref() != Some(id) {
			return;
		}
		let map_item = |item_id: &str| -> String {
			if item_id.starts_with(&(id.to_string() + "/"))
				|| item_id.starts_with(&(id.to_string() + "."))
				|| !item_id.contains('.')
			{
				self.itemKey(sf, item_id)
			} else {
				item_id.to_string()
			}
		};
		let ui = match event.ev() {
			"toggle" => NativeUiEvent::Toggle {
				key: target.keypath.clone(),
				collapsed: event.get_bool("collapsed").unwrap_or(false),
			},
			"select" => NativeUiEvent::Select {
				key: target.keypath.clone(),
				item: map_item(event.get_str("item").unwrap_or_default()),
			},
			"activate" => NativeUiEvent::Activate {
				key: target.keypath.clone(),
				item: map_item(event.get_str("item").unwrap_or_default()),
			},
			"action" => NativeUiEvent::Action {
				key: target.keypath.clone(),
				act: event.get_str("act").unwrap_or_default().to_string(),
				value: event.get("value").cloned(),
				mods: event
					.get("mods")
					.and_then(Value::as_array)
					.map(|mods| mods.iter().filter_map(Value::as_str).map(str::to_string).collect())
					.unwrap_or_default(),
			},
			"change" => NativeUiEvent::Change {
				key: target.keypath.clone(),
				item: event.get_str("item").unwrap_or_default().to_string(),
				value: event.get("value").cloned().unwrap_or(Value::Null),
			},
			"edit" => NativeUiEvent::Edit {
				key: target.keypath.clone(),
				from: event.get_i64("from").unwrap_or(0).max(0) as usize,
				to: event.get_i64("to").unwrap_or(0).max(0) as usize,
				text: event.get_str("text").unwrap_or_default().to_string(),
				cursor: event.get_i64("cursor").unwrap_or(0).max(0) as usize,
				len: event.get_i64("len").unwrap_or(0).max(0) as usize,
			},
			"undo" => NativeUiEvent::Undo {
				key: target.keypath.clone(),
			},
			"send" => NativeUiEvent::Send {
				key: target.keypath.clone(),
				text: event.get_str("text").unwrap_or_default().to_string(),
			},
			_ => return,
		};
		target.component.handleNativeEvent(&ui);
		self.host.requestRender();
	}
}

fn empty_node() -> NativeNode {
	make_node(NodeKind::Col, Props::new(), Vec::new(), None)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::terminal::virt::VirtualTerminal;
	use std::cell::RefCell;
	use std::rc::Rc;

	#[derive(Clone, Debug, PartialEq, Eq, Hash)]
	struct Id(&'static str);

	#[derive(Clone, Default)]
	struct Harness {
		focus_calls: Rc<RefCell<Vec<(Vec<Id>, Option<Id>)>>>,
		events: Rc<RefCell<Vec<(Id, NativeUiEvent)>>>,
	}

	#[derive(Clone)]
	struct Comp {
		id: Id,
		node: NativeNode,
		harness: Harness,
		sheet: bool,
	}

	impl Comp {
		fn new(id: &'static str, node: NativeNode, harness: Harness) -> Self {
			Comp {
				id: Id(id),
				node,
				harness,
				sheet: false,
			}
		}
		fn editor(id: &'static str, harness: Harness) -> Self {
			let mut props = Props::new();
			props.set_text("hello");
			let node = NativeNode::with_props(NodeKind::Editor, props);
			Comp::new(id, node, harness)
		}
	}

	impl PartialEq for Comp {
		fn eq(&self, other: &Self) -> bool {
			self.id == other.id
		}
	}

	impl NativeComponent for Comp {
		fn describe(&self, _cx: &DescribeContext) -> Option<NativeNode> {
			Some(self.node.clone())
		}
		fn nativeSheet(&self, _cx: &DescribeContext) -> bool {
			self.sheet
		}
		fn handleNativeEvent(&self, event: &NativeUiEvent) {
			self.harness
				.events
				.borrow_mut()
				.push((self.id.clone(), event.clone()));
		}
	}

	struct TestHost {
		term: VirtualTerminal,
		regions: NativeRegions<Comp>,
		overlays: Vec<NativeOverlay<Comp>>,
		focused: Option<Comp>,
		harness: Harness,
	}

	impl TestHost {
		fn new(harness: Harness) -> Self {
			TestHost {
				term: VirtualTerminal::new(80, 24),
				regions: NativeRegions::default(),
				overlays: Vec::new(),
				focused: None,
				harness,
			}
		}
	}

	impl NativeHost<Comp> for TestHost {
		fn terminal(&mut self) -> &mut dyn Terminal {
			&mut self.term
		}
		fn describeSurface(&mut self, _cx: &DescribeContext) -> NativeRegions<Comp> {
			self.regions.clone()
		}
		fn overlays(&mut self) -> Vec<NativeOverlay<Comp>> {
			self.overlays.clone()
		}
		fn focused(&self) -> Option<Comp> {
			self.focused.clone()
		}
		fn focusFromPointer(
			&mut self,
			owners: Vec<Comp>,
			field: Option<Comp>,
			_sheet: &dyn Fn(&Comp) -> bool,
		) {
			self.harness.focus_calls.borrow_mut().push((
				owners.into_iter().map(|c| c.id).collect(),
				field.map(|c| c.id),
			));
		}
		fn requestRender(&mut self) {}
		fn appearanceChanged(&mut self, _dark: bool) {}
		fn motionChanged(&mut self, _reduce: bool) {}
		fn invalidate(&mut self) {}
	}

	fn base_hello() -> TspHello {
		Hello {
			v: TSP_VERSION as i64,
			term: "tern".to_string(),
			kinds: NodeKind::ALL.iter().map(|k| k.as_str().to_string()).collect(),
			features: vec!["blobs".to_string(), "button".to_string(), "scroll".to_string()],
			apc: Some(TSP_DEFAULT_APC_LIMIT as u32),
			credits: Some(TSP_DEFAULT_CREDITS),
			cols: Some(80),
			dark: Some(true),
			reduce_motion: Some(false),
			hour12: None,
			ver: None,
			extra: serde_json::Map::new(),
		}
	}

	#[test]
	fn assumed_hello_filters_unassumed_kinds_and_sets_defaults() {
		struct Term;
		impl Terminal for Term {
			fn start(&mut self) -> std::io::Result<()> { Ok(()) }
			fn stop(&mut self) -> std::io::Result<()> { Ok(()) }
			fn write(&mut self, _data: &str) {}
			fn event(&mut self, _timeout: std::time::Duration) -> Option<crate::terminal::TtyEvent> { None }
			fn columns(&self) -> u16 { 100 }
			fn rows(&self) -> u16 { 40 }
			fn move_by(&mut self, _lines: i32) {}
			fn hide_cursor(&mut self, _force: bool) {}
			fn show_cursor(&mut self, _force: bool) {}
			fn clear_line(&mut self) {}
			fn clear_from_cursor(&mut self) {}
			fn clear_screen(&mut self) {}
		}
		let hello = assumedTspHello(&Term);
		assert_eq!(hello.term.as_str(), "tern");
		assert!(hello.kinds.iter().all(|k| !UNASSUMED_KINDS.contains(&k.as_str())));
		assert_eq!(hello.cols, Some(100));
	}

	#[test]
	fn overlay_anchor_and_size_table() {
		let cases = vec![
			(OverlayOptions { anchor: None, width: None, fullscreen: false }, 80u16, ("center", "md")),
			(OverlayOptions { anchor: Some("top-left".into()), width: None, fullscreen: false }, 80, ("top", "md")),
			(OverlayOptions { anchor: Some("bottom-x".into()), width: None, fullscreen: false }, 80, ("bottom", "md")),
			(OverlayOptions { anchor: None, width: None, fullscreen: true }, 80, ("center", "full")),
			(OverlayOptions { anchor: None, width: Some(OverlayWidth::Percent("80%".into())), fullscreen: false }, 80, ("center", "lg")),
			(OverlayOptions { anchor: None, width: Some(OverlayWidth::Percent("50%".into())), fullscreen: false }, 80, ("center", "md")),
			(OverlayOptions { anchor: None, width: Some(OverlayWidth::Percent("20%".into())), fullscreen: false }, 80, ("center", "sm")),
		];
		for (options, cols, (anchor, size)) in cases {
			assert_eq!(overlayAnchor(Some(&options)), anchor);
			assert_eq!(overlaySize(Some(&options), cols), size);
		}
	}

	#[test]
	fn routing_tracks_component_target_owners_and_item_keys() {
		let harness = Harness::default();
		let editor = Comp::editor("ed", harness.clone());
		let mut host = TestHost::new(harness.clone());
		host.regions.main.push(NativeChild::Component(editor.clone()));
		let mut backend = NativeBackend::new(
			host,
			base_hello(),
			NativeBackendOptions { mirror: true, recordPath: None, stats: false },
		);
		backend.start();
		backend.render();
		let sf = "s:1";
		let owners = backend.owners(sf, "main.0");
		assert_eq!(owners.first().map(|c| c.id.0), Some("ed"));
		let target = backend.target(sf, "main.0.child/key").unwrap();
		assert_eq!(target.component.id.0, "ed");
		assert_eq!(target.keypath, vec!["child".to_string(), "key".to_string()]);
		assert_eq!(backend.itemKey(sf, "main.0.a%2Fb"), "a/b");
		assert_eq!(backend.itemKey(sf, "main.0.list.7"), "7");
	}

	#[test]
	fn pointer_focus_and_event_dispatch_route_to_component() {
		let harness = Harness::default();
		let editor = Comp::editor("ed", harness.clone());
		let mut host = TestHost::new(harness.clone());
		host.regions.main.push(NativeChild::Component(editor.clone()));
		let mut backend = NativeBackend::new(
			host,
			base_hello(),
			NativeBackendOptions { mirror: true, recordPath: None, stats: false },
		);
		backend.start();
		backend.render();
		let focus = "\x1b_tsp;e;{\"ev\":\"focus\",\"sf\":\"s:1\",\"id\":\"main.0\"}\x1b\\";
		assert!(backend.handleInput(focus));
		let calls = harness.focus_calls.borrow();
		assert_eq!(calls.len(), 1);
		assert_eq!(calls[0].0.first().map(|id| id.0), Some("ed"));
		assert_eq!(calls[0].1.as_ref().map(|id| id.0), Some("ed"));
		drop(calls);
		let evt = "\x1b_tsp;e;{\"ev\":\"select\",\"sf\":\"s:1\",\"id\":\"main.0\",\"item\":\"main.0.a%2Fb\"}\x1b\\";
		assert!(backend.handleInput(evt));
		let events = harness.events.borrow();
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].0.0, "ed");
		match &events[0].1 {
			NativeUiEvent::Select { key, item } => {
				assert!(key.is_empty());
				assert_eq!(item, "a/b");
			}
			other => panic!("unexpected {other:?}"),
		}
	}

	#[test]
	fn forget_records_gone_ids_and_detach_live_resets_inline_routes() {
		let harness = Harness::default();
		let editor = Comp::editor("ed", harness.clone());
		let mut host = TestHost::new(harness.clone());
		host.regions.main.push(NativeChild::Component(editor.clone()));
		let mut backend = NativeBackend::new(
			host,
			base_hello(),
			NativeBackendOptions { mirror: true, recordPath: None, stats: false },
		);
		backend.start();
		backend.render();
		backend.forget("s:1", &["main.0".to_string()]);
		assert!(backend.inline.routes.gone.contains("main.0"));
		backend.detachLive();
		assert!(backend.inline.focus.is_none());
		assert!(backend.inline.routes.boundaries.is_empty());
	}
}
