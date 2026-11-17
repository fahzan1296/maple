//! Reference: `l/src/overlays/hook-selector.ts`.
//!
//! Generic selector for hooks: a fuzzy-filtered option list with keyboard
//! navigation, optional numbered quick-select, an orthogonal left/right
//! slider, radio/checkbox markers, disabled rows, a countdown that confirms
//! the current option, and the native picker/card description.
//!
//! Deviations:
//! - `theme`, `TUI`, and `Text`/`Container` rendering are not held. Colors and
//!   glyphs come from the injected [`HookSelectorStyle`]; logical rows are
//!   exposed through [`HookSelectorComponent::rows`] and
//!   [`HookSelectorComponent::describe`].
//! - The type-to-search field is a plain query buffer here (append/backspace/
//!   paste) rather than a full [`crate::components::Input`]; editing keys that
//!   the selector does not bind and the buffer cannot apply are dropped.
//! - The countdown is driven by injected `now_ms` ([`CountdownTimer`]), not a
//!   host `setInterval`.
//! - `DescribeContext` is not consulted: the card description is always
//!   produced and the host may pick it over the picker sheet.

use std::collections::HashSet;

use crate::chrome::countdown_timer::CountdownTimer;
use crate::components::menu_selection::{get_menu_window, MenuSource, MenuState};
use crate::keys::{
	is_arrow_down, is_arrow_left, is_arrow_right, is_arrow_up, is_backspace, is_enter, is_escape,
	Key,
};
use crate::native::describe::{md, node, row, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{hints_row, overlay_card, NativeHint};
use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// Root role of the native card.
pub const ROLE: &str = "omp.overlay.hook-select";

/// Role prefix of the parts.
pub const PART: &str = "omp.hook-select";

/// One slider segment: a label plus an optional detail line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookSelectorSliderSegment {
	pub label: String,
	/// Secondary line rendered under the track when this segment is selected.
	pub detail: Option<String>,
}

/// A horizontal left/right selector rendered above the option list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookSelectorSlider {
	/// Dim caption rendered before the slider track.
	pub caption: Option<String>,
	pub segments: Vec<HookSelectorSliderSegment>,
	/// Initially highlighted segment index.
	pub index: usize,
}

/// How markable option rows are prefixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMarker {
	/// Single choice: the marker fills on the cursor row.
	Radio,
	/// Multi-select: each row reflects [`HookSelectorComponent::checked_indices`].
	Checkbox,
}

/// One option.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookSelectorOption {
	pub label: String,
	pub description: Option<String>,
}

/// Either a bare label or a full option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookSelectorOptionInput {
	Label(String),
	Option(HookSelectorOption),
}

impl From<&str> for HookSelectorOptionInput {
	fn from(label: &str) -> Self {
		HookSelectorOptionInput::Label(label.to_string())
	}
}

impl From<String> for HookSelectorOptionInput {
	fn from(label: String) -> Self {
		HookSelectorOptionInput::Label(label)
	}
}

impl From<HookSelectorOption> for HookSelectorOptionInput {
	fn from(option: HookSelectorOption) -> Self {
		HookSelectorOptionInput::Option(option)
	}
}

/// Normalize a string option or a described option.
#[must_use]
pub fn normalize_option(option: HookSelectorOptionInput) -> HookSelectorOption {
	match option {
		HookSelectorOptionInput::Label(label) => HookSelectorOption {
			label,
			description: None,
		},
		HookSelectorOptionInput::Option(option) => {
			let description = option
				.description
				.as_deref()
				.map(str::trim)
				.filter(|description| !description.is_empty())
				.map(str::to_string);
			HookSelectorOption {
				label: option.label,
				description,
			}
		}
	}
}

/// Split `line` into its leading spaces (clamped to `width - 1`) and the body
/// that follows, so wrapped continuation rows keep the indent.
#[must_use]
pub fn split_leading_spaces_for_wrap(line: &str, width: usize) -> (String, String) {
	let indent_length = line.bytes().take_while(|byte| *byte == b' ').count();
	let clamped = indent_length.min(width.saturating_sub(1));
	(line[..clamped].to_string(), line[indent_length..].to_string())
}

/// Glyphs the selector paints before markable rows.
pub trait HookSelectorStyle {
	/// Radio glyph for the row at `index`.
	fn radio(&self, selected: bool, disabled: bool) -> String;
	/// Checkbox glyph for the row at `index`.
	fn checkbox(&self, checked: bool, selected: bool, disabled: bool) -> String;
	/// The dim help line under the list.
	fn hint(&self, text: &str) -> String;
}

/// Default Unicode glyphs.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnicodeSelectorStyle;

impl HookSelectorStyle for UnicodeSelectorStyle {
	fn radio(&self, selected: bool, _disabled: bool) -> String {
		if selected { "\u{25c9}" } else { "\u{25cb}" }.to_string()
	}

	fn checkbox(&self, checked: bool, _selected: bool, _disabled: bool) -> String {
		if checked { "\u{2611}" } else { "\u{2610}" }.to_string()
	}

	fn hint(&self, text: &str) -> String {
		text.to_string()
	}
}

/// A filtered option with its index into the original list.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FilteredOption {
	option: HookSelectorOption,
	index: usize,
}

impl FilteredOption {
	fn key(&self) -> String {
		self.index.to_string()
	}
}

struct OptionSource {
	disabled: HashSet<usize>,
}

impl MenuSource<FilteredOption> for OptionSource {
	fn key(&self, item: &FilteredOption) -> String {
		item.key()
	}

	fn search_text(&self, item: &FilteredOption) -> String {
		format!(
			"{} {}",
			item.option.label,
			item.option.description.as_deref().unwrap_or("")
		)
	}

	fn disabled(&self, item: &FilteredOption) -> bool {
		self.disabled.contains(&item.index)
	}
}

/// Callbacks the host wires into the selector.
#[derive(Default)]
pub struct HookSelectorCallbacks {
	pub on_select: Option<Box<dyn FnMut(&str)>>,
	pub on_cancel: Option<Box<dyn FnMut()>>,
	pub on_left: Option<Box<dyn FnMut()>>,
	pub on_right: Option<Box<dyn FnMut()>>,
	pub on_external_editor: Option<Box<dyn FnMut()>>,
	pub on_timeout: Option<Box<dyn FnMut()>>,
	pub on_timeout_start: Option<Box<dyn FnMut()>>,
	pub on_timeout_reset: Option<Box<dyn FnMut()>>,
}

/// Options of the selector.
pub struct HookSelectorOptions {
	/// Countdown in milliseconds; zero disables it.
	pub timeout_ms: u64,
	pub initial_index: usize,
	pub outline: bool,
	pub max_visible: usize,
	pub help_text: Option<String>,
	pub slider: Option<HookSelectorSlider>,
	/// Original indices that cannot be selected.
	pub disabled_indices: Vec<usize>,
	pub selection_marker: Option<SelectionMarker>,
	/// Original indices currently checked.
	pub checked_indices: Vec<usize>,
	/// Leading options that receive a selection marker (defaults to all).
	pub markable_count: Option<usize>,
	/// Describe as a docked card even when the host supports a picker.
	pub inline: bool,
	pub callbacks: HookSelectorCallbacks,
}

impl Default for HookSelectorOptions {
	fn default() -> Self {
		HookSelectorOptions {
			timeout_ms: 0,
			initial_index: 0,
			outline: false,
			max_visible: 12,
			help_text: None,
			slider: None,
			disabled_indices: Vec::new(),
			selection_marker: None,
			checked_indices: Vec::new(),
			markable_count: None,
			inline: false,
			callbacks: HookSelectorCallbacks::default(),
		}
	}
}

/// One row of the option window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorRow {
	pub text: String,
	/// Paint this row with the selection band.
	pub highlight: bool,
}

/// Text keys shown in the hint line.
pub const NAVIGATE_HINT: &str = "↑/↓ navigate  enter select  escape cancel";

/// The selector.
pub struct HookSelectorComponent {
	options: Vec<HookSelectorOption>,
	menu: MenuState<FilteredOption>,
	query: String,
	query_cursor: usize,
	disabled: HashSet<usize>,
	marker: Option<SelectionMarker>,
	checked: HashSet<usize>,
	markable_count: usize,
	max_visible: usize,
	outline: bool,
	base_title: String,
	detail_lines: Vec<String>,
	help_text: Option<String>,
	inline: bool,
	slider: Option<HookSelectorSlider>,
	slider_index: usize,
	countdown: Option<CountdownTimer>,
	callbacks: HookSelectorCallbacks,
	style: Box<dyn HookSelectorStyle>,
}

impl HookSelectorComponent {
	pub fn new(
		title: impl Into<String>,
		options: Vec<HookSelectorOptionInput>,
		opts: HookSelectorOptions,
	) -> Self {
		let title = title.into();
		let mut lines = title.split('\n');
		let base_title = lines.next().unwrap_or("").trim_end_matches('\r').to_string();
		let detail_lines: Vec<String> = lines.map(|line| line.trim_end_matches('\r').to_string()).collect();
		let options: Vec<HookSelectorOption> = options.into_iter().map(normalize_option).collect();
		let disabled: HashSet<usize> = opts
			.disabled_indices
			.into_iter()
			.filter(|index| *index < options.len())
			.collect();
		let items: Vec<FilteredOption> = options
			.iter()
			.cloned()
			.enumerate()
			.map(|(index, option)| FilteredOption { option, index })
			.collect();
		let mut menu = MenuState::new(
			items,
			Box::new(OptionSource {
				disabled: disabled.clone(),
			}),
		);
		menu.set_selected_index(opts.initial_index.min(options.len().saturating_sub(1)) as isize, 1);
		let markable_count = opts
			.markable_count
			.unwrap_or(options.len())
			.min(options.len());
		let checked = opts
			.checked_indices
			.into_iter()
			.filter(|index| *index < options.len())
			.collect();
		let slider = opts.slider.filter(|slider| !slider.segments.is_empty());
		let slider_index = slider.as_ref().map_or(0, |slider| {
			slider.index.min(slider.segments.len().saturating_sub(1))
		});
		let mut callbacks = opts.callbacks;
		let mut countdown = None;
		if opts.timeout_ms > 0 {
			if let Some(start) = callbacks.on_timeout_start.as_mut() {
				start();
			}
			countdown = Some(CountdownTimer::new(opts.timeout_ms, 0));
		}
		HookSelectorComponent {
			options,
			menu,
			query: String::new(),
			query_cursor: 0,
			disabled,
			marker: opts.selection_marker,
			checked,
			markable_count,
			max_visible: opts.max_visible.max(3),
			outline: opts.outline,
			base_title,
			detail_lines,
			help_text: opts.help_text,
			inline: opts.inline,
			slider,
			slider_index,
			countdown,
			callbacks,
			style: Box::new(UnicodeSelectorStyle),
		}
	}

	pub fn set_style(&mut self, style: impl HookSelectorStyle + 'static) {
		self.style = Box::new(style);
	}

	#[must_use]
	pub fn base_title(&self) -> &str {
		&self.base_title
	}

	/// Title with the live countdown suffix, when one runs.
	#[must_use]
	pub fn title(&self) -> String {
		match &self.countdown {
			Some(countdown) => format!("{} ({}s)", self.base_title, countdown.remaining_seconds()),
			None => self.base_title.clone(),
		}
	}

	#[must_use]
	pub fn detail_lines(&self) -> &[String] {
		&self.detail_lines
	}

	#[must_use]
	pub fn options(&self) -> &[HookSelectorOption] {
		&self.options
	}

	#[must_use]
	pub fn outline(&self) -> bool {
		self.outline
	}

	#[must_use]
	pub fn inline(&self) -> bool {
		self.inline
	}

	#[must_use]
	pub fn has_slider(&self) -> bool {
		self.slider.is_some()
	}

	#[must_use]
	pub fn has_countdown(&self) -> bool {
		self.countdown.is_some()
	}

	#[must_use]
	pub fn slider_index(&self) -> usize {
		self.slider_index
	}

	#[must_use]
	pub fn query(&self) -> &str {
		&self.query
	}

	#[must_use]
	pub fn checked_indices(&self) -> Vec<usize> {
		let mut checked: Vec<usize> = self.checked.iter().copied().collect();
		checked.sort_unstable();
		checked
	}

	/// Toggle a checkbox row (`selectionMarker: "checkbox"`).
	pub fn toggle_checked(&mut self, index: usize) {
		if index >= self.options.len() || index >= self.markable_count {
			return;
		}
		if !self.checked.remove(&index) {
			self.checked.insert(index);
		}
	}

	/// Label of the option under the cursor, when it is selectable.
	#[must_use]
	pub fn selected_label(&self) -> Option<String> {
		self.menu
			.selected_item()
			.filter(|item| !self.menu.is_disabled(item))
			.map(|item| item.option.label.clone())
	}

	/// Cursor position within the visible (filtered) list.
	#[must_use]
	pub fn selected_index(&self) -> usize {
		self.menu.selected_index().max(0) as usize
	}

	/// Original indices of the visible options, in render order.
	#[must_use]
	pub fn visible_indices(&self) -> Vec<usize> {
		self.menu
			.visible_items()
			.iter()
			.map(|item| item.index)
			.collect()
	}

	/// Whether `index` (original) is disabled.
	#[must_use]
	pub fn is_disabled(&self, index: usize) -> bool {
		self.disabled.contains(&index)
	}

	/// Visual row budget each option needs when `compact` collapses
	/// descriptions.
	fn option_row_count(&self, option: &HookSelectorOption, compact: bool) -> usize {
		if !compact && option.description.is_some() {
			2
		} else {
			1
		}
	}

	/// Total rows the fully expanded list would need.
	#[must_use]
	pub fn total_option_rows(&self) -> usize {
		self.options
			.iter()
			.map(|option| self.option_row_count(option, false))
			.sum()
	}

	/// Type-to-search turns on exactly when the expanded list overflows the
	/// window (or a query is already typed).
	#[must_use]
	pub fn is_search_enabled(&self) -> bool {
		self.total_option_rows() > self.max_visible
	}

	/// A compact list collapses descriptions; the highlighted one keeps its
	/// description in the remaining budget.
	#[must_use]
	pub fn compact(&self) -> bool {
		self.is_search_enabled()
	}

	/// Move the cursor by `delta`, skipping disabled rows, stopping at the list
	/// edge.
	pub fn move_selection(&mut self, delta: isize) -> bool {
		self.menu.move_selection(delta, false)
	}

	/// Highlight the visible row at `index`; disabled rows are ignored.
	pub fn select_visible_row(&mut self, index: usize) -> bool {
		let Some(item) = self.menu.visible_items().get(index) else {
			return false;
		};
		if self.menu.is_disabled(item) {
			return false;
		}
		self.menu.set_selected_index(index as isize, 1)
	}

	/// Move the slider by `delta`, clamped, returning the new index when it
	/// changed.
	pub fn move_slider(&mut self, delta: isize) -> Option<usize> {
		let slider = self.slider.as_ref()?;
		let last = slider.segments.len().saturating_sub(1);
		let next = (self.slider_index as isize + delta).clamp(0, last as isize) as usize;
		if next == self.slider_index {
			return None;
		}
		self.slider_index = next;
		Some(next)
	}

	/// Set the slider to `index`, clamped; reports whether it moved.
	pub fn set_slider_index(&mut self, index: usize) -> bool {
		let Some(slider) = self.slider.as_ref() else {
			return false;
		};
		let next = index.min(slider.segments.len().saturating_sub(1));
		if next == self.slider_index {
			return false;
		}
		self.slider_index = next;
		true
	}

	/// Slider line: caption, arrows, segments, and the active detail.
	#[must_use]
	pub fn slider_line(&self) -> String {
		let Some(slider) = self.slider.as_ref() else {
			return String::new();
		};
		let last = slider.segments.len().saturating_sub(1);
		let left = if self.slider_index > 0 { "◂" } else { "·" };
		let right = if self.slider_index < last { "▸" } else { "·" };
		let track = slider
			.segments
			.iter()
			.enumerate()
			.map(|(index, segment)| {
				let label = &segment.label;
				if index == self.slider_index {
					format!("\u{2588}{label}\u{2588}")
				} else {
					label.clone()
				}
			})
			.collect::<Vec<_>>()
			.join(" · ");
		let caption = slider
			.caption
			.as_deref()
			.map(|caption| format!("{caption}  "))
			.unwrap_or_default();
		let line = format!("{caption}{left}  {track}  {right}");
		match slider
			.segments
			.get(self.slider_index)
			.and_then(|segment| segment.detail.as_deref())
		{
			Some(detail) => format!("{line}\n  ↳ {detail}"),
			None => line,
		}
	}

	/// Countdown status line, when a timeout runs.
	#[must_use]
	pub fn countdown_line(&self) -> Option<String> {
		self.countdown
			.as_ref()
			.map(|countdown| format!("{}s", countdown.remaining_seconds()))
	}

	/// Reset the countdown after user activity.
	pub fn reset_countdown(&mut self) {
		if let Some(countdown) = self.countdown.as_mut() {
			countdown.reset(0);
			if let Some(reset) = self.callbacks.on_timeout_reset.as_mut() {
				reset();
			}
		}
	}

	/// Advance the countdown. On expiry, confirm the cursor option when it is
	/// selectable, otherwise cancel.
	pub fn poll_timeout(&mut self, now_ms: u64) -> bool {
		let Some(countdown) = self.countdown.as_mut() else {
			return false;
		};
		if !countdown.expire_if_due(now_ms) {
			return false;
		}
		if let Some(timeout) = self.callbacks.on_timeout.as_mut() {
			timeout();
		}
		if let Some(label) = self.selected_label() {
			if let Some(select) = self.callbacks.on_select.as_mut() {
				select(&label);
			}
		} else if let Some(cancel) = self.callbacks.on_cancel.as_mut() {
			cancel();
		}
		true
	}

	fn select_label(&mut self, label: &str) {
		if let Some(select) = self.callbacks.on_select.as_mut() {
			select(label);
		}
	}

	fn cancel(&mut self) {
		if let Some(cancel) = self.callbacks.on_cancel.as_mut() {
			cancel();
		}
	}

	fn sync_query(&mut self, query: &str) {
		self.menu.set_query(query, false);
	}

	/// Numbered quick-select: `1.`-prefixed options jump and confirm unless a
	/// checkbox menu is open or a search query is active.
	fn handle_quick_select(&mut self, key: &Key) -> bool {
		let Key::Char(digit) = key else {
			return false;
		};
		if !('1'..='9').contains(digit) || !self.query.is_empty() {
			return false;
		}
		let prefix = format!("{digit}. ");
		let Some(position) = self
			.menu
			.visible_items()
			.iter()
			.position(|item| item.option.label.starts_with(&prefix))
		else {
			return false;
		};
		let target = &self.menu.visible_items()[position];
		if self.menu.is_disabled(target) {
			return true;
		}
		let label = target.option.label.clone();
		self.menu.set_selected_index(position as isize, 1);
		if self.marker != Some(SelectionMarker::Checkbox) {
			self.select_label(&label);
		}
		true
	}

	/// Feed keys the selector does not bind to the search buffer. Backspace on
	/// an empty query and a leading space bubble back to the host.
	fn handle_search_input(&mut self, key: &Key) -> bool {
		if !self.is_search_enabled() {
			return false;
		}
		if self.query.is_empty() {
			if is_backspace(key) {
				return false;
			}
			if matches!(key, Key::Char(' ')) {
				return false;
			}
		}
		match key {
			Key::Char(ch) if !ch.is_control() => {
				let index = self.query_cursor.min(self.query.len());
				self.query.insert(index, *ch);
				self.query_cursor = index + ch.len_utf8();
			}
			Key::Paste(text) => {
				let index = self.query_cursor.min(self.query.len());
				self.query.insert_str(index, text);
				self.query_cursor = index + text.len();
			}
			Key::Backspace if self.query_cursor > 0 => {
				let index = self.query_cursor;
				let previous = self.query[..index]
					.char_indices()
					.next_back()
					.map(|(at, _)| at)
					.unwrap_or(0);
				self.query.replace_range(previous..index, "");
				self.query_cursor = previous;
			}
			Key::Delete if self.query_cursor < self.query.len() => {
				let index = self.query_cursor;
				let next = self.query[index..]
					.char_indices()
					.nth(1)
					.map(|(at, _)| index + at)
					.unwrap_or(self.query.len());
				self.query.replace_range(index..next, "");
			}
			Key::Left => self.query_cursor = self.query[..self.query_cursor.min(self.query.len())]
				.char_indices()
				.last()
				.map(|(at, _)| at)
				.unwrap_or(0),
			Key::Right => {
				let index = self.query_cursor.min(self.query.len());
				self.query_cursor = self.query[index..]
					.char_indices()
					.nth(1)
					.map(|(at, _)| index + at)
					.unwrap_or(self.query.len());
			}
			Key::Home => self.query_cursor = 0,
			Key::End => self.query_cursor = self.query.len(),
			_ => return false,
		}
		let query = self.query.clone();
		self.sync_query(&query);
		true
	}

	/// One decoded key.
	pub fn handle_key(&mut self, key: &Key) {
		self.reset_countdown();
		if is_escape(key) {
			self.cancel();
			return;
		}
		if self.handle_quick_select(key) {
			return;
		}
		if is_arrow_up(key) {
			self.move_selection(-1);
		} else if is_arrow_down(key) {
			self.move_selection(1);
		} else if is_enter(key) {
			if let Some(label) = self.selected_label() {
				self.select_label(&label);
			}
		} else if is_arrow_left(key) || (self.slider.is_some() && !self.is_search_enabled() && *key == Key::Char('h'))
		{
			if self.slider.is_some() {
				self.move_slider(-1);
			} else if let Some(left) = self.callbacks.on_left.as_mut() {
				left();
			}
		} else if is_arrow_right(key)
			|| (self.slider.is_some() && !self.is_search_enabled() && *key == Key::Char('l'))
		{
			if self.slider.is_some() {
				self.move_slider(1);
			} else if let Some(right) = self.callbacks.on_right.as_mut() {
				right();
			}
		} else if *key == Key::Ctrl('g') {
			if let Some(external) = self.callbacks.on_external_editor.as_mut() {
				external();
			}
		} else {
			self.handle_search_input(key);
		}
	}

	/// Native picker action (`confirm` / `close` / `cancel` / `clear`).
	pub fn handle_native_action(&mut self, act: &str) {
		self.reset_countdown();
		match act {
			"close" | "cancel" => self.cancel(),
			"clear" => {
				self.query.clear();
				self.query_cursor = 0;
				self.sync_query("");
			}
			"confirm" => {
				if let Some(label) = self.selected_label() {
					self.select_label(&label);
				}
			}
			_ => {}
		}
	}

	/// Native row activation: selects and confirms a visible row keyed by its
	/// original index.
	pub fn activate_row(&mut self, index: usize) -> bool {
		let Some(position) = self
			.menu
			.visible_items()
			.iter()
			.position(|item| item.index == index)
		else {
			return false;
		};
		let item = &self.menu.visible_items()[position];
		if self.menu.is_disabled(item) {
			return false;
		}
		let label = item.option.label.clone();
		self.reset_countdown();
		self.menu.set_selected_index(position as isize, 1);
		self.select_label(&label);
		true
	}

	/// Bounds-checked window of visible options plus a status row.
	#[must_use]
	pub fn rows(&self) -> Vec<SelectorRow> {
		let items = self.menu.visible_items();
		if items.is_empty() {
			return vec![SelectorRow {
				text: "No matching options".to_string(),
				highlight: false,
			}];
		}
		let compact = self.compact();
		let row_counts: Vec<usize> = items
			.iter()
			.map(|item| {
				let desc = if compact { 0 } else { usize::from(item.option.description.is_some()) };
				1 + desc
			})
			.collect();
		let window = get_menu_window(
			&row_counts,
			self.selected_index(),
			self.max_visible.max(1),
			None,
		);
		let mut rows = Vec::new();
		for position in window.start_index..window.end_index {
			let Some(item) = items.get(position) else {
				continue;
			};
			let selected = position as isize == self.menu.selected_index();
			let disabled = self.menu.is_disabled(item);
			let prefix = self.marker_prefix(item.index, selected, disabled);
			let label = match prefix {
				Some(prefix) => format!("{prefix}{}", item.option.label),
				None => format!(
					"{}{}",
					if selected { "> " } else { "  " },
					item.option.label
				),
			};
			rows.push(SelectorRow {
				text: label,
				highlight: selected && !disabled,
			});
			if let Some(description) = item.option.description.as_deref() {
				if !compact {
					for line in wrap_text_with_ansi(description, 76) {
						rows.push(SelectorRow {
							text: format!("    {line}"),
							highlight: selected && !disabled,
						});
					}
				} else if selected && !disabled {
					let first = wrap_text_with_ansi(description, 76)
						.first()
						.cloned()
						.unwrap_or_default();
					rows.push(SelectorRow {
						text: format!("    {first}"),
						highlight: true,
					});
				}
			}
		}
		if window.start_index > 0 || window.end_index < items.len() || self.is_search_enabled() || !self.query.is_empty() {
			rows.push(SelectorRow {
				text: self.status_line(),
				highlight: false,
			});
		}
		rows
	}

	/// Styled leading marker for a markable row, or `None` when the classic
	/// cursor prefix applies.
	fn marker_prefix(&self, index: usize, selected: bool, disabled: bool) -> Option<String> {
		match self.marker? {
			_ if index >= self.markable_count => None,
			SelectionMarker::Radio => Some(format!(
				"{} ",
				self.style.radio(selected, disabled)
			)),
			SelectionMarker::Checkbox => Some(format!(
				"{} ",
				self.style
					.checkbox(self.checked.contains(&index), selected, disabled)
			)),
		}
	}

	/// `(selected/total)  Type to search` or the live query.
	#[must_use]
	pub fn status_line(&self) -> String {
		let total = self.menu.visible_items().len();
		let selected_count = if total == 0 { 0 } else { self.selected_index() + 1 };
		let count = if !self.query.trim().is_empty() && total != self.options.len() {
			format!("{selected_count}/{total} of {}", self.options.len())
		} else {
			format!("{selected_count}/{total}")
		};
		if self.query.trim().is_empty() {
			return format!("({count})  Type to search");
		}
		format!("({count})  Search: {}", self.query)
	}

	/// Help line under the list.
	#[must_use]
	pub fn hint_line(&self) -> String {
		let hint = self.help_text.as_deref().unwrap_or(NAVIGATE_HINT);
		self.style.hint(hint)
	}

	/// Native card: title, detail lines, slider tabs, the option list, the
	/// search line, and the key hints.
	#[must_use]
	pub fn describe(&self) -> NativeNode {
		let mut children: Vec<NativeNode> = Vec::new();
		if let Some(countdown) = &self.countdown {
			children.push(row(
				vec![
					text(NodeText::Plain(self.title()), Props::new()),
					countdown.describe(0),
				],
				{
					let mut props = Props::new();
					props.set_str("gap", "sm");
					props.set_str("align", "baseline");
					props
				},
			));
		}
		if !self.detail_lines.is_empty() {
			children.push(node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_spans(vec![span(self.detail_lines.join("\n"), Some("accent"))]);
					props.set_str("wrap", "word");
					props.set_str("role", format!("{PART}.detail"));
					props
				},
				Vec::new(),
				Some("detail"),
			));
		}
		if let Some(slider) = &self.slider {
			if let Some(caption) = slider.caption.as_deref() {
				children.push(node(
					NodeKind::Text,
					{
						let mut props = Props::new();
						props.set_spans(vec![span(caption, Some("dim"))]);
						props
					},
					Vec::new(),
					Some("slider-caption"),
				));
			}
			children.push(node(
				NodeKind::Tabs,
				{
					let mut props = Props::new();
					props.insert(
						"items",
						serde_json::json!(slider
							.segments
							.iter()
							.enumerate()
							.map(|(index, segment)| serde_json::json!({
								"id": index.to_string(),
								"label": segment.label,
							}))
							.collect::<Vec<_>>()),
					);
					props.set_str("active", self.slider_index.to_string());
					props
				},
				Vec::new(),
				Some("slider"),
			));
			if let Some(detail) = slider
				.segments
				.get(self.slider_index)
				.and_then(|segment| segment.detail.as_deref())
			{
				children.push(node(
					NodeKind::Text,
					{
						let mut props = Props::new();
						props.set_spans(vec![span("↳ ", Some("dim")), span(detail, Some("muted"))]);
						props.set_str("wrap", "word");
						props
					},
					Vec::new(),
					Some("slider-detail"),
				));
			}
		}

		let selected_item = self.menu.selected_item().cloned();
		let visible = self.menu.visible_items().to_vec();
		let rows: Vec<NativeNode> = visible
			.iter()
			.map(|item| self.describe_option(item, selected_item.as_ref() == Some(item)))
			.collect();
		children.push(node(
			NodeKind::List,
			{
				let mut props = Props::new();
				props.insert(
					"selected",
					match &selected_item {
						Some(item) if !self.menu.is_disabled(item) => {
							serde_json::Value::String(item.index.to_string())
						}
						_ => serde_json::Value::Null,
					},
				);
				if !self.query.trim().is_empty() {
					props.set_str("filter", self.query.trim());
				}
				props.set_str("empty", "No matching options");
				props.insert("max", serde_json::json!({ "lines": self.max_visible }));
				props
			},
			rows,
			Some("list"),
		));
		if !self.query.is_empty() {
			children.push(node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_spans(vec![span("Search: ", Some("dim")), span(&self.query, None)]);
					props.set_str("wrap", "none");
					props
				},
				Vec::new(),
				Some("search"),
			));
		}
		children.push(self.describe_hints());
		overlay_card(
			ROLE,
			if self.countdown.is_some() {
				None
			} else {
				Some(NodeText::Plain(self.title()))
			},
			children,
			None,
		)
	}

	fn describe_hints(&self) -> NativeNode {
		match self.help_text.as_deref() {
			Some(help) => node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_spans(vec![span(help, Some("dim"))]);
					props.set_str("wrap", "word");
					props
				},
				Vec::new(),
				Some("hints"),
			),
			None => hints_row(
				&[
					Some(NativeHint {
						keys: vec!["↑".to_string(), "↓".to_string()],
						label: "navigate".to_string(),
					}),
					Some(NativeHint {
						keys: vec!["enter".to_string()],
						label: "select".to_string(),
					}),
					Some(NativeHint {
						keys: vec!["escape".to_string()],
						label: "cancel".to_string(),
					}),
				],
				"hints",
			),
		}
	}

	fn describe_option(&self, item: &FilteredOption, selected: bool) -> NativeNode {
		let disabled = self.menu.is_disabled(item);
		let radio = self.marker == Some(SelectionMarker::Radio) && item.index < self.markable_count;
		let checkbox = self.marker == Some(SelectionMarker::Checkbox) && item.index < self.markable_count;
		let glyph = if radio {
			Some(self.style.radio(selected, disabled))
		} else if checkbox {
			Some(
				self.style
					.checkbox(self.checked.contains(&item.index), selected, disabled),
			)
		} else {
			None
		};
		let mut props = Props::new();
		let mut label = Vec::new();
		if let Some(glyph) = glyph {
			let color = if disabled {
				"dim"
			} else if selected {
				"accent"
			} else {
				"dim"
			};
			label.push(span(format!("{glyph} "), Some(color)));
		}
		label.push(span(
			plain_inline(&item.option.label),
			Some(if disabled {
				"dim"
			} else if selected {
				"accent"
			} else {
				"text"
			}),
		));
		props.insert(
			"label",
			serde_json::Value::Array(
				label
					.into_iter()
					.map(|span| {
						serde_json::json!({
							"text": span.t,
							"color": span.s,
						})
					})
					.collect(),
			),
		);
		if let Some(description) = item.option.description.as_deref() {
			props.insert(
				"detail",
				serde_json::json!([{ "text": plain_inline(description), "color": "muted" }]),
			);
		}
		if disabled {
			props.insert("disabled", serde_json::Value::Bool(true));
		}
		node(
			NodeKind::Item,
			props,
			Vec::new(),
			Some(&item.index.to_string()),
		)
	}

	/// Dispose semantics: cancel any running countdown.
	pub fn dispose(&mut self) {
		if let Some(countdown) = self.countdown.as_mut() {
			countdown.dispose();
		}
	}
}

/// Strip inline Markdown markers to plain text (no ANSI is produced here).
#[must_use]
pub fn plain_inline(source: &str) -> String {
	let mut out = String::with_capacity(source.len());
	let mut chars = source.chars().peekable();
	while let Some(ch) = chars.next() {
		match ch {
			'*' | '_' | '`' => {}
			'[' => {
				let mut label = String::new();
				let mut closed = false;
				while let Some(next) = chars.next() {
					if next == ']' {
						closed = true;
						break;
					}
					label.push(next);
				}
				if closed && chars.peek() == Some(&'(') {
					for next in chars.by_ref() {
						if next == ')' {
							break;
						}
					}
					out.push_str(&label);
				} else {
					out.push('[');
					out.push_str(&label);
					if closed {
						out.push(']');
					}
				}
			}
			_ => out.push(ch),
		}
	}
	out
}

/// Truncate to `width` with an ellipsis (helper shared with the picker).
#[must_use]
pub fn ellipsize(text: &str, width: usize) -> String {
	if visible_width(text) <= width {
		text.to_string()
	} else {
		truncate_to_width(text, width, Ellipsis::Unicode)
	}
}

/// Re-exported Markdown node builder for hosts that render the detail line.
#[must_use]
pub fn detail_markdown(source: &str) -> NativeNode {
	md(source, {
		let mut props = Props::new();
		props.set_str("role", "full");
		props
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;


	fn selector(
		title: &str,
		labels: &[&str],
		opts: HookSelectorOptions,
	) -> (HookSelectorComponent, Rc<RefCell<Vec<String>>>, Rc<RefCell<usize>>) {
		let selected = Rc::new(RefCell::new(Vec::new()));
		let cancelled = Rc::new(RefCell::new(0usize));
		let mut opts = opts;
		let sink = selected.clone();
		opts.callbacks.on_select = Some(Box::new(move |label| sink.borrow_mut().push(label.to_string())));
		let cancel_sink = cancelled.clone();
		opts.callbacks.on_cancel = Some(Box::new(move || *cancel_sink.borrow_mut() += 1));
		let options = labels.iter().map(|label| (*label).into()).collect();
		(
			HookSelectorComponent::new(title, options, opts),
			selected,
			cancelled,
		)
	}

	#[test]
	fn normalize_trims_description_and_drops_blank() {
		let described = normalize_option(HookSelectorOptionInput::Option(HookSelectorOption {
			label: "A".to_string(),
			description: Some("  keep  ".to_string()),
		}));
		assert_eq!(described.description.as_deref(), Some("keep"));
		let blank = normalize_option(HookSelectorOptionInput::Option(HookSelectorOption {
			label: "B".to_string(),
			description: Some("   ".to_string()),
		}));
		assert_eq!(blank.description, None);
		assert_eq!(
			normalize_option("C".into()),
			HookSelectorOption {
				label: "C".to_string(),
				description: None
			}
		);
	}

	#[test]
	fn split_leading_spaces_clamps_the_indent() {
		assert_eq!(
			split_leading_spaces_for_wrap("    body", 10),
			("    ".to_string(), "body".to_string())
		);
		assert_eq!(
			split_leading_spaces_for_wrap("        body", 4),
			("   ".to_string(), "body".to_string())
		);
		assert_eq!(
			split_leading_spaces_for_wrap("body", 0),
			(String::new(), "body".to_string())
		);
	}

	#[test]
	fn arrow_navigation_skips_disabled_rows() {
		let (mut dialog, selected, _) = selector(
			"Pick",
			&["one", "two", "three"],
			HookSelectorOptions {
				disabled_indices: vec![1],
				..HookSelectorOptions::default()
			},
		);
		dialog.handle_key(&Key::Down);
		assert_eq!(dialog.selected_index(), 2, "row 1 is disabled");
		dialog.handle_key(&Key::Down);
		assert_eq!(dialog.selected_index(), 2, "stops at the edge");
		dialog.handle_key(&Key::Up);
		assert_eq!(dialog.selected_index(), 0);
		dialog.handle_key(&Key::Enter);
		assert_eq!(&*selected.borrow(), &["one".to_string()]);
	}

	#[test]
	fn enter_on_empty_or_disabled_selection_does_nothing() {
		let (mut dialog, selected, _) = selector(
			"Pick",
			&["one"],
			HookSelectorOptions {
				disabled_indices: vec![0],
				..HookSelectorOptions::default()
			},
		);
		dialog.handle_key(&Key::Enter);
		assert!(selected.borrow().is_empty());
	}

	#[test]
	fn numbered_quick_select_jumps_and_confirms() {
		let (mut dialog, selected, _) = selector(
			"Review",
			&["Detected PR", "1. Approve", "2. Request changes"],
			HookSelectorOptions::default(),
		);
		assert!(dialog.handle_quick_select(&Key::Char('2')));
		assert_eq!(dialog.selected_label().as_deref(), Some("2. Request changes"));
		assert_eq!(&*selected.borrow(), &["2. Request changes".to_string()]);
	}

	#[test]
	fn quick_select_ignores_digits_while_searching() {
		let labels: Vec<String> = (1..=15).map(|n| format!("{n}. Row {n}")).collect();
		let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
		let (mut dialog, selected, _) = selector("Big", &labels, HookSelectorOptions::default());
		assert!(dialog.is_search_enabled());
		dialog.handle_key(&Key::Char('1'));
		assert_eq!(dialog.query(), "1");
		assert!(selected.borrow().is_empty());
	}

	#[test]
	fn type_to_search_filters_by_description() {
		let options = vec![
			HookSelectorOptionInput::Option(HookSelectorOption {
				label: "Alpha".to_string(),
				description: Some("first letter".to_string()),
			}),
			HookSelectorOptionInput::Label("Beta".to_string()),
		];
		let mut opts = HookSelectorOptions {
			max_visible: 1,
			..HookSelectorOptions::default()
		};
		let selected = Rc::new(RefCell::new(Vec::new()));
		let sink = selected.clone();
		opts.callbacks.on_select = Some(Box::new(move |label| sink.borrow_mut().push(label.to_string())));
		let mut dialog = HookSelectorComponent::new("Pick", options, opts);
		assert!(dialog.is_search_enabled(), "two rows overflow one visible");
		dialog.handle_key(&Key::Char('l'));
		dialog.handle_key(&Key::Char('e'));
		assert_eq!(dialog.query(), "le");
		assert_eq!(dialog.visible_indices(), vec![0]);
		dialog.handle_key(&Key::Backspace);
		assert_eq!(dialog.query(), "l");
		dialog.handle_key(&Key::Backspace);
		assert_eq!(dialog.query(), "");
	}

	#[test]
	fn backspace_on_empty_query_bubbles() {
		let labels: Vec<String> = (1..=15).map(|n| format!("Row {n}")).collect();
		let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
		let (mut dialog, _, _) = selector("Big", &labels, HookSelectorOptions::default());
		dialog.handle_key(&Key::Backspace);
		assert_eq!(dialog.query(), "");
	}

	#[test]
	fn slider_moves_with_arrows_and_clamps() {
		let (mut dialog, _, _) = selector(
			"Pick",
			&["a", "b"],
			HookSelectorOptions {
				slider: Some(HookSelectorSlider {
					caption: Some("continue with".to_string()),
					segments: vec![
						HookSelectorSliderSegment {
							label: "fast".to_string(),
							detail: Some("cheap model".to_string()),
						},
						HookSelectorSliderSegment {
							label: "smart".to_string(),
							detail: None,
						},
					],
					index: 0,
				}),
				..HookSelectorOptions::default()
			},
		);
		assert_eq!(dialog.slider_index(), 0);
		dialog.handle_key(&Key::Right);
		assert_eq!(dialog.slider_index(), 1);
		dialog.handle_key(&Key::Right);
		assert_eq!(dialog.slider_index(), 1, "clamped");
		let line = dialog.slider_line();
		assert!(line.contains("continue with"));
		assert!(line.contains("smart"));
		assert!(!line.contains("cheap model"));
		dialog.handle_key(&Key::Left);
		assert!(dialog.slider_line().contains("cheap model"));
	}

	#[test]
	fn left_right_callbacks_fire_without_a_slider() {
		let left = Rc::new(RefCell::new(0));
		let right = Rc::new(RefCell::new(0));
		let left_sink = left.clone();
		let right_sink = right.clone();
		let mut opts = HookSelectorOptions::default();
		opts.callbacks.on_left = Some(Box::new(move || *left_sink.borrow_mut() += 1));
		opts.callbacks.on_right = Some(Box::new(move || *right_sink.borrow_mut() += 1));
		let mut dialog = HookSelectorComponent::new("Pick", vec!["a".into()], opts);
		dialog.handle_key(&Key::Left);
		dialog.handle_key(&Key::Right);
		assert_eq!(*left.borrow(), 1);
		assert_eq!(*right.borrow(), 1);
	}

	#[test]
	fn radio_and_checkbox_markers_follow_cursor_and_checked_state() {
		let (mut dialog, _, _) = selector(
			"Ask",
			&["one", "two", "Other"],
			HookSelectorOptions {
				selection_marker: Some(SelectionMarker::Radio),
				markable_count: Some(2),
				..HookSelectorOptions::default()
			},
		);
		let rows = dialog.rows();
		assert!(rows[0].text.starts_with('\u{25c9}'), "cursor row filled");
		assert!(rows[0].highlight);
		dialog.move_selection(1);
		let rows = dialog.rows();
		assert!(rows[1].text.starts_with('\u{25c9}'));
		assert!(rows[0].text.starts_with('\u{25cb}'));
		dialog.move_selection(1);
		let rows = dialog.rows();
		assert!(
			rows[2].text.starts_with("> "),
			"control rows keep the cursor prefix: {:?}",
			rows[2]
		);
	}

	#[test]
	fn checkbox_toggles_only_markable_rows() {
		let mut opts = HookSelectorOptions {
			selection_marker: Some(SelectionMarker::Checkbox),
			markable_count: Some(1),
			max_visible: 12,
			..HookSelectorOptions::default()
		};
		opts.checked_indices = vec![0];
		let mut dialog = HookSelectorComponent::new("Todo", vec!["a".into(), "b".into()], opts);
		assert_eq!(dialog.checked_indices(), vec![0]);
		dialog.toggle_checked(0);
		assert!(dialog.checked_indices().is_empty());
		dialog.toggle_checked(1);
		assert!(dialog.checked_indices().is_empty(), "b is a control row");
	}

	#[test]
	fn checkbox_quick_select_only_moves_the_cursor() {
		let (mut dialog, selected, _) = selector(
			"Todo",
			&["1. a", "2. b"],
			HookSelectorOptions {
				selection_marker: Some(SelectionMarker::Checkbox),
				..HookSelectorOptions::default()
			},
		);
		dialog.handle_key(&Key::Char('2'));
		assert_eq!(dialog.selected_index(), 1);
		assert!(selected.borrow().is_empty());
	}

	#[test]
	fn countdown_confirms_the_current_option_on_expiry() {
		let (mut dialog, selected, cancelled) = selector(
			"Timeout",
			&["yes", "no"],
			HookSelectorOptions {
				timeout_ms: 1_000,
				..HookSelectorOptions::default()
			},
		);
		assert!(dialog.has_countdown());
		assert!(!dialog.poll_timeout(500));
		assert!(dialog.poll_timeout(1_100));
		assert_eq!(&*selected.borrow(), &["yes".to_string()]);
		assert_eq!(*cancelled.borrow(), 0);
	}

	#[test]
	fn countdown_cancels_when_the_cursor_row_is_disabled() {
		let (mut dialog, selected, cancelled) = selector(
			"Timeout",
			&["blocked"],
			HookSelectorOptions {
				timeout_ms: 1_000,
				disabled_indices: vec![0],
				..HookSelectorOptions::default()
			},
		);
		assert!(dialog.poll_timeout(1_100));
		assert!(selected.borrow().is_empty());
		assert_eq!(*cancelled.borrow(), 1);
	}

	#[test]
	fn escape_cancels_and_native_actions_route() {
		let (mut dialog, selected, cancelled) = selector(
			"Pick",
			&["a", "b"],
			HookSelectorOptions {
				max_visible: 3,
				..HookSelectorOptions::default()
			},
		);
		dialog.handle_native_action("confirm");
		assert_eq!(&*selected.borrow(), &["a".to_string()]);
		dialog.handle_native_action("cancel");
		assert_eq!(*cancelled.borrow(), 1);
		dialog.handle_key(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 2);

		dialog.handle_key(&Key::Char('z'));
		assert_eq!(dialog.query(), "z");
		dialog.handle_native_action("clear");
		assert_eq!(dialog.query(), "");
	}

	#[test]
	fn activate_row_confirms_a_visible_row_and_ignores_disabled() {
		let (mut dialog, selected, _) = selector(
			"Pick",
			&["a", "b", "c"],
			HookSelectorOptions {
				disabled_indices: vec![2],
				..HookSelectorOptions::default()
			},
		);
		assert!(dialog.activate_row(1));
		assert_eq!(&*selected.borrow(), &["b".to_string()]);
		assert!(!dialog.activate_row(2));
		assert!(!dialog.activate_row(9));
	}

	#[test]
	fn outline_and_inline_flags_are_exposed() {
		let (dialog, _, _) = selector(
			"Pick",
			&["a"],
			HookSelectorOptions {
				outline: true,
				inline: true,
				..HookSelectorOptions::default()
			},
		);
		assert!(dialog.outline());
		assert!(dialog.inline());
		assert!(!dialog.has_slider());
	}

	#[test]
	fn describe_builds_a_list_with_selected_key_and_hints() {
		let (dialog, _, _) = selector(
			"Pick\nsecond line",
			&["a", "b"],
			HookSelectorOptions::default(),
		);
		let described = dialog.describe();
		assert_eq!(described.props.get_str("role"), Some(ROLE));
		assert_eq!(described.children[0].props.get_str("role"), Some("omp.hook-select.detail"));
		let list = &described.children[1];
		assert_eq!(list.props.get_str("selected"), Some("0"));
		assert_eq!(list.children.len(), 2);
		assert_eq!(list.children[0].key.as_deref(), Some("0"));
		assert!(dialog.rows().iter().any(|row| row.text.starts_with("> a")));
	}

	#[test]
	fn status_and_hint_lines_report_the_window() {
		let labels: Vec<String> = (1..=15).map(|n| format!("Row {n}")).collect();
		let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
		let (dialog, _, _) = selector("Big", &labels, HookSelectorOptions::default());
		assert!(dialog.compact());
		assert_eq!(dialog.status_line(), "(1/15)  Type to search");
		assert_eq!(dialog.hint_line(), NAVIGATE_HINT);
	}

	#[test]
	fn plain_inline_strips_markers_and_links() {
		assert_eq!(plain_inline("**bold** and _it_ and `code`"), "bold and it and code");
		assert_eq!(plain_inline("[label](https://example.com)"), "label");
		assert_eq!(plain_inline("[unclosed"), "[unclosed");
	}

	#[test]
	fn ellipsize_keeps_short_text_and_truncates_long() {
		assert_eq!(ellipsize("abc", 5), "abc");
		assert!(ellipsize("abcdefgh", 4).ends_with('…'));
	}

	#[test]
	fn detail_markdown_node_carries_the_role() {
		let node = detail_markdown("**hi**");
		assert_eq!(node.props.get_str("role"), Some("full"));
	}

}
