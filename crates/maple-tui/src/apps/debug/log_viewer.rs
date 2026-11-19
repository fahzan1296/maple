//! Interactive debug log viewer.
//! Reference: `l/src/apps/debug/log-viewer.ts`.

use std::collections::{HashMap, HashSet};

use crate::apps::debug::log_formatting::{
	format_debug_log_expanded_lines, parse_debug_log_pid, parse_debug_log_timestamp_ms,
};
use crate::component::Component;
use crate::components::input::Input;
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::overlays::extensions::display_text::sanitize_display_text;
use crate::render::render_utils::padding;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::default_theme;

/// Host capabilities for copying and fetching earlier log entries.
pub trait LogViewerDeps {
	fn copy_to_clipboard(&self, text: &str) -> Result<(), String>;
	fn has_older_logs(&self) -> bool {
		false
	}
}

/// Separator marking logs captured before the current process started.
pub const SESSION_BOUNDARY_WARNING: &str = "### WARNING - Logs above are older than current session!";
/// Action row for loading earlier log entries.
pub const LOAD_OLDER_LABEL: &str = "### MOVE UP TO LOAD MORE...";

const INITIAL_LOG_CHUNK: usize = 50;
const LOAD_OLDER_CHUNK: usize = 50;
/// Most entries kept; older history beyond it is not loaded.
pub const MAX_LOG_ENTRIES: usize = 50_000;
const MIN_LOG_VIEWER_WIDTH: u16 = 48;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LogEntry {
	raw_line: String,
	timestamp_ms: Option<i64>,
	pid: Option<i64>,
	lower_line: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewerRow {
	Warning,
	LoadOlder,
	Log { log_index: usize },
}

impl ViewerRow {
	pub fn kind(&self) -> &'static str {
		match self {
			ViewerRow::Warning => "warning",
			ViewerRow::LoadOlder => "load-older",
			ViewerRow::Log { .. } => "log",
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedLogRow {
	width: usize,
	selected: bool,
	active: bool,
	expanded: bool,
	lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorToken {
	Log { log_index: usize },
	LoadOlder,
}

#[derive(Clone, Debug)]
pub struct DebugLogViewerModelOptions {
	pub process_start_ms: Option<i64>,
	pub process_pid: Option<i64>,
	pub has_older_logs: bool,
	pub max_log_entries: usize,
}

impl Default for DebugLogViewerModelOptions {
	fn default() -> Self {
		DebugLogViewerModelOptions {
			process_start_ms: None,
			process_pid: None,
			has_older_logs: false,
			max_log_entries: MAX_LOG_ENTRIES,
		}
	}
}

/// Filter, paginate, expand, and select captured log entries.
pub struct DebugLogViewerModel {
	entries: Vec<LogEntry>,
	max_log_entries: usize,
	rows: Vec<ViewerRow>,
	visible_log_indices: Vec<usize>,
	selectable_row_indices: Vec<usize>,
	cursor_selectable_index: usize,
	selection_anchor_selectable_index: Option<usize>,
	expanded_log_indices: HashSet<usize>,
	filter_query: String,
	process_start_ms: i64,
	loaded_start_index: usize,
	process_filter_enabled: bool,
	process_pid: i64,
	has_older_logs_flag: bool,
}

impl DebugLogViewerModel {
	pub fn new(log_text: &str, options: DebugLogViewerModelOptions) -> Self {
		let mut entries = parse_log_entries(log_text);
		entries.reverse();
		let max_log_entries = options.max_log_entries.max(1);
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
			expanded_log_indices: HashSet::new(),
			filter_query: String::new(),
			process_start_ms: options.process_start_ms.unwrap_or(0),
			loaded_start_index,
			process_filter_enabled: false,
			process_pid: options.process_pid.unwrap_or(0),
			has_older_logs_flag: options.has_older_logs,
		};
		model.rebuild_rows(None, None);
		model
	}

	pub fn log_count(&self) -> usize { self.entries.len() }
	pub fn history_limit_reached(&self) -> bool { self.entries.len() >= self.max_log_entries }
	pub fn visible_log_count(&self) -> usize { self.visible_log_indices.len() }
	pub fn rows(&self) -> &[ViewerRow] { &self.rows }
	pub fn cursor_row_index(&self) -> Option<usize> { self.selectable_row_indices.get(self.cursor_selectable_index).copied() }
	pub fn cursor_log_index(&self) -> Option<usize> { match self.cursor_row() { Some(ViewerRow::Log { log_index }) => Some(*log_index), _ => None } }
	pub fn filter_query(&self) -> &str { &self.filter_query }
	pub fn cursor_row_kind(&self) -> Option<&'static str> { self.cursor_row().map(|r| r.kind()) }
	pub fn expanded_count(&self) -> usize { self.expanded_log_indices.len() }
	pub fn is_process_filter_enabled(&self) -> bool { self.process_filter_enabled }
	pub fn is_cursor_at_first_selectable_row(&self) -> bool { self.cursor_selectable_index == 0 }
	pub fn get_raw_line(&self, log_index: usize) -> &str { self.entry_at(log_index).map(|e| e.raw_line.as_str()).unwrap_or("") }

	pub fn set_filter_query(&mut self, query: String) {
		if query == self.filter_query { return; }
		self.filter_query = query;
		self.rebuild_rows(self.cursor_token(), self.anchor_log_index());
	}

	pub fn toggle_process_filter(&mut self) {
		self.process_filter_enabled = !self.process_filter_enabled;
		self.rebuild_rows(self.cursor_token(), self.anchor_log_index());
	}

	pub fn set_has_older_logs(&mut self, has: bool) { self.has_older_logs_flag = has; }

	pub fn move_cursor(&mut self, delta: isize, extend_selection: bool) {
		if self.selectable_row_indices.is_empty() { return; }
		if extend_selection && self.selection_anchor_selectable_index.is_none() && matches!(self.cursor_row(), Some(ViewerRow::Log { .. })) {
			self.selection_anchor_selectable_index = Some(self.cursor_selectable_index);
		}
		let len = self.selectable_row_indices.len() as isize;
		let next = (self.cursor_selectable_index as isize + delta).clamp(0, len - 1) as usize;
		self.cursor_selectable_index = next;
		if !extend_selection || !matches!(self.cursor_row(), Some(ViewerRow::Log { .. })) {
			self.selection_anchor_selectable_index = None;
		}
	}

	pub fn move_cursor_to_row(&mut self, row_index: usize, extend_selection: bool) -> bool {
		let Some(selectable_index) = self.selectable_row_indices.iter().position(|&i| i == row_index) else { return false; };
		if extend_selection && self.selection_anchor_selectable_index.is_none() && matches!(self.cursor_row(), Some(ViewerRow::Log { .. })) {
			self.selection_anchor_selectable_index = Some(self.cursor_selectable_index);
		}
		self.cursor_selectable_index = selectable_index;
		if !extend_selection || !matches!(self.cursor_row(), Some(ViewerRow::Log { .. })) {
			self.selection_anchor_selectable_index = None;
		}
		true
	}

	pub fn get_selected_log_indices(&self) -> Vec<usize> { self.compute_selected_log_indices() }
	pub fn get_selected_count(&self) -> usize { self.get_selected_log_indices().len() }
	pub fn is_selected(&self, log_index: usize) -> bool { self.get_selected_log_indices().contains(&log_index) }
	pub fn is_expanded(&self, log_index: usize) -> bool { self.expanded_log_indices.contains(&log_index) }
	pub fn expand_selected(&mut self) { for index in self.get_selected_log_indices() { self.expanded_log_indices.insert(index); } }
	pub fn collapse_selected(&mut self) { for index in self.get_selected_log_indices() { self.expanded_log_indices.remove(&index); } }
	pub fn get_selected_raw_lines(&self) -> Vec<String> { self.get_selected_log_indices().into_iter().map(|i| self.get_raw_line(i).to_string()).collect() }

	pub fn select_all_visible(&mut self) {
		if self.selectable_row_indices.is_empty() { return; }
		let mut first: Option<usize> = None;
		let mut last: Option<usize> = None;
		for (i, &row_index) in self.selectable_row_indices.iter().enumerate() {
			if matches!(self.rows.get(row_index), Some(ViewerRow::Log { .. })) {
				if first.is_none() { first = Some(i); }
				last = Some(i);
			}
		}
		if let (Some(f), Some(l)) = (first, last) {
			self.selection_anchor_selectable_index = Some(f);
			self.cursor_selectable_index = l;
		}
	}

	pub fn can_load_older(&self) -> bool { self.loaded_start_index > 0 || self.has_external_older_logs() }

	pub fn load_older_in_memory(&mut self, additional_count: usize) -> bool {
		if self.loaded_start_index == 0 { return false; }
		let requested = additional_count.max(1);
		let next_start = self.loaded_start_index.saturating_sub(requested);
		if next_start == self.loaded_start_index { return false; }
		self.loaded_start_index = next_start;
		self.rebuild_rows(self.cursor_token(), self.anchor_log_index());
		true
	}

	pub fn prepend_logs(&mut self, log_text: &str) -> usize {
		let previous_cursor = self.cursor_token();
		let previous_anchor = self.anchor_log_index();
		let room = self.max_log_entries.saturating_sub(self.entries.len());
		let parsed = if room > 0 { parse_log_entries(log_text) } else { Vec::new() };
		let new_entries = if parsed.len() > room { parsed[parsed.len() - room..].to_vec() } else { parsed };
		if new_entries.is_empty() { return 0; }
		let offset = new_entries.len();
		for e in new_entries.into_iter().rev() { self.entries.push(e); }
		self.loaded_start_index += offset;
		self.expanded_log_indices = self.expanded_log_indices.iter().map(|&i| i + offset).collect();
		let adjusted_cursor = match previous_cursor {
			Some(CursorToken::Log { log_index }) => Some(CursorToken::Log { log_index: log_index + offset }),
			other => other,
		};
		let adjusted_anchor = previous_anchor.map(|a| a + offset);
		self.rebuild_rows(adjusted_cursor, adjusted_anchor);
		offset
	}

	fn rebuild_rows(&mut self, previous_cursor: Option<CursorToken>, previous_anchor_log_index: Option<usize>) {
		let query = self.filter_query.to_lowercase();
		let mut visible = Vec::new();
		for i in self.loaded_start_index..self.entries.len() {
			if self.matches_filters(i, &query) { visible.push(i); }
		}
		self.visible_log_indices = visible;
		let mut rows = Vec::new();
		if self.has_older_entries(&query) { rows.push(ViewerRow::LoadOlder); }
		let mut older_seen = false;
		let mut warning_inserted = false;
		for &log_index in &self.visible_log_indices {
			let ts = self.entry_at(log_index).and_then(|e| e.timestamp_ms);
			if let Some(timestamp_ms) = ts {
				if timestamp_ms < self.process_start_ms {
					older_seen = true;
				} else if older_seen && !warning_inserted {
					rows.push(ViewerRow::Warning);
					warning_inserted = true;
				}
			}
			rows.push(ViewerRow::Log { log_index });
		}
		self.rows = rows;
		self.selectable_row_indices = self.rows.iter().enumerate().filter_map(|(i, r)| (!matches!(r, ViewerRow::Warning)).then_some(i)).collect();
		if self.selectable_row_indices.is_empty() {
			self.cursor_selectable_index = 0;
			self.selection_anchor_selectable_index = None;
			return;
		}
		match previous_cursor {
			Some(CursorToken::Log { log_index }) => {
				let row_index = self.rows.iter().position(|r| matches!(r, ViewerRow::Log { log_index: li } if *li == log_index));
				self.cursor_selectable_index = row_index.and_then(|ri| self.selectable_row_indices.iter().position(|&x| x == ri)).unwrap_or(self.selectable_row_indices.len() - 1);
			}
			Some(CursorToken::LoadOlder) => {
				let row_index = self.rows.iter().position(|r| matches!(r, ViewerRow::LoadOlder));
				self.cursor_selectable_index = row_index.and_then(|ri| self.selectable_row_indices.iter().position(|&x| x == ri)).unwrap_or(self.selectable_row_indices.len() - 1);
			}
			None => { self.cursor_selectable_index = self.selectable_row_indices.len() - 1; }
		}
		if let Some(prev_anchor) = previous_anchor_log_index {
			let row_index = self.rows.iter().position(|r| matches!(r, ViewerRow::Log { log_index } if *log_index == prev_anchor));
			self.selection_anchor_selectable_index = row_index.and_then(|ri| self.selectable_row_indices.iter().position(|&x| x == ri));
		} else {
			self.selection_anchor_selectable_index = None;
		}
	}

	fn matches_filters(&self, log_index: usize, query: &str) -> bool {
		let Some(entry) = self.entry_at(log_index) else { return false; };
		if !query.is_empty() {
			let hay = entry.lower_line.as_deref().map(|s| s.to_string()).unwrap_or_else(|| entry.raw_line.to_lowercase());
			if !hay.contains(query) { return false; }
		}
		if !self.process_filter_enabled { return true; }
		entry.pid == Some(self.process_pid)
	}

	fn has_older_entries(&self, query: &str) -> bool {
		if self.has_external_older_logs() { return true; }
		if self.loaded_start_index == 0 { return false; }
		for i in 0..self.loaded_start_index { if self.matches_filters(i, query) { return true; } }
		false
	}

	fn has_external_older_logs(&self) -> bool { !self.history_limit_reached() && self.has_older_logs_flag }
	fn entry_at(&self, log_index: usize) -> Option<&LogEntry> { if log_index >= self.entries.len() { return None; } self.entries.get(self.entries.len() - 1 - log_index) }

	fn compute_selected_log_indices(&self) -> Vec<usize> {
		if self.selectable_row_indices.is_empty() { return Vec::new(); }
		let cursor_row = self.cursor_row();
		let Some(anchor) = self.selection_anchor_selectable_index else {
			return match cursor_row { Some(ViewerRow::Log { log_index }) => vec![*log_index], _ => Vec::new() };
		};
		let min = anchor.min(self.cursor_selectable_index);
		let max = anchor.max(self.cursor_selectable_index);
		let mut selected = Vec::new();
		for i in min..=max {
			if let Some(&row_index) = self.selectable_row_indices.get(i) {
				if let Some(ViewerRow::Log { log_index }) = self.rows.get(row_index) { selected.push(*log_index); }
			}
		}
		selected
	}

	fn cursor_row(&self) -> Option<&ViewerRow> { let row_index = self.cursor_row_index()?; self.rows.get(row_index) }
	fn cursor_token(&self) -> Option<CursorToken> {
		match self.cursor_row()? {
			ViewerRow::Log { log_index } => Some(CursorToken::Log { log_index: *log_index }),
			ViewerRow::LoadOlder => Some(CursorToken::LoadOlder),
			ViewerRow::Warning => None,
		}
	}
	fn anchor_log_index(&self) -> Option<usize> {
		let anchor = self.selection_anchor_selectable_index?;
		let row_index = *self.selectable_row_indices.get(anchor)?;
		match self.rows.get(row_index)? { ViewerRow::Log { log_index } => Some(*log_index), _ => None }
	}
}

fn parse_log_entries(log_text: &str) -> Vec<LogEntry> {
	log_text.lines().filter(|l| !l.is_empty()).map(|raw_line| LogEntry {
		timestamp_ms: parse_debug_log_timestamp_ms(raw_line),
		pid: parse_debug_log_pid(raw_line).map(|p| p as i64),
		raw_line: raw_line.to_string(),
		lower_line: None,
	}).collect()
}

/// Split raw log text into nonempty entries.
pub fn split_log_text(log_text: &str) -> Vec<String> {
	log_text.lines().filter(|l| !l.is_empty()).map(|s| s.to_string()).collect()
}

/// Build sanitized clipboard text from selected log entries.
pub fn build_log_copy_payload(lines: &[String]) -> String {
	lines.iter().map(|line| sanitize_display_text(line)).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
}

pub struct DebugLogViewerComponentOptions<D: LogViewerDeps> {
	pub logs: String,
	pub terminal_rows: usize,
	pub deps: D,
	pub on_exit: Box<dyn FnMut()>,
	pub process_start_ms: Option<i64>,
	pub process_pid: Option<i64>,
}

/// Interactive log viewer with host-provided storage and clipboard capabilities.
pub struct DebugLogViewerComponent<D: LogViewerDeps> {
	model: DebugLogViewerModel,
	filter: Input,
	terminal_rows: usize,
	deps: D,
	on_exit: Box<dyn FnMut()>,
	status_message: Option<String>,
	body_line_to_row_index: Vec<Option<usize>>,
	row_cache: HashMap<usize, CachedLogRow>,
	row_cache_generation: usize,
}

impl<D: LogViewerDeps> DebugLogViewerComponent<D> {
	pub fn new(options: DebugLogViewerComponentOptions<D>) -> Self {
		let model = DebugLogViewerModel::new(&options.logs, DebugLogViewerModelOptions {
			process_start_ms: options.process_start_ms,
			process_pid: options.process_pid,
			has_older_logs: options.deps.has_older_logs(),
			max_log_entries: MAX_LOG_ENTRIES,
		});
		DebugLogViewerComponent {
			model,
			filter: Input::new(),
			terminal_rows: options.terminal_rows.max(8),
			deps: options.deps,
			on_exit: options.on_exit,
			status_message: None,
			body_line_to_row_index: Vec::new(),
			row_cache: HashMap::new(),
			row_cache_generation: 0,
		}
	}

	fn body_height(&self) -> usize { self.terminal_rows.saturating_sub(2 + 2 + 4).max(3) }

	fn copy_selected(&mut self) {
		let selected = self.model.get_selected_raw_lines();
		if selected.is_empty() { self.status_message = Some("No log entry selected".to_string()); return; }
		let payload = build_log_copy_payload(&selected);
		match self.deps.copy_to_clipboard(&payload) {
			Ok(()) => { self.status_message = Some(format!("Copied {} log {}", selected.len(), if selected.len() == 1 { "entry" } else { "entries" })); }
			Err(e) => { self.status_message = Some(format!("Copy failed: {e}")); }
		}
	}

	fn handle_load_older(&mut self, additional: usize) {
		if !self.model.can_load_older() { return; }
		if self.model.load_older_in_memory(additional) { return; }
		self.status_message = Some("Older logs unavailable".to_string());
		self.model.set_has_older_logs(false);
	}

	fn render_rows(&mut self, inner_width: usize) -> Vec<(Vec<String>, usize)> {
		let theme = default_theme();
		let generation = self.model.log_count();
		if self.row_cache_generation != generation { self.row_cache.clear(); self.row_cache_generation = generation; }
		let cursor_row_index = self.model.cursor_row_index();
		let cursor_log_index = self.model.cursor_log_index();
		let rows_snapshot = self.model.rows().to_vec();
		let mut rendered = Vec::new();
		for (row_index, row) in rows_snapshot.into_iter().enumerate() {
			match row {
				ViewerRow::Warning => {
					let line = theme.fg("muted", &truncate_to_width(SESSION_BOUNDARY_WARNING, inner_width, Ellipsis::Omit)).unwrap_or_else(|_| truncate_to_width(SESSION_BOUNDARY_WARNING, inner_width, Ellipsis::Omit));
					rendered.push((vec![line], row_index));
				}
				ViewerRow::LoadOlder => {
					let active = cursor_row_index == Some(row_index);
					let marker = if active { theme.fg("accent", "❯").unwrap_or_else(|_| "❯".into()) } else { " ".to_string() };
					let prefix = format!("{marker}  ");
					let content_width = inner_width.saturating_sub(visible_width(&prefix)).max(1);
					let label = truncate_to_width(LOAD_OLDER_LABEL, content_width, Ellipsis::Omit);
					let muted = theme.fg("muted", &label).unwrap_or(label);
					rendered.push((vec![truncate_to_width(&format!("{prefix}{muted}"), inner_width, Ellipsis::Omit)], row_index));
				}
				ViewerRow::Log { log_index } => {
					let selected = self.model.is_selected(log_index);
					let active = cursor_log_index == Some(log_index);
					let expanded = self.model.is_expanded(log_index);
					if let Some(cached) = self.row_cache.get(&log_index) {
						if cached.width == inner_width && cached.selected == selected && cached.active == active && cached.expanded == expanded {
							rendered.push((cached.lines.clone(), row_index)); continue;
						}
					}
					let marker = if active { theme.fg("accent", "❯").unwrap_or_else(|_| "❯".into()) } else if selected { theme.fg("accent", "•").unwrap_or_else(|_| "•".into()) } else { " ".to_string() };
					let fold = if expanded { theme.fg("accent", "▾").unwrap_or_else(|_| "▾".into()) } else { theme.fg("muted", "▸").unwrap_or_else(|_| "▸".into()) };
					let prefix = format!("{marker}{fold} ");
					let content_width = inner_width.saturating_sub(visible_width(&prefix)).max(1);
					let lines = if expanded {
						let wrapped = format_debug_log_expanded_lines(self.model.get_raw_line(log_index), content_width);
						let indent = padding(visible_width(&prefix));
						wrapped.into_iter().enumerate().map(|(i, seg)| {
							let content = if selected { theme.bold(&seg) } else { seg };
							let head = if i == 0 { &prefix } else { &indent };
							truncate_to_width(&format!("{head}{content}"), inner_width, Ellipsis::Omit)
						}).collect()
					} else {
						let preview = truncate_to_width(&sanitize_display_text(self.model.get_raw_line(log_index)), content_width, Ellipsis::Omit);
						let content = if selected { theme.bold(&preview) } else { preview };
						vec![truncate_to_width(&format!("{prefix}{content}"), inner_width, Ellipsis::Omit)]
					};
					self.row_cache.insert(log_index, CachedLogRow { width: inner_width, selected, active, expanded, lines: lines.clone() });
					rendered.push((lines, row_index));
				}
			}
		}
		rendered
	}
}

impl<D: LogViewerDeps> Component for DebugLogViewerComponent<D> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let theme = default_theme();
		let frame_width = width.max(MIN_LOG_VIEWER_WIDTH);
		let inner_width = (frame_width as usize).saturating_sub(5).max(1);
		let body_height = self.body_height();
		let rendered = self.render_rows(inner_width);
		let mut lines = Vec::new();
		let mut mapping: Vec<Option<usize>> = Vec::new();
		if rendered.is_empty() {
			lines.push(theme.fg("muted", "no matches").unwrap_or_else(|_| "no matches".into()));
			mapping.push(None);
		} else {
			for (row_lines, row_index) in rendered { for line in row_lines { lines.push(line); mapping.push(Some(row_index)); } }
		}
		let cursor_row_index = self.model.cursor_row_index();
		let mut cursor_line = 0usize;
		if let Some(cri) = cursor_row_index { for (i, m) in mapping.iter().enumerate() { if *m == Some(cri) { cursor_line = i; break; } } }
		let mut start = 0usize;
		if lines.len() > body_height { if cursor_line >= body_height { start = cursor_line + 1 - body_height; } start = start.min(lines.len().saturating_sub(body_height)); }
		self.body_line_to_row_index = mapping.iter().skip(start).take(body_height).cloned().collect();
		let mut body: Vec<String> = lines.into_iter().skip(start).take(body_height).collect();
		if body.len() < body_height { body.resize(body_height, String::new()); }

		let selected = self.model.get_selected_count();
		let expanded = self.model.expanded_count();
		let summary = format!(
			"{} {}  {} {}  {} {}{}",
			theme.fg("muted", "showing").unwrap_or_default(),
			theme.fg("accent", &format!("{}/{}", self.model.visible_log_count(), self.model.log_count())).unwrap_or_default(),
			theme.fg("muted", "selected").unwrap_or_default(),
			theme.fg(if selected > 0 { "accent" } else { "muted" }, &selected.to_string()).unwrap_or_default(),
			theme.fg("muted", "expanded").unwrap_or_default(),
			theme.fg(if expanded > 0 { "accent" } else { "muted" }, &expanded.to_string()).unwrap_or_default(),
			if self.model.history_limit_reached() { format!("  {}", theme.fg("warning", "history limit reached").unwrap_or_default()) } else { String::new() }
		);
		let value = self.filter.value().to_string();
		let pid_status = if self.model.is_process_filter_enabled() { theme.fg("success", "pid on").unwrap_or_default() } else { theme.fg("muted", "pid off").unwrap_or_default() };
		let filter_text = format!("{} {}  {pid_status}", theme.fg("muted", "filter").unwrap_or_default(), if value.is_empty() { theme.fg("muted", "type to filter").unwrap_or_default() } else { theme.fg("accent", &value).unwrap_or_default() });
		let status = if let Some(msg) = &self.status_message { theme.fg("success", msg).unwrap_or_else(|_| msg.clone()) } else { theme.fg("dim", "enter loads older when highlighted; printable keys update filter").unwrap_or_default() };
		let controls = theme.fg("dim", "esc close · ctrl+c copy · up/down move · pgup/pgdn page · home/end · left/right fold · ctrl+a all · ctrl+o older · ctrl+p pid").unwrap_or_default();
		let box_theme = crate::chrome::overlay_box::OverlayTheme::default();
		let mut rows = Vec::new();
		rows.push(crate::chrome::overlay_box::top_border(frame_width, "Recent Logs", &box_theme));
		rows.push(crate::chrome::overlay_box::row(&summary, frame_width, &box_theme));
		rows.push(crate::chrome::overlay_box::row(&filter_text, frame_width, &box_theme));
		rows.push(crate::chrome::overlay_box::divider(frame_width, &box_theme));
		for b in &body { rows.push(crate::chrome::overlay_box::row(b, frame_width, &box_theme)); }
		rows.push(crate::chrome::overlay_box::divider(frame_width, &box_theme));
		rows.push(crate::chrome::overlay_box::row(&status, frame_width, &box_theme));
		rows.push(crate::chrome::overlay_box::row(&controls, frame_width, &box_theme));
		rows.push(crate::chrome::overlay_box::bottom_border(frame_width, &box_theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		match key {
			Key::Esc => { (self.on_exit)(); return; }
			Key::Ctrl('c') => { self.copy_selected(); return; }
			Key::Ctrl('p') => { self.status_message = None; self.model.toggle_process_filter(); return; }
			Key::Ctrl('a') => { self.status_message = None; self.model.select_all_visible(); return; }
			Key::Ctrl('o') => { self.status_message = None; self.handle_load_older(self.body_height() + 1); return; }
			Key::Enter => { if self.model.cursor_row_kind() == Some("load-older") { self.status_message = None; self.handle_load_older(LOAD_OLDER_CHUNK); } return; }
			Key::Up => { self.status_message = None; self.model.move_cursor(-1, false); return; }
			Key::Down => { self.status_message = None; self.model.move_cursor(1, false); return; }
			Key::PageUp => { self.status_message = None; self.model.move_cursor(-(self.body_height() as isize), false); return; }
			Key::PageDown => { self.status_message = None; self.model.move_cursor(self.body_height() as isize, false); return; }
			Key::Home => { self.status_message = None; self.model.move_cursor(-(self.model.rows().len() as isize), false); return; }
			Key::End => { self.status_message = None; self.model.move_cursor(self.model.rows().len() as isize, false); return; }
			Key::Right => { self.status_message = None; if self.model.cursor_row_kind() == Some("load-older") { self.handle_load_older(LOAD_OLDER_CHUNK); return; } self.model.expand_selected(); return; }
			Key::Left => { self.status_message = None; self.model.collapse_selected(); return; }
			_ => {}
		}
		let before = self.filter.value().to_string();
		self.filter.handle_input(key);
		let after = self.filter.value().to_string();
		if after != before { self.status_message = None; self.model.set_filter_query(after); }
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		match event.kind {
			MouseKind::ScrollUp => { self.status_message = None; self.model.move_cursor(-3, false); }
			MouseKind::ScrollDown => { self.status_message = None; self.model.move_cursor(3, false); }
			MouseKind::Down(MouseButtonKind::Left) => {
				let body_start = 5usize;
				if (event.row as usize) < body_start { return; }
				let local = event.row as usize - body_start;
				if local >= self.body_line_to_row_index.len() { return; }
				let Some(row_index) = self.body_line_to_row_index.get(local).copied().flatten() else { return; };
				match self.model.rows().get(row_index).cloned() {
					Some(ViewerRow::LoadOlder) => { self.status_message = None; self.model.move_cursor_to_row(row_index, false); self.handle_load_older(LOAD_OLDER_CHUNK); }
					Some(ViewerRow::Log { log_index }) => {
						self.status_message = None; self.model.move_cursor_to_row(row_index, false);
						if self.model.is_expanded(log_index) { self.model.collapse_selected(); } else { self.model.expand_selected(); }
					}
					_ => {}
				}
			}
			_ => {}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	struct NoopDeps;
	impl LogViewerDeps for NoopDeps { fn copy_to_clipboard(&self, _text: &str) -> Result<(), String> { Ok(()) } }

	#[test]
	fn split_and_copy_payload() {
		assert_eq!(split_log_text("a\n\nb\n"), vec!["a".to_string(), "b".to_string()]);
		assert_eq!(build_log_copy_payload(&["x".into(), "".into(), "y".into()]), "x\ny");
	}

	#[test]
	fn model_basic() {
		let logs = "{\"message\":\"hello\"}\n{\"message\":\"world\"}\n";
		let mut m = DebugLogViewerModel::new(logs, DebugLogViewerModelOptions { process_start_ms: Some(0), process_pid: Some(1), ..Default::default() });
		assert_eq!(m.log_count(), 2);
		m.set_filter_query("hello".into());
		assert!(m.visible_log_count() <= 2);
		m.select_all_visible();
		assert!(m.get_selected_count() <= m.visible_log_count().max(1));
	}

	#[test]
	fn constants_present() {
		assert!(SESSION_BOUNDARY_WARNING.contains("WARNING"));
		assert!(LOAD_OLDER_LABEL.contains("LOAD"));
		assert_eq!(MAX_LOG_ENTRIES, 50_000);
	}
}
