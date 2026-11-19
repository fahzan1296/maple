//! Transcript container: owns transcript order, live capacity, and ordered
//! immutable retirement.
//!
//! Ported from `l/src/chrome/transcript-container.ts`. The reference is a
//! `Container` subclass holding `Component[]` and duck-typing a set of optional
//! hooks (`isTranscriptBlockFinalized`, `isTranscriptBlockCompact`,
//! `transcriptBlockMode`, `setTranscriptAllocation`, `nativeTrailers`, …) on
//! plain objects. Rust has no trait upcasting from `dyn Component` and no
//! structural typing, so those hooks are declared once as the object-safe
//! [`TranscriptChild`] contract and every stored block implements it.
//!
//! Deviations forced by the Rust component model (behaviour is unchanged):
//! - [`TranscriptContainer`] owns its entries directly; the reference's public
//!   mutable `children` array, `#syncEntries`, `#syncedChildren` and
//!   `#entriesUnverified` have no equivalent and are omitted. Removal is
//!   index-based ([`TranscriptContainer::remove_block`]); component identity is
//!   available through [`TranscriptContainer::block_index`] and the stable
//!   `block_id` carried by [`TranscriptViewportSpan`].
//! - [`TranscriptChild`] is the storage contract. `add_child` wraps any plain
//!   [`Component`] (all hooks default to "no"/"mutable"); `add_block` accepts a
//!   block that implements the reference interfaces. A component implementing
//!   [`TranscriptPresentationTarget`], [`AppendOnlyTranscriptBlock`],
//!   [`CompactTranscriptBlock`] and [`FinalizableBlock`] automatically satisfies
//!   [`TranscriptChild`].
//! - `HistoryBatch` becomes [`TranscriptHistoryBatch`] with an explicit
//!   [`TranscriptBatchKind`] (the reference's `kind` is optional).
//! - `isUsageRowBlock` (a `WeakSet` registry in `l/src/overlays/usage-row.ts`)
//!   becomes the [`UsageRowBlock::is_usage_row`] hook plus the
//!   [`is_usage_row_block`] predicate; `isToolActivityComponent`
//!   (`l/src/chrome/tool-activity.ts`) becomes [`ToolActivityBlock`] plus
//!   [`is_tool_activity_component`].
//! - `measureRender`/`renderForScrollback` (`l/src/components/image.ts`) are
//!   passthroughs: the crate has no image budget, so [`MeasuredBlock::images`]
//!   is always `false` and the display-order re-render is never triggered by a
//!   measurement.
//! - `Bun.hash` becomes [`crate::render::render_utils::Hasher`], `Bun.stripANSI`
//!   becomes [`crate::ansi_scan::strip_ansi`], and `\p{L}\p{N}` becomes
//!   `char::is_alphanumeric`.
//! - `ownDescription`/`isNativeSettled`/`settleNative` become the
//!   [`TranscriptChild::describe_node`] hook plus a `rows` fallback; the
//!   per-component settle hint is omitted (the crate's settle memo lives in
//!   `native::state`). `nativeTrailers` yields `NativeNode`s here, so
//!   [`TranscriptContainer::native_blocks`] returns `Vec<NativeNode>`.
//! - `logger.warn` output is collected in [`TranscriptContainer::take_warnings`]
//!   so rendering stays side-effect free; `pushLoopPhase`/`popLoopPhase` are
//!   omitted. `Date.now()` becomes [`AnimationFrame::now`] and
//!   `performance.now()` becomes [`std::time::Instant`].
//! - `completeScrollback` is a private field with accessors.
//! - `FinalizableBlock` is module-private in the reference; it is public here
//!   because it is one of [`TranscriptChild`]'s hook supertraits.

use crate::ansi_scan::strip_ansi;
use crate::component::Component;
use crate::native::describe::col;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::native::same_items;
use crate::render::render_utils::Hasher;
use crate::terminal_capabilities::{terminal, ImageProtocol, RuntimeTerminal};
use std::collections::HashMap;
use std::time::Instant;

/// Maximum number of live (not yet committed) transcript blocks.
pub const MAX_LIVE_BLOCKS: usize = 256;
/// Grace before a pressure-blocked frontier is reported; a streaming block may
/// legitimately hold it briefly.
pub const PINNED_FRONTIER_WARN_MS: u64 = 30_000;
/// Wall-clock budget for composing one retirement batch, in milliseconds.
pub const RETIREMENT_BUDGET_MS: u64 = 8;

/// Shared animation time supplied by the constrained transcript root.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimationFrame {
	/// Monotonic frame counter.
	pub tick: u64,
	/// Wall-clock milliseconds for this frame.
	pub now: u64,
}

/// Lets an active block adapt its presentation to its allocated viewport rows.
///
/// The reference declares both methods optional; the Rust trait gives them
/// default no-op bodies so a block overrides only what it needs.
pub trait TranscriptPresentationTarget {
	/// Allocate `rows` viewport rows for this block during `frame`.
	fn set_transcript_allocation(&mut self, _rows: usize, _frame: AnimationFrame) {}
	/// Freeze whatever layout keeps finished rows changing (a table's column
	/// widths) so they can go to scrollback.
	fn lock_streaming_layout(&mut self) {}
}

/// Presentation declaration captured permanently when a block is added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptBlockMode {
	/// The block may still change in place.
	Mutable,
	/// The block only ever appends stable semantic rows.
	AppendOnly,
}

/// Immutable width-independent identity for one stable semantic row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptStableRow {
	/// Width-independent identity of the row.
	pub key: String,
}

/// Explicit semantic-row contract for a block whose stable head may enter
/// native history before finalization.
pub trait AppendOnlyTranscriptBlock {
	/// The captured presentation mode; [`TranscriptBlockMode::AppendOnly`]
	/// opts into stable-row publication.
	fn transcript_block_mode(&self) -> TranscriptBlockMode {
		TranscriptBlockMode::Mutable
	}
	/// Every published stable row, in order. Each call must extend the prior
	/// keys exactly.
	fn get_transcript_stable_rows(&self) -> Vec<TranscriptStableRow> {
		Vec::new()
	}
	/// Render the first `count` semantic rows at `width`; the result must be a
	/// byte prefix of the block's full render at the same width.
	fn render_transcript_stable_rows(&mut self, _count: usize, _width: u16) -> Vec<String> {
		Vec::new()
	}
	/// Discard every published stable row. Optional in the reference; a no-op
	/// default is provided.
	fn reset_transcript_stable_rows(&mut self) {}
}

/// Lets a block pack against its neighbors.
pub trait CompactTranscriptBlock {
	/// Whether the block's current presentation is compact rows.
	fn is_transcript_block_compact(&self) -> bool {
		false
	}
}

/// Finalization hooks. Module-private in the reference; public here because it
/// is a [`TranscriptChild`] supertrait.
pub trait FinalizableBlock {
	/// Whether the block has finished; `None` means "no opinion" (finalized).
	fn is_transcript_block_finalized(&self) -> Option<bool> {
		None
	}
	/// Whether a finalized block still waits on async work.
	fn is_transcript_block_pending(&self) -> bool {
		false
	}
	/// Render the row that must remain represented under emergency viewport
	/// pressure.
	fn render_transcript_block_emergency_row(&self, _width: u16) -> Option<String> {
		None
	}
}

/// Ported from `l/src/chrome/tool-activity.ts` (`ToolActivityComponent`).
pub trait ToolActivityBlock {
	/// Whether this block presents dynamic tool activity.
	fn is_tool_activity(&self) -> bool {
		false
	}
	/// Forward the tool-activity visibility toggle.
	fn set_tool_activity_visible(&mut self, _visible: bool) {}
}

/// Ported from `l/src/overlays/usage-row.ts` (`isUsageRowBlock`).
pub trait UsageRowBlock {
	/// Whether this block is a per-turn usage/metrics row.
	fn is_usage_row(&self) -> bool {
		false
	}
}

/// The storage contract for one transcript block.
///
/// Implemented automatically by every `Component` that also implements
/// [`TranscriptPresentationTarget`], [`AppendOnlyTranscriptBlock`],
/// [`CompactTranscriptBlock`], [`FinalizableBlock`], [`ToolActivityBlock`] and
/// [`UsageRowBlock`].
pub trait TranscriptChild:
	Component
	+ TranscriptPresentationTarget
	+ AppendOnlyTranscriptBlock
	+ CompactTranscriptBlock
	+ FinalizableBlock
	+ ToolActivityBlock
	+ UsageRowBlock
{
	/// This block's own native description, or `None` for a `rows` fallback.
	fn describe_node(&self, _cx: &DescribeContext) -> Option<NativeNode> {
		None
	}
	/// Drop memoized render output after the block retires. The reference's
	/// failure `WeakSet` is unnecessary: the hook is infallible.
	fn release_render_caches(&mut self) {}
	/// Native nodes drawn right after this block as siblings.
	fn native_trailers(&self) -> Vec<NativeNode> {
		Vec::new()
	}
}

impl<T> TranscriptChild for T where
	T: Component
		+ TranscriptPresentationTarget
		+ AppendOnlyTranscriptBlock
		+ CompactTranscriptBlock
		+ FinalizableBlock
		+ ToolActivityBlock
		+ UsageRowBlock
{
}

/// Whether `component` is a per-turn usage/metrics row.
///
/// Ported from `l/src/overlays/usage-row.ts` `isUsageRowBlock`, whose `WeakSet`
/// registry has no crate equivalent; the trait query is the Rust form.
pub fn is_usage_row_block(component: &dyn TranscriptChild) -> bool {
	component.is_usage_row()
}

/// Whether `component` presents dynamic tool activity.
///
/// Ported from `l/src/chrome/tool-activity.ts` `isToolActivityComponent`.
pub fn is_tool_activity_component(component: &dyn TranscriptChild) -> bool {
	component.is_tool_activity()
}

/// Wrapper giving a plain [`Component`] the default transcript hooks, so a
/// caller can add any component without implementing the hook traits.
struct PlainBlock<C: Component> {
	inner: C,
}

impl<C: Component> PlainBlock<C> {
	fn new(inner: C) -> Self {
		PlainBlock { inner }
	}
}

impl<C: Component> Component for PlainBlock<C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.inner.render(width)
	}
	fn invalidate(&mut self) {
		self.inner.invalidate();
	}
	fn dispose(&mut self) {
		self.inner.dispose();
	}
}

impl<C: Component> TranscriptPresentationTarget for PlainBlock<C> {}
impl<C: Component> AppendOnlyTranscriptBlock for PlainBlock<C> {}
impl<C: Component> CompactTranscriptBlock for PlainBlock<C> {}
impl<C: Component> FinalizableBlock for PlainBlock<C> {}
impl<C: Component> ToolActivityBlock for PlainBlock<C> {}
impl<C: Component> UsageRowBlock for PlainBlock<C> {}

/// Groups sibling rows into one conservative mutable semantic transcript block.
///
/// Ported from the reference's `export class TranscriptBlock extends Container {}`.
#[derive(Default)]
pub struct TranscriptBlock {
	children: Vec<Box<dyn TranscriptChild>>,
}

impl TranscriptBlock {
	/// An empty block.
	pub fn new() -> Self {
		TranscriptBlock { children: Vec::new() }
	}
	/// Append a block that implements the transcript hook contract.
	pub fn add_block<B: TranscriptChild + 'static>(&mut self, block: B) -> &mut Self {
		self.children.push(Box::new(block));
		self
	}
	/// Append any component; it gets the default (mutable) transcript hooks.
	pub fn add_row<C: Component + 'static>(&mut self, row: C) -> &mut Self {
		self.add_block(PlainBlock::new(row));
		self
	}
	/// The grouped children.
	pub fn children(&self) -> &[Box<dyn TranscriptChild>] {
		&self.children
	}
	/// Remove and return the child at `index`, if present.
	pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn TranscriptChild>> {
		if index < self.children.len() {
			Some(self.children.remove(index))
		} else {
			None
		}
	}
	/// Drop every grouped child.
	pub fn clear(&mut self) {
		self.children.clear();
	}
}

impl Component for TranscriptBlock {
	fn render(&mut self, width: u16) -> Vec<String> {
		let mut rows = Vec::new();
		for child in &mut self.children {
			rows.extend(child.render(width));
		}
		rows
	}
	fn invalidate(&mut self) {
		for child in &mut self.children {
			child.invalidate();
		}
	}
	fn dispose(&mut self) {
		for child in &mut self.children {
			child.dispose();
		}
	}
}

impl TranscriptPresentationTarget for TranscriptBlock {}
impl AppendOnlyTranscriptBlock for TranscriptBlock {}
impl CompactTranscriptBlock for TranscriptBlock {}
impl FinalizableBlock for TranscriptBlock {}
impl ToolActivityBlock for TranscriptBlock {}
impl UsageRowBlock for TranscriptBlock {}

/// One live block's row span in the last [`TranscriptContainer::render_viewport`]
/// output (half-open `[start, end)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranscriptViewportSpan {
	/// Stable identity of the block; see [`TranscriptContainer::block_index`].
	pub block_id: u64,
	/// First output row owned by the block.
	pub start: usize,
	/// One past the last output row owned by the block.
	pub end: usize,
}

/// Kind of a history batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptBatchKind {
	/// Appends finalized or naturally emitted rows.
	Append,
	/// The complete logical ledger.
	Replay,
}

/// Immutable append or complete replay offered until the terminal accepts its
/// identifier. Ported from `l/src/tui.ts` `HistoryBatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptHistoryBatch {
	/// Offer identifier acknowledged by
	/// [`TranscriptContainer::acknowledge_finalized_batch`].
	pub id: u64,
	/// Rows the batch carries.
	pub rows: Vec<String>,
	/// Whether this is an append or a complete replay.
	pub kind: TranscriptBatchKind,
}

/// Block lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockState {
	/// Still mutating; renders live and counts against tool admission.
	Active,
	/// Finalized but retained in the mutable viewport until pressure.
	Settled,
	/// Logically retired; replay never rewinds this state.
	Committed,
}

/// Compactness of the last non-empty block above when its separator is still
/// owed; `None` when none is (nothing above, or rows ending in a blank).
type RowsAbove = Option<bool>;

/// A committed physical row prefix of one block.
#[derive(Clone, Debug)]
struct RowSplit {
	rows: usize,
	width: u16,
	hash: u64,
	text: String,
	images: Vec<ImageSeam>,
	located: Option<LocatedSplit>,
}

/// An inline image in the committed rows, with the length of `text` before it.
#[derive(Clone, Debug)]
struct ImageSeam {
	payload: String,
	offset: usize,
}

/// Seam of the last render a split was located in.
#[derive(Clone, Debug)]
struct LocatedSplit {
	rendered: Vec<String>,
	count: usize,
}

/// A block's full-allocation render, plus whether the render observed a
/// budgeted image (always `false` here; see the module doc).
#[derive(Clone, Debug)]
struct MeasuredBlock {
	rows: Vec<String>,
	images: bool,
}

/// Rows a progressive-append retirement offers, and the stable count they bring
/// the head to.
#[derive(Clone, Debug)]
struct AppendBatch {
	rows: Vec<String>,
	emitted_end: usize,
}

/// The unacknowledged offer.
#[derive(Clone, Debug)]
enum Offered {
	/// Stable-head emission for one block.
	Append {
		batch: TranscriptHistoryBatch,
		entry: usize,
		emitted_end: usize,
		lead: bool,
	},
	/// Whole blocks `[frontier, end)`, then optionally a split head or a
	/// streaming block's stable rows.
	Commit {
		batch: TranscriptHistoryBatch,
		end: usize,
		split: Option<RowSplit>,
		head_emitted_end: Option<usize>,
		history_end: RowsAbove,
	},
	/// A prepared complete replay.
	Replay {
		batch: TranscriptHistoryBatch,
		history_end: RowsAbove,
	},
}

impl Offered {
	fn batch_id(&self) -> u64 {
		match self {
			Offered::Append { batch, .. }
			| Offered::Commit { batch, .. }
			| Offered::Replay { batch, .. } => batch.id,
		}
	}

	fn batch_clone(&self) -> TranscriptHistoryBatch {
		match self {
			Offered::Append { batch, .. }
			| Offered::Commit { batch, .. }
			| Offered::Replay { batch, .. } => batch.clone(),
		}
	}
}

/// Watchdog state for a pressure-blocked frontier.
#[derive(Clone, Copy, Debug)]
struct PinnedFrontier {
	index: usize,
	since: u64,
	logged: bool,
}

/// Result of [`TranscriptContainer::split_offer`].
#[derive(Clone, Debug)]
enum SplitOffer {
	Rows {
		rows: Vec<String>,
		split: RowSplit,
	},
	Whole,
}

/// Result of [`TranscriptContainer::render_range`].
#[derive(Clone, Debug)]
struct RangeResult {
	rows: Vec<String>,
	end: usize,
	above: RowsAbove,
}

/// Which prefix a [`TranscriptContainer::peek_batch`] call may retire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetirementPolicy {
	Pressure,
	Flush,
}

/// One transcript block's ledger entry.
struct Entry {
	id: u64,
	component: Box<dyn TranscriptChild>,
	state: BlockState,
	mode: TranscriptBlockMode,
	stable_rows: Vec<TranscriptStableRow>,
	rendered_stable_by_width: HashMap<u16, Vec<String>>,
	stable_row_count_by_width: HashMap<u16, HashMap<usize, usize>>,
	emitted: usize,
	stable_frozen: bool,
	split: Option<RowSplit>,
}

impl Entry {
	fn new(id: u64, component: Box<dyn TranscriptChild>, mode: TranscriptBlockMode) -> Self {
		Entry {
			id,
			component,
			state: BlockState::Active,
			mode,
			stable_rows: Vec::new(),
			rendered_stable_by_width: HashMap::new(),
			stable_row_count_by_width: HashMap::new(),
			emitted: 0,
			stable_frozen: false,
			split: None,
		}
	}
}

/// Owns transcript order, live capacity, and ordered immutable retirement.
pub struct TranscriptContainer {
	entries: Vec<Entry>,
	frontier: usize,
	next_batch_id: u64,
	next_entry_id: u64,
	offered: Option<Offered>,
	replay_pending: bool,
	replay_requested: bool,
	tool_activity_visible: bool,
	last_frame: AnimationFrame,
	/// Rendered row where a block begins in the last full render.
	child_start_rows: HashMap<u64, usize>,
	pinned_frontier: Option<PinnedFrontier>,
	last_viewport_spans: Vec<TranscriptViewportSpan>,
open_frame: Option<AnimationFrame>,
	frame_rows: HashMap<u64, MeasuredBlock>,
	frame_rows_width: u16,
	native_blocks_cache: Vec<NativeNode>,
	native_node: Option<NativeNode>,
	warnings: Vec<String>,
	complete_scrollback: bool,
}

impl Default for TranscriptContainer {
	fn default() -> Self {
		Self::new()
	}
}

impl TranscriptContainer {
	/// An empty container.
	pub fn new() -> Self {
		TranscriptContainer {
			entries: Vec::new(),
			frontier: 0,
			next_batch_id: 1,
			next_entry_id: 1,
			offered: None,
			replay_pending: false,
			replay_requested: false,
			tool_activity_visible: true,
			last_frame: AnimationFrame::default(),
			child_start_rows: HashMap::new(),
			pinned_frontier: None,
			last_viewport_spans: Vec::new(),
			open_frame: None,
			frame_rows: HashMap::new(),
			frame_rows_width: 0,
			native_blocks_cache: Vec::new(),
			native_node: None,
			warnings: Vec::new(),
			complete_scrollback: false,
		}
	}

	/// Append any component as a transcript block with the default hooks.
	pub fn add_child<C: Component + 'static>(&mut self, child: C) {
		self.add_block(PlainBlock::new(child));
	}

	/// Append a block implementing the transcript hook contract.
	pub fn add_block<B: TranscriptChild + 'static>(&mut self, mut block: B) {
		let mode = block.transcript_block_mode();
		if block.is_tool_activity() {
			block.set_tool_activity_visible(self.tool_activity_visible);
		}
		let id = self.next_entry_id;
		self.next_entry_id += 1;
		self.entries.push(Entry::new(id, Box::new(block), mode));
	}

	/// Append any component at `index`, keeping transcript order.
	pub fn insert_child_at<C: Component + 'static>(&mut self, index: usize, child: C) {
		self.insert_block_at(index, PlainBlock::new(child));
	}

	/// Insert a hook-capable block at `index`, keeping transcript order.
	pub fn insert_block_at<B: TranscriptChild + 'static>(&mut self, index: usize, block: B) {
		self.add_block(block);
		let from = self.entries.len() - 1;
		if index >= from {
			return;
		}
		let moved = self.entries.remove(from);
		self.entries.insert(index, moved);
	}

	/// Remove the block at `index` when [`TranscriptContainer::can_remove_block`]
	/// allows it. Returns whether it was removed.
	///
	/// Deviation: the reference's `removeChild(component)` takes the component;
	/// Rust cannot hold a borrow into the container across a `&mut self` call,
	/// so removal is index-based and [`TranscriptContainer::block_index`] maps a
	/// component reference to its index.
	pub fn remove_block(&mut self, index: usize) -> bool {
		if index >= self.entries.len() || self.removable_index(index).is_none() {
			return false;
		}
		let removed = self.entries.remove(index);
		self.frontier = self.frontier.min(self.entries.len());
		self.child_start_rows.remove(&removed.id);
		self.frame_rows.remove(&removed.id);
		true
	}

	/// Drop every block and reset the retirement ledger.
	pub fn clear(&mut self) {
		self.entries.clear();
		self.frontier = 0;
		self.offered = None;
		self.child_start_rows.clear();
		self.pinned_frontier = None;
		self.replay_pending = false;
		self.replay_requested = false;
		self.last_viewport_spans = Vec::new();
		self.history_end_reset();
	}

	/// Reset the projected history tail after a full clear.
	fn history_end_reset(&mut self) {
		// `history_end` lives in `offered`; a cleared container has none.
		self.set_offered_history_end(None);
	}

	/// Update the pending offer's projected history tail.
	fn set_offered_history_end(&mut self, value: RowsAbove) {
		if let Some(offered) = &mut self.offered {
			match offered {
				Offered::Commit { history_end, .. } | Offered::Replay { history_end, .. } => {
					*history_end = value;
				}
				Offered::Append { .. } => {}
			}
		}
	}

	/// Forward the tool-activity visibility toggle to every tool-activity block.
	pub fn set_tool_activity_visible(&mut self, visible: bool) {
		if self.tool_activity_visible == visible {
			return;
		}
		self.tool_activity_visible = visible;
		for index in 0..self.entries.len() {
			if self.entries[index].component.is_tool_activity() {
				self.entries[index].component.set_tool_activity_visible(visible);
			}
		}
		self.invalidate();
	}

	/// Whether a streaming block sends stable rows that no longer fit to
	/// scrollback as soon as they are stable.
	pub fn complete_scrollback(&self) -> bool {
		self.complete_scrollback
	}

	/// Set [`Self::complete_scrollback`].
	pub fn set_complete_scrollback(&mut self, enabled: bool) {
		self.complete_scrollback = enabled;
	}

	/// The index of `block`, if it is a child of this container.
	pub fn block_index(&self, block: &dyn TranscriptChild) -> Option<usize> {
		self.entries
			.iter()
			.rposition(|entry| std::ptr::eq(&*entry.component, block))
	}

	/// Forget the append-only emission ledger for every block and ask each
	/// append-only block to drop its own published rows.
	///
	/// Callers must pair this with a scrollback-clearing display reset.
	pub fn reset_stable_emission(&mut self) {
		let clear_offer = match &self.offered {
			Some(Offered::Append { .. }) => true,
			Some(Offered::Commit { split, head_emitted_end, .. }) => {
				split.is_some() || head_emitted_end.is_some()
			}
			_ => false,
		};
		if clear_offer {
			self.offered = None;
		}
		for index in 0..self.entries.len() {
			{
				let entry = &mut self.entries[index];
				entry.emitted = 0;
				entry.stable_rows.clear();
				entry.rendered_stable_by_width.clear();
				entry.stable_row_count_by_width.clear();
				entry.stable_frozen = false;
				entry.split = None;
			}
			if self.entries[index].mode == TranscriptBlockMode::AppendOnly {
				self.entries[index].component.reset_transcript_stable_rows();
			}
		}
		self.set_offered_history_end(None);
	}

	/// Whether a transient block may be discarded without leaving tape history.
	pub fn can_remove_block(&self, block: &dyn TranscriptChild) -> bool {
		match self.block_index(block) {
			Some(index) => self.removable_index(index).is_some(),
			None => false,
		}
	}

	/// Whether a superseded snapshot block may be replaced by its successor.
	/// A block directly above a usage row stays: that row describes the turn the
	/// block belongs to.
	pub fn can_displace_block(&self, block: &dyn TranscriptChild) -> bool {
		let Some(index) = self.block_index(block) else {
			return false;
		};
		if self.removable_index(index).is_none() {
			return false;
		}
		let next = index + 1;
		next >= self.entries.len() || !self.entries[next].component.is_usage_row()
	}

	/// Lifecycle state per block in transcript order (diagnostics and tests).
	pub fn block_states(&self) -> Vec<BlockState> {
		self.entries.iter().map(|entry| entry.state).collect()
	}

	/// Permanently captured presentation mode per block (diagnostics and tests).
	pub fn block_modes(&self) -> Vec<TranscriptBlockMode> {
		self.entries.iter().map(|entry| entry.mode).collect()
	}

	/// Emitted stable semantic-row counts in transcript order.
	pub fn emitted_stable_rows(&self) -> Vec<usize> {
		self.entries.iter().map(|entry| entry.emitted).collect()
	}

	/// Whether any block is still streaming or otherwise unfinalized.
	pub fn has_active_block(&mut self) -> bool {
		self.settle_finalized();
		for index in self.frontier..self.entries.len() {
			if self.entries[index].state == BlockState::Active {
				return true;
			}
		}
		false
	}

	/// Whether visible active capacity and live-block memory permit another
	/// admission.
	pub fn can_admit(&self, rows: usize) -> bool {
		let active = self.entries.iter().filter(|entry| entry.state == BlockState::Active).count();
		rows > active && self.live_count() < MAX_LIVE_BLOCKS
	}

	/// Prepares one atomic replay of the committed ledger and an emitted
	/// active-head prefix.
	pub fn begin_replay(&mut self) {
		if self.offered.is_some() {
			self.replay_requested = true;
			return;
		}
		self.start_replay();
	}

	/// Drop a not-yet-offered replay so a shutdown flush emits only un-retired
	/// rows.
	pub fn cancel_replay(&mut self) {
		self.replay_pending = false;
		self.replay_requested = false;
	}

	/// Open one composed frame: every live-block measurement until this frame's
	/// [`Self::render_viewport`] returns renders against `frame` and is taken
	/// once.
	pub fn begin_frame(&mut self, frame: AnimationFrame) {
		self.last_frame = frame;
		self.frame_rows.clear();
		self.open_frame = Some(frame);
	}

	/// Total rows the live, un-emitted tail occupies at `width`.
	///
	/// `limit` stops the walk once the total passes it; past `limit` the result
	/// is a lower bound. Pass `usize::MAX` for the reference's `Infinity`.
	pub fn live_row_count(&mut self, width: u16, limit: usize) -> usize {
		self.settle_finalized();
		let start = self.live_start();
		let mut total = 0usize;
		let mut above = self.projected_history_end();
		for index in start..self.entries.len() {
			let block = self.live_block(index, width);
			if block.rows.is_empty() {
				continue;
			}
			let compact = self.is_compact(index);
			total += separator_rows(above, compact) + block.rows.len();
			above = Some(compact);
			if total > limit {
				break;
			}
		}
		total
	}

	/// Block spans of the last [`Self::render_viewport`] output, in output
	/// coordinates. Empty when the tail is empty.
	pub fn get_last_viewport_spans(&self) -> &[TranscriptViewportSpan] {
		&self.last_viewport_spans
	}

	/// Render the live tail, constrained to the supplied transcript height.
	/// Closes the frame [`Self::begin_frame`] opened.
	pub fn render_viewport(&mut self, width: u16, rows: usize, frame: AnimationFrame) -> Vec<String> {
		if Some(frame) != self.open_frame {
			self.close_frame();
		}
		self.last_frame = frame;
		let output = self.compose_viewport(width, rows, frame);
		self.close_frame();
		output
	}

	/// Offers stable-head emission or the shortest finalized prefix needed
	/// under pressure.
	pub fn peek_finalized_batch(&mut self, width: u16, capacity: usize) -> Option<TranscriptHistoryBatch> {
		self.peek_batch(width, capacity, RetirementPolicy::Pressure)
	}

	/// Returns only a prepared complete replay, never a normal retirement
	/// offer.
	pub fn peek_replay_batch(&mut self, width: u16) -> Option<TranscriptHistoryBatch> {
		self.settle_finalized();
		self.peek_replay_batch_inner(width)
	}

	/// Offers the complete currently eligible prefix for graceful shutdown.
	pub fn peek_flush_batch(&mut self, width: u16) -> Option<TranscriptHistoryBatch> {
		self.peek_batch(width, 0, RetirementPolicy::Flush)
	}

	/// Recompose the unacknowledged batch so a discarded TUI frame can be
	/// rendered again.
	pub fn rerender_offered_batch(&mut self, width: u16) -> Option<TranscriptHistoryBatch> {
		let offered = self.offered.clone()?;
		let mut updated = offered.clone();
		let rows: Vec<String>;
		match &offered {
			Offered::Append { entry, emitted_end, lead, .. } => {
				let entry_index = *entry;
				let emitted = match self.entries.get(entry_index) {
					Some(entry) => entry.emitted,
					None => return None,
				};
				let before = self.render_stable_prefix(entry_index, emitted, width);
				let after = self.render_stable_prefix(entry_index, *emitted_end, width);
				let start = before.len().min(after.len());
				let mut added = after[start..].to_vec();
				rows = if *lead {
					let mut rows = Vec::with_capacity(added.len() + 1);
					rows.push(String::new());
					rows.append(&mut added);
					rows
				} else {
					added
				};
			}
			Offered::Commit { end, split, head_emitted_end, .. } => {
				let end = *end;
				let mut retirement = self.render_range(self.frontier, end, width, self.projected_history_end(), None);
				let above = retirement.above;
				let mut rows_local = std::mem::take(&mut retirement.rows);
				let head = if split.is_some() && end < self.entries.len() {
					Some(end)
				} else {
					None
				};
				if let (Some(head_index), Some(previous)) = (head, split.clone()) {
					let start = self.emitted_row_count(head_index, width);
					let scrollback = self.render_entry_index(head_index, width);
					let mut scratch = previous.clone();
					let cut = if width == previous.width {
						previous.rows
					} else {
						self.split_row_count(head_index, &mut scratch, width)
					};
					let end_row = cut.min(scrollback.len());
					let fresh = row_split(&scrollback, end_row, width);
					if let Offered::Commit { split: stored, .. } = &mut updated {
						*stored = Some(fresh);
					}
					if separator_rows(above, self.is_compact(head_index)) > 0 {
						rows_local.push(String::new());
					}
					let start_row = start.min(end_row);
					rows_local.extend_from_slice(&scrollback[start_row..end_row]);
				} else {
					let history_end = self.close_range(&mut rows_local, above, end, width);
					if let Offered::Commit { history_end: stored, .. } = &mut updated {
						*stored = history_end;
					}
				}
				if let Some(streaming_end) = head_emitted_end {
					let streaming_end = *streaming_end;
					if let Some(streaming) = self.entries.get(end) {
						let emitted = streaming.emitted;
						let before = self.render_stable_prefix(end, emitted, width);
						let after = self.render_stable_prefix(end, streaming_end, width);
						let start = before.len().min(after.len());
						if is_row_prefix(&before, &after) {
							rows_local.extend_from_slice(&after[start..]);
							if let Offered::Commit { history_end, .. } = &mut updated {
								*history_end = None;
							}
						} else if let Offered::Commit { head_emitted_end, .. } = &mut updated {
							*head_emitted_end = None;
						}
					}
				}
				rows = rows_local;
			}
			Offered::Replay { .. } => {
				let replay = self.render_replay(width);
				if let Offered::Replay { history_end, .. } = &mut updated {
					*history_end = replay.1;
				}
				rows = replay.0;
			}
		}
		let batch = TranscriptHistoryBatch {
			id: offered.batch_id(),
			rows,
			kind: match &offered {
				Offered::Append { batch, .. }
				| Offered::Commit { batch, .. }
				| Offered::Replay { batch, .. } => batch.kind,
			},
		};
		match &mut updated {
			Offered::Append { batch: stored, .. }
			| Offered::Commit { batch: stored, .. }
			| Offered::Replay { batch: stored, .. } => *stored = batch.clone(),
		}
		self.offered = Some(updated);
		Some(batch)
	}

	/// Acknowledges exactly the most recently offered append, commit, or replay
	/// transaction.
	pub fn acknowledge_finalized_batch(&mut self, id: u64) {
		let offered = match &self.offered {
			Some(offered) if offered.batch_id() == id => offered.clone(),
			_ => return,
		};
		match offered {
			Offered::Append { entry, emitted_end, .. } => {
				let emitted = match self.entries.get(entry) {
					Some(entry) => entry.emitted,
					None => return,
				};
				if entry != self.frontier || emitted_end <= emitted {
					return;
				}
				self.entries[entry].emitted = emitted_end;
				self.set_offered_history_end(None);
			}
			Offered::Commit { end, split, head_emitted_end, history_end: _, .. } => {
				for index in self.frontier..end {
					self.retire_entry(index);
				}
				self.frontier = end;
				if let Some(head) = self.entries.get_mut(end) {
					head.split = split;
				}
				if let Some(streaming_end) = head_emitted_end {
					if let Some(streaming) = self.entries.get_mut(end) {
						if streaming_end > streaming.emitted {
							streaming.emitted = streaming_end;
						}
					}
				}
			}
			Offered::Replay { history_end: _, .. } => {
				self.release_committed_render_caches();
			}
		}
		self.offered = None;
		if self.replay_requested {
			self.start_replay();
		}
	}

	/// Render only the trailing `max_rows` semantic rows, walking blocks
	/// bottom-up. Used by the transient resize-buffer repaint.
	pub fn render_tail(&mut self, width: u16, max_rows: usize) -> Vec<String> {
		if max_rows == 0 {
			return Vec::new();
		}
		let mut collected: Vec<Vec<String>> = Vec::new();
		let mut total = 0usize;
		let mut below: Option<bool> = None;
		for index in (0..self.entries.len()).rev() {
			self.set_allocation(index, usize::MAX, self.last_frame);
			let block = {
				let component = &mut self.entries[index].component;
				trim_blank_edges(&component.render(width))
			};
			if self.entries[index].state == BlockState::Committed {
				self.release_render_caches(index);
			}
			if block.is_empty() {
				continue;
			}
			let compact = self.is_compact(index);
			let mut group = Vec::new();
			if below.is_some() && separator_rows(below, compact) > 0 {
				group.push(String::new());
			}
			group.extend(block);
			total += group.len();
			collected.push(group);
			below = Some(compact);
			if total >= max_rows {
				break;
			}
		}
		let mut rows = Vec::new();
		for group in collected.into_iter().rev() {
			rows.extend(group);
		}
		if rows.len() > max_rows {
			rows.split_off(rows.len() - max_rows)
		} else {
			rows
		}
	}

	/// The transcript's blocks for a native (TSP) surface, one `main` child per
	/// block in transcript order, with each run of consecutive compact blocks
	/// folded into one gapless `omp.compact` column.
	///
	/// Deviation: the reference returns `Component[]` (members stay component
	/// children so they keep their wire id and native events); the crate's
	/// describe model has component children resolved to nodes before a tree
	/// reaches the reconciler, so this returns `Vec<NativeNode>`. The result is
	/// the same array while the blocks are unchanged.
	pub fn native_blocks(&mut self, cx: &DescribeContext) -> Vec<NativeNode> {
		let mut flat: Vec<NativeNode> = Vec::new();
		let mut owners: Vec<usize> = Vec::new();
		for index in 0..self.entries.len() {
			flat.push(self.describe_child_node(index, cx));
			owners.push(index);
			for trailer in self.entries[index].component.native_trailers() {
				flat.push(trailer);
				owners.push(index);
			}
		}
		let mut blocks: Vec<NativeNode> = Vec::new();
		let mut index = 0;
		while index < flat.len() {
			let mut last = index;
			if self.is_compact(owners[index]) {
				let mut probe = index + 1;
				while probe < flat.len() {
					if self.is_compact(owners[probe]) {
						last = probe;
					} else if !is_void_node(&flat[probe], 0) {
						break;
					}
					probe += 1;
				}
			}
			if last == index {
				blocks.push(flat[index].clone());
				index += 1;
				continue;
			}
			let mut props = Props::new();
			props.set_role("omp.compact");
			props.set_str("gap", "none");
			blocks.push(col(flat[index..=last].to_vec(), props));
			index = last + 1;
		}
		if same_items(&self.native_blocks_cache, &blocks) {
			return self.native_blocks_cache.clone();
		}
		self.native_blocks_cache = blocks.clone();
		self.native_node = None;
		blocks
	}

	/// The container's native description: a column of [`Self::native_blocks`].
	pub fn describe_transcript_node(&mut self, cx: &DescribeContext) -> NativeNode {
		let blocks = self.native_blocks(cx);
		if let Some(node) = &self.native_node {
			return node.clone();
		}
		let mut props = Props::new();
		props.set_role("omp.transcript");
		let node = col(blocks, props);
		self.native_node = Some(node.clone());
		node
	}

	/// Full semantic render used by exports and non-terminal commands.
	pub fn render_semantic(&mut self, width: u16) -> Vec<String> {
		self.child_start_rows.clear();
		let mut rows: Vec<String> = Vec::new();
		let mut above: RowsAbove = None;
		for index in 0..self.entries.len() {
			self.set_allocation(index, usize::MAX, self.last_frame);
			let block = self.render_entry_index(index, width);
			if self.entries[index].state == BlockState::Committed {
				self.release_render_caches(index);
			}
			if block.is_empty() {
				continue;
			}
			let compact = self.is_compact(index);
			if separator_rows(above, compact) > 0 {
				rows.push(String::new());
			}
			self.child_start_rows.insert(self.entries[index].id, rows.len());
			rows.extend(block);
			above = Some(compact);
		}
		rows
	}

	/// Rendered row where a child's block begins in the last full render.
	pub fn get_child_start_row(&self, child: &dyn TranscriptChild) -> Option<usize> {
		let index = self.block_index(child)?;
		let id = self.entries[index].id;
		self.child_start_rows.get(&id).copied()
	}

	/// Drain the collected warning lines.
	pub fn take_warnings(&mut self) -> Vec<String> {
		std::mem::take(&mut self.warnings)
	}

	/// The collected warning lines.
	pub fn warnings(&self) -> &[String] {
		&self.warnings
	}

	/// Record a warning instead of logging, keeping rendering side-effect free.
	fn warn(&mut self, message: String) {
		self.warnings.push(message);
	}

	/// Live indices from the retirement start to the end.
	fn live_entries(&self) -> Vec<usize> {
		let start = self.live_start();
		(start..self.entries.len()).collect()
	}

	/// First live index: the offered commit's end, else the frontier.
	fn live_start(&self) -> usize {
		match &self.offered {
			Some(Offered::Commit { end, .. }) => *end,
			_ => self.frontier,
		}
	}

	/// Number of live blocks.
	fn live_count(&self) -> usize {
		self.entries.len() - self.frontier
	}

	/// What native history ends with once the unacknowledged offer lands.
	fn projected_history_end(&self) -> RowsAbove {
		match &self.offered {
			None => None,
			Some(Offered::Append { .. }) => None,
			Some(Offered::Commit { history_end, .. }) => *history_end,
			Some(Offered::Replay { history_end, .. }) => *history_end,
		}
	}

	/// Index into `live` of the first non-empty block when history owes it a
	/// separator, else `None`.
	fn owed_lead(&mut self, live: &[usize], width: u16) -> Option<usize> {
		let history_end = self.projected_history_end();
		if history_end.is_none() {
			return None;
		}
		for (cursor, &index) in live.iter().enumerate() {
			if self.live_block(index, width).rows.is_empty() {
				continue;
			}
			return if separator_rows(history_end, self.is_compact(index)) > 0 {
				Some(cursor)
			} else {
				None
			};
		}
		None
	}

	/// One live block's un-emitted rows at `width`.
	fn live_block(&mut self, index: usize, width: u16) -> MeasuredBlock {
		let measured = self.measure_index(index, width);
		let emitted = self.projected_emitted_row_count(index, width);
		if emitted == 0 {
			return measured;
		}
		let rows = if emitted < measured.rows.len() {
			measured.rows[emitted..].to_vec()
		} else {
			Vec::new()
		};
		MeasuredBlock { rows, images: measured.images }
	}

	/// One block's blank-trimmed render at its full-height allocation, memoized
	/// inside an open frame.
	fn measure_index(&mut self, index: usize, width: u16) -> MeasuredBlock {
		if self.open_frame.is_none() {
			return self.measure_entry_index(index, width);
		}
		let id = self.entries[index].id;
		if self.frame_rows_width != width {
			self.frame_rows.clear();
			self.frame_rows_width = width;
		}
		if let Some(measured) = self.frame_rows.get(&id) {
			return measured.clone();
		}
		let measured = self.measure_entry_index(index, width);
		self.frame_rows.insert(id, measured.clone());
		measured
	}

	/// Measure one block outside display order.
	fn measure_entry_index(&mut self, index: usize, width: u16) -> MeasuredBlock {
		self.set_allocation(index, usize::MAX, self.last_frame);
		MeasuredBlock { rows: self.render_entry_index(index, width), images: false }
	}

	/// Close the open frame, discarding shared measurements.
	fn close_frame(&mut self) {
		self.open_frame = None;
		self.frame_rows.clear();
	}

	/// Render one block, validating and recording its append-only stable
	/// publication.
	fn render_entry_index(&mut self, index: usize, width: u16) -> Vec<String> {
		let (rendered, stable, state, mode, frozen, emitted) = {
			let entry = &mut self.entries[index];
			let rendered = trim_blank_edges(&entry.component.render(width));
			let stable = entry.component.get_transcript_stable_rows();
			(rendered, stable, entry.state, entry.mode, entry.stable_frozen, entry.emitted)
		};
		if state == BlockState::Committed || mode == TranscriptBlockMode::Mutable || frozen {
			return rendered;
		}
		if !is_stable_prefix(&self.entries[index].stable_rows, &stable) {
			return self.freeze_stable_rows(index, &rendered, "publication retracted the published prefix");
		}
		if emitted > stable.len() {
			return self.freeze_stable_rows(index, &rendered, "publication retracted emitted history");
		}
		let published: Vec<TranscriptStableRow> = if stable.len() > self.entries[index].stable_rows.len() {
			let mut published = self.entries[index].stable_rows.clone();
			published.extend_from_slice(&stable[self.entries[index].stable_rows.len()..]);
			published
		} else {
			self.entries[index].stable_rows.clone()
		};
		let stable_rendered = self.entries[index]
			.component
			.render_transcript_stable_rows(published.len(), width);
		if !is_row_prefix(&stable_rendered, &rendered) {
			return self.freeze_stable_rows(index, &rendered, "stable rows no longer render as a prefix of the block");
		}
		let prior_render = self.entries[index].rendered_stable_by_width.get(&width).cloned();
		if let Some(prior) = &prior_render {
			if !is_row_prefix(prior, &stable_rendered) {
				return self.freeze_stable_rows(index, &rendered, "stable rows changed within a width epoch");
			}
		}
		self.entries[index].stable_rows = published;
		if prior_render.as_ref().map_or(true, |prior| prior.len() != stable_rendered.len()) {
			self.entries[index]
				.rendered_stable_by_width
				.insert(width, stable_rendered.clone());
		}
		let count = self.entries[index].stable_rows.len();
		self.entries[index]
			.stable_row_count_by_width
			.entry(width)
			.or_default()
			.insert(count, stable_rendered.len());
		rendered
	}

	/// Demote a drifting append-only publication: keep the last good stable
	/// state and stop mid-stream emission for this block.
	fn freeze_stable_rows(&mut self, index: usize, rendered: &[String], reason: &str) -> Vec<String> {
		self.entries[index].stable_frozen = true;
		let emitted = self.entries[index].emitted;
		self.warn(format!(
			"Append-only transcript block frozen reason={reason} emitted={emitted}"
		));
		rendered.to_vec()
	}

	/// Render the first `count` stable semantic rows of one block.
	fn render_stable_prefix(&mut self, index: usize, count: usize, width: u16) -> Vec<String> {
		if count == 0 {
			return Vec::new();
		}
		let stable_len = self.entries[index].stable_rows.len();
		let count = count.min(stable_len);
		self.entries[index].component.render_transcript_stable_rows(count, width)
	}

	/// Rows of `entry` at `width` already in native history or in the
	/// unacknowledged offer.
	fn projected_emitted_row_count(&mut self, index: usize, width: u16) -> usize {
		let commit = match &self.offered {
			Some(Offered::Commit { end, split, head_emitted_end, .. }) if *end == index => {
				Some((split.is_some(), *head_emitted_end))
			}
			_ => None,
		};
		if let Some((has_split, head_emitted_end)) = commit {
			if has_split {
				return self.offered_split_row_count(index, width);
			}
			if let Some(head) = head_emitted_end {
				if self.entries[index].split.is_none() {
					return self.stable_prefix_row_count(index, head, width);
				}
			}
		}
		if let Some(Offered::Append { entry, emitted_end, .. }) = &self.offered {
			if *entry == index && self.entries[index].split.is_none() {
				return self.stable_prefix_row_count(index, *emitted_end, width);
			}
		}
		self.emitted_row_count(index, width)
	}

	/// Rows of `entry` at `width` already acknowledged into native history.
	fn emitted_row_count(&mut self, index: usize, width: u16) -> usize {
		if self.entries[index].split.is_some() {
			self.stored_split_row_count(index, width)
		} else {
			let emitted = self.entries[index].emitted;
			self.stable_prefix_row_count(index, emitted, width)
		}
	}

	/// Length-only variant of [`Self::render_stable_prefix`].
	fn stable_prefix_row_count(&mut self, index: usize, count: usize, width: u16) -> usize {
		if count == 0 {
			return 0;
		}
		let stable_len = self.entries[index].stable_rows.len();
		let key = count.min(stable_len);
		if let Some(memo) = self.entries[index]
			.stable_row_count_by_width
			.get(&width)
			.and_then(|counts| counts.get(&key))
		{
			return *memo;
		}
		self.render_stable_prefix(index, count, width).len()
	}

	/// Rows of `entry`'s current render at `width` that `split` committed.
	fn split_row_count(&mut self, index: usize, split: &mut RowSplit, width: u16) -> usize {
		let rendered = self.measure_index(index, width).rows;
		if let Some(located) = &split.located {
			if located.rendered == rendered {
				return located.count;
			}
		}
		let mut count = 0usize;
		if width == split.width
			&& rendered.len() >= split.rows
			&& hash_rows(&rendered[..split.rows]) == split.hash
		{
			count = split.rows;
		} else {
			let mut consumed = 0usize;
			let mut image = 0usize;
			let info = terminal();
			while count < rendered.len() && (consumed < split.text.len() || image < split.images.len()) {
				let row = &rendered[count];
				let content = content_of(std::slice::from_ref(row));
				if !content.is_empty() {
					if consumed > split.text.len()
						|| !split.text.is_char_boundary(consumed)
						|| !split.text[consumed..].starts_with(&content)
					{
						break;
					}
					consumed += content.len();
					count += 1;
					continue;
				}
				let mut probe = count;
				while probe < rendered.len()
					&& !info.is_image_line(&rendered[probe])
					&& content_of(std::slice::from_ref(&rendered[probe])).is_empty()
				{
					probe += 1;
				}
				let Some(image_row) = rendered.get(probe) else { break };
				let payload = iterm2_image_payload(image_row).unwrap_or_else(|| image_row.clone());
				let Some(seam) = split.images.get(image) else { break };
				if payload != seam.payload
					|| consumed != seam.offset
					|| probe - count != direct_placement_lift(image_row)
				{
					break;
				}
				count = probe + 1;
				image += 1;
			}
		}
		split.located = Some(LocatedSplit { rendered, count });
		count
	}

	/// [`Self::split_row_count`] for the entry's committed split.
	fn stored_split_row_count(&mut self, index: usize, width: u16) -> usize {
		let mut split = match self.entries[index].split.take() {
			Some(split) => split,
			None => return 0,
		};
		let count = self.split_row_count(index, &mut split, width);
		self.entries[index].split = Some(split);
		count
	}

	/// [`Self::split_row_count`] for the offered commit's split.
	fn offered_split_row_count(&mut self, index: usize, width: u16) -> usize {
		let mut split = match self.offered.as_ref() {
			Some(Offered::Commit { split: Some(split), .. }) => split.clone(),
			_ => return 0,
		};
		let count = self.split_row_count(index, &mut split, width);
		if let Some(Offered::Commit { split: stored, .. }) = &mut self.offered {
			*stored = Some(split);
		}
		count
	}

	/// Size a progressive-append batch from recorded stable renders, without
	/// rendering any prefix.
	fn measured_append_batch(&mut self, index: usize, width: u16, overflow: isize) -> Option<AppendBatch> {
		let (counts, newest, target, emitted) = {
			let entry = &self.entries[index];
			let counts = entry.stable_row_count_by_width.get(&width)?.clone();
			let newest = entry.rendered_stable_by_width.get(&width)?.clone();
			(counts, newest, entry.stable_rows.len(), entry.emitted)
		};
		if counts.get(&target) != Some(&newest.len()) {
			return None;
		}
		let start = if emitted == 0 {
			0
		} else {
			match counts.get(&emitted) {
				Some(value) => *value,
				None => return None,
			}
		};
		let mut emitted_end = emitted;
		let mut end = start;
		while emitted_end < target
			&& !(self.complete_scrollback && (end - start) as isize >= overflow)
		{
			let next = match counts.get(&(emitted_end + 1)) {
				Some(value) => *value,
				None => return None,
			};
			if next <= start {
				if emitted_end == emitted {
					self.freeze_stable_rows(index, &[], "semantic row render added no suffix");
				}
				break;
			}
			if !self.complete_scrollback && (next - start) as isize > overflow {
				break;
			}
			end = next;
			emitted_end += 1;
		}
		let rows = if emitted_end > emitted {
			newest[start.min(end)..end].to_vec()
		} else {
			Vec::new()
		};
		Some(AppendBatch { rows, emitted_end })
	}

	/// Size a progressive-append batch by rendering each further stable prefix.
	fn rendered_append_batch(&mut self, index: usize, width: u16, overflow: isize) -> AppendBatch {
		let emitted = self.entries[index].emitted;
		let before = self.render_stable_prefix(index, emitted, width);
		let target = self.entries[index].stable_rows.len();
		let mut emitted_end = emitted;
		let mut after = before.clone();
		while emitted_end < target
			&& !(self.complete_scrollback && (after.len() - before.len()) as isize >= overflow)
		{
			let next = self.render_stable_prefix(index, emitted_end + 1, width);
			if !is_row_prefix(&before, &next) || next.len() == before.len() {
				if emitted_end == emitted {
					self.freeze_stable_rows(index, &[], "semantic row render added no suffix");
				}
				break;
			}
			if !self.complete_scrollback && (next.len() - before.len()) as isize > overflow {
				break;
			}
			after = next;
			emitted_end += 1;
		}
		let rows = if emitted_end > emitted {
			after[before.len().min(after.len())..].to_vec()
		} else {
			Vec::new()
		};
		AppendBatch { rows, emitted_end }
	}

	/// Record that pressure retirement is blocked behind a not-yet-settled
	/// frontier block.
	fn note_pinned_frontier(&mut self) {
		let index = self.frontier;
		if self.entries.get(index).is_none() {
			return;
		}
		let now = self.last_frame.now;
		let should_log = match &self.pinned_frontier {
			Some(pinned) if pinned.index == index => {
				!pinned.logged && now.saturating_sub(pinned.since) >= PINNED_FRONTIER_WARN_MS
			}
			_ => false,
		};
		if should_log {
			if let Some(pinned) = &mut self.pinned_frontier {
				pinned.logged = true;
			}
			let (state, mode, live_blocks) = {
				let entry = &self.entries[index];
				(entry.state, entry.mode, self.live_count())
			};
			self.warn(format!(
				"Transcript retirement pinned by unfinalized frontier block state={state:?} mode={mode:?} live_blocks={live_blocks}"
			));
		} else if self
			.pinned_frontier
			.as_ref()
			.map_or(true, |pinned| pinned.index != index)
		{
			self.pinned_frontier = Some(PinnedFrontier { index, since: now, logged: false });
		}
	}

	/// Compose entries `[start, end)` as one ordered retirement payload.
	fn render_range(
		&mut self,
		start: usize,
		end: usize,
		width: u16,
		above: RowsAbove,
		budget_ms: Option<u64>,
	) -> RangeResult {
		let mut rows: Vec<String> = Vec::new();
		let started_at = budget_ms.map(|_| Instant::now());
		let mut reached = start;
		let mut above = above;
		for index in start..end {
			self.set_allocation(index, usize::MAX, self.last_frame);
			let rendered = if index == start {
				self.render_entry_index(index, width)
			} else {
				let component = &mut self.entries[index].component;
				trim_blank_edges(&component.render(width))
			};
			let emitted_rows = if index == start {
				self.emitted_row_count(index, width)
			} else {
				0
			};
			let block = if emitted_rows < rendered.len() {
				rendered[emitted_rows..].to_vec()
			} else {
				Vec::new()
			};
			reached = index + 1;
			if !block.is_empty() {
				let compact = self.is_compact(index);
				if separator_rows(above, compact) > 0 {
					rows.push(String::new());
				}
				rows.extend(block);
				above = Some(compact);
			}
			if let (Some(budget), Some(started_at)) = (budget_ms, started_at) {
				if started_at.elapsed().as_millis() as u64 >= budget {
					break;
				}
			}
		}
		RangeResult { rows, end: reached, above }
	}

	/// End retired `rows` with the separator their last block owes the first
	/// non-empty live block from `next`.
	fn close_range(&mut self, rows: &mut Vec<String>, above: RowsAbove, next: usize, width: u16) -> RowsAbove {
		let Some(above) = above else {
			return None;
		};
		if above {
			for index in next..self.entries.len() {
				if self.live_block(index, width).rows.is_empty() {
					continue;
				}
				if self.is_compact(index) {
					return Some(above);
				}
				rows.push(String::new());
				return None;
			}
			return Some(above);
		}
		rows.push(String::new());
		None
	}

	/// The committed ledger and the head rows already in history.
	fn render_replay(&mut self, width: u16) -> (Vec<String>, RowsAbove) {
		let range = self.render_range(0, self.frontier, width, None, None);
		let mut rows = range.rows;
		let above = range.above;
		let head = self.frontier;
		let mut head_rows: Vec<String> = Vec::new();
		let head_state = self.entries.get(head).map(|entry| (entry.mode, entry.emitted, entry.split.is_some()));
		if let Some((mode, emitted, has_split)) = head_state {
			if has_split {
				self.set_allocation(head, usize::MAX, self.last_frame);
				let committed = self.stored_split_row_count(head, width);
				head_rows = self.render_entry_index(head, width);
				head_rows.truncate(committed);
			} else if mode == TranscriptBlockMode::AppendOnly && emitted > 0 {
				self.set_allocation(head, usize::MAX, self.last_frame);
				self.render_entry_index(head, width);
				head_rows = self.render_stable_prefix(head, emitted, width);
			}
		}
		if head_rows.is_empty() {
			let history_end = self.close_range(&mut rows, above, self.frontier, width);
			return (rows, history_end);
		}
		if separator_rows(above, self.is_compact(head)) > 0 {
			rows.push(String::new());
		}
		rows.extend(head_rows);
		(rows, None)
	}

	/// Retire every settled head whose stable rows are fully emitted.
	fn complete_fully_emitted_heads(&mut self, width: u16) {
		while self.frontier < self.entries.len() {
			let index = self.frontier;
			if self.entries[index].state != BlockState::Settled {
				return;
			}
			if self.entries[index].split.is_some() {
				let count = self.stored_split_row_count(index, width);
				if count < self.measure_index(index, width).rows.len() {
					return;
				}
			} else {
				if self.entries[index].mode != TranscriptBlockMode::AppendOnly {
					return;
				}
				let rendered_len = self.measure_index(index, width).rows.len();
				if self.entries[index].emitted != self.entries[index].stable_rows.len() {
					return;
				}
				let emitted = self.entries[index].emitted;
				if self.render_stable_prefix(index, emitted, width).len() != rendered_len {
					return;
				}
			}
			self.retire_entry(index);
			self.frontier += 1;
		}
	}

	/// Mark one entry committed and drop its caches.
	fn retire_entry(&mut self, index: usize) {
		let id = self.entries[index].id;
		{
			let entry = &mut self.entries[index];
			entry.state = BlockState::Committed;
			entry.emitted = 0;
			entry.split = None;
			entry.stable_rows.clear();
			entry.rendered_stable_by_width.clear();
			entry.stable_row_count_by_width.clear();
		}
		self.frame_rows.remove(&id);
		self.release_render_caches(index);
	}

	/// Release the render caches of every committed entry.
	fn release_committed_render_caches(&mut self) {
		for index in 0..self.frontier {
			self.release_render_caches(index);
		}
	}

	/// Call the block's cache-release hook.
	fn release_render_caches(&mut self, index: usize) {
		self.entries[index].component.release_render_caches();
	}

	/// Prepare a replay when the committed ledger or an emitted head exists.
	fn start_replay(&mut self) {
		let head = self.frontier;
		self.replay_pending = self.frontier > 0
			|| self.entries.get(head).map_or(false, |entry| entry.split.is_some())
			|| self
				.entries
				.get(head)
				.map_or(false, |entry| entry.mode == TranscriptBlockMode::AppendOnly && entry.emitted > 0);
		self.replay_requested = false;
	}

	/// Return a prepared replay batch, if one is pending.
	fn peek_replay_batch_inner(&mut self, width: u16) -> Option<TranscriptHistoryBatch> {
		if let Some(offered) = &self.offered {
			return match offered {
				Offered::Replay { batch, .. } => Some(batch.clone()),
				_ => None,
			};
		}
		if !self.replay_pending {
			return None;
		}
		let replay = self.render_replay(width);
		self.replay_pending = false;
		if replay.0.is_empty() {
			self.set_offered_history_end(None);
			self.release_committed_render_caches();
			return None;
		}
		let batch = TranscriptHistoryBatch {
			id: self.next_batch_id(),
			rows: replay.0,
			kind: TranscriptBatchKind::Replay,
		};
		self.offered = Some(Offered::Replay { batch: batch.clone(), history_end: replay.1 });
		Some(batch)
	}

	/// Whether every live block older than `live[cursor]` is finalized.
	fn retires_through(&self, live: &[usize], cursor: usize, clipped: bool) -> bool {
		let end = if clipped { cursor + 1 } else { cursor };
		for index in 0..end {
			let entry_index = live[index];
			if self.entries[entry_index].state == BlockState::Settled {
				continue;
			}
			if self.entries[entry_index].component.is_transcript_block_finalized() == Some(false) {
				return false;
			}
		}
		true
	}

	/// Offer stable-head emission or the shortest finalized prefix needed under
	/// pressure.
	fn peek_batch(
		&mut self,
		width: u16,
		capacity: usize,
		policy: RetirementPolicy,
	) -> Option<TranscriptHistoryBatch> {
		self.settle_finalized();
		if let Some(offered) = &self.offered {
			return Some(offered.batch_clone());
		}
		if let Some(replay) = self.peek_replay_batch_inner(width) {
			return Some(replay);
		}
		self.complete_fully_emitted_heads(width);
		let room = capacity;
		let live = self.live_entries();
		if live.is_empty() {
			return None;
		}
		if self.frontier < self.entries.len() {
			let head = self.frontier;
			self.measure_index(head, width);
		}
		let append_head: Option<usize> = {
			let entry = &self.entries[self.frontier];
			if policy == RetirementPolicy::Pressure
				&& entry.mode == TranscriptBlockMode::AppendOnly
				&& !entry.stable_frozen
				&& entry.state != BlockState::Committed
				&& entry.split.is_none()
				&& entry.emitted < entry.stable_rows.len()
			{
				Some(self.frontier)
			} else {
				None
			}
		};
		let lead = self.owed_lead(&live, width);
		let mut tail_rows = 0usize;
		let mut live_rows = 0usize;
		let mut keep = 0usize;
		let mut fits = true;
		let mut below: Option<bool> = None;
		let mut boundary_gap = 0usize;
		let mut lead_only = false;
		for cursor in (0..live.len()).rev() {
			let index = live[cursor];
			let rows = self.live_block(index, width).rows.len();
			if rows > 0 {
				let compact = self.is_compact(index);
				let gap = if below.is_none() { 0 } else { separator_rows(below, compact) };
				let height = rows + gap + if Some(cursor) == lead { 1 } else { 0 };
				live_rows += height;
				if fits {
					if tail_rows + height > room {
						fits = false;
						keep = cursor + 1;
						boundary_gap = gap;
						lead_only = Some(cursor) == lead && tail_rows + height - 1 <= room;
					} else {
						tail_rows += height;
					}
				}
				below = Some(compact);
			}
			if !fits && append_head.is_none() {
				break;
			}
		}
		let overflowing = keep > 0 || self.live_count() >= MAX_LIVE_BLOCKS;
		if policy == RetirementPolicy::Pressure && !overflowing {
			self.pinned_frontier = None;
			return None;
		}
		if lead_only && policy == RetirementPolicy::Pressure && self.live_count() < MAX_LIVE_BLOCKS {
			let batch = TranscriptHistoryBatch {
				id: self.next_batch_id(),
				rows: vec![String::new()],
				kind: TranscriptBatchKind::Append,
			};
			self.offered = Some(Offered::Commit {
				batch: batch.clone(),
				end: self.frontier,
				split: None,
				head_emitted_end: None,
				history_end: None,
			});
			self.pinned_frontier = None;
			return Some(batch);
		}
		if let Some(append_index) = append_head {
			if keep > 0 {
				let lead_rows = if lead == Some(0) { 1 } else { 0 };
				let overflow = live_rows as isize - room as isize - lead_rows as isize;
				let append_batch = match self.measured_append_batch(append_index, width, overflow) {
					Some(batch) => batch,
					None => self.rendered_append_batch(append_index, width, overflow),
				};
				if self.complete_scrollback && (append_batch.rows.len() as isize) < overflow {
					self.lock_streaming_layout(append_index);
				}
				let emitted_end = append_batch.emitted_end;
				if emitted_end > self.entries[append_index].emitted {
					let rows = if lead_rows > 0 {
						let mut rows = Vec::with_capacity(append_batch.rows.len() + 1);
						rows.push(String::new());
						rows.extend(append_batch.rows);
						rows
					} else {
						append_batch.rows
					};
					let batch = TranscriptHistoryBatch {
						id: self.next_batch_id(),
						rows,
						kind: TranscriptBatchKind::Append,
					};
					self.offered = Some(Offered::Append {
						batch: batch.clone(),
						entry: self.frontier,
						emitted_end,
						lead: lead_rows > 0,
					});
					self.pinned_frontier = None;
					return Some(batch);
				}
			}
		}
		let mut limit = self.frontier + keep;
		if self.live_count() >= MAX_LIVE_BLOCKS {
			limit = limit.max(self.frontier + (self.live_count() - (MAX_LIVE_BLOCKS - 1)));
		}
		if policy == RetirementPolicy::Flush {
			limit = self.entries.len();
		}
		let mut split: Option<(usize, Vec<String>, RowSplit)> = None;
		if policy == RetirementPolicy::Pressure && keep > 0 && limit == self.frontier + keep {
			let boundary = live[keep - 1];
			let visible = room as isize - tail_rows as isize - boundary_gap as isize;
			if self.entries[boundary].state == BlockState::Settled && visible > 0 {
				match self.split_offer(boundary, width, visible as usize) {
					Some(SplitOffer::Whole) => limit = boundary + 1,
					Some(SplitOffer::Rows { rows, split: row_split }) => {
						split = Some((boundary, rows, row_split));
						limit = boundary;
					}
					None => {}
				}
			}
		}
		let mut end = self.frontier;
		while end < limit && end < self.entries.len() && self.entries[end].state == BlockState::Settled {
			end += 1;
		}
		if split.as_ref().map_or(false, |(index, _, _)| end != *index) {
			split = None;
		}
		if end == self.frontier && split.is_none() {
			if policy == RetirementPolicy::Pressure {
				self.note_pinned_frontier();
				if self.complete_scrollback && keep > 0 {
					let boundary = live[keep - 1];
					self.lock_streaming_layout(boundary);
				}
			}
			return None;
		}
		self.pinned_frontier = None;
		let mut retirement = self.render_range(
			self.frontier,
			end,
			width,
			self.projected_history_end(),
			if policy == RetirementPolicy::Flush { None } else { Some(RETIREMENT_BUDGET_MS) },
		);
		let reached = retirement.end;
		let above = retirement.above;
		let mut rows = std::mem::take(&mut retirement.rows);
		if split.as_ref().map_or(false, |(index, _, _)| reached != *index) {
			split = None;
		}
		let mut history_end = if let Some((index, split_rows, _)) = &split {
			if separator_rows(above, self.is_compact(*index)) > 0 {
				rows.push(String::new());
			}
			rows.extend_from_slice(split_rows);
			None
		} else {
			self.close_range(&mut rows, above, reached, width)
		};
		let mut streaming_head: Option<AppendBatch> = None;
		if self.complete_scrollback && policy == RetirementPolicy::Pressure && split.is_none() && keep > 0 {
			let boundary = live[keep - 1];
			let (mode, frozen, has_split, emitted, stable_len) = {
				let entry = &self.entries[boundary];
				(entry.mode, entry.stable_frozen, entry.split.is_some(), entry.emitted, entry.stable_rows.len())
			};
			if boundary == reached
				&& mode == TranscriptBlockMode::AppendOnly
				&& !frozen
				&& !has_split
				&& emitted < stable_len
			{
				let height = self.live_block(boundary, width).rows.len();
				let overflow = height as isize + boundary_gap as isize + tail_rows as isize - room as isize;
				if overflow > 0 {
					let batch = match self.measured_append_batch(boundary, width, overflow) {
						Some(batch) => batch,
						None => self.rendered_append_batch(boundary, width, overflow),
					};
					let rows_len = batch.rows.len();
					if batch.emitted_end > self.entries[boundary].emitted {
						streaming_head = Some(batch);
					}
					if (rows_len as isize) < overflow {
						self.lock_streaming_layout(boundary);
					}
				}
			}
		}
		if let Some(batch) = &streaming_head {
			rows.extend_from_slice(&batch.rows);
			history_end = None;
		}
		let batch = TranscriptHistoryBatch {
			id: self.next_batch_id(),
			rows,
			kind: TranscriptBatchKind::Append,
		};
		self.offered = Some(Offered::Commit {
			batch: batch.clone(),
			end: reached,
			split: split.map(|(_, _, row_split)| row_split),
			head_emitted_end: streaming_head.map(|batch| batch.emitted_end),
			history_end,
		});
		Some(batch)
	}

	/// The head rows of settled `index` beyond its committed prefix that leave
	/// `visible` rows live.
	fn split_offer(&mut self, index: usize, width: u16, visible: usize) -> Option<SplitOffer> {
		let measured = self.measure_index(index, width).rows;
		let start = self.emitted_row_count(index, width);
		let cut = measured.len().saturating_sub(visible);
		if cut <= start {
			return None;
		}
		let scrollback = self.render_entry_index(index, width);
		if scrollback.len() != measured.len() {
			return None;
		}
		let total = cut_below_whole_image(&scrollback, cut);
		if total >= scrollback.len() {
			return Some(SplitOffer::Whole);
		}
		let rows = scrollback[start..total].to_vec();
		Some(SplitOffer::Rows { rows, split: row_split(&scrollback, total, width) })
	}

	/// A streaming block that overflows with too few stable rows to cover it.
	fn lock_streaming_layout(&mut self, index: usize) {
		if self.entries[index].state == BlockState::Settled {
			return;
		}
		self.entries[index].component.lock_streaming_layout();
	}

	/// Forward a viewport allocation to one block.
	fn set_allocation(&mut self, index: usize, rows: usize, frame: AnimationFrame) {
		self.entries[index].component.set_transcript_allocation(rows, frame);
	}

	/// Mark finalized active blocks settled.
	fn settle_finalized(&mut self) {
		for index in self.frontier..self.entries.len() {
			if self.entries[index].state == BlockState::Active && self.is_finalized(index) {
				self.entries[index].state = BlockState::Settled;
			}
		}
	}

	/// Whether a block reports finalized and not pending.
	fn is_finalized(&self, index: usize) -> bool {
		let component = &*self.entries[index].component;
		component.is_transcript_block_finalized().unwrap_or(true) && !component.is_transcript_block_pending()
	}

	/// Whether a block currently presents compact rows.
	fn is_compact(&self, index: usize) -> bool {
		self.entries[index].component.is_transcript_block_compact()
	}

	/// Collapse a per-line owner list into run-length block spans.
	fn commit_viewport_spans(&mut self, owners: &[Option<u64>], length: usize) {
		let mut spans: Vec<TranscriptViewportSpan> = Vec::new();
		let mut index = 0;
		while index < length {
			let block = owners.get(index).copied().flatten();
			let Some(block) = block else {
				index += 1;
				continue;
			};
			let mut end = index + 1;
			while end < length && owners.get(end).copied().flatten() == Some(block) {
				end += 1;
			}
			spans.push(TranscriptViewportSpan { block_id: block, start: index, end });
			index = end;
		}
		self.last_viewport_spans = spans;
	}

	/// Compose the live viewport rows.
	fn compose_viewport(&mut self, width: u16, rows: usize, frame: AnimationFrame) -> Vec<String> {
		self.settle_finalized();
		let capacity = rows;
		let live = self.live_entries();
		if live.is_empty() || capacity == 0 {
			self.last_viewport_spans = Vec::new();
			return Vec::new();
		}
		let mut shown: Vec<usize> = Vec::new();
		let mut blocks: Vec<Vec<String>> = Vec::new();
		let mut measured_images: Vec<bool> = Vec::new();
		let mut unrendered = 0usize;
		let mut filled = 0usize;
		let mut compact_below: Option<bool> = None;
		let mut clip_decided = false;
		let mut clip = false;
		for cursor in (0..live.len()).rev() {
			if shown.len() > capacity {
				unrendered = cursor + 1;
				break;
			}
			let index = live[cursor];
			let block = self.live_block(index, width);
			if block.rows.is_empty() {
				continue;
			}
			shown.push(index);
			let row_count = block.rows.len();
			blocks.push(block.rows);
			measured_images.push(block.images);
			let compact = self.is_compact(index);
			filled += row_count
				+ if compact_below.is_none() { 0 } else { separator_rows(compact_below, compact) };
			compact_below = Some(compact);
			if !clip_decided && filled >= capacity {
				clip_decided = true;
				clip = self.retires_through(&live, cursor, filled > capacity);
				if clip {
					break;
				}
			}
		}
		shown.reverse();
		blocks.reverse();
		measured_images.reverse();
		if shown.is_empty() {
			self.last_viewport_spans = Vec::new();
			return Vec::new();
		}
		if !clip && shown.len() > capacity {
			let behind = live[..unrendered].to_vec();
			return self.render_emergency(&shown, &behind, width, capacity, frame);
		}
		for block_index in 0..shown.len() {
			if !measured_images[block_index] {
				continue;
			}
			let index = shown[block_index];
			self.set_allocation(index, usize::MAX, frame);
			let rendered = self.render_entry_index(index, width);
			let emitted = self.projected_emitted_row_count(index, width);
			blocks[block_index] = if emitted < rendered.len() {
				rendered[emitted..].to_vec()
			} else {
				Vec::new()
			};
		}
		let mut gaps: Vec<usize> = Vec::new();
		let mut above = self.projected_history_end();
		let mut total = 0usize;
		for block_index in 0..shown.len() {
			let compact = self.is_compact(shown[block_index]);
			let gap = separator_rows(above, compact);
			gaps.push(gap);
			total += gap + blocks[block_index].len();
			above = Some(compact);
		}
		if clip || total <= capacity {
			let mut output: Vec<String> = Vec::new();
			let mut owners: Vec<Option<u64>> = Vec::new();
			for block_index in 0..blocks.len() {
				if gaps[block_index] > 0 {
					output.push(String::new());
					owners.push(None);
				}
				let component = shown[block_index];
				let id = self.entries[component].id;
				for line in &blocks[block_index] {
					output.push(line.clone());
					owners.push(Some(id));
				}
			}
			let drop = output.len().saturating_sub(capacity);
			let head_clipped = drop.saturating_sub(gaps[0]);
			if head_clipped > 0 {
				let below = cut_below_whole_image(&blocks[0], head_clipped);
				if below > head_clipped {
					let row_index = gaps[0] + below - 1;
					if let Some(slot) = output.get_mut(row_index) {
						*slot = String::new();
					}
				}
			}
			let kept = drop.min(owners.len());
			self.commit_viewport_spans(&owners[kept..], output.len() - drop);
			if drop > 0 {
				output.split_off(drop)
			} else {
				output
			}
		} else {
			let mut allocation: Vec<usize> = vec![1; shown.len()];
			let mut surplus = capacity - shown.len();
			let mut order: Vec<usize> = Vec::new();
			for index in (0..shown.len()).rev() {
				if !self.entries[shown[index]].component.is_tool_activity() {
					order.push(index);
				}
			}
			for index in (0..shown.len()).rev() {
				if self.entries[shown[index]].component.is_tool_activity() {
					order.push(index);
				}
			}
			for index in order {
				if surplus == 0 {
					break;
				}
				let extra = blocks[index].len().saturating_sub(1).min(surplus);
				allocation[index] += extra;
				surplus -= extra;
			}
			let mut output: Vec<String> = Vec::new();
			let mut owners: Vec<Option<u64>> = Vec::new();
			for index in 0..shown.len() {
				let candidate = shown[index];
				let allocated = allocation[index];
				self.set_allocation(candidate, allocated, frame);
				let rendered = self.render_entry_index(candidate, width);
				let emitted = self.projected_emitted_row_count(candidate, width);
				let unemitted: &[String] = if emitted < rendered.len() {
					&rendered[emitted..]
				} else {
					&[]
				};
				let visible: &[String] = if unemitted.len() <= allocated {
					unemitted
				} else {
					&unemitted[unemitted.len() - allocated..]
				};
				let id = self.entries[candidate].id;
				for line in visible {
					output.push(line.clone());
					owners.push(Some(id));
				}
			}
			let drop = output.len().saturating_sub(capacity);
			let kept = drop.min(owners.len());
			self.commit_viewport_spans(&owners[kept..], output.len() - drop);
			if drop > 0 {
				output.split_off(drop)
			} else {
				output
			}
		}
	}

	/// One-row-per-block fallback for a live region that cannot fit the
	/// viewport.
	fn render_emergency(
		&mut self,
		shown: &[usize],
		behind: &[usize],
		width: u16,
		rows: usize,
		frame: AnimationFrame,
	) -> Vec<String> {
		let mut hidden_below = 0usize;
		for &index in behind {
			if self.entries[index].state != BlockState::Active {
				continue;
			}
			if !self.live_block(index, width).rows.is_empty() {
				hidden_below += 1;
			}
		}
		let mut behind_cache: Option<Option<(usize, String)>> = None;
		let mut visible_rows = rows;
		let mut visible: Vec<usize> = Vec::new();
		let mut emergency_candidate: Option<usize> = None;
		let mut emergency_row: Option<String> = None;
		let mut hidden_active = 0usize;
		for attempt in 0..2 {
			visible = if visible_rows > 0 {
				shown[shown.len().saturating_sub(visible_rows)..].to_vec()
			} else {
				Vec::new()
			};
			emergency_candidate = None;
			emergency_row = None;
			let visible_start = shown.len().saturating_sub(visible_rows);
			for index in (0..visible_start).rev() {
				let candidate = shown[index];
				let row = if self.entries[candidate].state == BlockState::Settled {
					self.entries[candidate]
						.component
						.render_transcript_block_emergency_row(width)
				} else {
					None
				};
				if row.is_none() {
					continue;
				}
				emergency_candidate = Some(candidate);
				emergency_row = row;
				if !visible.is_empty() {
					visible.remove(0);
				}
				visible.insert(0, candidate);
				break;
			}
			if emergency_candidate.is_none() {
				if let Some((candidate, row)) = self.behind_emergency(behind, width, &mut behind_cache) {
					emergency_candidate = Some(candidate);
					emergency_row = Some(row);
					if !visible.is_empty() {
						visible.remove(0);
					}
					visible.insert(0, candidate);
				}
			}
			let mut active_total = hidden_below;
			for &candidate in shown {
				if self.entries[candidate].state == BlockState::Active {
					active_total += 1;
				}
			}
			hidden_active = active_total;
			for &candidate in &visible {
				if self.entries[candidate].state == BlockState::Active {
					hidden_active -= 1;
				}
			}
			if hidden_active == active_total && hidden_active > 0 {
				hidden_active -= 1;
			}
			if attempt == 0 && hidden_active > 0 {
				visible_rows = rows.saturating_sub(1);
				continue;
			}
			break;
		}
		let mut output: Vec<String> = Vec::new();
		let mut owners: Vec<Option<u64>> = Vec::new();
		if hidden_active > 0 {
			output.push(format!("{hidden_active} more transcript blocks active"));
			owners.push(None);
		}
		for &candidate in &visible {
			if Some(candidate) == emergency_candidate {
				output.push(emergency_row.clone().unwrap_or_default());
				owners.push(Some(self.entries[candidate].id));
				continue;
			}
			self.set_allocation(candidate, 1, frame);
			let emergency = self.entries[candidate]
				.component
				.render_transcript_block_emergency_row(width);
			if let Some(row) = emergency {
				output.push(row);
				owners.push(Some(self.entries[candidate].id));
				continue;
			}
			let rendered = self.render_entry_index(candidate, width);
			let emitted = self.projected_emitted_row_count(candidate, width);
			let first = if emitted < rendered.len() {
				rendered[emitted].clone()
			} else {
				String::new()
			};
			output.push(first);
			owners.push(Some(self.entries[candidate].id));
		}
		let visible_output: Vec<String> = output.into_iter().take(rows).collect();
		self.commit_viewport_spans(&owners, visible_output.len());
		visible_output
	}

	/// Find the newest settled block behind the shown region that can offer an
	/// emergency row.
	fn behind_emergency(
		&mut self,
		behind: &[usize],
		width: u16,
		cache: &mut Option<Option<(usize, String)>>,
	) -> Option<(usize, String)> {
		if let Some(cached) = cache {
			return cached.clone();
		}
		let mut found = None;
		for &index in behind.iter().rev() {
			let (state, row) = {
				let entry = &self.entries[index];
				(entry.state, entry.component.render_transcript_block_emergency_row(width))
			};
			if state != BlockState::Settled {
				continue;
			}
			let Some(row) = row else { continue };
			if self.live_block(index, width).rows.is_empty() {
				continue;
			}
			found = Some((index, row));
			break;
		}
		*cache = Some(found.clone());
		found
	}

	/// A block's native description, or a `rows` fallback.
	fn describe_child_node(&mut self, index: usize, cx: &DescribeContext) -> NativeNode {
		let component = &self.entries[index].component;
		if let Some(node) = component.describe_node(cx) {
			return node;
		}
		self.rows_fallback(index, cx)
	}

	/// Render a block with no native description to a `rows` fallback node.
	fn rows_fallback(&mut self, index: usize, cx: &DescribeContext) -> NativeNode {
		let cols = cx.cols.max(1);
		let lines: Vec<String> = {
			let component = &mut self.entries[index].component;
			component
				.render(cols)
				.into_iter()
				.map(|line| line.replace(crate::native::CURSOR_MARKER, ""))
				.collect()
		};
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cols));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}

	/// Index of `component` among the entries, scanning from the live tail.
	fn removable_index(&self, index: usize) -> Option<usize> {
		let entry = &self.entries[index];
		if entry.state == BlockState::Committed || entry.emitted > 0 || entry.split.is_some() {
			return None;
		}
		let carries_boundary = match &self.offered {
			Some(Offered::Commit { split, head_emitted_end, .. }) => {
				split.is_some() || head_emitted_end.is_some()
			}
			_ => false,
		};
		if let Some(Offered::Commit { end, .. }) = &self.offered {
			if index < *end + carries_boundary as usize {
				return None;
			}
		}
		if let Some(Offered::Append { entry: offered_entry, .. }) = &self.offered {
			if index == *offered_entry {
				return None;
			}
		}
		Some(index)
	}

	/// Allocate the next batch id.
	fn next_batch_id(&mut self) -> u64 {
		let id = self.next_batch_id;
		self.next_batch_id += 1;
		id
	}
}

impl Component for TranscriptContainer {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_semantic(width)
	}

	fn invalidate(&mut self) {
		for index in 0..self.entries.len() {
			self.entries[index].component.invalidate();
		}
	}

	fn dispose(&mut self) {
		for index in 0..self.entries.len() {
			self.entries[index].component.dispose();
		}
	}
}

/// Whether `prefix` matches `rows` byte-for-byte from the top.
pub fn is_row_prefix(prefix: &[String], rows: &[String]) -> bool {
	if prefix.len() > rows.len() {
		return false;
	}
	prefix.iter().zip(rows).all(|(a, b)| a == b)
}

/// Whether `prefix` matches `rows` by stable key from the top.
fn is_stable_prefix(prefix: &[TranscriptStableRow], rows: &[TranscriptStableRow]) -> bool {
	if prefix.len() > rows.len() {
		return false;
	}
	prefix.iter().zip(rows).all(|(a, b)| a.key == b.key)
}

/// Strip leading/trailing all-blank rows; the viewport allocator measures blocks
/// by this trimmed height.
pub fn trim_blank_edges(rows: &[String]) -> Vec<String> {
	let mut start = 0usize;
	let mut end = rows.len();
	while start < end && is_plain_blank(&rows[start]) {
		start += 1;
	}
	while end > start && is_plain_blank(&rows[end - 1]) {
		end -= 1;
	}
	rows[start..end].to_vec()
}

/// Whether a line has no non-whitespace character.
fn is_plain_blank(line: &str) -> bool {
	!line.chars().any(|ch| !ch.is_whitespace())
}

/// Blank rows before a non-empty block: one, unless none is owed or both
/// blocks are compact.
fn separator_rows(above: RowsAbove, compact: bool) -> usize {
	if above.is_none() || (above == Some(true) && compact) {
		0
	} else {
		1
	}
}

/// Letters and digits of `rows` in reading order: the content rewrapping
/// preserves.
fn content_of(rows: &[String]) -> String {
	let mut text = String::new();
	for row in rows {
		text.extend(strip_ansi(row).chars().filter(|ch| ch.is_alphanumeric()));
	}
	text
}

/// Fingerprint the scrollback bytes history receives.
fn hash_rows(rows: &[String]) -> u64 {
	let mut hasher = Hasher::new();
	hasher.update(&rows.join("\n"));
	hasher.digest()
}

/// Split committing the first `rows` rows of `scrollback` at `width`.
fn row_split(scrollback: &[String], rows: usize, width: u16) -> RowSplit {
	let rows = rows.min(scrollback.len());
	let committed = &scrollback[..rows];
	let mut images: Vec<ImageSeam> = Vec::new();
	let mut text = String::new();
	let info = terminal();
	for row in committed {
		if info.is_image_line(row) {
			let payload = iterm2_image_payload(row).unwrap_or_else(|| row.clone());
			images.push(ImageSeam { payload, offset: text.len() });
		}
		text.push_str(&content_of(std::slice::from_ref(row)));
	}
	RowSplit { rows, width, hash: hash_rows(committed), text, images, located: None }
}

/// `cut` moved to just below an image it would split, on terminals that cannot
/// draw part of an image.
fn cut_below_whole_image(rows: &[String], cut: usize) -> usize {
	let info = terminal();
	cut_below_whole_image_with_protocol(rows, cut, info.image_protocol, &info)
}

/// [`Self::cut_below_whole_image`] with an explicit protocol and capability
/// profile, so tests need not touch the process-global terminal state.
pub fn cut_below_whole_image_with_protocol(
	rows: &[String],
	cut: usize,
	protocol: Option<ImageProtocol>,
	info: &RuntimeTerminal,
) -> usize {
	if protocol.is_none() || protocol == Some(ImageProtocol::Kitty) || protocol == Some(ImageProtocol::Sixel) {
		return cut;
	}
	for index in cut..rows.len() {
		let row = &rows[index];
		if !info.is_image_line(row) {
			continue;
		}
		return if index.saturating_sub(direct_placement_lift(row)) < cut {
			index + 1
		} else {
			cut
		};
	}
	cut
}

/// Rows above its own line that an image direct-placement line paints over.
///
/// Ported from `l/src/terminal-capabilities.ts` `directPlacementLift`.
pub fn direct_placement_lift(line: &str) -> usize {
	let bytes = line.as_bytes();
	if bytes.len() < 5 || bytes[0] != 0x1b || bytes[1] != b'7' || bytes[2] != 0x1b || bytes[3] != b'[' {
		return 0;
	}
	let mut index = 4usize;
	let mut value = 0usize;
	while index < bytes.len() && bytes[index].is_ascii_digit() {
		value = value * 10 + usize::from(bytes[index] - b'0');
		index += 1;
	}
	if index > 4 && index < bytes.len() && bytes[index] == b'A' {
		value
	} else {
		0
	}
}

/// The encoded iTerm2 image payload of `line`, if any.
///
/// Ported from `l/src/terminal-capabilities.ts` `iterm2ImagePayload`.
pub fn iterm2_image_payload(line: &str) -> Option<String> {
	let marker = ImageProtocol::Iterm2.marker();
	let start = line.find(marker)?;
	let after_marker = start + marker.len();
	let colon_relative = line[after_marker..].find(':')?;
	let colon = after_marker + colon_relative;
	let payload_start = colon + 1;
	let rest = &line[payload_start..];
	let end = rest.find('\x07').unwrap_or(rest.len());
	Some(rest[..end].to_string())
}

/// Whether a native node lays out nothing (a hidden node, or a `col`/`row` of
/// such nodes).
fn is_void_node(node: &NativeNode, depth: usize) -> bool {
	if node.props.hidden() == Some(true) {
		return true;
	}
	if !(node.kind == NodeKind::Col || node.kind == NodeKind::Row) || depth > 16 {
		return false;
	}
	node.children.iter().all(|child| is_void_node(child, depth + 1))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::component::Component;

	struct FixedRows(Vec<String>);

	impl Component for FixedRows {
		fn render(&mut self, _width: u16) -> Vec<String> {
			self.0.clone()
		}
	}

	#[test]
	fn row_prefix_requires_a_byte_identical_top() {
		let rows = vec!["a".to_string(), "b".to_string(), "c".to_string()];
		assert!(is_row_prefix(&[], &rows));
		assert!(is_row_prefix(&rows[0..2], &rows));
		assert!(is_row_prefix(&rows, &rows));
		assert!(!is_row_prefix(&rows, &rows[0..2]));
		let mut mismatch = rows.clone();
		mismatch[1] = "x".to_string();
		assert!(!is_row_prefix(&rows[0..2], &mismatch));
	}

	#[test]
	fn trim_blank_edges_strips_only_outer_blank_rows() {
		let rows = vec![
			"".to_string(),
			"   ".to_string(),
			"a".to_string(),
			"  b  ".to_string(),
			"\t".to_string(),
			"".to_string(),
		];
		assert_eq!(
			trim_blank_edges(&rows),
			vec!["a".to_string(), "  b  ".to_string()]
		);
		assert!(trim_blank_edges(&["  ".to_string(), "\t".to_string()]).is_empty());
		assert!(trim_blank_edges(&[]).is_empty());
	}

	#[test]
	fn separator_rows_follow_the_compact_rule() {
		assert_eq!(separator_rows(None, false), 0);
		assert_eq!(separator_rows(None, true), 0);
		assert_eq!(separator_rows(Some(true), true), 0);
		assert_eq!(separator_rows(Some(true), false), 1);
		assert_eq!(separator_rows(Some(false), true), 1);
		assert_eq!(separator_rows(Some(false), false), 1);
	}

	#[test]
	fn transcript_block_groups_children_in_order() {
		let mut block = TranscriptBlock::new();
		assert!(block.children().is_empty());
		block.add_row(FixedRows(vec!["one".to_string()]));
		block.add_row(FixedRows(vec!["two".to_string(), "three".to_string()]));
		assert_eq!(block.children().len(), 2);
		assert_eq!(
			block.render(20),
			vec!["one".to_string(), "two".to_string(), "three".to_string()]
		);

		let removed = block.remove_child(0).expect("first child");
		assert_eq!(removed.render(20), vec!["one".to_string()]);
		assert_eq!(block.children().len(), 1);
		assert!(block.remove_child(9).is_none());

		block.clear();
		assert!(block.children().is_empty());
		assert!(block.render(20).is_empty());
	}

	#[test]
	fn content_of_keeps_letters_and_digits_only() {
		let rows = vec!["a b-c".to_string(), "\u{1b}[31m12\u{1b}[0m".to_string()];
		assert_eq!(content_of(&rows), "abc12");
	}
}
