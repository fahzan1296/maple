//! Reference: `l/src/overlays/history-search.ts`.
//!
//! Prompt-history search: query field, filtered result list with token
//! highlights and ages, deletion of the selected prompt, and a picker sheet
//! form of the same data.
//!
//! Deviations:
//! - `Theme` is injected through [`HistorySearchStyler`]; the reference reads a
//!   module-global `theme`.
//! - `Bun.hash` is not available. [`native_entry_key`] hashes with SHA-256 and
//!   renders the first 8 bytes in base 36; keys stay stable within a process
//!   exactly like the reference, but the digits differ.
//! - Debounced searches are driven by [`HistorySearchComponent::tick`] with a
//!   host-supplied monotonic millisecond clock instead of `setTimeout`.
//! - The reference delegates editing to `Input`; this port carries the query
//!   text/caret state itself so the component has no private-API dependency.
//! - `renderScrollableList` is not applied: the caller adds the scrollbar.

use crate::components::menu_selection::{MenuSource, MenuState};
use crate::components::scroll_viewport::centered_viewport_range;
use crate::chrome::selector_helpers::content_row_width;
use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::overlay_card;
use crate::native::picker::{
	picker, picker_age, picker_date, picker_hits, PickerActionOptions, PickerItem, PickerKeys,
	PickerProps,
};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::theme_class::Theme;
use sha2::{Digest, Sha256};

/// Prompt history fields displayed in search results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistorySearchEntry {
	/// Stable identifier of the stored prompt.
	pub id: i64,
	pub prompt: String,
	pub created_at: i64,
	/// Project folder the prompt was typed in.
	pub cwd: Option<String>,
}

/// Searchable prompt history supplied by the host.
pub trait HistorySource {
	fn search(&self, query: &str, limit: usize) -> Vec<HistorySearchEntry>;
	fn get_recent(&self, limit: usize) -> Vec<HistorySearchEntry>;
	/// Remove a prompt from history.
	fn delete(&mut self, id: i64);
}

/// Key of the `picker` child a dock-mounted history search describes.
pub const PICKER_KEY: &str = "picker";

/// Visible result rows; also the jump distance for PageUp/PageDown.
pub const MAX_VISIBLE: usize = 10;

/// Quiet period after the last query edit before the synchronous storage
/// search runs. Only applied when a repaint hook is wired, since the deferred
/// results need a repaint to show up.
pub const SEARCH_DEBOUNCE_MS: u64 = 100;

/// Maximum number of results fetched from the host.
pub const RESULT_LIMIT: usize = 100;

/// Styling hooks the host supplies (the reference reads a global `theme`).
pub trait HistorySearchStyler {
	fn fg(&self, token: &str, text: &str) -> String;
	fn bg(&self, token: &str, text: &str) -> String;
	fn bold(&self, text: &str) -> String;
	fn symbol(&self, key: &str) -> String;
}

impl HistorySearchStyler for Theme {
	fn fg(&self, token: &str, text: &str) -> String {
		Theme::fg(self, token, text).unwrap_or_else(|_| text.to_string())
	}
	fn bg(&self, token: &str, text: &str) -> String {
		Theme::bg(self, token, text).unwrap_or_else(|_| text.to_string())
	}
	fn bold(&self, text: &str) -> String {
		Theme::bold(self, text)
	}
	fn symbol(&self, key: &str) -> String {
		Theme::symbol(self, key).to_string()
	}
}

fn base36(mut value: u64) -> String {
	const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
	if value == 0 {
		return "0".to_string();
	}
	let mut out = Vec::new();
	while value > 0 {
		out.push(DIGITS[(value % 36) as usize]);
		value /= 36;
	}
	out.reverse();
	String::from_utf8(out).unwrap_or_default()
}

/// Native item key of a history entry: key-path safe (prompts may hold `/` and
/// newlines), stable across queries.
#[must_use]
pub fn native_entry_key(entry: &HistorySearchEntry) -> String {
	let digest = Sha256::digest(entry.prompt.as_bytes());
	let mut head = [0u8; 8];
	head.copy_from_slice(&digest[..8]);
	format!("{}-{}", entry.created_at, base36(u64::from_be_bytes(head)))
}

/// Split a query the same way the storage tokenizes it, so highlights align
/// with matches.
#[must_use]
pub fn query_tokens(query: &str) -> Vec<String> {
	query
		.to_lowercase()
		.split(|ch: char| !ch.is_alphanumeric())
		.filter(|token| !token.is_empty())
		.map(str::to_string)
		.collect()
}

/// Wrap every case-insensitive occurrence of any token in `text` with the
/// accent color.
#[must_use]
pub fn highlight_tokens(
	text: &str,
	tokens: &[String],
	styler: &dyn HistorySearchStyler,
) -> String {
	let owned: Vec<&str> = tokens.iter().map(String::as_str).collect();
	let ranges = picker_hits(text, &owned);
	if ranges.is_empty() {
		return text.to_string();
	}
	// `picker_hits` returns UTF-16 offsets; walk chars, not bytes.
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::new();
	let mut pos = 0usize;
	for (start, end) in ranges {
		if start > pos {
			out.extend(&chars[pos.min(chars.len())..start.min(chars.len())]);
		}
		let hit: String = chars
			.get(start.min(chars.len())..end.min(chars.len()))
			.unwrap_or(&[])
			.iter()
			.collect();
		out.push_str(&styler.fg("accent", &hit));
		pos = end;
	}
	if pos < chars.len() {
		out.extend(&chars[pos..]);
	}
	out
}

/// A past prompt as a picker row: its first line (with query hits), age and
/// project folder. `now_ms` is the epoch-millisecond clock for the age.
#[must_use]
pub fn history_picker_item(
	entry: &HistorySearchEntry,
	tokens: &[String],
	now_ms: i64,
) -> PickerItem {
	let first_line = entry.prompt.trim().split('\n').next().unwrap_or("");
	let label: String = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
	let owned: Vec<&str> = tokens.iter().map(String::as_str).collect();
	let hits = picker_hits(&label, &owned);

	let mut item = PickerItem::new(native_entry_key(entry), label);
	if let Some(cwd) = &entry.cwd {
		item.rest.set_str("detail", shorten_path(cwd));
	}
	item.rest
		.insert("facts", serde_json::json!({ "when": picker_age(entry.created_at * 1000, now_ms) }));
	if !hits.is_empty() {
		item.rest.insert(
			"hits",
			serde_json::Value::Array(
				hits.iter()
					.map(|(start, end)| serde_json::json!([start, end]))
					.collect(),
			),
		);
	}
	item.rest.set_str("title", picker_date(entry.created_at * 1000));
	item
}

/// `shortenPath` from `render/render-utils`: the home directory collapses to
/// `~`. The crate has no shared helper yet, so keep the one rule here.
fn shorten_path(path: &str) -> String {
	match std::env::var_os("HOME") {
		Some(home) => {
			let home = home.to_string_lossy();
			match path.strip_prefix(home.as_ref()) {
				Some(rest) if rest.is_empty() => "~".to_string(),
				Some(rest) if rest.starts_with('/') => format!("~{rest}"),
				_ => path.to_string(),
			}
		}
		None => path.to_string(),
	}
}

/// Identity/filter behaviour of a history entry inside a menu.
struct HistoryEntrySource;

impl MenuSource<HistorySearchEntry> for HistoryEntrySource {
	fn key(&self, item: &HistorySearchEntry) -> String {
		format!("{}:{}", item.created_at, item.prompt)
	}
	fn search_text(&self, item: &HistorySearchEntry) -> String {
		item.prompt.clone()
	}
}

/// A query field: text and caret.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryInput {
	value: String,
	cursor: usize,
}

impl QueryInput {
	#[must_use]
	pub fn value(&self) -> &str {
		&self.value
	}

	#[must_use]
	pub fn cursor(&self) -> usize {
		self.cursor
	}

	/// Replace the text, moving the caret to the end.
	pub fn set_value(&mut self, value: impl Into<String>) {
		self.value = value.into();
		self.cursor = self.value.len();
	}

	fn insert(&mut self, text: &str) {
		self.value.insert_str(self.cursor, text);
		self.cursor += text.len();
	}

	fn backspace(&mut self) {
		if self.cursor == 0 {
			return;
		}
		let previous = self.value[..self.cursor]
			.char_indices()
			.next_back()
			.map_or(0, |(offset, _)| offset);
		self.value.replace_range(previous..self.cursor, "");
		self.cursor = previous;
	}

	fn delete_forward(&mut self) {
		if self.cursor >= self.value.len() {
			return;
		}
		let next = self.value[self.cursor..]
			.char_indices()
			.nth(1)
			.map_or(self.value.len(), |(offset, _)| self.cursor + offset);
		self.value.replace_range(self.cursor..next, "");
	}

	fn move_left(&mut self) {
		if self.cursor > 0 {
			self.cursor = self.value[..self.cursor]
				.char_indices()
				.next_back()
				.map_or(0, |(offset, _)| offset);
		}
	}

	fn move_right(&mut self) {
		if self.cursor < self.value.len() {
			self.cursor = self.value[self.cursor..]
				.char_indices()
				.nth(1)
				.map_or(self.value.len(), |(offset, _)| self.cursor + offset);
		}
	}

	fn handle(&mut self, key: &HistoryKey) -> bool {
		match key {
			HistoryKey::Char(ch) => {
				self.insert(&ch.to_string());
				true
			}
			HistoryKey::Backspace => {
				self.backspace();
				true
			}
			HistoryKey::Delete => {
				self.delete_forward();
				true
			}
			HistoryKey::Left => {
				self.move_left();
				true
			}
			HistoryKey::Right => {
				self.move_right();
				true
			}
			HistoryKey::Home => {
				self.cursor = 0;
				true
			}
			HistoryKey::End => {
				self.cursor = self.value.len();
				true
			}
			_ => false,
		}
	}
}

/// Keys the history search reacts to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryKey {
	Up,
	Down,
	PageUp,
	PageDown,
	Home,
	End,
	Delete,
	Enter,
	Cancel,
	Char(char),
	Backspace,
	Left,
	Right,
}

impl HistoryKey {
	/// Translate a reference key name (`"up"`, `"escape"`, `"a"`) into a key.
	/// Unrecognized names fall through to [`HistoryKey::Char`] only for single
	/// characters.
	#[must_use]
	pub fn parse(name: &str) -> Option<HistoryKey> {
		let key = match name {
			"up" | "shift+up" | "ctrl+p" => HistoryKey::Up,
			"down" | "shift+down" | "ctrl+n" => HistoryKey::Down,
			"pageup" | "shift+pageup" => HistoryKey::PageUp,
			"pagedown" | "shift+pagedown" => HistoryKey::PageDown,
			"home" => HistoryKey::Home,
			"end" => HistoryKey::End,
			"delete" => HistoryKey::Delete,
			"enter" | "return" | "\n" | "\r" => HistoryKey::Enter,
			"escape" | "esc" | "ctrl+c" => HistoryKey::Cancel,
			"backspace" => HistoryKey::Backspace,
			"left" => HistoryKey::Left,
			"right" => HistoryKey::Right,
			other => {
				let mut chars = other.chars();
				match (chars.next(), chars.next()) {
					(Some(ch), None) => HistoryKey::Char(ch),
					_ => return None,
				}
			}
		};
		Some(key)
	}
}

/// Token-highlighted results list with a per-entry highlight cache.
pub struct HistoryResultsList {
	tokens: Vec<String>,
	max_visible: usize,
	/// `(token generation, budget) -> highlighted text`, per entry id.
	cache: std::collections::HashMap<i64, (u64, usize, String)>,
	generation: u64,
}

impl Default for HistoryResultsList {
	fn default() -> Self {
		Self::new()
	}
}

impl HistoryResultsList {
	#[must_use]
	pub fn new() -> Self {
		HistoryResultsList {
			tokens: Vec::new(),
			max_visible: MAX_VISIBLE,
			cache: std::collections::HashMap::new(),
			generation: 0,
		}
	}

	pub fn set_tokens(&mut self, tokens: Vec<String>) {
		self.tokens = tokens;
		self.generation = self.generation.wrapping_add(1);
	}

	#[must_use]
	pub fn tokens(&self) -> &[String] {
		&self.tokens
	}

	pub fn set_max_visible(&mut self, rows: usize) {
		self.max_visible = rows.max(1);
	}

	/// Drop cached highlights (theme change).
	pub fn invalidate(&mut self) {
		self.cache.clear();
		self.generation = self.generation.wrapping_add(1);
	}

	fn highlighted_prompt(
		&mut self,
		entry: &HistorySearchEntry,
		budget: usize,
		_now_ms: i64,
		styler: &dyn HistorySearchStyler,
	) -> String {
		if let Some((generation, cached_budget, text)) = self.cache.get(&entry.id) {
			if *generation == self.generation && *cached_budget == budget {
				return text.clone();
			}
		}
		let normalized: String = entry.prompt.split_whitespace().collect::<Vec<_>>().join(" ");
		let text = highlight_tokens(
			&truncate_to_width(&normalized, budget, Ellipsis::Omit),
			&self.tokens,
			styler,
		);
		self.cache
			.insert(entry.id, (self.generation, budget, text.clone()));
		text
	}

	/// Render the visible result rows for `width`. `now_ms` drives ages.
	#[must_use]
	pub fn render(
		&mut self,
		items: &[HistorySearchEntry],
		selected_index: isize,
		width: usize,
		now_ms: i64,
		styler: &dyn HistorySearchStyler,
	) -> Vec<String> {
		if items.is_empty() {
			let message = if self.tokens.is_empty() {
				"No history yet"
			} else {
				"No matching history"
			};
			return vec![styler.fg(
				"muted",
				&format!("  {} {message}", styler.symbol("status.info")),
			)];
		}

		let cursor_symbol = format!("{} ", styler.symbol("nav.cursor"));
		let gutter_width = visible_width(&cursor_symbol);
		let selected = selected_index.max(0) as usize;
		let range = centered_viewport_range(selected, items.len(), self.max_visible);
		let row_width = content_row_width(width, items.len(), self.max_visible);

		let mut rows = Vec::new();
		for index in range.start..range.end {
			let Some(entry) = items.get(index) else {
				continue;
			};
			let is_selected = index as isize == selected_index;
			let time_str = picker_age(entry.created_at * 1000, now_ms);
			let time_width = visible_width(&time_str);
			let show_time = row_width >= gutter_width + 12 + time_width;
			let prompt_budget = row_width
				.saturating_sub(gutter_width)
				.saturating_sub(if show_time { time_width + 1 } else { 0 })
				.max(4);
			let highlighted = self.highlighted_prompt(entry, prompt_budget, now_ms, styler);

			let cursor = if is_selected {
				styler.fg("accent", &cursor_symbol)
			} else {
				" ".repeat(gutter_width)
			};
			let mut line = if is_selected {
				format!("{cursor}{}", styler.bold(&highlighted))
			} else {
				format!("{cursor}{highlighted}")
			};

			if show_time {
				// Pad the prompt region so the timestamp sits flush right with a
				// one-cell gap.
				let mut head = truncate_to_width(&line, row_width.saturating_sub(time_width + 1), Ellipsis::Unicode);
				let head_width = visible_width(&head);
				if head_width < row_width.saturating_sub(time_width + 1) {
					head.push_str(&" ".repeat(row_width.saturating_sub(time_width + 1) - head_width));
				}
				line = format!("{head} {}", styler.fg("dim", &time_str));
			}

			rows.push(if is_selected {
				let clipped = truncate_to_width(&line, row_width, Ellipsis::Omit);
				let clipped_width = visible_width(&clipped);
				let padded = if clipped_width < row_width {
					format!("{clipped}{}", " ".repeat(row_width - clipped_width))
				} else {
					clipped
				};
				styler.bg("selectedBg", &padded)
			} else {
				truncate_to_width(&line, row_width, Ellipsis::Omit)
			});
		}
		rows
	}
}

/// Search overlay state: query, results, selection, and debounce.
pub struct HistorySearchComponent<S: HistorySource> {
	history_storage: S,
	query: QueryInput,
	menu: MenuState<HistorySearchEntry>,
	results_list: HistoryResultsList,
	on_select: Box<dyn FnMut(&str)>,
	on_cancel: Box<dyn FnMut()>,
	on_request_render: Option<Box<dyn FnMut()>>,
	/// Trimmed query the current results were fetched for; `None` before the
	/// first fetch.
	results_query: Option<String>,
	/// Deadline (monotonic ms) of a pending debounced search.
	pending_deadline_ms: Option<u64>,
	/// Generation of the cached picker rows and the items they describe.
	picker_items: Option<(Vec<HistorySearchEntry>, Vec<PickerItem>)>,
}

impl<S: HistorySource> HistorySearchComponent<S> {
	/// Role of the overlay root (`omp.overlay.history`).
	pub const ROLE: &'static str = "omp.overlay.history";

	/// Title shown in the overlay head.
	pub const TITLE: &'static str = "History";

	pub fn new(
		history_storage: S,
		on_select: impl FnMut(&str) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		// Reference starts with an empty result set; the first fetch runs below.
		let mut component = HistorySearchComponent {
			history_storage,
			query: QueryInput::default(),
			menu: MenuState::new(Vec::new(), Box::new(HistoryEntrySource)),
			results_list: HistoryResultsList::new(),
			on_select: Box::new(on_select),
			on_cancel: Box::new(on_cancel),
			on_request_render: None,
			results_query: None,
			pending_deadline_ms: None,
			picker_items: None,
		};
		component.update_results();
		component
	}

	/// Repaint hook for debounced result updates. Once set, query edits search
	/// storage after a [`SEARCH_DEBOUNCE_MS`] quiet period instead of on every
	/// key; without it every edit searches synchronously.
	pub fn set_on_request_render(&mut self, callback: impl FnMut() + 'static) {
		self.on_request_render = Some(Box::new(callback));
	}

	#[must_use]
	pub fn query(&self) -> &str {
		self.query.value()
	}

	#[must_use]
	pub fn cursor(&self) -> usize {
		self.query.cursor()
	}

	#[must_use]
	pub fn results(&self) -> &[HistorySearchEntry] {
		self.menu.visible_items()
	}

	#[must_use]
	pub fn selected(&self) -> Option<&HistorySearchEntry> {
		self.menu.selected_item()
	}

	#[must_use]
	pub fn selected_index(&self) -> isize {
		self.menu.selected_index()
	}

	/// Whether a debounced search is waiting for its deadline.
	#[must_use]
	pub fn has_pending_search(&self) -> bool {
		self.pending_deadline_ms.is_some()
	}

	#[must_use]
	pub fn results_list(&self) -> &HistoryResultsList {
		&self.results_list
	}

	#[must_use]
	pub fn results_list_mut(&mut self) -> &mut HistoryResultsList {
		&mut self.results_list
	}

	/// Handle one key. `now_ms` is a monotonic clock for the debounce timer.
	/// Returns whether the key was consumed.
	pub fn handle_key(&mut self, key: HistoryKey, now_ms: u64) -> bool {
		match key {
			HistoryKey::Up => {
				self.flush_pending_search();
				self.menu.move_selection(-1, false);
				true
			}
			HistoryKey::Down => {
				self.flush_pending_search();
				self.menu.move_selection(1, false);
				true
			}
			HistoryKey::PageUp => {
				self.flush_pending_search();
				self.menu.move_selection(-(MAX_VISIBLE as isize), false);
				true
			}
			HistoryKey::PageDown => {
				self.flush_pending_search();
				self.menu.move_selection(MAX_VISIBLE as isize, false);
				true
			}
			HistoryKey::Home => {
				self.flush_pending_search();
				self.menu.move_to_boundary(true);
				true
			}
			HistoryKey::End => {
				self.flush_pending_search();
				self.menu.move_to_boundary(false);
				true
			}
			HistoryKey::Delete if self.query.cursor() == self.query.value().len() => {
				self.flush_pending_search();
				self.delete_selected();
				true
			}
			HistoryKey::Enter => {
				self.flush_pending_search();
				if let Some(prompt) = self.selected().map(|entry| entry.prompt.clone()) {
					(self.on_select)(&prompt);
				}
				true
			}
			HistoryKey::Cancel => {
				(self.on_cancel)();
				true
			}
			other => {
				let handled = self.query.handle(&other);
				self.schedule_results(now_ms);
				handled
			}
		}
	}

	/// Delete the selected prompt and refresh.
	pub fn delete_selected(&mut self) {
		let Some(id) = self.selected().map(|entry| entry.id) else {
			return;
		};
		self.history_storage.delete(id);
		self.update_results();
	}

	/// Select the entry whose native key is `key`; returns whether it existed.
	pub fn select_native_key(&mut self, key: &str) -> bool {
		let index = self
			.menu
			.visible_items()
			.iter()
			.position(|entry| native_entry_key(entry) == key);
		match index {
			Some(index) => {
				self.menu.set_selected_index(index as isize, 0);
				true
			}
			None => false,
		}
	}

	/// Native `select`/`activate` event for a result. Flushes a pending search
	/// first: a row picked while a debounced search is pending belongs to the
	/// previous query, so results are refreshed and the pick only lands if the
	/// entry is still among them.
	pub fn activate_native_key(&mut self, key: &str) -> bool {
		self.flush_pending_search();
		if !self.select_native_key(key) {
			return false;
		}
		if let Some(prompt) = self.selected().map(|entry| entry.prompt.clone()) {
			(self.on_select)(&prompt);
			true
		} else {
			false
		}
	}

	/// Clear the query and refresh (`Clear search` picker action).
	pub fn clear_query(&mut self) {
		self.query.set_value("");
		self.update_results();
	}

	/// Picker action `close`.
	pub fn cancel(&mut self) {
		self.cancel_pending_search();
		(self.on_cancel)();
	}

	/// Run a pending debounced search now, so selection acts on the current
	/// query's results.
	pub fn flush_pending_search(&mut self) {
		if self.pending_deadline_ms.take().is_none() {
			return;
		}
		self.update_results();
	}

	/// Advance the debounce clock; runs the search and repaint hook when the
	/// quiet period elapsed. Returns whether a search ran.
	pub fn tick(&mut self, now_ms: u64) -> bool {
		match self.pending_deadline_ms {
			Some(deadline) if now_ms >= deadline => {
				self.pending_deadline_ms = None;
				self.update_results();
				if let Some(callback) = self.on_request_render.as_mut() {
					callback();
				}
				true
			}
			_ => false,
		}
	}

	fn cancel_pending_search(&mut self) {
		self.pending_deadline_ms = None;
	}

	/// Refresh results after a query edit: no-op when the trimmed query is
	/// unchanged (cursor moves, trailing spaces), immediate for the empty query
	/// or when no repaint hook is wired, debounced otherwise.
	fn schedule_results(&mut self, now_ms: u64) {
		let query = self.query.value().trim().to_string();
		if Some(query.as_str()) == self.results_query.as_deref() {
			self.cancel_pending_search();
			return;
		}
		if query.is_empty() || self.on_request_render.is_none() {
			self.update_results();
			return;
		}
		self.pending_deadline_ms = Some(now_ms + SEARCH_DEBOUNCE_MS);
	}

	/// Re-run the search for the current query and reset the selection.
	pub fn update_results(&mut self) {
		self.cancel_pending_search();
		let query = self.query.value().trim().to_string();
		self.results_query = Some(query.clone());
		let results = if query.is_empty() {
			self.history_storage.get_recent(RESULT_LIMIT)
		} else {
			self.history_storage.search(&query, RESULT_LIMIT)
		};
		self.menu.set_items(results);
		self.menu.move_to_boundary(true);
		self.results_list.set_tokens(if query.is_empty() {
			Vec::new()
		} else {
			query_tokens(&query)
		});
		self.picker_items = None;
	}

	/// Render the result list as terminal lines.
	#[must_use]
	pub fn render_results(
		&mut self,
		width: usize,
		now_ms: i64,
		styler: &dyn HistorySearchStyler,
	) -> Vec<String> {
		let items = self.menu.visible_items().to_vec();
		let selected = self.menu.selected_index();
		self.results_list
			.render(&items, selected, width, now_ms, styler)
	}

	/// Picker rows for the current results, memoized per item generation.
	fn picker_rows(&mut self, now_ms: i64) -> &[PickerItem] {
		let items = self.menu.visible_items().to_vec();
		let stale = match &self.picker_items {
			Some((cached, _)) => cached != &items,
			None => true,
		};
		if stale {
			let tokens = query_tokens(self.query.value().trim());
			let rows = items
				.iter()
				.map(|entry| history_picker_item(entry, &tokens, now_ms))
				.collect();
			self.picker_items = Some((items, rows));
		}
		&self.picker_items.as_ref().map_or(&[][..], |(_, rows)| rows)
	}

	/// Native description of the picker sheet (`size:"md"`, rows layout).
	pub fn describe_picker(&mut self, now_ms: i64) -> NativeNode {
		let selected = self.menu.selected_item().map(native_entry_key);
		let query = self.query.value().to_string();
		let cursor = self.query.cursor();
		let rows = self.picker_rows(now_ms).to_vec();
		let props = PickerProps {
			title: Self::TITLE.to_string(),
			icon: Some("history".to_string()),
			noun: "prompts".to_string(),
			size: "md".to_string(),
			layout: "rows".to_string(),
			preview: "none".to_string(),
			query: Some(query),
			cursor: Some(cursor),
			items: rows,
			selected,
			columns: Some(vec![crate::native::picker::PickerColumn {
				id: "when".to_string(),
				head: None,
				format: Some("time".to_string()),
				align: None,
				grow: None,
				priority: None,
			}]),
			actions: vec![
				crate::native::picker::picker_action(
					"insert",
					"Insert",
					Some(PickerKeys::Id("enter")),
					PickerActionOptions {
						primary: true,
						..PickerActionOptions::default()
					},
				),
				crate::native::picker::picker_action(
					"close",
					"Close",
					Some(PickerKeys::Id("escape")),
					PickerActionOptions {
						end: true,
						..PickerActionOptions::default()
					},
				),
			],
			..PickerProps::default()
		};
		let mut wire = props.to_props();
		wire.set_str("placeholder", "Search prompts…");
		wire.set_str("empty", "No history yet");

		node(
			NodeKind::Col,
			Props::new(),
			vec![picker(wire, None)],
			Some(PICKER_KEY),
		)
	}

	/// Native description of the docked list form: the query field, result
	/// items keyed by entry, and the key hints.
	pub fn describe_list(&mut self, now_ms: i64) -> NativeNode {
		let items = self.menu.visible_items();
		let selected = self.menu.selected_item().map(native_entry_key);
		let query = self.query.value().trim().to_string();

		let rows: Vec<NativeNode> = items
			.iter()
			.map(|entry| {
				let label: String = entry.prompt.split_whitespace().collect::<Vec<_>>().join(" ");
				node(
					NodeKind::Item,
					{
						let mut props = Props::new();
						props.set_str("label", label);
						props.set_str("value", picker_age(entry.created_at * 1000, now_ms));
						props
					},
					Vec::new(),
					Some(&native_entry_key(entry)),
				)
			})
			.collect();
		let mut list_props = Props::new();
		if let Some(selected) = selected {
			list_props.set_str("selected", selected);
		} else {
			list_props.insert("selected", serde_json::Value::Null);
		}
		if !query.is_empty() {
			list_props.set_str("filter", &query);
		}
		list_props.set_str(
			"empty",
			if query.is_empty() {
				"No history yet"
			} else {
				"No matching history"
			},
		);
		list_props.insert("max", serde_json::json!({ "lines": MAX_VISIBLE }));
		list_props.set_bool("virtual", true);
		let list = node(NodeKind::List, list_props, rows, Some("list"));

		let mut input_props = Props::new();
		input_props.set_str("value", self.query.value());
		input_props.set_i64("cursor", self.query.cursor() as i64);
		input_props.set_str("placeholder", "Search prompts…");
		let field = node(NodeKind::Input, input_props, Vec::new(), Some("query"));

		let hints = node(
			NodeKind::Row,
			{
				let mut props = Props::new();
				props.set_role("omp.overlay.hints");
				props
			},
			Vec::new(),
			Some("hints"),
		);

		let children = vec![field, list, hints];
		overlay_card(
			Self::ROLE,
			Some(NodeText::Plain(Self::TITLE.to_string())),
			children,
			None,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	struct Styler;

	impl HistorySearchStyler for Styler {
		fn fg(&self, token: &str, text: &str) -> String {
			format!("<{token}>{text}</{token}>")
		}
		fn bg(&self, token: &str, text: &str) -> String {
			format!("[{token}]{text}[/{token}]")
		}
		fn bold(&self, text: &str) -> String {
			format!("*{text}*")
		}
		fn symbol(&self, key: &str) -> String {
			match key {
				"nav.cursor" => "❯".to_string(),
				"status.info" => "ⓘ".to_string(),
				_ => String::new(),
			}
		}
	}

	#[derive(Clone)]
	struct MemoryHistory {
		entries: Vec<HistorySearchEntry>,
	}

	impl HistorySource for MemoryHistory {
		fn search(&self, query: &str, limit: usize) -> Vec<HistorySearchEntry> {
			let tokens = query_tokens(query);
			self.entries
				.iter()
				.filter(|entry| {
					let lower = entry.prompt.to_lowercase();
					tokens.iter().all(|token| lower.contains(token))
				})
				.take(limit)
				.cloned()
				.collect()
		}

		fn get_recent(&self, limit: usize) -> Vec<HistorySearchEntry> {
			self.entries.iter().rev().take(limit).cloned().collect()
		}

		fn delete(&mut self, id: i64) {
			self.entries.retain(|entry| entry.id != id);
		}
	}

	fn entry(id: i64, prompt: &str) -> HistorySearchEntry {
		HistorySearchEntry {
			id,
			prompt: prompt.to_string(),
			created_at: 1_700_000_000 + id,
			cwd: None,
		}
	}

	fn history() -> MemoryHistory {
		MemoryHistory {
			entries: vec![
				entry(1, "fix the parser"),
				entry(2, "add history search"),
				entry(3, "search tokens"),
			],
		}
	}

	#[test]
	fn native_entry_key_is_stable_and_distinct() {
		let a = entry(1, "prompt with /slash and\nnewline");
		let same = entry(1, "prompt with /slash and\nnewline");
		let other = entry(2, "prompt with /slash and\nnewline");
		assert_eq!(native_entry_key(&a), native_entry_key(&same));
		assert_ne!(native_entry_key(&a), native_entry_key(&other));
		assert!(!native_entry_key(&a).contains('/'));
		assert!(!native_entry_key(&a).contains('\n'));
	}

	#[test]
	fn query_tokens_lowercases_and_splits_on_punctuation() {
		assert_eq!(
			query_tokens("Fix, the PARSER!"),
			vec!["fix", "the", "parser"]
		);
		assert!(query_tokens("   ").is_empty());
	}

	#[test]
	fn highlight_wraps_every_hit() {
		let tokens = query_tokens("parser fix");
		let styled = highlight_tokens("fix the parser", &tokens, &Styler);
		assert_eq!(styled, "<accent>fix</accent> the <accent>parser</accent>");
	}

	#[test]
	fn picker_item_uses_first_line_and_age() {
		let mut item = entry(7, "first line\nsecond line");
		item.cwd = Some("/tmp/project".to_string());
		let item = history_picker_item(&item, &query_tokens("first"), 1_700_000_000_000 + 120_000);
		assert_eq!(item.label, "first line");
		assert_eq!(item.rest.get_str("detail"), Some("/tmp/project"));
		assert_eq!(
			item.rest.get("facts").and_then(|f| f.get("when")).and_then(|w| w.as_str()),
			Some("1m")
		);
		assert!(item.rest.get_str("title").is_some());
	}

	#[test]
	fn empty_query_lists_recent_and_selection_starts_first() {
		let component = HistorySearchComponent::new(history(), |_| {}, || {});
		assert_eq!(component.results().len(), 3);
		assert_eq!(component.selected().map(|e| e.id), Some(3));
		assert!(component.results_list().tokens().is_empty());
	}

	#[test]
	fn typing_filters_results() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		for ch in "parser".chars() {
			assert!(component.handle_key(HistoryKey::Char(ch), 0));
		}
		assert_eq!(component.query(), "parser");
		assert_eq!(component.results().len(), 1);
		assert_eq!(component.results()[0].id, 1);
		assert_eq!(component.results_list().tokens(), &["parser".to_string()]);
	}

	#[test]
	fn enter_selects_the_highlighted_prompt() {
		let picked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = picked.clone();
		let mut component = HistorySearchComponent::new(
			history(),
			move |prompt| sink.borrow_mut().push(prompt.to_string()),
			|| {},
		);
		component.handle_key(HistoryKey::Up, 0);
		assert_eq!(component.selected().map(|e| e.id), Some(2));
		assert!(component.handle_key(HistoryKey::Enter, 0));
		assert_eq!(&*picked.borrow(), &["add history search".to_string()]);
	}

	#[test]
	fn delete_at_end_removes_the_selected_entry() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		assert!(component.handle_key(HistoryKey::Delete, 0));
		assert_eq!(component.results().len(), 2);
		assert!(component.results().iter().all(|entry| entry.id != 3));
	}

	#[test]
	fn delete_mid_query_edits_text_instead() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		component.handle_key(HistoryKey::Char('a'), 0);
		component.handle_key(HistoryKey::Char('d'), 0);
		component.handle_key(HistoryKey::Left, 0);
		assert!(component.handle_key(HistoryKey::Delete, 0));
		assert_eq!(component.query(), "a");
		assert_eq!(component.results().len(), 3);
	}

	#[test]
	fn page_down_jumps_a_window() {
		let mut many = MemoryHistory {
			entries: (0..30).map(|i| entry(i, &format!("prompt {i}"))).collect(),
		};
		many.entries.reverse();
		let mut component = HistorySearchComponent::new(many, |_| {}, || {});
		component.handle_key(HistoryKey::PageDown, 0);
		assert_eq!(component.selected().map(|e| e.id), Some(10));
		component.handle_key(HistoryKey::End, 0);
		assert_eq!(component.selected().map(|e| e.id), Some(29));
	}

	#[test]
	fn cancel_runs_the_cancel_callback() {
		let cancelled = Rc::new(RefCell::new(0));
		let sink = cancelled.clone();
		let mut component = HistorySearchComponent::new(history(), |_| {}, move || {
			*sink.borrow_mut() += 1;
		});
		component.handle_key(HistoryKey::Cancel, 0);
		component.cancel();
		assert_eq!(*cancelled.borrow(), 2);
	}

	#[test]
	fn query_edits_debounce_only_when_a_repaint_hook_is_wired() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		component.handle_key(HistoryKey::Char('p'), 1_000);
		// No hook: searched immediately.
		assert!(!component.has_pending_search());
		assert_eq!(component.results().len(), 1);

		let renders = Rc::new(RefCell::new(0));
		let sink = renders.clone();
		component.set_on_request_render(move || *sink.borrow_mut() += 1);
		component.handle_key(HistoryKey::Char('a'), 2_000);
		assert!(component.has_pending_search());
		assert!(!component.tick(2_050));
		assert!(component.tick(2_100));
		assert_eq!(*renders.borrow(), 1);
		assert_eq!(component.query(), "pa");
	}

	#[test]
	fn cursor_moves_with_trailing_spaces_do_not_requery() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		component.handle_key(HistoryKey::Char('p'), 0);
		component.handle_key(HistoryKey::Char(' '), 0);
		// Trimmed query unchanged: results stay those for "p".
		assert_eq!(component.results().len(), 1);
		component.handle_key(HistoryKey::Left, 0);
		assert_eq!(component.cursor(), 1);
	}

	#[test]
	fn activating_a_native_key_selects_and_reports() {
		let picked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
		let sink = picked.clone();
		let mut component = HistorySearchComponent::new(
			history(),
			move |prompt| sink.borrow_mut().push(prompt.to_string()),
			|| {},
		);
		let key = native_entry_key(&entry(1, "fix the parser"));
		assert!(component.activate_native_key(&key));
		assert_eq!(component.selected().map(|e| e.id), Some(1));
		assert_eq!(&*picked.borrow(), &["fix the parser".to_string()]);
		assert!(!component.activate_native_key("missing-key"));
	}

	#[test]
	fn results_render_marks_selection_and_empty_state() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		let lines = component.render_results(60, 1_700_000_010_000, &Styler);
		assert_eq!(lines.len(), 3);
		assert!(lines[0].starts_with("[selectedBg]"));
		assert!(lines[0].contains("<accent>❯ </accent>"));

		for ch in "nothing".chars() {
			component.handle_key(HistoryKey::Char(ch), 0);
		}
		let lines = component.render_results(60, 1_700_000_010_000, &Styler);
		assert_eq!(lines.len(), 1);
		assert!(lines[0].contains("No matching history"));
	}

	#[test]
	fn picker_description_carries_rows_and_actions() {
		let mut component = HistorySearchComponent::new(history(), |_| {}, || {});
		let described = component.describe_picker(1_700_000_010_000);
		let picker = &described.children[0];
		assert_eq!(
			picker.prop("items").and_then(|i| i.as_array()).map(Vec::len),
			Some(3)
		);
		assert_eq!(
			picker.prop("placeholder").and_then(|v| v.as_str()),
			Some("Search prompts…")
		);
		let actions = picker
			.prop("actions")
			.and_then(|a| a.as_array())
			.expect("actions");
		assert_eq!(actions.len(), 2);
		assert_eq!(
			actions[0].get("id").and_then(|v| v.as_str()),
			Some("insert")
		);
	}
}
