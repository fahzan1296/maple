//! Reference: `l/src/components/image.ts`.
//!
//! Inline images: [`ImageBudget`], which bounds how many images a surface shows
//! as live terminal graphics, and [`Image`], which draws one image through
//! Kitty placeholders/direct placement, SIXEL or iTerm2, else its text
//! fallback with a native `image` node describing the bytes.
//!
//! Deviations from the TypeScript original, all runtime-only:
//!
//! - Off-thread SIXEL encoding (`encodeSixelAsync` from the Bun native package)
//!   needs an image decoder the crate does not carry, so encoding is delegated
//!   to a [`SixelEncoder`] registered with [`set_sixel_encoder`], which this
//!   component calls synchronously. Without a registered encoder the encode
//!   fails, exactly as a failed native encode does, and the image draws its text
//!   fallback. An encode is therefore never *pending*: the original's
//!   reserved-rows-for-an-in-flight-encode states collapse into the settled one.
//! - `renderForScrollback`/`measureRender` keep their depth counters in
//!   thread-local storage; the original's module-level counters assume a single
//!   JS thread.
//! - Budget image ids seed from a process-wide counter rather than
//!   `Math.random()`, so ids are reproducible.
//! - `describe` memoizes its node behind a `Mutex`, since the crate's
//!   [`NativeDescribe`] hook takes `&self`.
//! - Queued transmits drain in id order; the original's `Map` drains in
//!   insertion order. The sequences are independent, so the emitted bytes match.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use crate::component::Component;
use crate::kitty_graphics::{
	get_kitty_graphics, kitty_placeholders_fit, render_kitty_placeholder_lines,
};
use crate::native::blobs::{image_dimensions as blob_image_dimensions, image_node, openable_image_props};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, Props};
use crate::prompt::image_references::{image_links_enabled, linked_image_text, transcript_image_link};
use crate::prompt::image_source::ImageContent;
use crate::terminal_capabilities::{
	cell_dimensions, ensure_sixel_raster_width, get_terminal_background_color, terminal,
	wrap_tmux_passthrough_if_needed, CellDimensions, ImageProtocol, ProcessEnv,
};

/// Pixel size of an image's source raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageDimensions {
	/// Source width in pixels.
	pub width_px: u32,
	/// Source height in pixels.
	pub height_px: u32,
}

impl ImageDimensions {
	/// Dimensions of a `width_px`×`height_px` raster.
	pub fn new(width_px: u32, height_px: u32) -> Self {
		ImageDimensions { width_px, height_px }
	}
}

/// Theme hooks an [`Image`] renders with.
#[derive(Clone)]
pub struct ImageTheme {
	/// Styler for the text fallback line.
	pub fallback_color: Arc<dyn Fn(&str) -> String + Send + Sync>,
}

impl ImageTheme {
	/// A theme whose fallback line is styled by `fallback_color`.
	pub fn new(fallback_color: impl Fn(&str) -> String + Send + Sync + 'static) -> Self {
		ImageTheme { fallback_color: Arc::new(fallback_color) }
	}

	/// A theme that leaves the fallback line unstyled.
	pub fn plain() -> Self {
		ImageTheme::new(|text| text.to_string())
	}
}

/// Construction options for an [`Image`].
#[derive(Clone, Default)]
pub struct ImageOptions {
	/// Widest the image may draw, in cells.
	pub max_width_cells: Option<u16>,
	/// Tallest the image may draw, in cells.
	pub max_height_cells: Option<u16>,
	/// Display name for the text fallback.
	pub filename: Option<String>,
	/// File the image was read from; its link target. Images without one link to
	/// a blob copy of their bytes.
	pub source_path: Option<String>,
	/// Shared budget that caps how many inline images render as live graphics.
	pub budget: Option<Arc<Mutex<ImageBudget>>>,
	/// Stable identity for the underlying image (e.g. `toolCallId:index`). Lets
	/// the budget hand back the same graphics id across component re-creations so
	/// a repaint replaces the placement instead of stacking a duplicate.
	pub image_key: Option<String>,
	/// Schedules a repaint once this image's SIXEL encode lands. Defaults to the
	/// budget's repaint; a host without a budget must pass one, or the image's
	/// reserved rows stay blank until an unrelated repaint.
	pub request_render: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Schedules a repaint, e.g. once an image's SIXEL encode settles.
pub type RequestRender = Arc<dyn Fn() + Send + Sync>;

/// Encodes `bytes` to a SIXEL sequence at `width_px`×`height_px`, compositing
/// onto `background_rgb`; `None` when the encode fails.
pub type SixelEncoder = Arc<dyn Fn(&[u8], u32, u32, u32) -> Option<String> + Send + Sync>;

static SIXEL_ENCODER: LazyLock<RwLock<Option<SixelEncoder>>> = LazyLock::new(|| RwLock::new(None));

/// Install (or clear) the SIXEL encoder used for inline SIXEL placements.
pub fn set_sixel_encoder(encoder: Option<SixelEncoder>) {
	*SIXEL_ENCODER.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = encoder;
}

/// The installed SIXEL encoder, if any.
pub fn sixel_encoder() -> Option<SixelEncoder> {
	SIXEL_ENCODER.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

thread_local! {
	/// Renders in progress whose rows are bound for native scrollback.
	static SCROLLBACK_RENDER_DEPTH: Cell<u32> = const { Cell::new(0) };
	/// Renders in progress that measure rows outside display order.
	static MEASURE_RENDER_DEPTH: Cell<u32> = const { Cell::new(0) };
	/// Budgeted image observations made under [`measure_render`]; only
	/// differences are meaningful.
	static MEASURED_IMAGE_OBSERVATIONS: Cell<u64> = const { Cell::new(0) };
}

fn scrollback_render_depth() -> u32 {
	SCROLLBACK_RENDER_DEPTH.with(Cell::get)
}

fn measure_render_depth() -> u32 {
	MEASURE_RENDER_DEPTH.with(Cell::get)
}

/// A depth counter that unwinds with its scope.
struct DepthGuard(&'static std::thread::LocalKey<Cell<u32>>);

impl DepthGuard {
	fn enter(key: &'static std::thread::LocalKey<Cell<u32>>) -> Self {
		key.with(|depth| depth.set(depth.get() + 1));
		DepthGuard(key)
	}
}

impl Drop for DepthGuard {
	fn drop(&mut self) {
		self.0.with(|depth| depth.set(depth.get().saturating_sub(1)));
	}
}

/// Run `render` for rows bound for native scrollback. Those rows are never
/// repainted, so an image whose SIXEL encode has not landed encodes
/// synchronously instead of committing its reserved blank rows.
pub fn render_for_scrollback<T>(render: impl FnOnce() -> T) -> T {
	let _guard = DepthGuard::enter(&SCROLLBACK_RENDER_DEPTH);
	render()
}

/// The measured value of a [`measure_render`] scope, plus whether any budgeted
/// image was observed.
pub struct MeasuredRender<T> {
	/// What the measured render returned.
	pub value: T,
	/// Whether a budgeted image was observed inside the scope.
	pub images: bool,
}

/// Run `render` to measure rows outside display order — a newest-first height
/// walk, say. [`ImageBudget`] derives demotion from the order images are
/// observed in, so a budgeted [`Image`] inside this scope claims no display
/// slot: it replays the decision its slot in the surface's last full pass
/// implies. `images` reports whether any budgeted image was observed; the caller
/// MUST render every such block it lays out again, outside this scope and in
/// display order, or its images keep no slot in the pass.
pub fn measure_render<T>(render: impl FnOnce() -> T) -> MeasuredRender<T> {
	let before = MEASURED_IMAGE_OBSERVATIONS.with(Cell::get);
	let value = {
		let _guard = DepthGuard::enter(&MEASURE_RENDER_DEPTH);
		render()
	};
	let after = MEASURED_IMAGE_OBSERVATIONS.with(Cell::get);
	MeasuredRender { value, images: after != before }
}

/// Direct placements reserve height with leading zero-width rows. Keep them
/// non-plain so transcript blank-edge trimming does not collapse image-only
/// blocks.
const RESERVED_IMAGE_ROW: &str = "\x1b[0m";
const SAVE_CURSOR: &str = "\x1b7";
const RESTORE_CURSOR: &str = "\x1b8";

/// Widest an [`Image`] rendered at `width` columns draws; callers sizing a
/// raster for it match it.
pub fn image_max_columns(width: u16) -> u16 {
	width.saturating_sub(2).max(1)
}

/// Default count of inline images kept as live graphics before older ones fall
/// back to text.
pub const DEFAULT_MAX_INLINE_IMAGES: u32 = 8;

/// A surface the renderer paints frames on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
	Screen,
	Alt,
}

impl Surface {
	fn index(self) -> usize {
		match self {
			Surface::Screen => 0,
			Surface::Alt => 1,
		}
	}
}

const SURFACES: [Surface; 2] = [Surface::Screen, Surface::Alt];

/// The live/text split of one drawing surface. The normal screen and the
/// alternate buffer hold separate frames with separate display orders, so each
/// carries its own thresholds: a modal's split describes the modal, and applying
/// it to the transcript would demote — and purge — images the modal never
/// showed.
struct SurfaceSplit {
	/// Suppress threshold reflected in the frame currently on this surface:
	/// images at display indices `[0, on_terminal)` are shown as text there.
	on_terminal: usize,
	/// Suppress threshold the current/next render of this surface should apply.
	planned: usize,
	/// Images the last full pass on this surface observed.
	last_total: usize,
	/// Display slot of every image the last full pass on this surface observed,
	/// keyed by id. A pass that cannot derive display order from its own walk — a
	/// partial `stable` pass, or a [`measure_render`] scope — replays its
	/// decisions from these slots instead.
	slots: HashMap<u32, usize>,
}

impl SurfaceSplit {
	fn new() -> Self {
		SurfaceSplit { on_terminal: 0, planned: 0, last_total: 0, slots: HashMap::new() }
	}

	/// Return a split to "nothing has been painted on this surface yet", in place.
	fn reset(&mut self) {
		self.on_terminal = 0;
		self.planned = 0;
		self.last_total = 0;
		self.slots.clear();
	}
}

/// Budget-id seed. The original starts from `Math.random()`, which no port needs;
/// the counter still advances by `0x10000` per budget.
static NEXT_IMAGE_BUDGET_SEED: AtomicU32 = AtomicU32::new(0);

fn next_image_id_seed() -> u32 {
	let previous = NEXT_IMAGE_BUDGET_SEED.fetch_add(0x10000, Ordering::Relaxed);
	let seed = (previous + 0x10000) & 0xffffff;
	if seed == 0 {
		1
	} else {
		seed
	}
}

/// Bounds how many inline images render as live terminal graphics at once.
///
/// Terminal graphics protocols — Kitty especially — keep every transmitted image
/// in a per-screen store and re-draw placements as content scrolls; placements
/// that left the viewport survive text-clear escapes (`CSI 2 J`). Unbounded, a
/// session that shows many images piles up placements plus store memory and
/// leaves ghosts in scrollback.
///
/// The budget keeps the most recent `cap` images live and demotes older ones to
/// their text fallback. Demotion needs a full redraw (so off-screen rows are
/// rewritten) plus an explicit graphics purge of the demoted ids. [`Image`]
/// reports display order via [`ImageBudget::observe`]; when that reveals a
/// stricter split, the TUI repeats the pass before emitting its terminal frame.
/// Retired frames no longer observe their images, so the resident store is also
/// bounded across passes. Evicting retired graphics removes their scrollback
/// placements; a later replay can render or demote those images again.
///
/// `cap` bounds one surface's live images, not the terminal's whole store. A
/// fullscreen overlay's frame and the normal screen standing behind it are both
/// on the terminal, and neither may delete the other's graphics — see
/// [`ImageBudget::limit_resident_images`] — so while a modal is up the store
/// legitimately holds up to `cap` per surface. Read `cap` as "how many images
/// one frame shows as graphics", not as a hard residency ceiling.
///
/// `cap == 0` disables budgeting: every image stays a live graphic.
pub struct ImageBudget {
	cap: u32,
	request_render: RequestRender,
	next_id: u32,
	key_to_id: HashMap<String, u32>,
	id_to_key: HashMap<u32, String>,
	/// Display-order image ids observed during the in-flight pass.
	pass_ids: Vec<u32>,
	/// Per-id suppression decision from the first observation in this pass.
	pass_suppression: HashMap<u32, bool>,
	/// Display index each observation was decided at, so `pass_shows_live` can
	/// re-check it against a reconciled threshold without scanning `pass_ids`
	/// once per image.
	pass_index: HashMap<u32, usize>,
	/// Live/text split of the normal screen.
	screen_split: SurfaceSplit,
	/// Live/text split of the alternate buffer (fullscreen overlay, resize borrow).
	alt_split: SurfaceSplit,
	/// The split the in-flight pass reads and writes.
	split_surface: Surface,
	/// True while the in-flight pass applies a stricter threshold than the
	/// terminal shows — the demotion frame that must purge graphics and fully
	/// repaint.
	applying_reset: bool,
	purge_ids: Vec<u32>,
	/// Deletions that belong to a pending destructive reset, kept out of
	/// `purge_ids` so only that reset's own repaint can emit them.
	reset_purge_ids: Vec<u32>,
	/// Image ids whose data is believed to be loaded in each screen's store.
	/// Kitty and Ghostty keep the normal and alternate buffers' graphics apart
	/// and wipe the alternate store on every `?1049h`, so data sent while one
	/// buffer was active is unknown to the other: a placement there resolves
	/// only after its own transmit.
	transmitted: [HashSet<u32>; 2],
	/// Transmit sequences (full base64) to write once per surface, before that
	/// frame's placements.
	pending_transmits: [BTreeMap<u32, String>; 2],
	/// True while the in-flight pass is a partial/throwaway pass (the
	/// non-multiplexer resize viewport fast path) that walks only the visible
	/// tail, bottom-up. Such a pass cannot derive display order from `observe`
	/// call order, so its suppression decisions replay the committed split.
	stable_pass: bool,
	/// The surface the in-flight pass composes for.
	surface: Surface,
	/// Image ids rendered as live graphics by the frame standing on each
	/// surface. A pass walks one surface, so the other's entry is what stops
	/// `retire` from deleting a graphic that is merely out of view — the
	/// transcript behind a fullscreen overlay keeps its placements and is
	/// restored from cache on exit.
	live_ids: [HashSet<u32>; 2],
	/// Source pixel dimensions for clipping direct placements at the viewport edge.
	placement_geometry: HashMap<u32, ImageDimensions>,
	/// Ids whose owner replaced them for good ([`ImageBudget::release`]); retired
	/// once no frame shows them.
	released: HashSet<u32>,
}

impl Default for ImageBudget {
	fn default() -> Self {
		ImageBudget::new(DEFAULT_MAX_INLINE_IMAGES, Arc::new(|| {}))
	}
}

impl ImageBudget {
	/// A budget keeping `cap` images live per surface, repainting through
	/// `request_render`.
	pub fn new(cap: u32, request_render: RequestRender) -> Self {
		ImageBudget {
			cap,
			request_render,
			next_id: next_image_id_seed(),
			key_to_id: HashMap::new(),
			id_to_key: HashMap::new(),
			pass_ids: Vec::new(),
			pass_suppression: HashMap::new(),
			pass_index: HashMap::new(),
			screen_split: SurfaceSplit::new(),
			alt_split: SurfaceSplit::new(),
			split_surface: Surface::Screen,
			applying_reset: false,
			purge_ids: Vec::new(),
			reset_purge_ids: Vec::new(),
			transmitted: [HashSet::new(), HashSet::new()],
			pending_transmits: [BTreeMap::new(), BTreeMap::new()],
			stable_pass: false,
			surface: Surface::Screen,
			live_ids: [HashSet::new(), HashSet::new()],
			placement_geometry: HashMap::new(),
			released: HashSet::new(),
		}
	}

	/// How many images one surface keeps live.
	pub fn cap(&self) -> u32 {
		self.cap
	}

	/// Whether budgeting is active (`cap > 0`).
	pub fn enabled(&self) -> bool {
		self.cap > 0
	}

	/// Replace the repaint callback.
	pub fn set_request_render(&mut self, request_render: RequestRender) {
		self.request_render = request_render;
	}

	/// Ask for a repaint, e.g. once an image's encode settles.
	pub fn request_render(&self) {
		(self.request_render)();
	}

	/// Change the per-surface cap, repainting when the split must change.
	pub fn set_cap(&mut self, cap: u32) {
		if cap == self.cap {
			return;
		}
		self.cap = cap;
		if !self.reconcile(self.current_split().last_total) {
			self.request_render();
		}
	}

	/// Stable graphics id for a logical image. A non-empty `key` maps to the same
	/// id across re-creations (so repaints replace the placement); a missing key
	/// gets a fresh id every call.
	pub fn acquire_id(&mut self, key: Option<&str>) -> u32 {
		if let Some(key) = key.filter(|key| !key.is_empty()) {
			if let Some(existing) = self.key_to_id.get(key) {
				return *existing;
			}
			let id = self.advance_id();
			self.key_to_id.insert(key.to_string(), id);
			self.id_to_key.insert(id, key.to_string());
			return id;
		}
		self.advance_id()
	}

	fn advance_id(&mut self) -> u32 {
		let id = self.next_id;
		let next = (self.next_id + 1) & 0xffffff;
		self.next_id = if next == 0 { 1 } else { next };
		id
	}

	/// Retire the graphic held under `key` as soon as no frame shows it, instead
	/// of when the residency sweep reaches it. For an owner that replaced the
	/// image for good, so superseded revisions never push older images (scrollback
	/// included) out of the store.
	pub fn release(&mut self, key: &str) {
		if let Some(id) = self.key_to_id.get(key).copied() {
			self.release_id(id);
		}
	}

	/// [`ImageBudget::release`] by id, for an owner holding the component: an
	/// image retired by the residency sweep and transmitted again keeps its id but
	/// no longer maps from its key.
	pub fn release_id(&mut self, id: u32) {
		self.released.insert(id);
	}

	/// Start an alternate-buffer lifecycle. Call once per `?1049h`, before the
	/// first pass of the fullscreen overlay or resize borrow that owns the buffer.
	///
	/// The alt split is a claim about the frame standing on that surface, and
	/// `?1049h` hands over a cleared one: the previous occupant's threshold would
	/// suppress this buffer's leading images against a frame that no longer
	/// exists, painting them as text until a corrective render lands. Passes
	/// *within* one lifecycle must keep sharing the split — that is what lets an
	/// over-cap discovery pass converge before the frame is emitted.
	pub fn begin_alt_screen_lifecycle(&mut self) {
		self.alt_split.reset();
		self.live_ids[Surface::Alt.index()].clear();
		// `?1049h` hands over an empty graphics store as well as a cleared grid.
		self.transmitted[Surface::Alt.index()].clear();
	}

	/// Begin a render pass. Called by the renderer before composing the frame.
	/// Pass `stable` for a partial/throwaway pass that does not walk the whole
	/// tree in display order (the resize viewport fast path): [`ImageBudget::observe`]
	/// then replays the last committed per-id decision instead of one derived from
	/// call order, and the pass must NOT be closed with [`ImageBudget::end_pass`].
	///
	/// Pass `alt_screen` when the frame is painted on the alternate buffer
	/// (fullscreen overlay, resize borrow). The pass then reads and writes that
	/// surface's own [`SurfaceSplit`] and its live set adds to the recorded
	/// normal-screen one instead of replacing it, so a modal's threshold never
	/// reaches the transcript standing behind it.
	pub fn begin_pass(&mut self, stable: bool, alt_screen: bool) {
		self.pass_ids.clear();
		self.pass_suppression.clear();
		self.pass_index.clear();
		self.stable_pass = stable;
		self.surface = if alt_screen { Surface::Alt } else { Surface::Screen };
		self.split_surface = self.surface;
		// Composing for the screen means the alternate buffer holds no frame to
		// protect: live renders reach the normal-screen paths only when the
		// renderer has ruled out both alt-buffer owners, and the one caller
		// outside that dispatch — the shutdown history flush — writes `?1049l`
		// first. Note that leaving alt mode is not the same as unstacking a
		// fullscreen overlay: the flush must exclude one that is still stacked
		// from the pass itself, which is that caller's job, not this line's.
		if !alt_screen {
			self.live_ids[Surface::Alt.index()].clear();
		}
		self.applying_reset = !stable && self.cap > 0 && self.current_split().planned > self.current_split().on_terminal;
	}

	/// Record an image in display order and report whether it must render its
	/// text fallback this frame. Called by every [`Image`] during render —
	/// including on a cache hit, so the image keeps its display-order slot.
	///
	/// During a `stable` pass ([`ImageBudget::begin_pass`]) the call order and
	/// visible subset are not authoritative, so the decision is the surface's
	/// committed on-terminal split, keyed by id — order- and
	/// partiality-independent.
	///
	/// Inside [`measure_render`] the call claims no slot: it answers what the
	/// image's slot in the last full pass implies under the planned threshold,
	/// which is what the display-order render that follows will decide once the
	/// split has converged.
	pub fn observe(&mut self, image_id: u32) -> bool {
		if measure_render_depth() > 0 {
			MEASURED_IMAGE_OBSERVATIONS.with(|count| count.set(count.get() + 1));
		}
		if let Some(existing) = self.pass_suppression.get(&image_id) {
			return *existing;
		}
		if self.stable_pass {
			let suppressed = self.cap > 0 && self.last_slot_below(image_id, self.current_split().on_terminal);
			self.pass_suppression.insert(image_id, suppressed);
			if suppressed {
				self.forget_key_for_id(image_id);
			}
			return suppressed;
		}
		if measure_render_depth() > 0 {
			return self.cap > 0 && self.last_slot_below(image_id, self.current_split().planned);
		}
		let index = self.pass_ids.len();
		self.pass_ids.push(image_id);
		let suppressed = self.cap > 0 && index < self.current_split().planned;
		self.pass_suppression.insert(image_id, suppressed);
		self.pass_index.insert(image_id, index);
		if suppressed {
			self.forget_key_for_id(image_id);
		}
		suppressed
	}

	/// Whether `image_id` held a display slot below `threshold` in the last full
	/// pass on this surface.
	fn last_slot_below(&self, image_id: u32, threshold: usize) -> bool {
		self.current_split().slots.get(&image_id).is_some_and(|slot| *slot < threshold)
	}

	/// End a render pass. Returns true when the pass discovered a stricter budget
	/// and must be repeated before its terminal frame is emitted.
	pub fn end_pass(&mut self) -> bool {
		let total = self.pass_ids.len();
		let surface = self.split_surface.index();
		if self.applying_reset {
			// This frame replaced these with their text fallback, so their graphics
			// are retired as far as this surface is concerned.
			let start = self.screen_or_alt(surface).on_terminal;
			let end = self.screen_or_alt(surface).planned;
			for index in start..end.min(total) {
				self.retire(self.pass_ids[index]);
			}
			self.screen_or_alt_mut(surface).on_terminal = end;
			self.applying_reset = false;
		}
		self.screen_or_alt_mut(surface).last_total = total;
		let retry = self.reconcile(total);
		// Snapshot display order by id. Partial passes and measuring renders replay
		// their decisions from it instead of re-deriving them from a reversed or
		// tail-only walk.
		let mut slots = std::mem::take(&mut self.screen_or_alt_mut(surface).slots);
		slots.clear();
		for (index, id) in self.pass_ids.iter().enumerate() {
			slots.insert(*id, index);
		}
		self.screen_or_alt_mut(surface).slots = slots;
		retry
	}

	/// Bound the terminal's image store to `cap`. Demotion ([`ImageBudget::end_pass`])
	/// already retires the graphics this frame replaced with text; this sweeps the
	/// ones no frame shows any more — images the pass simply stopped observing.
	///
	/// Also records what this frame leaves standing on its surface, which is how
	/// the next pass on the *other* surface knows what it may not destroy.
	pub fn limit_resident_images(&mut self) {
		let surface = self.surface.index();
		let live: Vec<u32> =
			self.pass_ids.iter().copied().filter(|id| self.pass_shows_live(*id)).collect();
		self.live_ids[surface].clear();
		self.live_ids[surface].extend(live);
		let released: Vec<u32> = self.released.iter().copied().collect();
		for id in released {
			// `retire` refuses while this pass or the other surface still shows it.
			if self.transmitted[surface].contains(&id) {
				self.retire(id);
			}
			if self.is_transmitted(id) {
				continue;
			}
			self.forget_key_for_id(id);
			self.released.remove(&id);
		}
		if self.cap == 0 || self.transmitted[surface].len() <= self.cap as usize {
			return;
		}
		let transmitted: Vec<u32> = self.transmitted[surface].iter().copied().collect();
		for id in transmitted {
			if self.transmitted[surface].len() <= self.cap as usize {
				break;
			}
			self.retire(id);
		}
	}

	/// Whether this pass leaves `image_id` on its surface as a live graphic.
	///
	/// Not simply "was not suppressed". A pass decides suppression from the
	/// threshold standing at [`ImageBudget::begin_pass`], and [`ImageBudget::end_pass`]
	/// may then reconcile that threshold *downwards* — the frame is emitted with a
	/// text fallback the very next frame will replace with the graphic again.
	/// Reading such a decision as retirement would delete an image the surface is
	/// about to show, and `d=I` takes placements no repaint can restore. So a
	/// suppression the reconcile has since undercut counts as live.
	fn pass_shows_live(&self, image_id: u32) -> bool {
		let Some(suppressed) = self.pass_suppression.get(&image_id) else {
			return false;
		};
		if !*suppressed {
			return true;
		}
		// Absent (a `stable` pass replays the committed split rather than deriving
		// an order) counts as live, so an unknown decision never authorises a
		// delete.
		self.pass_index.get(&image_id).is_none_or(|index| *index >= self.current_split().planned)
	}

	/// Drop `image_id` from the painted surface's image store: queue its `d=I` (or
	/// cancel a transmit that never went out) and, once no surface holds its data,
	/// forget its placement ledger and key.
	///
	/// The single gate on every destruction path. `d=I` removes an image's
	/// placements everywhere on its screen, scrollback included, and a frame diff
	/// only rewrites rows whose text changed — so a graphic some standing frame
	/// still shows cannot be repaired once deleted, and must never be a
	/// candidate. Refuses when the in-flight pass renders the image live, or when
	/// the frame on any surface this pass is not repainting does. Returns whether
	/// it was retired.
	fn retire(&mut self, image_id: u32) -> bool {
		if self.pass_shows_live(image_id) {
			return false;
		}
		for surface in SURFACES {
			if surface != self.surface && self.live_ids[surface.index()].contains(&image_id) {
				return false;
			}
		}
		// A transmit queued by a discarded discovery pass never reached the
		// terminal, so cancel it instead of transmitting then purging.
		if self.pending_transmits[self.surface.index()].remove(&image_id).is_none() {
			self.purge_ids.push(image_id);
		}
		self.transmitted[self.surface.index()].remove(&image_id);
		if !self.is_transmitted(image_id) {
			self.placement_geometry.remove(&image_id);
		}
		self.forget_key_for_id(image_id);
		true
	}

	/// Whether any screen's store is believed to hold `image_id`'s data.
	fn is_transmitted(&self, image_id: u32) -> bool {
		SURFACES.iter().any(|surface| self.transmitted[surface.index()].contains(&image_id))
	}

	/// Image ids a destructive reset must delete explicitly, alongside its `d=A`.
	/// Emit only from that reset's repaint; clears the queue.
	pub fn take_reset_purge_ids(&mut self) -> Vec<u32> {
		std::mem::take(&mut self.reset_purge_ids)
	}

	/// Image ids to delete from the terminal this frame; clears the pending set.
	pub fn take_purge_ids(&mut self) -> Vec<u32> {
		std::mem::take(&mut self.purge_ids)
	}

	/// All image ids believed to be loaded in any screen's store; clears tracking.
	pub fn take_all_transmitted_ids(&mut self) -> Vec<u32> {
		let mut ids: Vec<u32> = Vec::new();
		for surface in SURFACES {
			for id in &self.transmitted[surface.index()] {
				if !ids.contains(id) {
					ids.push(*id);
				}
			}
			self.transmitted[surface.index()].clear();
			self.pending_transmits[surface.index()].clear();
		}
		if ids.is_empty() {
			return ids;
		}
		ids.sort_unstable();
		self.purge_ids.clear();
		self.reset_purge_ids.clear();
		self.key_to_id.clear();
		self.id_to_key.clear();
		self.placement_geometry.clear();
		self.released.clear();
		for surface in SURFACES {
			self.live_ids[surface.index()].clear();
		}
		ids
	}

	/// Whether `image_id`'s data still needs to be transmitted to the surface the
	/// in-flight pass paints.
	pub fn should_transmit(&self, image_id: u32) -> bool {
		!self.transmitted[self.surface.index()].contains(&image_id)
	}

	/// Record a direct-placement image's source pixel geometry so the renderer can
	/// clip its placement to the visible slice at write time; cleared when the
	/// image is purged from the terminal store.
	pub fn register_placement_geometry(&mut self, image_id: u32, width_px: u32, height_px: u32) {
		self.placement_geometry.insert(image_id, ImageDimensions::new(width_px, height_px));
	}

	/// Source dimensions used by the frame writer to clip a direct placement.
	pub fn get_placement_geometry(&self, image_id: u32) -> Option<ImageDimensions> {
		self.placement_geometry.get(&image_id).copied()
	}

	/// Queue a one-time transmit for `image_id` on the surface the in-flight pass
	/// paints. No-op if that surface already holds the data, so a repeated call
	/// (e.g. a width-change re-render) never re-sends it.
	pub fn enqueue_transmit(&mut self, image_id: u32, sequence: String) {
		let surface = self.surface.index();
		if !self.transmitted[surface].insert(image_id) {
			return;
		}
		self.pending_transmits[surface].insert(image_id, sequence);
	}

	/// Whether the in-flight pass's surface has image data queued but not yet
	/// written.
	pub fn has_pending_transmits(&self) -> bool {
		!self.pending_transmits[self.surface.index()].is_empty()
	}

	/// True when the budget has nothing in flight on either surface: no live
	/// images observed on the last pass, no queued transmits, no pending purges,
	/// and no stricter threshold left to apply. A component-scoped frame may skip
	/// the observe pass only then — a partial tree walk would under-count display
	/// order.
	pub fn quiescent(&self) -> bool {
		if !self.purge_ids.is_empty() {
			return false;
		}
		for surface in SURFACES {
			if !self.pending_transmits[surface.index()].is_empty() {
				return false;
			}
		}
		for split in [&self.screen_split, &self.alt_split] {
			if split.last_total != 0 || split.planned != split.on_terminal {
				return false;
			}
		}
		true
	}

	/// Transmit sequences to write before this frame's placements on its surface;
	/// clears that queue.
	pub fn take_transmits(&mut self) -> Vec<String> {
		let pending = &mut self.pending_transmits[self.surface.index()];
		if pending.is_empty() {
			return Vec::new();
		}
		std::mem::take(pending).into_values().collect()
	}

	/// Drop the normal screen's transmit tracking so every still-live image
	/// re-enqueues its data (`a=t`) on the next render. Recovers when the terminal
	/// dropped the original transmit — e.g. Ghostty discarding graphics sent during
	/// its post-startup window — where a placement-only replay can never bind a
	/// Unicode placeholder. Pair with a component invalidate + forced repaint so
	/// the data and placement re-emit together; keeps no base64 in budget state
	/// (the transmit-once design).
	pub fn forget_transmitted(&mut self) {
		let transmitted = std::mem::take(&mut self.transmitted[Surface::Screen.index()]);
		let pending = std::mem::take(&mut self.pending_transmits[Surface::Screen.index()]);
		if transmitted.is_empty() && pending.is_empty() {
			return;
		}
		for id in transmitted {
			if !pending.contains_key(&id) {
				self.reset_purge_ids.push(id);
			}
		}
		// The ids go to `reset_purge_ids`, drained only by the destructive repaint
		// itself — never to `purge_ids`, which any frame drains. That is how a
		// deletion used to ride out on an alternate-buffer frame and leave the
		// normal screen blank with no repaint left to restore it.
		//
		// `d=A` alone is not enough to skip these: Kitty excludes *virtual*
		// placements from it, and erasing placeholder text does not remove the
		// prototype either. Forgetting drops the id from tracking, so without an
		// explicit `d=I` no later sweep can ever find that placement again.
	}

	/// Release `id`'s stable key so a component recreated under it gets a fresh id
	/// — but only once the terminal no longer holds `id`'s data, because a key must
	/// never resolve to an id whose graphic is gone.
	///
	/// Key lifetime follows residency, not the live/text split. The two usually
	/// agree: an image shown as text has had its graphic purged. They diverge when
	/// `retire` refuses, which leaves a suppressed image resident on another
	/// surface — and releasing a resident id's key orphans it. The recreation mints
	/// a new id, nothing observes the old one again, and the next pass retires it,
	/// deleting every placement it ever made including scrollback copies.
	fn forget_key_for_id(&mut self, id: u32) {
		if self.is_transmitted(id) {
			return;
		}
		let Some(key) = self.id_to_key.remove(&id) else {
			return;
		};
		if self.key_to_id.get(&key).is_some_and(|mapped| *mapped == id) {
			self.key_to_id.remove(&key);
		}
	}

	fn reconcile(&mut self, total: usize) -> bool {
		let desired = if self.cap > 0 { total.saturating_sub(self.cap as usize) } else { 0 };
		let split = self.current_split();
		if desired == split.planned {
			// Budget relaxed without a stricter frame (cap raised or images
			// removed): surviving graphics are untouched and re-exposed rows
			// repaint normally, so just track the looser threshold.
			let on_terminal = split.on_terminal.min(split.planned);
			self.current_split_mut().on_terminal = on_terminal;
			return false;
		}
		let retry = desired > split.on_terminal;
		let on_terminal = split.on_terminal;
		self.current_split_mut().planned = desired;
		// More images must be demoted than the terminal shows: schedule the purge
		// + full-redraw frame. Fewer: no ghosts to clear, so just catch the
		// tracking up — a normal repaint re-exposes the un-demoted images. Either
		// way a render is needed to apply the new threshold.
		if desired <= on_terminal {
			self.current_split_mut().on_terminal = desired;
		}
		self.request_render();
		retry
	}

	fn current_split(&self) -> &SurfaceSplit {
		match self.split_surface {
			Surface::Screen => &self.screen_split,
			Surface::Alt => &self.alt_split,
		}
	}

	fn current_split_mut(&mut self) -> &mut SurfaceSplit {
		match self.split_surface {
			Surface::Screen => &mut self.screen_split,
			Surface::Alt => &mut self.alt_split,
		}
	}

	fn screen_or_alt(&self, surface: usize) -> &SurfaceSplit {
		if surface == Surface::Alt.index() {
			&self.alt_split
		} else {
			&self.screen_split
		}
	}

	fn screen_or_alt_mut(&mut self, surface: usize) -> &mut SurfaceSplit {
		if surface == Surface::Alt.index() {
			&mut self.alt_split
		} else {
			&mut self.screen_split
		}
	}
}

/// Newest SIXEL encode: target size, compositing background, and settled
/// sequence (`None` while no encode has landed, `Some(None)` when it failed).
#[derive(Clone)]
struct SixelEncode {
	width_px: u32,
	height_px: u32,
	background_rgb: u32,
	sequence: Option<Option<String>>,
}

/// Outcome of asking [`Image`] for its SIXEL sequence. The original also has a
/// *pending* state while an off-thread encode runs; the synchronous encoder this
/// port delegates to always settles within the call, so it needs no variant.
enum SixelStatus {
	/// Settled sequence ready to place.
	Ready(String),
	/// The encode failed.
	Failed,
}

/// Column/row box an image is fitted into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageFit {
	/// Fitted cell columns.
	pub columns: usize,
	/// Fitted cell rows.
	pub rows: usize,
}

/// One frame's graphics output for an [`Image`].
struct RenderedGraphics {
	sequence: Option<String>,
	lines: Option<Vec<String>>,
	rows: usize,
	transmit: Option<String>,
}

/// The described `image` node plus the link policy it was built under (an `off`
/// switch drops its `href`).
#[derive(Clone)]
struct CachedNative {
	linked: bool,
	node: NativeNode,
}

/// One inline image.
pub struct Image {
	base64_data: String,
	mime_type: String,
	dimensions: ImageDimensions,
	theme: ImageTheme,
	options: ImageOptions,
	budget: Option<Arc<Mutex<ImageBudget>>>,
	image_id: Option<u32>,

	cached_lines: Option<Vec<String>>,
	cached_width: Option<u16>,
	cached_suppressed: bool,
	cached_image_protocol: Option<ImageProtocol>,
	cached_cell: CellDimensions,
	cached_sixel_background: Option<u32>,
	cached_kitty_unicode_placeholders: bool,
	cached_linked: bool,
	/// Tallest graphic placement this image has rendered. The text fallback pads
	/// itself to this height so a budget demotion never shrinks the block (its
	/// rows may already be committed to native scrollback).
	rendered_graphic_rows: usize,
	/// Described node and the link policy it was built under.
	native: Mutex<Option<CachedNative>>,
	/// Newest SIXEL encode.
	sixel: Option<SixelEncode>,
}

impl Image {
	/// An image of `base64_data`/`mime_type`; `dimensions` overrides the
	/// header-probed source size.
	pub fn new(
		base64_data: impl Into<String>,
		mime_type: impl Into<String>,
		theme: ImageTheme,
		options: ImageOptions,
		dimensions: Option<ImageDimensions>,
	) -> Self {
		let base64_data = base64_data.into();
		let mime_type = mime_type.into();
		let dimensions = dimensions
			.or_else(|| probe_image_dimensions(&base64_data, &mime_type))
			.unwrap_or(ImageDimensions::new(800, 600));
		let image_id = options.budget.as_ref().map(|budget| {
			budget
				.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner())
				.acquire_id(options.image_key.as_deref())
		});
		Image {
			base64_data,
			mime_type,
			dimensions,
			theme,
			budget: options.budget.clone(),
			options,
			image_id,
			cached_lines: None,
			cached_width: None,
			cached_suppressed: false,
			cached_image_protocol: None,
			cached_cell: CellDimensions::default(),
			cached_sixel_background: None,
			cached_kitty_unicode_placeholders: false,
			cached_linked: false,
			rendered_graphic_rows: 0,
			native: Mutex::new(None),
			sixel: None,
		}
	}

	/// Source metadata, without exposing the encoded image buffer.
	pub fn debug_state(&self) -> serde_json::Value {
		serde_json::json!({
			"mimeType": self.mime_type,
			"widthPx": self.dimensions.width_px,
			"heightPx": self.dimensions.height_px,
			"filename": self.options.filename,
			"imageId": self.image_id,
			"suppressed": self.cached_suppressed,
		})
	}

	/// Source pixel dimensions, as probed or passed in.
	pub fn dimensions(&self) -> ImageDimensions {
		self.dimensions
	}

	/// The budget-assigned graphics id, when this image is budgeted.
	pub fn image_id(&self) -> Option<u32> {
		self.image_id
	}

	/// Refit to a new row ceiling; a SIXEL reserves its rows until the encode
	/// lands. A demoted fallback keeps the height already drawn only within the
	/// new ceiling: the owner chose to shrink the block.
	pub fn set_max_height_cells(&mut self, max_height_cells: Option<u16>) {
		if self.options.max_height_cells == max_height_cells {
			return;
		}
		self.options.max_height_cells = max_height_cells;
		if let Some(max_height_cells) = max_height_cells {
			self.rendered_graphic_rows = self.rendered_graphic_rows.min(usize::from(max_height_cells));
		}
		*self.native.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
		self.invalidate();
	}

	/// Drop the cached rows; the next render rebuilds them.
	pub fn release_render_caches(&mut self) {
		self.cached_lines = None;
		self.cached_width = None;
	}

	/// Retire this image's graphic as soon as no frame shows it: for an owner
	/// closing it for good.
	pub fn release(&self) {
		if let (Some(image_id), Some(budget)) = (self.image_id, self.budget.as_ref()) {
			budget.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).release_id(image_id);
		}
	}

	/// SIXEL sequence for a target size and background. A settled encode is what
	/// this frame places, so the synchronous encode settles the cache entry here;
	/// only a failure falls back.
	fn sixel_sequence(&mut self, width_px: u32, height_px: u32, background_rgb: u32) -> SixelStatus {
		let same_input = self.sixel.as_ref().is_some_and(|sixel| {
			sixel.width_px == width_px && sixel.height_px == height_px && sixel.background_rgb == background_rgb
		});
		if same_input {
			if let Some(settled) = self.sixel.as_ref().and_then(|sixel| sixel.sequence.clone()) {
				return match settled {
					Some(sequence) => SixelStatus::Ready(sequence),
					None => SixelStatus::Failed,
				};
			}
		}
		// The registered encoder is synchronous, so the entry always settles here:
		// the original's off-thread branch differs only in when the cache is
		// filled and that it then asks for a repaint.
		let sequence = encode_sixel_now(&self.base64_data, width_px, height_px, background_rgb);
		let status = match sequence.clone() {
			Some(sequence) => SixelStatus::Ready(sequence),
			None => SixelStatus::Failed,
		};
		self.sixel = Some(SixelEncode { width_px, height_px, background_rgb, sequence: Some(sequence) });
		status
	}

	/// Ask for a repaint through the option's callback, else the budget's.
	fn request_render(&self) {
		if let Some(request_render) = self.options.request_render.as_ref() {
			request_render();
		} else if let Some(budget) = self.budget.as_ref() {
			budget.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).request_render();
		}
	}

	/// Whether this image's data still needs transmitting to the painted surface.
	fn should_transmit(&self) -> bool {
		match (self.budget.as_ref(), self.image_id) {
			(Some(budget), Some(image_id)) => budget
				.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner())
				.should_transmit(image_id),
			_ => false,
		}
	}

	/// Register this image with the budget in display order; answers whether it
	/// must render its text fallback.
	fn observe(&self) -> bool {
		match (self.budget.as_ref(), self.image_id) {
			(Some(budget), image_id) => budget
				.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner())
				.observe(image_id.unwrap_or(0)),
			_ => false,
		}
	}

	fn fit(&self, max_width_cells: u16) -> ImageFit {
		calculate_image_fit(
			self.dimensions,
			Some(usize::from(max_width_cells)),
			self.options.max_height_cells.map(usize::from),
			cell_dimensions(),
		)
	}

	/// Graphics output for this frame; `None` when the protocol cannot place it
	/// (a failed SIXEL encode), which renders the text fallback instead.
	fn render_graphics(
		&mut self,
		fit: ImageFit,
		protocol: ImageProtocol,
		unicode_placeholders: bool,
		sixel_background: Option<u32>,
		include_transmit: bool,
	) -> Option<RenderedGraphics> {
		match protocol {
			ImageProtocol::Kitty => {
				let Some(image_id) = self.image_id else {
					// No stable id (e.g. no budget): self-contained transmit-and-display.
					let sequence = encode_kitty(&self.base64_data, fit.columns, fit.rows);
					return Some(RenderedGraphics { sequence: Some(sequence), lines: None, rows: fit.rows, transmit: None });
				};
				let transmit =
					if include_transmit { Some(encode_kitty_transmit(&self.base64_data, image_id)) } else { None };
				// Unicode placeholders render the image as real text cells (which
				// survive horizontal slicing, reflow and overlaps) instead of a
				// cursor-positioned `a=p` placement. Falls back to direct placement
				// when disabled or when the grid exceeds the diacritic table's
				// addressable cell range.
				if unicode_placeholders && kitty_placeholders_fit(fit.columns, fit.rows) {
					let lines = render_kitty_placeholder_lines(
						image_id,
						Some(image_id),
						fit.columns,
						fit.rows,
						&ProcessEnv,
					);
					return Some(RenderedGraphics { sequence: None, lines: Some(lines), rows: fit.rows, transmit });
				}
				// Direct placement: re-emit only the tiny `a=p` on repaints.
				let sequence = encode_kitty_placement(image_id, Some(image_id), fit.columns, fit.rows);
				Some(RenderedGraphics { sequence: Some(sequence), lines: None, rows: fit.rows, transmit })
			}
			ImageProtocol::Sixel => {
				// Encode the cell box itself, so a raster drawn at the cell size is
				// placed 1:1 instead of resampled. The encoder drops a partial last
				// 6px band rather than letting the terminal pad it into the next row;
				// only a box under one band (sub-6px cells) still takes the rows of a
				// whole one.
				let cells = cell_dimensions();
				let cell_height = cells.height_px.max(1);
				let height_px = fit.rows as u32 * cells.height_px;
				let width_px = fit.columns as u32 * cells.width_px;
				let rows = if height_px < 6 { 6u32.div_ceil(cell_height) as usize } else { fit.rows };
				let sequence = match self.sixel_sequence(width_px, height_px, sixel_background.unwrap_or_default()) {
					SixelStatus::Ready(sequence) => sequence,
					SixelStatus::Failed => return None,
				};
				Some(RenderedGraphics { sequence: Some(sequence), lines: None, rows, transmit: None })
			}
			ImageProtocol::Iterm2 => {
				// The height is the rows reserved for the image, not `auto`: iTerm2
				// resolves `auto` only for a session on screen, so an image written to
				// a background tab is dropped, and its rows stay blank in scrollback
				// too.
				let sequence = encode_iterm2(&self.base64_data, fit.columns, fit.rows, true);
				Some(RenderedGraphics { sequence: Some(sequence), lines: None, rows: fit.rows, transmit: None })
			}
		}
	}

	/// Text fallback, height-preserving once a graphic has rendered: a demoted
	/// image must keep occupying the rows its placement used, because those rows
	/// may already be committed to native scrollback — shrinking the block would
	/// shift everything below it and force the renderer's commit-resync (stale
	/// band + recommit). Reserved rows stay non-plain so blank-edge trimming
	/// cannot collapse the block either.
	fn fallback_lines(&self) -> Vec<String> {
		let image = ImageContent::new(self.base64_data.clone(), self.mime_type.clone());
		let fallback = (self.theme.fallback_color)(&linked_image_text(
			&image_fallback(&self.mime_type, Some(self.dimensions), self.options.filename.as_deref()),
			&image,
			self.options.source_path.as_deref(),
		));
		if self.rendered_graphic_rows <= 1 {
			return vec![fallback];
		}
		let mut lines = vec![RESERVED_IMAGE_ROW.to_string(); self.rendered_graphic_rows - 1];
		lines.push(fallback);
		lines
	}

	/// A native `image` backed by a content-addressed blob; the terminal fits it.
	/// Cell caps become `ch`/`lines` bounds. The inline-image budget and graphics
	/// protocols do not apply.
	pub fn describe(&self, _cx: &DescribeContext) -> NativeNode {
		let linked = image_links_enabled();
		if let Some(cached) = self.native.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref() {
			if cached.linked == linked {
				return cached.node.clone();
			}
		}
		let max_width = self.options.max_width_cells;
		let max_height = self.options.max_height_cells;
		let image = ImageContent::new(self.base64_data.clone(), self.mime_type.clone());
		let href = transcript_image_link(&image, self.options.source_path.as_deref());
		let mut props = Props::new();
		props.set_str(
			"alt",
			image_fallback(&self.mime_type, Some(self.dimensions), self.options.filename.as_deref()),
		);
		props.set_i64("w", i64::from(self.dimensions.width_px));
		props.set_i64("h", i64::from(self.dimensions.height_px));
		if max_width.unwrap_or(0) > 0 || max_height.unwrap_or(0) > 0 {
			let mut bounds = serde_json::Map::new();
			if let Some(width) = max_width.filter(|width| *width > 0) {
				bounds.insert("w".to_string(), serde_json::json!(format!("{width}ch")));
			}
			if let Some(height) = max_height.filter(|height| *height > 0) {
				bounds.insert("h".to_string(), serde_json::json!(format!("{height}lines")));
			}
			props.insert("max", serde_json::Value::Object(bounds));
		}
		for (key, value) in openable_image_props(href.as_deref()).into_map() {
			props.insert(key, value);
		}
		let bytes = decode_base64(&self.base64_data).unwrap_or_default();
		let node = image_node(&bytes, &self.mime_type, props, None);
		*self.native.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(CachedNative { linked, node: node.clone() });
		node
	}
}

impl Component for Image {
	fn render(&mut self, width: u16) -> Vec<String> {
		let image_protocol = terminal().image_protocol;
		let has_protocol = image_protocol.is_some();
		let cells = cell_dimensions();
		let unicode_placeholders = get_kitty_graphics().unicode_placeholders;
		let sixel_background =
			if image_protocol == Some(ImageProtocol::Sixel) { Some(get_terminal_background_color()) } else { None };
		// `observe` must run on every pass — even a cache hit — so the image keeps
		// its display-order slot in the budget. Only graphics-capable frames count
		// toward (and are demoted by) the budget; without a protocol every image is
		// already text.
		let suppressed = has_protocol && self.observe();
		// Only Kitty images with a budget id transmit their data separately from
		// the placement; a pending re-transmit (after a purge or history clear) must
		// rebuild the lines. SIXEL and iTerm2 carry the image inside the line
		// itself and never register a transmit, so gating their cache on it would
		// re-encode the full image on every render pass.
		let transmits_separately = image_protocol == Some(ImageProtocol::Kitty) && self.image_id.is_some();
		let linked = image_links_enabled();
		let sixel_pending = self.sixel.as_ref().is_some_and(|sixel| sixel.sequence.is_none());

		if self.cached_lines.is_some()
			&& self.cached_width == Some(width)
			&& self.cached_suppressed == suppressed
			&& self.cached_image_protocol == image_protocol
			&& self.cached_cell == cells
			&& self.cached_sixel_background == sixel_background
			&& self.cached_kitty_unicode_placeholders == unicode_placeholders
			// The text fallback embeds a link only under an enabled hyperlink policy.
			&& self.cached_linked == linked
			&& !(transmits_separately && self.should_transmit())
			// Cached rows reserved for a pending SIXEL encode must not reach
			// scrollback.
			&& !(scrollback_render_depth() > 0 && sixel_pending)
		{
			return self.cached_lines.clone().unwrap_or_default();
		}

		let max_width = match self.options.max_width_cells {
			Some(cap) if cap > 0 => image_max_columns(width).min(cap),
			_ => image_max_columns(width),
		};

		let mut lines: Vec<String>;
		if has_protocol && !suppressed {
			let fit = self.fit(max_width);
			let protocol = image_protocol.unwrap_or(ImageProtocol::Kitty);
			let include_transmit = transmits_separately && self.should_transmit();
			match self.render_graphics(fit, protocol, unicode_placeholders, sixel_background, include_transmit) {
				Some(rendered) => {
					// Transmit the data once (keyed by id); thereafter the placement is
					// all that is re-emitted, so repaints never re-send the base64.
					if let (Some(transmit), Some(image_id), Some(budget)) =
						(rendered.transmit.as_ref(), self.image_id, self.budget.as_ref())
					{
						budget
							.lock()
							.unwrap_or_else(|poisoned| poisoned.into_inner())
							.enqueue_transmit(image_id, transmit.clone());
					}
					if let Some(placeholder_lines) = rendered.lines {
						// Unicode placeholders: the image is already a block of real
						// text-cell lines (line 0 carries the virtual-placement APC). No
						// cursor moves.
						lines = placeholder_lines;
					} else {
						// Direct placement: return `rows` lines so the renderer accounts
						// for image height. First (rows-1) lines are empty (the renderer
						// clears them); the last saves the final-row cursor, moves up to
						// the image origin, emits the image sequence, then restores the
						// final-row cursor. When the block straddles the viewport top, the
						// renderer crops the source raster for SIXEL or rewrites the Kitty
						// placement's visible source slice from the geometry registered
						// here.
						if let (Some(image_id), Some(budget)) = (self.image_id, self.budget.as_ref()) {
							budget
								.lock()
								.unwrap_or_else(|poisoned| poisoned.into_inner())
								.register_placement_geometry(
									image_id,
									self.dimensions.width_px,
									self.dimensions.height_px,
								);
						}
						let cursor_rows = rendered.rows.saturating_sub(1);
						lines = vec![RESERVED_IMAGE_ROW.to_string(); cursor_rows];
						let move_up = if cursor_rows > 0 { format!("\x1b[{cursor_rows}A") } else { String::new() };
						let placement = format!("{move_up}{}", rendered.sequence.unwrap_or_default());
						lines.push(if cursor_rows > 0 {
							format!("{SAVE_CURSOR}{placement}{RESTORE_CURSOR}")
						} else {
							placement
						});
					}
					self.rendered_graphic_rows = self.rendered_graphic_rows.max(lines.len());
				}
				None => lines = self.fallback_lines(),
			}
		} else {
			lines = self.fallback_lines();
		}

		self.cached_lines = Some(lines.clone());
		self.cached_width = Some(width);
		self.cached_suppressed = suppressed;
		self.cached_image_protocol = image_protocol;
		self.cached_cell = cells;
		self.cached_sixel_background = sixel_background;
		self.cached_kitty_unicode_placeholders = unicode_placeholders;
		self.cached_linked = linked;

		lines
	}

	fn invalidate(&mut self) {
		self.cached_lines = None;
		self.cached_width = None;
	}
}

impl NativeDescribe for Image {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		self.describe(cx)
	}
}

/// Largest column count the fit allows.
pub fn calculate_image_fit(
	image_dimensions: ImageDimensions,
	max_columns: Option<usize>,
	max_rows: Option<usize>,
	cell_dims: CellDimensions,
) -> ImageFit {
	let max_columns = max_columns.map(|columns| columns.max(1));
	let max_rows = max_rows.map(|rows| rows.max(1));
	let cell_width = f64::from(cell_dims.width_px);
	let cell_height = f64::from(cell_dims.height_px);
	let width_px = f64::from(image_dimensions.width_px.max(1));
	let height_px = f64::from(image_dimensions.height_px.max(1));

	if max_columns.is_none() && max_rows.is_none() {
		let columns = (width_px / cell_width).ceil().max(1.0);
		let rows = (height_px / cell_height).ceil().max(1.0);
		return ImageFit { columns: columns as usize, rows: rows as usize };
	}

	let max_width_px = max_columns.map_or(f64::INFINITY, |columns| columns as f64 * cell_width);
	let max_height_px = max_rows.map_or(f64::INFINITY, |rows| rows as f64 * cell_height);
	let scale = (max_width_px / width_px).min(max_height_px / height_px);
	let fitted_width_px = width_px * scale;
	let fitted_height_px = height_px * scale;

	let columns = (fitted_width_px / cell_width).floor().max(1.0);
	let rows = (fitted_height_px / cell_height).ceil().max(1.0);
	ImageFit {
		columns: max_columns.map_or(columns as usize, |max| (columns as usize).min(max)),
		rows: max_rows.map_or(rows as usize, |max| (rows as usize).min(max)),
	}
}

/// The text an image shows when it cannot be (or must not be) drawn as graphics.
fn image_fallback(mime_type: &str, dimensions: Option<ImageDimensions>, filename: Option<&str>) -> String {
	let mut parts: Vec<String> = Vec::new();
	if let Some(filename) = filename {
		parts.push(filename.to_string());
	}
	parts.push(format!("[{mime_type}]"));
	if let Some(dimensions) = dimensions {
		parts.push(format!("{}x{}", dimensions.width_px, dimensions.height_px));
	}
	format!("[Image: {}]", parts.join(" "))
}

/// Group a Kitty APC into 4096-character chunks, wrapping each for tmux.
fn chunk_kitty_apc(lead_params: &str, base64_data: &str) -> String {
	const CHUNK_SIZE: usize = 4096;
	if base64_data.len() <= CHUNK_SIZE {
		return wrap_tmux_passthrough_if_needed(&format!("\x1b_G{lead_params};{base64_data}\x1b\\"), &ProcessEnv);
	}
	let mut chunks = String::new();
	let mut offset = 0;
	let mut is_first = true;
	while offset < base64_data.len() {
		let end = (offset + CHUNK_SIZE).min(base64_data.len());
		let chunk = &base64_data[offset..end];
		let is_last = offset + CHUNK_SIZE >= base64_data.len();
		if is_first {
			chunks.push_str(&wrap_tmux_passthrough_if_needed(
				&format!("\x1b_G{lead_params},m=1;{chunk}\x1b\\"),
				&ProcessEnv,
			));
			is_first = false;
		} else if is_last {
			chunks.push_str(&wrap_tmux_passthrough_if_needed(&format!("\x1b_Gq=2,m=0;{chunk}\x1b\\"), &ProcessEnv));
		} else {
			chunks.push_str(&wrap_tmux_passthrough_if_needed(&format!("\x1b_Gq=2,m=1;{chunk}\x1b\\"), &ProcessEnv));
		}
		offset += CHUNK_SIZE;
	}
	chunks
}

/// Transmit-and-display (`a=T`) — the self-contained form used when no stable id
/// is available.
fn encode_kitty(base64_data: &str, columns: usize, rows: usize) -> String {
	let mut params = vec!["a=T".to_string(), "f=100".to_string(), "q=2".to_string(), "C=1".to_string()];
	if columns > 0 {
		params.push(format!("c={columns}"));
	}
	if rows > 0 {
		params.push(format!("r={rows}"));
	}
	chunk_kitty_apc(&params.join(","), base64_data)
}

/// Transmit image data only (`a=t`), keyed by `image_id`, without displaying it.
pub fn encode_kitty_transmit(base64_data: &str, image_id: u32) -> String {
	chunk_kitty_apc(&format!("a=t,f=100,q=2,i={image_id}"), base64_data)
}

/// Display a previously transmitted image (`a=p`) at the cursor.
fn encode_kitty_placement(image_id: u32, placement_id: Option<u32>, columns: usize, rows: usize) -> String {
	let mut params = vec!["a=p".to_string(), "q=2".to_string(), "C=1".to_string(), format!("i={image_id}")];
	if let Some(placement_id) = placement_id {
		params.push(format!("p={placement_id}"));
	}
	if columns > 0 {
		params.push(format!("c={columns}"));
	}
	if rows > 0 {
		params.push(format!("r={rows}"));
	}
	wrap_tmux_passthrough_if_needed(&format!("\x1b_G{}\x1b\\", params.join(",")), &ProcessEnv)
}

/// iTerm2 inline image (`OSC 1337;File`), sized in cells.
fn encode_iterm2(base64_data: &str, columns: usize, rows: usize, preserve_aspect_ratio: bool) -> String {
	let mut params = vec!["inline=1".to_string()];
	if columns > 0 {
		params.push(format!("width={columns}"));
	}
	if rows > 0 {
		params.push(format!("height={rows}"));
	}
	if !preserve_aspect_ratio {
		params.push("preserveAspectRatio=0".to_string());
	}
	format!("\x1b]1337;File={}:{base64_data}\x07", params.join(";"))
}

/// SIXEL sequence for `base64_data` at the given pixel size, encoded
/// synchronously; `None` when no encoder is registered or the encode fails.
fn encode_sixel_now(base64_data: &str, width_px: u32, height_px: u32, background_rgb: u32) -> Option<String> {
	let encoder = sixel_encoder()?;
	let bytes = decode_base64(base64_data)?;
	encoder(&bytes, width_px, height_px, background_rgb).map(|sequence| ensure_sixel_raster_width(&sequence))
}

/// Source pixel dimensions probed from the payload's header.
fn probe_image_dimensions(base64_data: &str, mime_type: &str) -> Option<ImageDimensions> {
	let bytes = decode_base64(base64_data)?;
	if mime_type == "image/webp" {
		return webp_dimensions(&bytes).map(|(width, height)| ImageDimensions::new(width, height));
	}
	blob_image_dimensions(&bytes, mime_type).map(|(width, height)| ImageDimensions::new(width, height))
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
	if bytes.len() < 30 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
		return None;
	}
	let chunk = &bytes[12..16];
	if chunk == b"VP8 " {
		let width = u32::from(u16::from_le_bytes([bytes[26], bytes[27]])) & 0x3fff;
		let height = u32::from(u16::from_le_bytes([bytes[28], bytes[29]])) & 0x3fff;
		return Some((width, height));
	}
	if chunk == b"VP8L" {
		if bytes.len() < 25 {
			return None;
		}
		let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
		return Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
	}
	if chunk == b"VP8X" {
		let width = u32::from(bytes[24]) | (u32::from(bytes[25]) << 8) | (u32::from(bytes[26]) << 16);
		let height = u32::from(bytes[27]) | (u32::from(bytes[28]) << 8) | (u32::from(bytes[29]) << 16);
		return Some((width + 1, height + 1));
	}
	None
}

/// Standard base64 (with padding) to bytes; `None` on an unexpected character.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
	fn sextet(byte: u8) -> Option<u32> {
		match byte {
			b'A'..=b'Z' => Some(u32::from(byte - b'A')),
			b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
			b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
			b'+' => Some(62),
			b'/' => Some(63),
			_ => None,
		}
	}

	let mut out = Vec::with_capacity(input.len() / 4 * 3);
	let mut accumulator = 0u32;
	let mut bits = 0u32;
	for byte in input.bytes() {
		if byte == b'=' || byte.is_ascii_whitespace() {
			continue;
		}
		let value = sextet(byte)?;
		accumulator = (accumulator << 6) | value;
		bits += 6;
		if bits >= 8 {
			bits -= 8;
			out.push((accumulator >> bits) as u8);
			accumulator &= (1 << bits) - 1;
		}
	}
	Some(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Serializes the tests that redirect the hosted terminal behaviour and the
	/// SIXEL encoder; both are process-wide.
	static HOSTED_STATE: Mutex<()> = Mutex::new(());

	/// Redirects the hosted terminal behaviour the render paths read.
	struct TerminalOverride {
		protocol: Option<ImageProtocol>,
		placeholders: bool,
		_guard: std::sync::MutexGuard<'static, ()>,
	}

	impl TerminalOverride {
		fn apply(protocol: Option<ImageProtocol>, placeholders: bool) -> Self {
			let guard = HOSTED_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
			let previous = TerminalOverride {
				protocol: terminal().image_protocol,
				placeholders: get_kitty_graphics().unicode_placeholders,
				_guard: guard,
			};
			crate::terminal_capabilities::set_terminal_image_protocol(protocol);
			crate::kitty_graphics::set_kitty_graphics(crate::kitty_graphics::KittyGraphicsFeatures {
				unicode_placeholders: placeholders,
			});
			previous
		}
	}

	impl Drop for TerminalOverride {
		fn drop(&mut self) {
			crate::terminal_capabilities::set_terminal_image_protocol(self.protocol);
			crate::kitty_graphics::set_kitty_graphics(crate::kitty_graphics::KittyGraphicsFeatures {
				unicode_placeholders: self.placeholders,
			});
		}
	}

	/// A 1x1 PNG, so dimension probing has a real header to read.
	const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

	fn image(options: ImageOptions) -> Image {
		Image::new(PNG_1X1, "image/png", ImageTheme::plain(), options, None)
	}

	fn budget(cap: u32) -> Arc<Mutex<ImageBudget>> {
		Arc::new(Mutex::new(ImageBudget::new(cap, Arc::new(|| {}))))
	}

	#[test]
	fn image_max_columns_keeps_a_column_per_edge() {
		assert_eq!(image_max_columns(80), 78);
		assert_eq!(image_max_columns(2), 1);
		assert_eq!(image_max_columns(1), 1);
	}

	#[test]
	fn image_fallback_names_filename_mime_and_size() {
		let dimensions = ImageDimensions::new(640, 480);
		assert_eq!(image_fallback("image/png", Some(dimensions), None), "[Image: [image/png] 640x480]");
		assert_eq!(
			image_fallback("image/png", Some(dimensions), Some("plot.png")),
			"[Image: plot.png [image/png] 640x480]"
		);
	}

	#[test]
	fn png_header_dimensions_are_probed() {
		let dimensions = probe_image_dimensions(PNG_1X1, "image/png").expect("png header");
		assert_eq!(dimensions, ImageDimensions::new(1, 1));
		// A payload the probe cannot read falls back to the 800x600 default.
		let image = Image::new("not base64!", "image/png", ImageTheme::plain(), ImageOptions::default(), None);
		assert_eq!(image.dimensions(), ImageDimensions::new(800, 600));
	}

	#[test]
	fn acquired_ids_are_stable_per_key_and_fresh_without_one() {
		let mut budget = ImageBudget::new(2, Arc::new(|| {}));
		let keyed = budget.acquire_id(Some("tool:0"));
		assert_eq!(budget.acquire_id(Some("tool:0")), keyed);
		assert_ne!(budget.acquire_id(Some("tool:1")), keyed);
		let unkeyed = budget.acquire_id(None);
		assert_ne!(budget.acquire_id(None), unkeyed);
	}

	#[test]
	fn over_cap_passes_demote_the_leading_image() {
		let budget = budget(2);
		let mut ids = Vec::new();
		{
			let mut locked = budget.lock().unwrap();
			locked.begin_pass(false, false);
			for _ in 0..3 {
				let id = locked.acquire_id(None);
				ids.push(id);
			}
			for id in &ids {
				assert!(!locked.observe(*id), "first pass shows every image as graphics");
			}
			assert!(locked.end_pass(), "a stricter split needs another pass");
			assert_eq!(locked.take_purge_ids(), Vec::<u32>::new());
		}
		{
			let mut locked = budget.lock().unwrap();
			locked.begin_pass(false, false);
			assert!(locked.observe(ids[0]), "the oldest image is demoted");
			assert!(!locked.observe(ids[1]));
			assert!(!locked.observe(ids[2]));
			assert!(!locked.end_pass(), "the demotion frame converges");
			assert_eq!(locked.take_purge_ids(), vec![ids[0]]);
		}
	}

	#[test]
	fn released_ids_are_swept_once_no_surface_shows_them() {
		let budget = budget(8);
		let id;
		{
			let mut locked = budget.lock().unwrap();
			locked.begin_pass(false, false);
			id = locked.acquire_id(Some("figure"));
			assert!(!locked.observe(id));
			assert!(locked.should_transmit(id));
			locked.enqueue_transmit(id, "\x1b_Gtransmit\x1b\\".to_string());
			assert_eq!(locked.take_transmits(), vec!["\x1b_Gtransmit\x1b\\".to_string()]);
			locked.end_pass();
			locked.limit_resident_images();
			locked.release("figure");
			// Still standing on this surface: the sweep must not delete it.
			locked.limit_resident_images();
			assert_eq!(locked.take_purge_ids(), Vec::<u32>::new());
		}
		{
			let mut locked = budget.lock().unwrap();
			locked.begin_pass(false, false);
			locked.end_pass();
			locked.limit_resident_images();
			assert_eq!(locked.take_purge_ids(), vec![id]);
		}
	}

	#[test]
	fn measure_render_reports_whether_a_budgeted_image_was_seen() {
		let budget = budget(4);
		let mut image = image(ImageOptions { budget: Some(budget.clone()), ..ImageOptions::default() });
		let _override = TerminalOverride::apply(Some(ImageProtocol::Kitty), false);
		let quiet = measure_render(|| 7);
		assert_eq!(quiet.value, 7);
		assert!(!quiet.images);
		let measured = measure_render(|| image.render(20).len());
		assert!(measured.images);
	}

	#[test]
	fn kitty_without_a_budget_transmits_and_places_at_once() {
		let _override = TerminalOverride::apply(Some(ImageProtocol::Kitty), false);
		let mut image = image(ImageOptions { max_height_cells: Some(2), ..ImageOptions::default() });
		let lines = image.render(20);
		assert_eq!(lines.len(), 2, "one reserved row plus the placement row");
		assert_eq!(lines[0], RESERVED_IMAGE_ROW);
		assert!(lines[1].contains("\x1b_Ga=T"));
		assert!(lines[1].starts_with("\x1b7\x1b[1A"));
	}

	#[test]
	fn kitty_placeholders_draw_a_text_cell_block() {
		let _override = TerminalOverride::apply(Some(ImageProtocol::Kitty), true);
		let budget = budget(4);
		let keyed = ImageOptions {
			budget: Some(budget.clone()),
			image_key: Some("figure".to_string()),
			..ImageOptions::default()
		};
		let mut image = image(keyed.clone());
		let image_id = image.image_id().expect("budgeted");
		let lines = image.render(20);
		assert!(lines[0].contains(&crate::kitty_graphics::KITTY_PLACEHOLDER.to_string()));
		assert!(lines.iter().all(|line| !line.starts_with("\x1b7")));
		// A re-created component under the same key reuses the graphics id, so a
		// repaint replaces the placement instead of stacking one.
		assert_eq!(image(keyed).image_id(), Some(image_id));
	}

	#[test]
	fn sixel_uses_the_registered_encoder_and_falls_back_without_one() {
		let _override = TerminalOverride::apply(Some(ImageProtocol::Sixel), false);
		let mut image = image(ImageOptions { max_height_cells: Some(3), ..ImageOptions::default() });
		assert_eq!(image.render(20), vec!["[Image: [image/png] 1x1]".to_string()]);

		set_sixel_encoder(Some(Arc::new(|_bytes, width_px, _height_px, background| {
			Some(format!("\x1bPq\"1;1;{width_px};9;{background}"))
		})));
		let mut image = image(ImageOptions { max_height_cells: Some(3), ..ImageOptions::default() });
		let lines = image.render(20);
		assert_eq!(lines.len(), 3);
		assert!(lines[2].contains("\x1bPq"));
		set_sixel_encoder(None);
	}

	#[test]
	fn demoted_images_keep_the_rows_their_graphic_used() {
		let _override = TerminalOverride::apply(Some(ImageProtocol::Kitty), false);
		let budget = budget(1);
		let mut first = image(ImageOptions { budget: Some(budget.clone()), ..ImageOptions::default() });
		let mut second = image(ImageOptions { budget: Some(budget.clone()), ..ImageOptions::default() });
		{
			let mut locked = budget.lock().unwrap();
			locked.begin_pass(false, false);
		}
		let first_rows = first.render(20).len();
		assert!(first_rows > 1);
		second.render(20);
		budget.lock().unwrap().end_pass();
		{
			let mut locked = budget.lock().unwrap();
			locked.begin_pass(false, false);
		}
		let demoted = first.render(20);
		assert_eq!(demoted.len(), first_rows, "the fallback pads to the graphic's height");
		assert_eq!(demoted[0], RESERVED_IMAGE_ROW);
		assert!(demoted[first_rows - 1].contains("[Image: [image/png] 1x1]"));
	}

	#[test]
	fn describe_builds_a_blob_backed_image_node() {
		let image = image(ImageOptions {
			max_width_cells: Some(40),
			max_height_cells: Some(6),
			filename: Some("plot.png".to_string()),
			..ImageOptions::default()
		});
		let node = image.describe(&DescribeContext::new(80));
		assert_eq!(node.kind, crate::native::node::NodeKind::Image);
		assert_eq!(node.props.get_str("blob").map(str::len), Some(64));
		assert_eq!(node.props.get_str("alt"), Some("[Image: plot.png [image/png] 1x1]"));
		assert_eq!(node.props.get_i64("w"), Some(1));
		assert_eq!(node.props.get_i64("h"), Some(1));
		let bounds = node.props.get("max").expect("cell bounds");
		assert_eq!(bounds.get("w").and_then(|value| value.as_str()), Some("40ch"));
		assert_eq!(bounds.get("h").and_then(|value| value.as_str()), Some("6lines"));
	}
}
