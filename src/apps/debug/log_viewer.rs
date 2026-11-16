//! Debug log viewer model: filtering, pagination, expansion and selection over
//! captured log entries.
//!
//! Reference: `l/src/apps/debug/log-viewer.ts` (`DebugLogViewerModel` and its
//! exported helpers). The interactive component is ported separately.
//!
//! Deviations: `sanitizeText` from `@oh-my-pi/pi-utils` is not vendored, so the
//! copy payload uses `strip_ansi` plus control-character removal; the external
//! "load older" fetch is synchronous (`FnMut`) because the port has no async
//! runtime; the default process start is the current time because std has no
//! process-uptime API.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use maple_tui::strip_ansi;

use super::log_formatting::{parse_debug_log_pid, parse_debug_log_timestamp_ms};

/// Separator marking logs captured before the current process started.
pub const SESSION_BOUNDARY_WARNING: &str = "### WARNING - Logs above are older than current session!";
/// Action row for loading earlier log entries.
pub const LOAD_OLDER_LABEL: &str = "### MOVE UP TO LOAD MORE...";

/// Rows loaded from the captured buffer on construction.
const INITIAL_LOG_CHUNK: usize = 50;
/// Rows revealed per "load older" step.
const LOAD_OLDER_CHUNK: usize = 50;
/// Entry cap; older history beyond it is not loaded.
const MAX_LOG_ENTRIES: usize = 50_000;

/// One captured log line.
#[derive(Clone, Debug)]
struct LogEntry {
	raw_line: String,
	timestamp_ms: Option<i64>,
	pid: Option<f64>,
	/// Lowercased `raw_line`, filled on first filter match.
	lower_line: Option<String>,
}

/// A selectable or marker row in the viewer body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerRow {
	/// Session boundary marker.
	Warning,
	/// "Load older" action row.
	LoadOlder,
	/// A log entry by index (0 = oldest).
	Log(usize),
}

/// Selection derived from the cursor and anchor, memoized until either moves.
#[derive(Clone, Debug)]
struct SelectionState {
	generation: u64,
	cursor: usize,
	anchor: Option<usize>,
	indices: Vec<usize>,
	set: HashSet<usize>,
}

/// Cursor position as a stable token across row rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorToken {
	Log(usize),
	LoadOlder,
}

/// Options for [`DebugLogViewerModel`].
pub struct DebugLogViewerModelOptions {
	/// Process start in ms since the epoch; entries older than this are flagged.
	pub process_start_ms: Option<i64>,
	/// Pid used by the process filter.
	pub process_pid: Option<f64>,
	/// Whether earlier history exists outside the captured buffer.
	pub has_older_logs: Option<Box<dyn Fn() -> bool>>,
	/// Fetch earlier history; the port has no async runtime.
	pub load_older_logs: Option<Box<dyn FnMut(Option<u64>) -> String>>,
	/// Most entries kept. Defaults to 50 000.
	pub max_log_entries: Option<usize>,
}

impl Default for DebugLogViewerModelOptions {
	fn default() -> Self {
		DebugLogViewerModelOptions {
			process_start_ms: None,
			process_pid: None,
			has_older_logs: None,
			load_older_logs: None,
			max_log_entries: None,
		}
	}
}

fn now_ms() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_millis() as i64)
		.unwrap_or(0)
}

fn parse_log_entries(log_text: &str) -> Vec<LogEntry> {
	log_text
		.split('\n')
		.filter(|line| !line.is_empty())
		.map(|raw_line| LogEntry {
			raw_line: raw_line.to_string(),
			timestamp_ms: parse_debug_log_timestamp_ms(raw_line),
			pid: parse_debug_log_pid(raw_line),
			lower_line: None,
		})
		.collect()
}

/// Split raw log text into nonempty entries.
pub fn split_log_text(log_text: &str) -> Vec<String> {
	log_text
		.split('\n')
		.filter(|line| !line.is_empty())
		.map(str::to_string)
		.collect()
}

/// Build sanitized clipboard text from selected log entries.
pub fn build_log_copy_payload(lines: &[String]) -> String {
	lines
		.iter()
		.map(|line| {
			let stripped = strip_ansi(line);
			stripped
				.chars()
				.filter(|ch| !ch.is_control() || matches!(ch, '\t' | '\n' | '\r'))
				.collect::<String>()
		})
		.filter(|line| !line.is_empty())
		.collect::<Vec<_>>()
		.join("\n")
}

/// Filter, paginate, expand, and select captured log entries.
pub struct DebugLogViewerModel {
	/// Loaded entries, newest first; log index `i` (0 = oldest) is `len - 1 - i`.
	entries: Vec<LogEntry>,
	max_log_entries: usize,
	rows: Vec<ViewerRow>,
	visible_log_indices: Vec<usize>,
	selectable_row_indices: Vec<usize>,
	cursor_selectable_index: usize,
	selection_anchor_selectable_index: Option<usize>,
	selection: Option<SelectionState>,
	expanded_log_indices: HashSet<usize>,
	filter_query: String,
	process_start_ms: i64,
	loaded_start_index: usize,
	process_filter_enabled: bool,
	process_pid: f64,
	rows_generation: u64,
	has_older_logs: Option<Box<dyn Fn() -> bool>>,
	load_older_logs: Option<Box<dyn FnMut(Option<u64>) -> String>>,
}

impl DebugLogViewerModel {
	/// Build a model over `log_text`, newest entries kept first.
	pub fn new(log_text: &str, options: DebugLogViewerModelOptions) -> Self {
		let max_log_entries = options.max_log_entries.unwrap_or(MAX_LOG_ENTRIES).max(1);
		let process_start_ms = options.process_start_ms.unwrap_or_else(now_ms);
		let process_pid = options.process_pid.unwrap_or_else(|| std::process::id() as f64);

		let mut entries = parse_log_entries(log_text);
		entries.reverse();
		if entries.len() > max_log_entries {
			entries.truncate(max_log_entries);
		}
		let loaded_start_index = entries.len().saturating_sub(INITIAL_LOG_CHUNK);

		let mut model = DebugLogViewerModel {
			entries,
			max_log_entries,
			rows: Vec::new(),
			visible_log_indices: Vec::new(),
			selectable_row_indices: Vec::new(),
			cursor_selectable_index: 0,
			selection_anchor_selectable_index: None,
			selection: None,
			expanded_log_indices: HashSet::new(),
			filter_query: String::new(),
			process_start_ms,
			loaded_start_index,
			process_filter_enabled: false,
			process_pid,
			rows_generation: 0,
			has_older_logs: options.has_older_logs,
			load_older_logs: options.load_older_logs,
		};
		model.rebuild_rows(None, None);
		model
	}

	/// Loaded entry count.
	pub fn log_count(&self) -> usize {
		self.entries.len()
	}

	/// True once the entry cap is reached, so older history no longer loads.
	pub fn history_limit_reached(&self) -> bool {
		self.entries.len() >= self.max_log_entries
	}

	/// Filtered entry count inside the loaded window.
	pub fn visible_log_count(&self) -> usize {
		self.visible_log_indices.len()
	}

	/// Body rows in render order.
	pub fn rows(&self) -> &[ViewerRow] {
		&self.rows
	}

	/// Row index of the cursor, if any.
	pub fn cursor_row_index(&self) -> Option<usize> {
		self.selectable_row_indices.get(self.cursor_selectable_index).copied()
	}

	/// Log index of the cursor, when it sits on a log row.
	pub fn cursor_log_index(&self) -> Option<usize> {
		match self.cursor_row()? {
			ViewerRow::Log(log_index) => Some(log_index),
			_ => None,
		}
	}

	/// Current filter query.
	pub fn filter_query(&self) -> &str {
		&self.filter_query
	}

	/// Kind of row under the cursor.
	pub fn cursor_row_kind(&self) -> Option<ViewerRow> {
		self.cursor_row()
	}

	/// Number of expanded entries.
	pub fn expanded_count(&self) -> usize {
		self.expanded_log_indices.len()
	}

	/// Whether the process filter is on.
	pub fn is_process_filter_enabled(&self) -> bool {
		self.process_filter_enabled
	}

	/// Whether the cursor is on the first selectable row.
	pub fn is_cursor_at_first_selectable_row(&self) -> bool {
		self.cursor_selectable_index == 0
	}

	/// Raw captured line for a log index.
	pub fn get_raw_line(&self, log_index: usize) -> String {
		self.entry_at(log_index)
			.map(|entry| entry.raw_line.clone())
			.unwrap_or_default()
	}

	/// Replace the filter query and rebuild the rows.
	pub fn set_filter_query(&mut self, query: &str) {
		if query == self.filter_query {
			return;
		}
		self.filter_query = query.to_string();
		let cursor = self.cursor_token();
		let anchor = self.anchor_log_index();
		self.rebuild_rows(cursor, anchor);
	}

	/// Toggle the process filter and rebuild the rows.
	pub fn toggle_process_filter(&mut self) {
		self.process_filter_enabled = !self.process_filter_enabled;
		let cursor = self.cursor_token();
		let anchor = self.anchor_log_index();
		self.rebuild_rows(cursor, anchor);
	}

	/// Move the cursor by `delta` selectable rows.
	pub fn move_cursor(&mut self, delta: i64, extend_selection: bool) {
		if self.selectable_row_indices.is_empty() {
			return;
		}
		if extend_selection && self.selection_anchor_selectable_index.is_none() {
			if matches!(self.cursor_row(), Some(ViewerRow::Log(_))) {
				self.selection_anchor_selectable_index = Some(self.cursor_selectable_index);
			}
		}
		let max = self.selectable_row_indices.len().saturating_sub(1) as i64;
		let next = (self.cursor_selectable_index as i64 + delta).clamp(0, max);
		self.cursor_selectable_index = next as usize;
		if !extend_selection {
			self.selection_anchor_selectable_index = None;
		}
		if !matches!(self.cursor_row(), Some(ViewerRow::Log(_))) && !extend_selection {
			self.selection_anchor_selectable_index = None;
		}
	}

	/// Move the cursor to a body row index.
	pub fn move_cursor_to_row(&mut self, row_index: usize, extend_selection: bool) -> bool {
		let Some(selectable_index) = self.selectable_row_indices.iter().position(|&index| index == row_index) else {
			return false;
		};
		if extend_selection && self.selection_anchor_selectable_index.is_none() {
			if matches!(self.cursor_row(), Some(ViewerRow::Log(_))) {
				self.selection_anchor_selectable_index = Some(self.cursor_selectable_index);
			}
		}
		self.cursor_selectable_index = selectable_index;
		if !extend_selection {
			self.selection_anchor_selectable_index = None;
		}
		if !matches!(self.cursor_row(), Some(ViewerRow::Log(_))) && !extend_selection {
			self.selection_anchor_selectable_index = None;
		}
		true
	}

	/// Selected log indices, ascending.
	pub fn get_selected_log_indices(&mut self) -> Vec<usize> {
		self.selection_state().indices.clone()
	}

	/// Selected entry count.
	pub fn get_selected_count(&mut self) -> usize {
		self.selection_state().indices.len()
	}

	/// Whether a log index is selected.
	pub fn is_selected(&mut self, log_index: usize) -> bool {
		self.selection_state().set.contains(&log_index)
	}

	/// Whether a log index is expanded.
	pub fn is_expanded(&self, log_index: usize) -> bool {
		self.expanded_log_indices.contains(&log_index)
	}

	/// Expand every selected entry.
	pub fn expand_selected(&mut self) {
		for index in self.get_selected_log_indices() {
			self.expanded_log_indices.insert(index);
		}
	}

	/// Collapse every selected entry.
	pub fn collapse_selected(&mut self) {
		for index in self.get_selected_log_indices() {
			self.expanded_log_indices.remove(&index);
		}
	}

	/// Raw lines of the selected entries.
	pub fn get_selected_raw_lines(&mut self) -> Vec<String> {
		let indices = self.selection_state().indices.clone();
		indices.iter().map(|&index| self.get_raw_line(index)).collect()
	}

	/// Select every visible log row.
	pub fn select_all_visible(&mut self) {
		if self.selectable_row_indices.is_empty() {
			return;
		}
		let mut first: Option<usize> = None;
		let mut last: Option<usize> = None;
		for (selectable_index, &row_index) in self.selectable_row_indices.iter().enumerate() {
			if matches!(self.rows[row_index], ViewerRow::Log(_)) {
				if first.is_none() {
					first = Some(selectable_index);
				}
				last = Some(selectable_index);
			}
		}
		if let (Some(first), Some(last)) = (first, last) {
			self.selection_anchor_selectable_index = Some(first);
			self.cursor_selectable_index = last;
		}
	}

	/// Whether earlier history can be loaded.
	pub fn can_load_older(&self) -> bool {
		self.loaded_start_index > 0 || self.has_external_older_logs()
	}

	/// Load earlier entries. External history is fetched through the injected
	/// callback because the port is synchronous.
	pub fn load_older(&mut self, additional_count: usize) -> bool {
		if self.loaded_start_index > 0 {
			return self.load_older_in_memory(additional_count);
		}
		if !self.has_external_older_logs() {
			return false;
		}
		let older_text = match self.load_older_logs.as_mut() {
			Some(fetch) => fetch(None),
			None => return false,
		};
		if older_text.is_empty() {
			if !self.has_external_older_logs() {
				let cursor = self.cursor_token();
				let anchor = self.anchor_log_index();
				self.rebuild_rows(cursor, anchor);
			}
			return false;
		}
		let added = self.prepend_logs(&older_text);
		if added == 0 {
			if !self.has_external_older_logs() {
				let cursor = self.cursor_token();
				let anchor = self.anchor_log_index();
				self.rebuild_rows(cursor, anchor);
			}
			return false;
		}
		self.load_older_in_memory(additional_count)
	}

	/// Prepend older log text, returning how many entries were added.
	pub fn prepend_logs(&mut self, log_text: &str) -> usize {
		let previous_cursor = self.cursor_token();
		let previous_anchor_log_index = self.anchor_log_index();
		let room = self.max_log_entries - self.entries.len();
		let parsed = if room > 0 { parse_log_entries(log_text) } else { Vec::new() };
		let new_entries = if parsed.len() > room { &parsed[parsed.len() - room..] } else { &parsed[..] };
		let offset = new_entries.len();
		if offset == 0 {
			return 0;
		}
		for entry in new_entries.iter().rev() {
			self.entries.push(entry.clone());
		}
		self.loaded_start_index += offset;
		self.expanded_log_indices = self
			.expanded_log_indices
			.iter()
			.map(|&log_index| log_index + offset)
			.collect();
		let adjusted_cursor = match previous_cursor {
			Some(CursorToken::Log(log_index)) => Some(CursorToken::Log(log_index + offset)),
			Some(CursorToken::LoadOlder) => Some(CursorToken::LoadOlder),
			None => None,
		};
		let adjusted_anchor = previous_anchor_log_index.map(|log_index| log_index + offset);
		self.rebuild_rows(adjusted_cursor, adjusted_anchor);
		offset
	}

	/// Whether the process filter matches an entry.
	fn entry_matches(&mut self, log_index: usize, query: &str) -> bool {
		let entry_index = self.entries.len().saturating_sub(1 + log_index);
		let (lower, pid) = {
			let entry = &mut self.entries[entry_index];
			let lower = entry
				.lower_line
				.get_or_insert_with(|| entry.raw_line.to_lowercase())
				.clone();
			(lower, entry.pid)
		};
		if !query.is_empty() && !lower.contains(query) {
			return false;
		}
		if !self.process_filter_enabled {
			return true;
		}
		pid == Some(self.process_pid)
	}

	fn has_older_entries(&mut self, query: &str) -> bool {
		if self.has_external_older_logs() {
			return true;
		}
		if self.loaded_start_index == 0 {
			return false;
		}
		(0..self.loaded_start_index).any(|log_index| self.entry_matches(log_index, query))
	}

	fn has_external_older_logs(&self) -> bool {
		!self.history_limit_reached() && self.has_older_logs.as_ref().map_or(false, |has| has())
	}

	fn entry_at(&self, log_index: usize) -> Option<&LogEntry> {
		self.entries.len().checked_sub(1 + log_index).and_then(|index| self.entries.get(index))
	}

	fn cursor_row(&self) -> Option<ViewerRow> {
		self.cursor_row_index().and_then(|index| self.rows.get(index).copied())
	}

	fn cursor_token(&self) -> Option<CursorToken> {
		match self.cursor_row()? {
			ViewerRow::Log(log_index) => Some(CursorToken::Log(log_index)),
			ViewerRow::LoadOlder => Some(CursorToken::LoadOlder),
			ViewerRow::Warning => None,
		}
	}

	fn anchor_log_index(&self) -> Option<usize> {
		let selectable = self.selection_anchor_selectable_index?;
		let row_index = *self.selectable_row_indices.get(selectable)?;
		match self.rows.get(row_index) {
			Some(ViewerRow::Log(log_index)) => Some(*log_index),
			_ => None,
		}
	}

	/// Reveal another chunk of the in-memory history.
	fn load_older_in_memory(&mut self, additional_count: usize) -> bool {
		if self.loaded_start_index == 0 {
			return false;
		}
		let requested = match additional_count {
			0 => LOAD_OLDER_CHUNK,
			count => count.max(1),
		};
		let next_start = self.loaded_start_index.saturating_sub(requested);
		if next_start == self.loaded_start_index {
			return false;
		}
		self.loaded_start_index = next_start;
		let cursor = self.cursor_token();
		let anchor = self.anchor_log_index();
		self.rebuild_rows(cursor, anchor);
		true
	}

	fn rebuild_rows(&mut self, previous_cursor: Option<CursorToken>, previous_anchor_log_index: Option<usize>) {
		let query = self.filter_query.to_lowercase();
		let mut visible = Vec::new();
		for log_index in self.loaded_start_index..self.entries.len() {
			if self.entry_matches(log_index, &query) {
				visible.push(log_index);
			}
		}
		self.visible_log_indices = visible;

		let mut rows: Vec<ViewerRow> = Vec::new();
		if self.has_older_entries(&query) {
			rows.push(ViewerRow::LoadOlder);
		}
		let mut older_seen = false;
		let mut warning_inserted = false;
		for &log_index in &self.visible_log_indices {
			if let Some(entry) = self.entry_at(log_index) {
				if let Some(timestamp_ms) = entry.timestamp_ms {
					if timestamp_ms < self.process_start_ms {
						older_seen = true;
					} else if older_seen && !warning_inserted {
						rows.push(ViewerRow::Warning);
						warning_inserted = true;
					}
				}
			}
			rows.push(ViewerRow::Log(log_index));
		}
		self.rows = rows;
		self.selectable_row_indices = self
			.rows
			.iter()
			.enumerate()
			.filter(|(_, row)| !matches!(row, ViewerRow::Warning))
			.map(|(index, _)| index)
			.collect();

		if self.selectable_row_indices.is_empty() {
			self.cursor_selectable_index = 0;
			self.selection_anchor_selectable_index = None;
			self.rows_generation += 1;
			return;
		}

		let fallback = self.selectable_row_indices.len() - 1;
		self.cursor_selectable_index = match previous_cursor {
			Some(CursorToken::Log(log_index)) => self
				.selectable_index_of_row(ViewerRow::Log(log_index))
				.unwrap_or(fallback),
			Some(CursorToken::LoadOlder) => self
				.selectable_index_of_row(ViewerRow::LoadOlder)
				.unwrap_or(fallback),
			None => fallback,
		};
		self.selection_anchor_selectable_index = match previous_anchor_log_index {
			Some(log_index) => self.selectable_index_of_row(ViewerRow::Log(log_index)),
			None => None,
		};
		self.rows_generation += 1;
	}

	fn selectable_index_of_row(&self, target: ViewerRow) -> Option<usize> {
		let row_index = self.rows.iter().position(|row| *row == target)?;
		self.selectable_row_indices
			.iter()
			.position(|&index| index == row_index)
	}

	/// Selected indices, recomputed only when the rows, cursor or anchor changed.
	fn selection_state(&mut self) -> &SelectionState {
		let stale = match &self.selection {
			Some(state) => {
				state.generation != self.rows_generation
					|| state.cursor != self.cursor_selectable_index
					|| state.anchor != self.selection_anchor_selectable_index
			}
			None => true,
		};
		if stale {
			let indices = self.compute_selected_log_indices();
			let set = indices.iter().copied().collect();
			self.selection = Some(SelectionState {
				generation: self.rows_generation,
				cursor: self.cursor_selectable_index,
				anchor: self.selection_anchor_selectable_index,
				indices,
				set,
			});
		}
		self.selection.as_ref().expect("selection state was just populated")
	}

	fn compute_selected_log_indices(&self) -> Vec<usize> {
		if self.selectable_row_indices.is_empty() {
			return Vec::new();
		}
		let cursor_row = match self.cursor_row() {
			Some(row) => row,
			None => return Vec::new(),
		};
		let Some(anchor) = self.selection_anchor_selectable_index else {
			return match cursor_row {
				ViewerRow::Log(log_index) => vec![log_index],
				_ => Vec::new(),
			};
		};
		let min = anchor.min(self.cursor_selectable_index);
		let max = anchor.max(self.cursor_selectable_index);
		(min..=max)
			.filter_map(|selectable_index| {
				let row_index = *self.selectable_row_indices.get(selectable_index)?;
				match self.rows.get(row_index) {
					Some(ViewerRow::Log(log_index)) => Some(*log_index),
					_ => None,
				}
			})
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn ms(text: &str) -> i64 {
		time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
			.expect("valid timestamp")
			.unix_timestamp()
			* 1000
	}

	fn line(timestamp: &str, pid: f64, message: &str) -> String {
		format!("{{\"timestamp\":\"{timestamp}\",\"pid\":{pid},\"level\":\"info\",\"message\":\"{message}\"}}")
	}

	fn model(lines: &[String], process_start_ms: i64) -> DebugLogViewerModel {
		let text = lines.join("\n");
		DebugLogViewerModel::new(
			&text,
			DebugLogViewerModelOptions {
				process_start_ms: Some(process_start_ms),
				process_pid: Some(4242.0),
				..DebugLogViewerModelOptions::default()
			},
		)
	}

	#[test]
	fn entries_are_newest_first_and_capped() {
		let lines = vec![
			line("2026-06-01T00:00:00Z", 4242.0, "oldest"),
			line("2026-06-01T00:00:01Z", 4242.0, "middle"),
			line("2026-06-01T00:00:02Z", 4242.0, "newest"),
		];
		let text = lines.join("\n");
		let viewer = DebugLogViewerModel::new(
			&text,
			DebugLogViewerModelOptions {
				process_start_ms: Some(ms("2026-01-01T00:00:00Z")),
				max_log_entries: Some(2),
				..DebugLogViewerModelOptions::default()
			},
		);
		assert_eq!(viewer.log_count(), 2);
		assert!(viewer.history_limit_reached());
		assert_eq!(viewer.get_raw_line(0), lines[1]);
		assert_eq!(viewer.get_raw_line(1), lines[2]);
		assert!(!viewer.can_load_older());
	}

	#[test]
	fn filter_and_process_filter_narrow_rows() {
		let lines = vec![
			line("2026-06-01T00:00:00Z", 4242.0, "alpha one"),
			line("2026-06-01T00:00:01Z", 9999.0, "beta two"),
			line("2026-06-01T00:00:02Z", 4242.0, "alpha three"),
		];
		let mut viewer = model(&lines, ms("2026-01-01T00:00:00Z"));
		assert_eq!(viewer.visible_log_count(), 3);
		viewer.set_filter_query("alpha");
		assert_eq!(viewer.visible_log_count(), 2);
		assert_eq!(viewer.filter_query(), "alpha");
		viewer.set_filter_query("alpha");
		assert_eq!(viewer.visible_log_count(), 2);
		viewer.toggle_process_filter();
		assert_eq!(viewer.visible_log_count(), 2);
		viewer.set_filter_query("beta");
		assert_eq!(viewer.visible_log_count(), 0);
	}

	#[test]
	fn cursor_moves_clamped_and_selection_extends() {
		let lines = vec![
			line("2026-06-01T00:00:00Z", 4242.0, "one"),
			line("2026-06-01T00:00:01Z", 4242.0, "two"),
			line("2026-06-01T00:00:02Z", 4242.0, "three"),
			line("2026-06-01T00:00:03Z", 4242.0, "four"),
		];
		let mut viewer = model(&lines, ms("2026-01-01T00:00:00Z"));
		assert_eq!(viewer.cursor_log_index(), Some(3));
		viewer.move_cursor(12, false);
		assert_eq!(viewer.cursor_log_index(), Some(3));
		viewer.move_cursor(-1, false);
		assert_eq!(viewer.cursor_log_index(), Some(2));
		assert_eq!(viewer.get_selected_count(), 1);
		viewer.move_cursor(1, true);
		assert_eq!(viewer.get_selected_log_indices(), vec![2, 3]);
		viewer.move_cursor(-1, false);
		assert_eq!(viewer.get_selected_log_indices(), vec![2]);
		assert!(viewer.move_cursor_to_row(0, false));
		assert_eq!(viewer.cursor_row_index(), Some(0));
		assert!(!viewer.move_cursor_to_row(99, false));
	}

	#[test]
	fn select_all_expand_and_collapse() {
		let lines = vec![
			line("2026-06-01T00:00:00Z", 4242.0, "one"),
			line("2026-06-01T00:00:01Z", 4242.0, "two"),
		];
		let mut viewer = model(&lines, ms("2026-01-01T00:00:00Z"));
		viewer.select_all_visible();
		assert_eq!(viewer.get_selected_log_indices(), vec![0, 1]);
		viewer.expand_selected();
		assert_eq!(viewer.expanded_count(), 2);
		assert!(viewer.is_expanded(0));
		viewer.collapse_selected();
		assert_eq!(viewer.expanded_count(), 0);
		assert_eq!(viewer.get_selected_raw_lines(), lines);
	}

	#[test]
	fn prepend_shifts_log_indices_and_keeps_cursor() {
		let lines = vec![
			line("2026-06-01T00:00:00Z", 4242.0, "oldest"),
			line("2026-06-01T00:00:01Z", 4242.0, "middle"),
			line("2026-06-01T00:00:02Z", 4242.0, "newest"),
		];
		let mut viewer = model(&lines, ms("2026-01-01T00:00:00Z"));
		assert_eq!(viewer.cursor_log_index(), Some(2));
		let older = vec![
			line("2026-05-01T00:00:00Z", 4242.0, "past one"),
			line("2026-05-01T00:00:01Z", 4242.0, "past two"),
		];
		assert_eq!(viewer.prepend_logs(&older.join("\n")), 2);
		assert_eq!(viewer.log_count(), 5);
		assert_eq!(viewer.cursor_log_index(), Some(4));
		assert_eq!(viewer.get_raw_line(4), lines[2]);
		assert_eq!(viewer.get_raw_line(0), older[0]);
	}

	#[test]
	fn load_older_pages_through_memory() {
		let mut lines = Vec::new();
		for index in 0..60 {
			lines.push(line("2026-06-01T00:00:00Z", 4242.0, &format!("row{index}")));
		}
		let mut viewer = model(&lines, ms("2026-01-01T00:00:00Z"));
		assert_eq!(viewer.visible_log_count(), 50);
		assert!(viewer.can_load_older());
		assert!(viewer.load_older(50));
		assert_eq!(viewer.visible_log_count(), 60);
		assert!(!viewer.can_load_older());
	}

	#[test]
	fn boundary_warning_separates_older_history() {
		let lines = vec![
			line("2026-01-01T00:00:00Z", 4242.0, "before"),
			line("2026-01-02T00:00:00Z", 4242.0, "also before"),
			line("2026-06-01T00:00:00Z", 4242.0, "after"),
		];
		let viewer = model(&lines, ms("2026-03-01T00:00:00Z"));
		assert_eq!(
			viewer.rows(),
			&[
				ViewerRow::Log(0),
				ViewerRow::Log(1),
				ViewerRow::Warning,
				ViewerRow::Log(2),
			]
		);
	}

	#[test]
	fn copy_payload_sanitizes_and_splits() {
		assert_eq!(split_log_text("a\n\nb\n"), vec!["a".to_string(), "b".to_string()]);
		let payload = build_log_copy_payload(&[
			"\u{1b}[31mred\u{1b}[39m".to_string(),
			"with\u{7}bell".to_string(),
		]);
		assert_eq!(payload, "red\nwithbell");
	}
}
