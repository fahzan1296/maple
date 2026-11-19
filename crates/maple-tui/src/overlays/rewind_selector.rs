//! Reference: `l/src/overlays/rewind-selector.ts`.
//!
//! Fullscreen esc-esc rewind selector.
//!
//! The current session's branch is replayed on the alternate screen and a
//! dotted outline moves over the rendered transcript block the rewind would
//! land on, instead of listing user messages in a detached picker. Entries that
//! render nothing are never outlined, so a selection always rewinds to
//! something visible.
//!
//! When the outlined turn has sibling branches in the session tree, the region
//! below the divergence renders as a strip of half-width columns — the current
//! path first, each alternate branch beside it — and Left/Right slide between
//! them with an eased camera animation.
//!
//! Keys: Up/Down step through rendered items in transcript order (within the
//! active column when a strip is open), Left/Right slide between branch
//! variants at a fork and jump between user turns elsewhere, `f` opens a filter
//! (typing narrows the current path to matching items; Esc leaves the filter
//! with the selection kept), Enter rewinds to the outlined item, `a` loads
//! earlier turns without changing selection, Esc cancels.
//!
//! Deviations, all injected rather than dropped:
//! - `ChatTranscriptBuilder` and `TranscriptBrowser` have no crate module yet.
//!   [`RewindTranscript`] is the host's replayer — one call yields a column's
//!   per-block rows plus its [`OutlineTarget`]s — and the module lays out,
//!   outlines and scrolls those rows itself (via `transcript_outline`).
//! - The reference colours rows through the process-global theme and reaches
//!   the clock through `Date.now`; the theme and a monotonic millisecond clock
//!   are [`RewindSelectorDeps`] fields so a test can pin them.
//! - The native page describes each replayed block from its rows: the crate has
//!   no per-block `Component::describe` for transcript blocks.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::chat::transcript_entry::{TranscriptEntryLike, recent_transcript_entries};
use crate::chat::transcript_outline::{
	ComposedColumn, OutlineStyle, OutlineTarget, compose_outline_column, is_user_turn_entry,
	outline_visibility, position_rail, user_turn_label,
};
use crate::component::Component;
use crate::components::input::Input;
use crate::keybinding_matchers::{
	matches_app_tools_expand, matches_select_cancel, matches_select_down, matches_select_up,
};
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{NativeDescribe, text};
use crate::native::node::{
	DescribeContext, NativeNode, NodeKind, NodeText, Props, Reveal, RevealAt, Span,
};
use crate::native::overlay::{action_bar, action_button, action_hint, hints_row, ActionButtonOptions};
use crate::render::render_utils::{padding, slice_by_column};
use crate::text_shape::pad_to_width;
use crate::theme::theme_class::Theme;

/// Click action that picks a block's target on the native page.
pub const PICK: &str = "rewind-pick";
/// Blank columns between branch-strip columns.
pub const STRIP_GAP: usize = 2;
/// Duration of the branch-swap camera slide.
pub const SLIDE_MS: u64 = 160;
/// Width below which a branch column is never squeezed.
pub const MIN_COLUMN_WIDTH: usize = 24;
/// Entries the selector replays before `a` loads the rest.
pub const TAIL_LIMIT: usize = 600;

/// One alternate branch at a divergence: its root and message path
/// root → most-recent leaf.
#[derive(Clone, Debug, PartialEq)]
pub struct BranchVariantPath {
	/// Root id of the sibling branch.
	pub root_id: String,
	/// The branch's message path, root first.
	pub entries: Vec<TranscriptEntryLike>,
}

/// A replayed transcript column: per-block rows plus the selectable targets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RewindColumn {
	/// One row array per transcript block, in replay order.
	pub rows: Vec<Vec<String>>,
	/// Selectable blocks with their entry/turn identity.
	pub targets: Vec<OutlineTarget>,
}

/// The host's transcript replayer.
///
/// The reference constructs a `ChatTranscriptBuilder` per column; the crate has
/// no builder yet, so the host builds the column (and can cache it) and hands
/// back rows plus targets from [`crate::chat::transcript_outline::append_outline_entries`].
pub trait RewindTranscript {
	/// Replay `entries` into a column as rendered rows and selectable targets.
	fn replay(&mut self, entries: &[TranscriptEntryLike], expanded: bool) -> RewindColumn;
	/// Adopt the tools-expanded presentation for later replays.
	fn set_expanded(&mut self, _expanded: bool) {}
}

/// Lazily built transcript column for one alternate branch.
#[derive(Clone, Debug)]
struct SiblingColumn {
	root_id: String,
	column: RewindColumn,
	/// Short label for the column header: the branch's first user prompt.
	label: String,
}

/// A monotonic camera slide between variant positions.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slide {
	from: f64,
	to: f64,
	started: Instant,
}

/// Everything the selector needs from its host.
pub struct RewindSelectorDeps {
	/// The host's transcript replayer.
	pub transcript: Rc<std::cell::RefCell<dyn RewindTranscript>>,
	/// Fully resolved theme.
	pub theme: Theme,
	/// Active keybindings.
	pub bindings: KeybindingsManager,
	/// Rows in the terminal viewport.
	pub height: usize,
	/// Sibling branch paths of `turn_id`'s turn, excluding the turn itself.
	pub sibling_paths: Option<Box<dyn Fn(&str) -> Vec<BranchVariantPath>>>,
	/// The entry the outline starts on; the latest target without one.
	pub start_at: Option<String>,
	/// Rewind the session to `entry_id`.
	pub on_select: Box<dyn FnMut(&str)>,
	/// Dismiss the selector.
	pub on_cancel: Box<dyn FnMut()>,
	/// Ask the host to re-render.
	pub request_render: Box<dyn FnMut()>,
	/// Monotonic milliseconds, for the camera ease.
	pub now_ms: Box<dyn Fn() -> u64>,
}

impl RewindSelectorDeps {
	fn now(&self) -> u64 {
		(self.now_ms)()
	}
}

/// The fullscreen rewind selector over a session branch.
pub struct RewindSelectorComponent {
	transcript: Rc<std::cell::RefCell<dyn RewindTranscript>>,
	theme: Theme,
	bindings: KeybindingsManager,
	height: usize,
	sibling_paths: Option<Box<dyn Fn(&str) -> Vec<BranchVariantPath>>>,
	on_select: Box<dyn FnMut(&str)>,
	on_cancel: Box<dyn FnMut()>,
	request_render: Box<dyn FnMut()>,
	now_ms: Box<dyn Fn() -> u64>,

	/// Whole branch; the selector may currently replay only its tail.
	entries: Vec<TranscriptEntryLike>,
	/// True while older history is still unreplayed.
	truncated: bool,
	/// The replayed main column.
	main: RewindColumn,
	targets: Vec<OutlineTarget>,
	selected: usize,
	/// Per-main-target "shows at least one non-blank row", refreshed per frame.
	main_visible: Option<Vec<bool>>,
	/// Same, for the active sibling column.
	sibling_visible: Option<Vec<bool>>,
	expanded: bool,

	/// Branch strip columns, cached per divergence (turn id).
	variant_cache: std::cell::RefCell<HashMap<String, Vec<SiblingColumn>>>,
	/// 0 = current path column; 1..n = sibling column index + 1.
	active_variant: usize,
	/// Selected target within the active sibling column.
	sibling_selected: usize,
	slide: Option<Slide>,

	/// Filter field while the filter prompt is open; `None` shows the full
	/// transcript.
	filter_input: Option<Input>,
	/// First body row of the scroll window.
	scroll: usize,
	/// Rows of the last rendered body, for clamping the scroll window.
	body_height: usize,
}

impl std::fmt::Debug for RewindSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RewindSelectorComponent")
			.field("targets", &self.targets.len())
			.field("selected", &self.selected)
			.field("active_variant", &self.active_variant)
			.field("truncated", &self.truncated)
			.finish()
	}
}

/// Index of the target in `targets` (sorted, disjoint) whose blocks include
/// child `child`; `None` for none.
#[must_use]
pub fn target_at(targets: &[OutlineTarget], child: usize) -> Option<usize> {
	let mut lo = 0usize;
	let mut hi = targets.len();
	while lo < hi {
		let mid = (lo + hi) / 2;
		let target = &targets[mid];
		if child < target.start {
			hi = mid;
		} else if child >= target.end {
			lo = mid + 1;
		} else {
			return Some(mid);
		}
	}
	None
}

/// Nearest visible target to `selected`, preferring the one above.
#[must_use]
pub fn nearest_visible_target(visible: &[bool], selected: usize) -> usize {
	if visible.get(selected).copied().unwrap_or(true) {
		return selected;
	}
	let mut up = selected;
	while up > 0 {
		up -= 1;
		if visible.get(up).copied().unwrap_or(true) {
			return up;
		}
	}
	for (index, shows) in visible.iter().enumerate().skip(selected) {
		if *shows {
			return index;
		}
	}
	selected
}

/// Eased camera position between `from` and `to` after `elapsed_ms`; ease-out
/// cubic, matching the reference's `1 - (1 - t)³`.
#[must_use]
pub fn eased_slide_position(from: f64, to: f64, elapsed_ms: u64) -> f64 {
	if SLIDE_MS == 0 {
		return to;
	}
	let t = (elapsed_ms as f64 / SLIDE_MS as f64).min(1.0);
	let eased = 1.0 - (1.0 - t).powi(3);
	from + (to - from) * eased
}

/// One compiled filter word: whole-word for Latin/digit/underscore words,
/// substring otherwise (so a Chinese query finds text inside a sentence).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterWord {
	/// Lowercased word.
	pub word: String,
	/// Whether the match must fall on word boundaries.
	pub whole_word: bool,
}

impl FilterWord {
	/// Parse one whitespace-separated query word.
	#[must_use]
	pub fn new(word: &str) -> Self {
		let word = word.to_lowercase();
		let whole_word = !word.is_empty()
			&& word
				.chars()
				.all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
		FilterWord { word, whole_word }
	}

	/// Whether `haystack` (already lowercased) contains this word.
	#[must_use]
	pub fn matches(&self, haystack: &str) -> bool {
		if self.word.is_empty() {
			return true;
		}
		if !self.whole_word {
			return haystack.contains(&self.word);
		}
		let mut from = 0usize;
		while let Some(found) = haystack[from..].find(&self.word) {
			let start = from + found;
			let end = start + self.word.len();
			let before = haystack[..start].chars().next_back();
			let after = haystack[end..].chars().next();
			let boundary = |ch: Option<char>| ch.is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
			if boundary(before) && boundary(after) {
				return true;
			}
			from = end;
		}
		false
	}
}

/// Compile a filter query into its words; empty for a blank query.
#[must_use]
pub fn compile_filter(query: &str) -> Vec<FilterWord> {
	query
		.split_whitespace()
		.filter(|word| !word.is_empty())
		.map(FilterWord::new)
		.collect()
}

/// Lowercased plain text of a block's rows.
#[must_use]
pub fn plain_block_text(rows: &[String]) -> String {
	crate::ansi_scan::strip_ansi(&rows.join("\n")).to_lowercase()
}

/// Whether every filter word matches somewhere in `texts` for this target.
#[must_use]
pub fn target_matches(words: &[FilterWord], text: &str) -> bool {
	words.iter().all(|word| word.matches(text))
}

impl RewindSelectorComponent {
	/// Build the selector over a session branch and replay its tail.
	///
	/// `start_at` replays the whole branch when the tail does not hold it, so a
	/// "Rewind to here…" from a collapsed turn always lands on its target.
	pub fn new(entries: Vec<TranscriptEntryLike>, deps: RewindSelectorDeps) -> Self {
		let tail = recent_transcript_entries(&entries, TAIL_LIMIT);
		let truncated = tail.len() < entries.len();
		let mut component = RewindSelectorComponent {
			transcript: Rc::clone(&deps.transcript),
			theme: deps.theme,
			bindings: deps.bindings,
			height: deps.height.max(3),
			sibling_paths: deps.sibling_paths,
			on_select: deps.on_select,
			on_cancel: deps.on_cancel,
			request_render: deps.request_render,
			now_ms: deps.now_ms,
			entries,
			truncated,
			main: RewindColumn::default(),
			targets: Vec::new(),
			selected: 0,
			main_visible: None,
			sibling_visible: None,
			expanded: false,
			variant_cache: std::cell::RefCell::new(HashMap::new()),
			active_variant: 0,
			sibling_selected: 0,
			slide: None,
			filter_input: None,
			scroll: 0,
			body_height: 0,
		};
		let main = component.replay(&tail);
		component.main = main;
		component.selected = component.targets.len().saturating_sub(1);
		if let Some(start_at) = deps.start_at {
			component.start_at(&start_at);
		}
		component
	}

	/// Number of selectable rewind points on the current path; hosts skip
	/// mounting when zero.
	#[must_use]
	pub fn target_count(&self) -> usize {
		self.targets.len()
	}

	/// The outlined main-path target index.
	#[must_use]
	pub fn selected_index(&self) -> usize {
		self.selected
	}

	/// The outlined target, in the active sibling column when one is open.
	#[must_use]
	pub fn outlined_target(&self) -> Option<OutlineTarget> {
		if self.active_variant > 0 {
			let index = self.active_variant_index();
			let columns = self.strip_columns();
			return columns
				.get(self.active_variant - 1)
				.and_then(|column| column.column.targets.get(index).cloned());
		}
		self.targets.get(self.selected).cloned()
	}

	/// The main-path target at `index`.
	#[must_use]
	pub fn target(&self, index: usize) -> Option<&OutlineTarget> {
		self.targets.get(index)
	}

	/// Index within the active column (sibling selection when a strip column is
	/// active, the main selection otherwise).
	#[must_use]
	pub fn active_position(&self) -> usize {
		self.active_variant_index()
	}

	fn active_variant_index(&self) -> usize {
		if self.active_variant > 0 {
			self.sibling_selected
		} else {
			self.selected
		}
	}

	/// Number of open sibling columns at the current divergence.
	#[must_use]
	pub fn column_count(&self) -> usize {
		self.strip_columns().len()
	}

	/// 0 for the current path, 1..n for sibling columns.
	#[must_use]
	pub fn active_variant(&self) -> usize {
		self.active_variant
	}

	/// Whether the replayed tail still hides earlier turns.
	#[must_use]
	pub fn is_truncated(&self) -> bool {
		self.truncated
	}

	/// The open filter query, if any.
	#[must_use]
	pub fn filter_query(&self) -> Option<&str> {
		self.filter_input.as_ref().map(Input::value)
	}

	/// The main column's rows from the last replay.
	#[must_use]
	pub fn main_rows(&self) -> &[Vec<String>] {
		&self.main.rows
	}

	/// Replay `entries` and adopt the targets they produce.
	fn replay(&mut self, entries: &[TranscriptEntryLike]) -> RewindColumn {
		let column = self.transcript.borrow_mut().replay(entries, self.expanded);
		self.targets = column.targets.clone();
		column
	}

	/// Starts the outline on the target holding `entry_id`, replaying the whole
	/// branch when the tail lacks it.
	fn start_at(&mut self, entry_id: &str) {
		let holds = |target: &OutlineTarget| target.entries.iter().any(|entry| entry_id_of(entry) == entry_id);
		let mut index = self.targets.iter().position(holds);
		if index.is_none() && self.truncated {
			self.load_full_history();
			index = self.targets.iter().position(holds);
		}
		if let Some(index) = index {
			self.selected = index;
		}
	}

	/// Replay the whole branch, keeping the main outline on the same turn.
	pub fn load_full_history(&mut self) {
		if !self.truncated {
			return;
		}
		let selected_id = self.targets.get(self.selected).map(|target| target.turn_id.clone());
		let entries = self.entries.clone();
		self.main = self.replay(&entries);
		self.truncated = false;
		self.main_visible = None;
		self.sibling_visible = None;
		self.scroll = 0;
		self.selected = selected_id
			.and_then(|id| self.targets.iter().position(|target| target.turn_id == id))
			.unwrap_or_else(|| self.targets.len().saturating_sub(1));
		self.render_requested();
	}

	fn render_requested(&mut self) {
		(self.request_render)();
	}

	// ========================================================================
	// Branch strip
	// ========================================================================

	/// Sibling columns for the selected turn, built lazily and cached per
	/// divergence.
	fn strip_columns(&self) -> Vec<SiblingColumn> {
		let Some(target) = self.targets.get(self.selected) else {
			return Vec::new();
		};
		if self.sibling_paths.is_none() {
			return Vec::new();
		}
		if let Some(cached) = self.variant_cache.borrow().get(&target.turn_id) {
			return cached.clone();
		}
		let columns = self.build_strip_columns(&target.turn_id);
		self.variant_cache
			.borrow_mut()
			.insert(target.turn_id.clone(), columns.clone());
		columns
	}

	fn build_strip_columns(&self, turn_id: &str) -> Vec<SiblingColumn> {
		let Some(paths) = &self.sibling_paths else {
			return Vec::new();
		};
		let mut columns = Vec::new();
		let mut transcript = self.transcript.borrow_mut();
		for sibling in paths(turn_id) {
			if sibling.entries.is_empty() {
				continue;
			}
			let column = transcript.replay(&sibling.entries, self.expanded);
			let first_user = sibling.entries.iter().find(|entry| is_user_turn_entry(entry));
			let label = first_user
				.and_then(user_turn_label)
				.unwrap_or_else(|| sibling.root_id.clone());
			columns.push(SiblingColumn {
				root_id: sibling.root_id,
				column,
				label,
			});
		}
		columns
	}

	fn slide_to(&mut self, variant: usize) {
		let from = self.slide_position(self.now_ms.as_ref()());
		self.slide = Some(Slide {
			from,
			to: variant as f64,
			started: Instant::now(),
		});
		self.active_variant = variant;
		self.render_requested();
	}

	fn stop_slide(&mut self) {
		self.slide = None;
	}

	/// Fractional variant position of the camera now (eased).
	#[must_use]
	pub fn slide_position(&self, _now_ms: u64) -> f64 {
		match self.slide {
			Some(slide) => {
				let elapsed = slide.started.elapsed().as_millis() as u64;
				eased_slide_position(slide.from, slide.to, elapsed)
			}
			None => self.active_variant as f64,
		}
	}

	// ========================================================================
	// Input
	// ========================================================================

	/// Forward one key press.
	pub fn handle_key(&mut self, key: &Key) {
		if self.filter_input.is_some() {
			self.handle_filter_input(key);
			return;
		}
		if matches_select_cancel(key, &self.bindings) || matches!(key, Key::Esc) {
			(self.on_cancel)();
			return;
		}
		if matches!(key, Key::Char('f')) {
			self.open_filter();
			return;
		}
		if matches_app_tools_expand(key, &self.bindings) {
			self.toggle_expanded();
			return;
		}
		if matches_select_up(key, &self.bindings) {
			self.move_vertical(-1);
			return;
		}
		if matches_select_down(key, &self.bindings) {
			self.move_vertical(1);
			return;
		}
		if matches!(key, Key::Left) {
			self.left();
			return;
		}
		if matches!(key, Key::Right) {
			self.right();
			return;
		}
		if matches!(key, Key::Char('a') | Key::Char('A')) {
			self.load_full_history();
			return;
		}
		if matches!(key, Key::Enter) {
			self.select_outlined();
			return;
		}
		if self.scroll_key(key) {
			self.render_requested();
		}
	}

	/// `f`: open the filter over the whole branch.
	fn open_filter(&mut self) {
		self.load_full_history();
		let mut input = Input::new();
		input.set_placeholder("words…");
		self.filter_input = Some(input);
		self.active_variant = 0;
		self.sibling_selected = 0;
		self.scroll = 0;
		self.stop_slide();
		self.render_requested();
	}

	/// Left: the previous branch at a fork, else the previous user turn.
	fn left(&mut self) {
		if self.active_variant > 0 {
			self.slide_to(self.active_variant - 1);
		} else {
			self.move_selection(-1, |target| target.is_user_turn);
		}
	}

	/// Right: the next branch at a fork, else the next user turn.
	fn right(&mut self) {
		let columns = self.strip_columns_len();
		if self.active_variant < columns {
			self.sibling_selected = 0;
			self.slide_to(self.active_variant + 1);
		} else if self.active_variant == 0 {
			self.move_selection(1, |target| target.is_user_turn);
		}
	}

	fn strip_columns_len(&self) -> usize {
		self.strip_columns().len()
	}

	/// Enter while filtering: rewind to the selection when it matches.
	fn select_filtered(&mut self) {
		let matches = self.filter_matches();
		if matches.contains(&self.selected) {
			if let Some(target) = self.targets.get(self.selected) {
				let entry_id = target.entry_id.clone();
				(self.on_select)(&entry_id);
			}
		}
	}

	/// Enter: rewind to the outlined turn (main path or the active branch
	/// column).
	fn select_outlined(&mut self) {
		if let Some(target) = self.outlined_target() {
			let entry_id = target.entry_id.clone();
			(self.on_select)(&entry_id);
		}
	}

	fn toggle_expanded(&mut self) {
		self.expanded = !self.expanded;
		self.transcript.borrow_mut().set_expanded(self.expanded);
		self.variant_cache.borrow_mut().clear();
		self.main_visible = None;
		self.sibling_visible = None;
		let entries = recent_transcript_entries(&self.entries, if self.truncated { TAIL_LIMIT } else { usize::MAX });
		self.main = self.replay(&entries);
		self.render_requested();
	}

	// ========================================================================
	// Filter
	// ========================================================================

	fn handle_filter_input(&mut self, key: &Key) {
		if matches_select_cancel(key, &self.bindings) || matches!(key, Key::Esc) {
			self.close_filter();
			return;
		}
		if matches!(key, Key::Enter) {
			self.select_filtered();
			return;
		}
		if matches_app_tools_expand(key, &self.bindings) {
			self.toggle_expanded();
			return;
		}
		if matches_select_up(key, &self.bindings) || matches!(key, Key::Left) {
			let user_turns_only = !matches_select_up(key, &self.bindings);
			self.step_filtered(-1, user_turns_only);
			return;
		}
		if matches_select_down(key, &self.bindings) || matches!(key, Key::Right) {
			let user_turns_only = !matches_select_down(key, &self.bindings);
			self.step_filtered(1, user_turns_only);
			return;
		}
		let before = self.filter_query().unwrap_or_default().to_string();
		if matches!(key, Key::Backspace) && before.is_empty() {
			self.close_filter();
			return;
		}
		let changed = match self.filter_input.as_mut() {
			Some(input) => {
				input.handle_input(key);
				input.value() != before
			}
			None => false,
		};
		if changed {
			self.filter_changed();
		} else {
			self.render_requested();
		}
	}

	/// Leave the filter, keeping the selected item outlined in the full
	/// transcript.
	fn close_filter(&mut self) {
		self.filter_input = None;
		self.render_requested();
	}

	/// The query changed; keep the selection when it still matches, else rest on
	/// the newest match above it.
	fn filter_changed(&mut self) {
		let matches = self.filter_matches();
		if !matches.contains(&self.selected) {
			self.selected = matches
				.iter()
				.rev()
				.find(|index| **index < self.selected)
				.copied()
				.or_else(|| matches.last().copied())
				.unwrap_or(self.selected);
		}
		self.render_requested();
	}

	fn step_filtered(&mut self, delta: i32, user_turns_only: bool) {
		let matches: Vec<usize> = self
			.filter_matches()
			.into_iter()
			.filter(|index| {
				!user_turns_only || self.targets.get(*index).is_some_and(|target| target.is_user_turn)
			})
			.collect();
		let next = if delta < 0 {
			matches.iter().rev().find(|index| **index < self.selected).copied()
		} else {
			matches.iter().find(|index| **index > self.selected).copied()
		};
		if let Some(next) = next {
			self.selected = next;
			self.render_requested();
		}
	}

	/// Visible main-path target indices whose rendered text contains every
	/// whitespace-separated query word. All visible targets match an empty query.
	///
	/// Matching the rendered rows keeps results honest: collapsed tool output
	/// only matches once it is expanded.
	#[must_use]
	pub fn filter_matches(&self) -> Vec<usize> {
		let query = self.filter_query().unwrap_or_default().to_string();
		let words = compile_filter(&query);
		let mut matches = Vec::new();
		for index in 0..self.targets.len() {
			if !self.is_main_selectable(index) {
				continue;
			}
			let target = &self.targets[index];
			let haystack: String = (target.start..target.end)
				.filter_map(|child| self.main.rows.get(child))
				.map(|rows| plain_block_text(rows))
				.collect::<Vec<String>>()
				.join("\n");
			if target_matches(&words, &haystack) {
				matches.push(index);
			}
		}
		matches
	}


	// ========================================================================
	// Stepping
	// ========================================================================

	/// Up/Down: step within the active column; leaving a sibling column's top
	/// exits the strip.
	fn move_vertical(&mut self, delta: i32) {
		if self.active_variant > 0 {
			let targets = self
				.strip_columns()
				.get(self.active_variant - 1)
				.map(|column| column.column.targets.clone())
				.unwrap_or_default();
			let mut index = self.sibling_selected as isize + delta as isize;
			while index >= 0
				&& (index as usize) < targets.len()
				&& self
					.sibling_visible
					.as_ref()
					.and_then(|visible| visible.get(index as usize))
					.is_some_and(|shows| !*shows)
			{
				index += delta as isize;
			}
			if index >= 0 && (index as usize) < targets.len() {
				self.sibling_selected = index as usize;
				self.render_requested();
			} else if delta == -1 {
				// Off the top of an alternate: return to the current path above
				// the fork.
				self.active_variant = 0;
				self.sibling_selected = 0;
				self.stop_slide();
				self.move_selection(-1, |_| true);
			}
			return;
		}
		self.move_selection(delta, |_| true);
	}

	/// Step the main selection by `delta` to the nearest visible target passing
	/// `accept`; stepping above the replayed tail loads the earlier history
	/// first.
	fn move_selection(&mut self, delta: i32, accept: impl Fn(&OutlineTarget) -> bool) {
		let mut index = self.selected as isize + delta as isize;
		while index >= 0 && (index as usize) < self.targets.len() {
			let target = &self.targets[index as usize];
			if self.is_main_selectable(index as usize) && accept(target) {
				self.selected = index as usize;
				self.active_variant = 0;
				self.sibling_selected = 0;
				self.stop_slide();
				self.render_requested();
				return;
			}
			index += delta as isize;
		}
		if delta < 0 && self.truncated {
			self.load_full_history();
			self.move_selection(delta, accept);
		}
	}

	fn is_main_selectable(&self, index: usize) -> bool {
		self.main_visible
			.as_ref()
			.and_then(|visible| visible.get(index))
			.copied()
			.unwrap_or(true)
	}

	/// Adopt the main path's per-target visibility. A selection on a target that
	/// shows nothing moves to the nearest one that shows and leaves the strip.
	fn adopt_visibility(&mut self, visible: Vec<bool>) {
		let selected = nearest_visible_target(&visible, self.selected);
		self.main_visible = Some(visible);
		if selected == self.selected {
			return;
		}
		self.selected = selected;
		self.active_variant = 0;
		self.sibling_selected = 0;
	}

	/// A click or tap on a turn: the outline moves to `targets[index]` of
	/// `column` (`main`, or a strip column's root id).
	pub fn pick(&mut self, column: &str, index: usize) {
		if column == "main" {
			if self.active_variant == 0 && self.selected == index {
				return;
			}
			self.selected = index;
			self.active_variant = 0;
			self.sibling_selected = 0;
			self.stop_slide();
		} else {
			let columns = self.strip_columns();
			let Some(position) = columns.iter().position(|candidate| candidate.root_id == column) else {
				return;
			};
			let variant = position + 1;
			if self.active_variant == variant && self.sibling_selected == index {
				return;
			}
			self.active_variant = variant;
			self.sibling_selected = index;
		}
		self.render_requested();
	}

	// ========================================================================
	// Geometry
	// ========================================================================

	fn outline_style(&self) -> OutlineStyle {
		OutlineStyle {
			color: Some("accent".to_string()),
			caption: None,
		}
	}

	/// Compose the main transcript column at `width`.
	#[must_use]
	pub fn compose_main(&mut self, width: usize) -> ComposedColumn {
		if self.main_visible.is_none() {
			let visible = outline_visibility(&self.main.rows, &self.targets);
			self.adopt_visibility(visible);
		}
		compose_outline_column(
			&self.theme,
			&self.main.rows,
			0,
			self.main.rows.len(),
			&self.targets,
			self.selected,
			width,
			None,
			&self.outline_style(),
		)
	}

	/// Only the matching items of the current path, concatenated; no branch
	/// strip.
	#[must_use]
	pub fn compose_filtered(&mut self, width: usize) -> ComposedColumn {
		let matches = self.filter_matches();
		let mut rows: Vec<Vec<String>> = Vec::new();
		let mut targets: Vec<OutlineTarget> = Vec::new();
		for index in matches.iter().copied() {
			let Some(target) = self.targets.get(index) else {
				continue;
			};
			let start = rows.len();
			for child in target.start..target.end {
				if let Some(child_rows) = self.main.rows.get(child) {
					rows.push(child_rows.clone());
				}
			}
			targets.push(OutlineTarget {
				entry_id: target.entry_id.clone(),
				turn_id: target.turn_id.clone(),
				is_user_turn: target.is_user_turn,
				start,
				end: rows.len(),
				entries: target.entries.clone(),
			});
		}
		let selected = matches.iter().position(|index| *index == self.selected);
		compose_outline_column(
			&self.theme,
			&rows,
			0,
			rows.len(),
			&targets,
			selected.unwrap_or(usize::MAX),
			width,
			None,
			&self.outline_style(),
		)
	}

	/// Two caption rows leading a strip column: `⎇ i/n · label` plus a spacer.
	#[must_use]
	pub fn column_header(&self, index: usize, count: usize, label: &str, column_width: usize) -> Vec<String> {
		let caption = format!("{} {}/{} {} {}", self.theme.icon().branch, index + 1, count, self.theme.sep().dot, label);
		let caption = crate::text_shape::truncate_to_width(&caption, column_width.saturating_sub(2), crate::text_shape::Ellipsis::Unicode);
		let active = index == self.active_variant;
		let tone = if active { "accent" } else { "dim" };
		vec![format!(" {}", self.theme.fg(tone, &caption).unwrap_or(caption)), String::new()]
	}

	/// Shared prefix at full width, then the divergence as a camera-positioned
	/// strip of half-width branch columns (current path first, siblings after).
	#[must_use]
	pub fn compose_strip(&mut self, width: usize) -> ComposedColumn {
		let columns = self.strip_columns();
		let Some(anchor) = self.targets.get(self.selected).cloned() else {
			return self.compose_main(width);
		};
		let col_width = ((width.saturating_sub(STRIP_GAP)) / 2).max(MIN_COLUMN_WIDTH);
		let count = columns.len() + 1;

		// Shared history above the fork, full width, never outlined.
		let prefix = compose_outline_column(
			&self.theme,
			&self.main.rows,
			0,
			anchor.start.min(self.main.rows.len()),
			&[],
			usize::MAX,
			width,
			None,
			&self.outline_style(),
		);

		// Column 0: the current path from the fork down, re-rendered at column
		// width by re-slicing the full-width composed rows.
		let suffix_rows: Vec<Vec<String>> = self.main.rows[anchor.start.min(self.main.rows.len())..]
			.iter()
			.map(|rows| {
				rows.iter()
					.map(|row| {
						let slice = slice_by_column(row, 0, col_width, true);
						pad_to_width(&slice.text, col_width)
					})
					.collect()
			})
			.collect();
		let suffix_targets: Vec<OutlineTarget> = self
			.targets
			.iter()
			.skip(self.selected)
			.map(|target| OutlineTarget {
				entry_id: target.entry_id.clone(),
				turn_id: target.turn_id.clone(),
				is_user_turn: target.is_user_turn,
				start: target.start.saturating_sub(anchor.start),
				end: target.end.saturating_sub(anchor.start),
				entries: target.entries.clone(),
			})
			.collect();
		let header = self.column_header(0, count, "current", col_width);
		let mut composed = vec![compose_outline_column(
			&self.theme,
			&suffix_rows,
			0,
			suffix_rows.len(),
			&suffix_targets,
			if self.active_variant == 0 { 0 } else { usize::MAX },
			col_width,
			Some(&header),
			&self.outline_style(),
		)];
		for (index, column) in columns.iter().enumerate() {
			let active = self.active_variant == index + 1;
			if active {
				let visible = outline_visibility(&column.column.rows, &column.column.targets);
				self.sibling_visible = Some(visible);
			}
			let header = self.column_header(index + 1, count, &column.label, col_width);
			composed.push(compose_outline_column(
				&self.theme,
				&column.column.rows,
				0,
				column.column.rows.len(),
				&column.column.targets,
				if active { self.sibling_selected } else { usize::MAX },
				col_width,
				Some(&header),
				&self.outline_style(),
			));
		}

		// Camera over the strip: keep the active (possibly mid-slide) column
		// centered.
		let stride = col_width + STRIP_GAP;
		let total_width = count * col_width + count.saturating_sub(1) * STRIP_GAP;
		let camera_at = |position: f64| -> f64 {
			let raw = position * stride as f64 - (width.saturating_sub(col_width)) as f64 / 2.0;
			raw.max(0.0)
				.min((total_width.saturating_sub(width)) as f64)
		};
		let now = self.now_ms.as_ref()();
		let position = self.slide_position(now);
		let camera = camera_at(position);

		let height = composed.iter().map(|column| column.lines.len()).max().unwrap_or(0);
		let mut lines = prefix.lines;
		if count > 2 {
			let settled = camera_at(self.active_variant as f64);
			lines.push(position_rail(
				&self.theme,
				count,
				self.active_variant,
				settled > 0.5,
				(settled + width as f64) < (total_width as f64 - 0.5),
				width,
			));
			lines.push(String::new());
		}
		let active = &composed[self.active_variant.min(composed.len().saturating_sub(1))];
		let sel_start = if active.sel_start >= 0 {
			lines.len() as isize + active.sel_start
		} else {
			-1
		};
		let sel_end = if active.sel_end >= 0 {
			lines.len() as isize + active.sel_end
		} else {
			-1
		};
		for row in 0..height {
			let mut line = String::new();
			let mut filled = 0usize;
			for (index, column) in composed.iter().enumerate() {
				let x0 = index as f64 * stride as f64 - camera;
				let x1 = x0 + col_width as f64;
				let visible0 = x0.max(0.0);
				let visible1 = (width as f64).min(x1);
				if visible1 <= visible0 {
					continue;
				}
				let source = column.lines.get(row).map(|line| pad_to_width(line, col_width)).unwrap_or_default();
				let slice = slice_by_column(
					&source,
					(visible0 - x0) as usize,
					(visible1 - visible0) as usize,
					true,
				);
				line.push_str(&padding((visible0 as usize).saturating_sub(filled)));
				if visible1 > visible0 {
					line.push_str(&pad_to_width(&slice.text, (visible1 - visible0) as usize));
				}
				filled = visible1 as usize;
			}
			lines.push(line);
		}
		ComposedColumn {
			lines,
			sel_start,
			sel_end,
		}
	}

	fn header_line(&self) -> String {
		let head = format!("{} {}", self.theme.icon().rewind, self.theme.bold("Rewind"));
		format!(
			"{head}{}{}",
			self.theme.sep().dot,
			self.theme
				.fg("dim", "pick the point to continue from")
				.unwrap_or_else(|_| "pick the point to continue from".to_string())
		)
	}

	fn footer_line(&self) -> String {
		let position = if self.targets.is_empty() {
			String::new()
		} else {
			format!("{}/{}  ", self.selected + 1, self.targets.len())
		};
		let lateral = if self.strip_columns_len() > 0 { "branches" } else { "user turns" };
		let earlier = if self.truncated { "  a earlier turns" } else { "" };
		let keys = format!("↑↓ step  ←→ {lateral}  f filter  enter rewind{earlier}  ctrl+o expand  esc cancel");
		format!(
			"{position}{}",
			self.theme.fg("dim", &keys).unwrap_or(keys)
		)
	}

	/// Scroll the body window by `delta` rows, clamped.
	pub fn scroll_by(&mut self, delta: isize) {
		let max = self.body_height.saturating_sub(self.height.saturating_sub(2));
		let next = (self.scroll as isize + delta).clamp(0, max as isize) as usize;
		if next != self.scroll {
			self.scroll = next;
			self.render_requested();
		}
	}

	/// PageUp/PageDown/Home/End and the scroll wheel without moving the
	/// selection.
	fn scroll_key(&mut self, key: &Key) -> bool {
		match key {
			Key::PageUp => self.scroll_by(-(self.height as isize - 2)),
			Key::PageDown => self.scroll_by(self.height as isize - 2),
			Key::Home => {
				self.scroll = 0;
			}
			Key::End => {
				self.scroll = self.body_height.saturating_sub(self.height.saturating_sub(2));
			}
			_ => return false,
		}
		true
	}

	/// Clamp the scroll window so the composed selection stays visible.
	fn reveal_selection(&mut self, composed: &ComposedColumn) {
		let height = self.height.saturating_sub(2).max(1);
		self.body_height = composed.lines.len();
		let max = self.body_height.saturating_sub(height);
		if composed.sel_start >= 0 {
			let start = composed.sel_start as usize;
			let end = composed.sel_end.max(composed.sel_start) as usize;
			if start < self.scroll {
				self.scroll = start;
			} else if end > self.scroll + height {
				self.scroll = end.saturating_sub(height);
			}
		}
		self.scroll = self.scroll.min(max);
	}

	/// The rows the selector renders, header and footer included.
	pub fn frame_rows(&mut self, width: u16) -> Vec<String> {
		let width = width as usize;
		let mut rows = vec![self.header_line()];
		if self.filter_input.is_some() {
			let query = self.filter_query().unwrap_or_default().to_string();
			let composed = self.compose_filtered(width);
			let matches = self.filter_matches();
			self.reveal_selection(&composed);
			let height = self.height.saturating_sub(2).max(1);
			if matches.is_empty() {
				rows.push(format!("  No items match \"{query}\""));
			} else {
				rows.extend(
					composed
						.lines
						.iter()
						.skip(self.scroll)
						.take(height)
						.cloned(),
				);
			}
			let at = matches.iter().position(|index| *index == self.selected);
			let count = if matches.is_empty() {
				"no matches".to_string()
			} else {
				format!("{}/{}", at.map(|at| at + 1).unwrap_or_default(), matches.len())
			};
			let input_row = self
				.filter_input
				.as_mut()
				.map(|input| input.render(width as u16).into_iter().next().unwrap_or_default())
				.unwrap_or_default();
			rows.push(format!(
				"filter: {query}  {count}  {}",
				self.theme.fg("dim", "↑↓ step  ←→ user turns  enter rewind  esc show all").unwrap_or_default()
			));
			rows.push(input_row);
			return rows;
		}
		let composed = if self.strip_columns_len() > 0 {
			self.compose_strip(width)
		} else {
			self.compose_main(width)
		};
		self.reveal_selection(&composed);
		let height = self.height.saturating_sub(2).max(1);
		rows.extend(
			composed
				.lines
				.iter()
				.skip(self.scroll)
				.take(height)
				.cloned(),
		);
		rows.push(self.footer_line());
		rows
	}

	/// The native page: the replayed transcript fills a screen surface block by
	/// block, the outlined turn marked `pick` under a "Continue from here"
	/// caption and what the rewind drops marked `drop`.
	#[must_use]
	pub fn describe_page(&mut self, _cx: &DescribeContext) -> Vec<NativeNode> {
		if self.main_visible.is_none() {
			let visible = outline_visibility(&self.main.rows, &self.targets);
			self.adopt_visibility(visible);
		}
		if let Some(query) = self.filter_query().map(str::to_string) {
			let matches = self.filter_matches();
			let mut page = Vec::new();
			for index in matches {
				let Some(target) = self.targets.get(index) else {
					continue;
				};
				let picked = index == self.selected;
				if picked {
					page.push(self.caption(&self.targets, index, "main"));
				}
				for child in target.start..target.end {
					if let Some(rows) = self.main.rows.get(child) {
						page.push(self.marked_block(rows, if picked { Some("pick") } else { None }, "main", Some(index)));
					}
				}
			}
			if page.is_empty() {
				page.push(text(
					NodeText::spans(vec![Span::styled(format!("No turns match \"{query}\""), "muted")]),
					Props::new(),
				));
			}
			return page;
		}
		let mut page = Vec::new();
		if self.truncated {
			page.push(earlier_turns_node());
		}
		let columns = self.strip_columns_len();
		if columns == 0 {
			self.mark_run(&mut page, &self.main.rows, 0, &self.targets.clone(), Some(self.selected), "main");
			return page;
		}
		let anchor_start = self.targets.get(self.selected).map(|target| target.start).unwrap_or(0);
		for child in 0..anchor_start.min(self.main.rows.len()) {
			page.push(self.marked_block(
				&self.main.rows[child],
				None,
				"main",
				target_at(&self.targets, child),
			));
		}
		let strip_columns = self.strip_columns();
		let count = strip_columns.len() + 1;
		let mut strip = Vec::new();
		let mut current_children = vec![self.column_head_node(0, count, "current")];
		self.mark_run(
			&mut current_children,
			&self.main.rows.clone(),
			anchor_start,
			&self.targets.clone(),
			if self.active_variant == 0 { Some(self.selected) } else { None },
			"main",
		);
		strip.push(self.column_node(0, current_children, "current"));
		for (index, column) in strip_columns.iter().enumerate() {
			let active = self.active_variant == index + 1;
			if active {
				self.sibling_visible = Some(outline_visibility(&column.column.rows, &column.column.targets));
			}
			let mut children = vec![self.column_head_node(index + 1, count, &column.label)];
			self.mark_run(
				&mut children,
				&column.column.rows,
				0,
				&column.column.targets,
				if active { Some(self.sibling_selected) } else { None },
				&column.root_id,
			);
			strip.push(self.column_node(index + 1, children, &column.root_id));
		}
		page.push(crate::native::describe::node(
			NodeKind::Row,
			Props::new(),
			strip,
			Some("strip"),
		));
		page
	}

	/// Push `blocks[from..]` marked for `targets[picked]` (`None`: none):
	/// unmarked above it, then the caption and `pick` on its blocks, `drop`
	/// below.
	fn mark_run(
		&self,
		out: &mut Vec<NativeNode>,
		blocks: &[Vec<String>],
		from: usize,
		targets: &[OutlineTarget],
		picked: Option<usize>,
		column: &str,
	) {
		let target = picked.and_then(|picked| targets.get(picked));
		for (index, rows) in blocks.iter().enumerate().skip(from) {
			if let Some(target) = target {
				if index == target.start {
					if let Some(picked) = picked {
						out.push(self.caption(targets, picked, column));
					}
				}
			}
			let mark = match target {
				None => None,
				Some(target) if index < target.start => None,
				Some(target) if index < target.end => Some("pick"),
				Some(_) => Some("drop"),
			};
			out.push(self.marked_block(rows, mark, column, target_at(targets, index)));
		}
	}

	/// The line over the outlined turn: what Enter does and what it drops.
	fn caption(&self, targets: &[OutlineTarget], picked: usize, column: &str) -> NativeNode {
		let Some(target) = targets.get(picked) else {
			return text(NodeText::plain(""), Props::new());
		};
		let below = targets.len().saturating_sub(picked + 1);
		let mut spans = vec![
			Span::styled(format!("{} ", self.theme.icon().rewind), "accent"),
			Span::styled(
				if target.is_user_turn { "Rewind to here" } else { "Continue from here" },
				"accent",
			),
		];
		if target.is_user_turn {
			spans.push(Span::styled(
				format!("{}the prompt returns to the editor", self.theme.sep().dot),
				"dim",
			));
		}
		spans.push(Span::styled(
			if below > 0 {
				format!(
					"{}{} turn{} below dropped",
					self.theme.sep().dot,
					below,
					if below == 1 { "" } else { "s" }
				)
			} else {
				format!("{}nothing below to drop", self.theme.sep().dot)
			},
			"dim",
		));
		let mut props = Props::new();
		props.set_str("role", "omp.rewind.here");
		let mut node = text(NodeText::spans(spans), props);
		node.key = Some(format!("here:{column}:{}", target.turn_id));
		node.reveal = Some(Reveal::At(RevealAt::Start));
		node
	}

	fn marked_block(
		&self,
		rows: &[String],
		mark: Option<&str>,
		column: &str,
		target: Option<usize>,
	) -> NativeNode {
		let mut props = Props::new();
		if let Some(mark) = mark {
			props.set_str("mark", mark);
		}
		if let Some(target) = target {
			props.insert(
				"actions",
				serde_json::json!({ "click": { "act": PICK, "value": format!("{column}\u{0}{target}") } }),
			);
		}
		let mut node = text(NodeText::plain(rows.join("\n")), props);
		node.key = Some(format!("block:{column}:{target:?}"));
		node
	}

	/// One branch column of the strip; the active one carries the accent tone.
	fn column_node(&self, index: usize, children: Vec<NativeNode>, key: &str) -> NativeNode {
		let mut props = Props::new();
		props.set_str("role", "omp.rewind.branch");
		props.set_str("gap", "lg");
		if index == self.active_variant {
			props.set_str("tone", "accent");
		}
		crate::native::describe::node(NodeKind::Col, props, children, Some(key))
	}

	/// A strip column's caption: `⎇ i/n · label`.
	fn column_head_node(&self, index: usize, count: usize, label: &str) -> NativeNode {
		let active = index == self.active_variant;
		let mut props = Props::new();
		props.set_str("role", "omp.rewind.branch.head");
		text(
			NodeText::spans(vec![
				Span::styled(
					format!("{} ", self.theme.icon().branch),
					if active { "accent" } else { "dim" },
				),
				Span::styled(format!("{}/{}", index + 1, count), if active { "accent" } else { "dim" }),
				Span::styled(
					format!("{}{label}", self.theme.sep().dot),
					if active { "strong" } else { "dim" },
				),
			]),
			props,
		)
	}

	/// The bar docked under the page: position, key hints, Cancel and Rewind;
	/// the filter field while filtering.
	#[must_use]
	pub fn describe_bar(&self) -> NativeNode {
		let filtering = self.filter_input.is_some();
		let mut children = vec![text(
			NodeText::spans(vec![
				Span::styled(format!("{} ", self.theme.icon().rewind), "accent"),
				Span::styled("Rewind", "strong"),
			]),
			Props::new(),
		)];
		if filtering {
			let matches = self.filter_matches();
			let at = matches.iter().position(|index| *index == self.selected);
			let count = if matches.is_empty() {
				Span::styled("no matches", "error")
			} else {
				Span::styled(
					format!("{}/{}", at.map(|at| at + 1).unwrap_or_default(), matches.len()),
					"dim",
				)
			};
			children.push(text(NodeText::spans(vec![count]), Props::new()));
			children.push(hints_row(
				&[
					action_hint(&self.bindings, &["tui.select.up", "tui.select.down"], "step"),
					Some(crate::native::overlay::NativeHint {
						keys: vec!["left".to_string(), "right".to_string()],
						label: "user turns".to_string(),
					}),
				],
				"rewind-filter-hints",
			));
			children.push(action_bar(
				&[
					None,
					Some(action_button(
						"Show all",
						"cancel",
						ActionButtonOptions {
							keys: Some("escape".to_string()),
							..ActionButtonOptions::default()
						},
					)),
					Some(action_button(
						"Rewind here",
						"rewind",
						ActionButtonOptions {
							keys: Some("enter".to_string()),
							tone: Some(crate::native::overlay::ActionButtonTone::Accent),
							..ActionButtonOptions::default()
						},
					)),
				],
				"rewind-filter-actions",
			));
		} else {
			let columns = self.strip_columns_len();
			let at = if self.active_variant > 0 { self.sibling_selected } else { self.selected };
			let of = if self.active_variant > 0 {
				self.strip_columns()
					.get(self.active_variant - 1)
					.map(|column| column.column.targets.len())
					.unwrap_or(0)
			} else {
				self.targets.len()
			};
			children.push(text(
				NodeText::spans(vec![Span::styled(format!("{}/{}", at + 1, of), "dim")]),
				Props::new(),
			));
			children.push(hints_row(
				&[
					action_hint(&self.bindings, &["tui.select.up", "tui.select.down"], "step"),
					Some(crate::native::overlay::NativeHint {
						keys: vec!["left".to_string(), "right".to_string()],
						label: if columns > 0 { "branches" } else { "user turns" }.to_string(),
					}),
					Some(crate::native::overlay::NativeHint {
						keys: vec!["f".to_string()],
						label: "filter".to_string(),
					}),
				],
				"rewind-hints",
			));
			children.push(action_bar(
				&[
					None,
					Some(action_button(
						"Cancel",
						"cancel",
						ActionButtonOptions {
							keys: Some("escape".to_string()),
							..ActionButtonOptions::default()
						},
					)),
					Some(action_button(
						"Rewind here",
						"rewind",
						ActionButtonOptions {
							keys: Some("enter".to_string()),
							tone: Some(crate::native::overlay::ActionButtonTone::Accent),
							..ActionButtonOptions::default()
						},
					)),
				],
				"rewind-actions",
			));
		}
		let mut props = Props::new();
		props.set_str("role", "omp.rewind.bar");
		props.set_str("gap", "md");
		crate::native::describe::node(NodeKind::Row, props, children, Some("bar"))
	}

	/// The bar's buttons run their keys' paths; a `pick` action moves the
	/// outline.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		let NativeUiEvent::Action { act, value, .. } = event else {
			return;
		};
		let filtering = self.filter_input.is_some();
		match act.as_str() {
			"rewind" => {
				if filtering {
					self.select_filtered();
				} else {
					self.select_outlined();
				}
			}
			"cancel" => {
				if filtering {
					self.close_filter();
				} else {
					(self.on_cancel)();
				}
			}
			PICK => {
				if let Some(value) = value.as_ref().and_then(|value| value.as_str()) {
					if let Some((column, index)) = value.split_once('\u{0}') {
						if let Ok(index) = index.parse::<usize>() {
							self.pick(column, index);
						}
					}
				}
			}
			_ => {}
		}
	}
}


fn entry_id_of(entry: &TranscriptEntryLike) -> &str {
	match entry {
		TranscriptEntryLike::Message(message) => message.id.as_str(),
		TranscriptEntryLike::CustomMessage(message) => message.id.as_str(),
	}
}

/// The page's first row while older history is unreplayed: `a` loads it.
fn earlier_turns_node() -> NativeNode {
	let mut props = Props::new();
	props.set_str("role", "omp.rewind.earlier");
	props.set_str("gap", "xs");
	crate::native::describe::node(
		NodeKind::Row,
		props,
		vec![
			crate::native::describe::text(
				NodeText::spans(vec![Span::styled("a", "accent")]),
				Props::new(),
			),
			text(
				NodeText::spans(vec![Span::styled("load earlier turns", "muted")]),
				Props::new(),
			),
		],
		Some("earlier"),
	)
}

impl Component for RewindSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.frame_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.handle_key(key);
	}
}

impl NativeDescribe for RewindSelectorComponent {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		// Describing is read-only on `self`; the page only needs the cached
		// replay, so build the blocks without refreshing visibility.
		let mut props = Props::new();
		props.set_str("role", "omp.rewind");
		let mut blocks = Vec::new();
		for target in &self.targets {
			for child in target.start..target.end {
				if let Some(rows) = self.main.rows.get(child) {
					blocks.push(text(
						NodeText::plain(rows.join("\n")),
						Props::new(),
					));
				}
			}
		}
		let _ = cx;
		crate::native::describe::node(NodeKind::Col, props, vec![
			crate::native::describe::node(NodeKind::Col, Props::new(), blocks, Some("transcript")),
			self.describe_bar(),
		], Some("rewind"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::transcript_entry::{SessionMessageEntryLike, TranscriptEntryLike};
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::create_theme;

	fn theme() -> Theme {
		let json = builtin_theme_json("dark").expect("builtin theme");
		create_theme(json, crate::theme::loader::CreateThemeOptions::default()).expect("theme")
	}

	fn user_entry(id: &str, turn: &str, text: &str) -> TranscriptEntryLike {
		TranscriptEntryLike::Message(SessionMessageEntryLike {
			id: id.to_string(),
			parent_id: Some(turn.to_string()),
			timestamp: "2026-01-01T00:00:00Z".to_string(),
			message: crate::chat::transcript_entry::AgentMessage::User(
				crate::chat::transcript_entry::UserMessage {
					content: crate::chat::transcript_entry::UserContent::Text(text.to_string()),
					timestamp: 0,
				},
			),
		})
	}

	/// Deterministic replayer: one block per entry, one target per entry.
	struct FakeTranscript {
		expanded: bool,
	}

	impl RewindTranscript for FakeTranscript {
		fn replay(&mut self, entries: &[TranscriptEntryLike], expanded: bool) -> RewindColumn {
			self.expanded = expanded;
			let mut rows = Vec::new();
			let mut targets = Vec::new();
			for (index, entry) in entries.iter().enumerate() {
				let text = match entry {
					TranscriptEntryLike::Message(message) => match &message.message {
						crate::chat::transcript_entry::AgentMessage::User(user) => {
							user.content.text(" ")
						}
						_ => String::new(),
					},
					TranscriptEntryLike::CustomMessage(_) => String::new(),
				};
				rows.push(vec![text.clone()]);
				targets.push(OutlineTarget {
					entry_id: entry_id_of(entry).to_string(),
					turn_id: entry_id_of(entry).to_string(),
					is_user_turn: !text.is_empty(),
					start: index,
					end: index + 1,
					entries: vec![entry.clone()],
				});
			}
			RewindColumn { rows, targets }
		}

		fn set_expanded(&mut self, expanded: bool) {
			self.expanded = expanded;
		}
	}

	fn deps(replayed: Rc<std::cell::RefCell<FakeTranscript>>) -> RewindSelectorDeps {
		RewindSelectorDeps {
			transcript: replayed,
			theme: theme(),
			bindings: KeybindingsManager::new(),
			height: 20,
			sibling_paths: None,
			start_at: None,
			on_select: Box::new(|_| {}),
			on_cancel: Box::new(|| {}),
			request_render: Box::new(|| {}),
			now_ms: Box::new(|| 0),
		}
	}

	fn selector(entries: Vec<TranscriptEntryLike>) -> RewindSelectorComponent {
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		RewindSelectorComponent::new(entries, deps(fake))
	}

	#[test]
	fn target_at_finds_the_owning_block() {
		let mut targets = Vec::new();
		for (index, (start, end)) in [(0usize, 2usize), (2, 5), (5, 6)].into_iter().enumerate() {
			targets.push(OutlineTarget {
				entry_id: format!("e{index}"),
				turn_id: format!("t{index}"),
				is_user_turn: true,
				start,
				end,
				entries: Vec::new(),
			});
		}
		assert_eq!(target_at(&targets, 0), Some(0));
		assert_eq!(target_at(&targets, 1), Some(0));
		assert_eq!(target_at(&targets, 2), Some(1));
		assert_eq!(target_at(&targets, 4), Some(1));
		assert_eq!(target_at(&targets, 5), Some(2));
		assert_eq!(target_at(&targets, 6), None);
		assert_eq!(target_at(&[], 0), None);
	}

	#[test]
	fn nearest_visible_prefers_above_then_below() {
		assert_eq!(nearest_visible_target(&[true, false, false, true], 1), 0);
		assert_eq!(nearest_visible_target(&[false, false, true], 0), 2);
		assert_eq!(nearest_visible_target(&[false, false, true], 2), 2);
	}

	#[test]
	fn the_outline_starts_on_the_latest_target() {
		let entries = vec![user_entry("e1", "e1", "first"), user_entry("e2", "e2", "second")];
		let component = selector(entries);
		assert_eq!(component.target_count(), 2);
		assert_eq!(component.selected_index(), 1);
		assert_eq!(component.outlined_target().map(|target| target.entry_id.as_str()), Some("e2"));
	}

	#[test]
	fn start_at_replays_the_whole_branch_when_the_tail_lacks_the_entry() {
		let mut entries: Vec<TranscriptEntryLike> = Vec::new();
		for index in 0..(TAIL_LIMIT + 5) {
			entries.push(user_entry(&format!("e{index}"), &format!("e{index}"), &format!("turn {index}")));
		}
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		let mut own = deps(Rc::clone(&fake));
		own.start_at = Some("e1".to_string());
		let component = RewindSelectorComponent::new(entries, own);
		assert!(!component.is_truncated());
		assert_eq!(component.selected_index(), 1);
	}

	#[test]
	fn stepping_skips_targets_that_render_nothing() {
		let entries = vec![user_entry("e1", "e1", "first"), user_entry("e2", "e2", "")];
		let mut component = selector(entries);
		assert_eq!(component.selected_index(), 1);
		// `e2` renders a blank row, so the frame adopts visibility and steps up.
		let _ = component.compose_main(40);
		assert_eq!(component.selected_index(), 0);
		component.handle_key(&Key::Down);
		assert_eq!(component.selected_index(), 0, "blank target is not selectable");
	}

	#[test]
	fn left_and_right_jump_between_user_turns() {
		let entries = vec![
			user_entry("e1", "e1", "first"),
			user_entry("e2", "e2", "second"),
			user_entry("e3", "e3", "third"),
		];
		let mut component = selector(entries);
		component.handle_key(&Key::Left);
		assert_eq!(component.selected_index(), 1);
		component.handle_key(&Key::Left);
		assert_eq!(component.selected_index(), 0);
		component.handle_key(&Key::Left);
		assert_eq!(component.selected_index(), 0, "clamped at the oldest turn");
		component.handle_key(&Key::Right);
		assert_eq!(component.selected_index(), 1);
	}

	#[test]
	fn enter_rewinds_to_the_outlined_entry() {
		let picked = Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = Rc::clone(&picked);
		let cancels = Rc::new(std::cell::RefCell::new(0));
		let cancel_sink = Rc::clone(&cancels);
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		let mut own = deps(fake);
		own.on_select = Box::new(move |id| sink.borrow_mut().push(id.to_string()));
		own.on_cancel = Box::new(move || *cancel_sink.borrow_mut() += 1);
		let mut component = RewindSelectorComponent::new(
			vec![user_entry("e1", "e1", "first"), user_entry("e2", "e2", "second")],
			own,
		);
		component.handle_key(&Key::Left);
		component.handle_key(&Key::Enter);
		assert_eq!(&*picked.borrow(), &["e1".to_string()]);
		component.handle_key(&Key::Up);
		component.handle_key(&Key::Esc);
		assert_eq!(*cancels.borrow(), 1);
	}

	#[test]
	fn the_filter_matches_whole_latin_words_but_cjk_substrings() {
		let words = compile_filter("ls 命令");
		assert_eq!(words.len(), 2);
		assert!(words[0].matches("run ls -la"));
		assert!(!words[0].matches("run tools -la"));
		assert!(words[1].matches("运行命令"));
		let upper = compile_filter("LS");
		assert!(upper[0].matches("run ls"));
		let cjk = compile_filter("命令");
		assert!(cjk[0].matches("please 命令 now"));
	}

	#[test]
	fn filtering_narrows_to_the_matching_turns_and_keeps_the_selection() {
		let entries = vec![
			user_entry("e1", "e1", "alpha beta"),
			user_entry("e2", "e2", "gamma"),
			user_entry("e3", "e3", "alpha delta"),
		];
		let mut component = selector(entries);
		assert_eq!(component.filter_matches(), vec![0, 1, 2]);
		component.handle_key(&Key::Char('f'));
		assert_eq!(component.filter_query(), Some(""));
		for ch in "beta".chars() {
			component.handle_key(&Key::Char(ch));
		}
		assert_eq!(component.filter_matches(), vec![0]);
		// The selection was the newest turn, which no longer matches: it rests on
		// the newest match above it.
		assert_eq!(component.selected_index(), 0);
		// Down steps to the next match, Enter rewinds to it.
		let picked = Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = Rc::clone(&picked);
		let mut own = deps(Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false })));
		own.on_select = Box::new(move |id| sink.borrow_mut().push(id.to_string()));
		let mut component = RewindSelectorComponent::new(
			vec![
				user_entry("e1", "e1", "alpha beta"),
				user_entry("e3", "e3", "alpha delta"),
				user_entry("e4", "e4", "gamma"),
			],
			own,
		);
		component.handle_key(&Key::Char('f'));
		for ch in "alpha".chars() {
			component.handle_key(&Key::Char(ch));
		}
		component.handle_key(&Key::Up);
		component.handle_key(&Key::Enter);
		assert_eq!(picked.borrow().len(), 1);
		assert_eq!(picked.borrow()[0], "e1");
	}

	#[test]
	fn escape_in_the_filter_keeps_the_selection_and_shows_all() {
		let mut component = selector(vec![user_entry("e1", "e1", "alpha"), user_entry("e2", "e2", "beta")]);
		component.handle_key(&Key::Char('f'));
		component.handle_key(&Key::Char('a'));
		component.handle_key(&Key::Esc);
		assert_eq!(component.filter_query(), None);
		let _ = component.frame_rows(40);
	}

	#[test]
	fn sibling_branches_become_strip_columns_with_user_prompt_labels() {
		let sibling = BranchVariantPath {
			root_id: "branch-2".to_string(),
			entries: vec![user_entry("s1", "s1", "alternate prompt")],
		};
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		let mut own = deps(fake);
		own.sibling_paths = Some(Box::new(move |_| vec![sibling.clone()]));
		let mut component = RewindSelectorComponent::new(vec![user_entry("e1", "e1", "main prompt")], own);
		assert_eq!(component.column_count(), 1);
		let strip = component.compose_strip(80);
		assert!(strip.lines.iter().any(|line| line.contains("1/2")), "{:?}", strip.lines);
		assert!(strip.lines.iter().any(|line| line.contains("alternate prompt")));
		// Right slides to the sibling column; Enter rewinds to its entry.
		let picked = Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = Rc::clone(&picked);
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		let mut own = deps(fake);
		own.on_select = Box::new(move |id| sink.borrow_mut().push(id.to_string()));
		own.sibling_paths = Some(Box::new(move |_| vec![BranchVariantPath {
			root_id: "branch-2".to_string(),
			entries: vec![user_entry("s1", "s1", "alternate prompt")],
		}]));
		let mut component = RewindSelectorComponent::new(vec![user_entry("e1", "e1", "main prompt")], own);
		component.handle_key(&Key::Right);
		assert_eq!(component.active_variant(), 1);
		component.handle_key(&Key::Enter);
		assert_eq!(&*picked.borrow(), &["s1".to_string()]);
	}

	#[test]
	fn leaving_the_top_of_a_sibling_column_returns_to_the_current_path() {
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		let mut own = deps(fake);
		own.sibling_paths = Some(Box::new(|_| {
			vec![BranchVariantPath {
				root_id: "branch-2".to_string(),
				entries: vec![user_entry("s1", "s1", "alternate")],
			}]
		}));
		let mut component = RewindSelectorComponent::new(
			vec![user_entry("e1", "e1", "main one"), user_entry("e2", "e2", "main two")],
			own,
		);
		component.handle_key(&Key::Right);
		assert_eq!(component.active_variant(), 1);
		component.handle_key(&Key::Up);
		assert_eq!(component.active_variant(), 0);
		assert_eq!(component.selected_index(), 0);
	}

	#[test]
	fn the_camera_eases_toward_the_target_column() {
		assert_eq!(eased_slide_position(0.0, 1.0, 0), 0.0);
		assert_eq!(eased_slide_position(0.0, 1.0, SLIDE_MS), 1.0);
		let half = eased_slide_position(0.0, 1.0, SLIDE_MS / 2);
		assert!(half > 0.5 && half < 1.0, "{half}");
		assert_eq!(eased_slide_position(2.0, 0.0, SLIDE_MS * 4), 0.0);
	}

	#[test]
	fn expanded_toggle_clears_the_strip_cache_and_requests_a_replay() {
		let fake = Rc::new(std::cell::RefCell::new(FakeTranscript { expanded: false }));
		let mut own = deps(Rc::clone(&fake));
		own.sibling_paths = Some(Box::new(|_| Vec::new()));
		let mut component = RewindSelectorComponent::new(vec![user_entry("e1", "e1", "main")], own);
		component.toggle_expanded();
		assert!(fake.borrow().expanded);
	}

	#[test]
	fn the_native_page_marks_the_outlined_turn_and_the_caption() {
		let entries = vec![
			user_entry("e1", "e1", "first"),
			user_entry("e2", "e2", "second"),
		];
		let mut component = selector(entries);
		let cx = DescribeContext::new(80);
		let page = component.describe_page(&cx);
		let joined = page
			.iter()
			.filter_map(|node| node.props.get_str("mark").map(str::to_string))
			.collect::<Vec<_>>();
		assert!(joined.contains(&"pick".to_string()), "{joined:?}");
		let bar = component.describe_bar();
		assert_eq!(bar.props.get_str("role"), Some("omp.rewind.bar"));
	}
}
