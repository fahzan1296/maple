//! Fullscreen transcript viewer.
//!
//! Reference: `l/src/overlays/agent-transcript-viewer.ts`.
//!
//! Deviations:
//! - `Component`/`ChatTranscriptBuilder`/`TranscriptBrowser`/`Editor`/`TUI`
//!   rendering, polling timers, async file reads, and terminal I/O are not
//!   ported. This module provides exported data/state types, constants, and
//!   pure helper functions (sentinel math/buffer slicing, status tone/badge
//!   labels) plus host traits for filesystem/parsing/remote/lifecycle access.
//! - `Node:fs` sync/async APIs are abstracted behind [`TranscriptFs`] and
//!   byte-buffer operations use `Vec<u8>`.
//! - `theme`/ANSI styling is represented as plain labels/tones; callers render
//!   with crate theme utilities.
//! - `AgentTranscriptViewer` interactive class behavior is host-driven via
//!   [`TranscriptViewerState`] snapshot fields and traits rather than a UI
//!   component implementation.

use std::collections::VecDeque;

use crate::overlays::agent_hub_types::AgentStatus;

/// Parsed transcript entry kinds relevant to the viewer (opaque payloads).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentTranscriptEntry {
	/// Session message entry (opaque in this port).
	Message(String),
	/// Model change marker.
	ModelChange { model: String },
}

/// Filesystem capabilities supplied by the host for local transcript reads.
pub trait TranscriptFs {
	fn read_file_sync(&self, path: &str) -> Result<Vec<u8>, String>;
	fn stat(&self, path: &str) -> Result<TranscriptStat, String>;
	fn read_range_sync(&self, path: &str, offset: usize, length: usize) -> Result<Vec<u8>, String> {
		let bytes = self.read_file_sync(path)?;
		if length == 0 || offset >= bytes.len() {
			return Ok(Vec::new());
		}
		let end = offset.saturating_add(length).min(bytes.len());
		Ok(bytes[offset..end].to_vec())
	}
}

/// File stat fields used for sentinel/incremental checks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranscriptStat {
	pub dev: u64,
	pub ino: u64,
	pub size: u64,
	pub mtime_ms: i64,
}

/// Local transcript parsing/source host.
pub trait AgentTranscriptSource {
	fn parse_entries(&self, text: &str) -> Vec<AgentTranscriptEntry>;
}

/// Remote transcript read result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentHubRemoteTranscript {
	pub text: String,
	pub new_size: u64,
	/// Terminal read failure reported by host.
	pub error: Option<String>,
}

/// Guest-side proxy for hub actions/transcripts on collab host.
pub trait AgentHubRemote {
	fn read_transcript(&self, id: &str, from_byte: u64) -> Result<Option<AgentHubRemoteTranscript>, String>;
}

/// How often to re-stat a file-backed transcript for growth.
pub const POLL_MS: u64 = 250;
/// Every Nth idle poll re-verifies sentinels.
pub const IDLE_SENTINEL_CHECK_EVERY: u32 = 5;
/// Sentinel window bytes.
pub const SENTINEL_BYTES: usize = 4096;

/// Sentinel sample.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalTranscriptSentinel {
	pub offset: usize,
	pub bytes: Vec<u8>,
}

/// Local transcript incremental read state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalTranscriptState {
	pub path: String,
	pub dev: u64,
	pub ino: u64,
	pub size: u64,
	pub mtime_ms: i64,
	pub offset: usize,
	pub pending: String,
	pub sentinels: Vec<LocalTranscriptSentinel>,
}

/// Width used for described/native text (effectively unbounded in TS; capped).
pub const NATIVE_LINE_WIDTH: usize = 0x7fff_ffff;

/// Status tone token for transcript badge.
#[must_use]
pub fn status_tone(status: AgentStatus) -> &'static str {
	match status {
		AgentStatus::Running => "success",
		AgentStatus::Idle => "accent",
		AgentStatus::Parked => "muted",
		AgentStatus::Aborted => "error",
	}
}

/// Status badge label.
#[must_use]
pub fn status_badge_label(status: AgentStatus) -> &'static str {
	match status {
		AgentStatus::Running => "running",
		AgentStatus::Idle => "idle",
		AgentStatus::Parked => "parked",
		AgentStatus::Aborted => "aborted",
	}
}

/// Compute sentinel offsets for a file size.
#[must_use]
pub fn sentinel_offsets(size: usize) -> Vec<usize> {
	if size == 0 {
		return Vec::new();
	}
	let length = SENTINEL_BYTES.min(size);
	let mid = (size.saturating_sub(length)) / 2;
	let end = size.saturating_sub(length);
	let mut offsets = vec![0usize, mid, end];
	offsets.sort_unstable();
	offsets.dedup();
	offsets
}

/// Build sentinels from an in-memory buffer.
#[must_use]
pub fn sentinels_from_buffer(buffer: &[u8]) -> Vec<LocalTranscriptSentinel> {
	let size = buffer.len();
	let length = SENTINEL_BYTES.min(size);
	sentinel_offsets(size)
		.into_iter()
		.map(|offset| {
			let end = offset.saturating_add(length).min(size);
			LocalTranscriptSentinel {
				offset,
				bytes: buffer[offset..end].to_vec(),
			}
		})
		.collect()
}

/// Read sentinels from filesystem host.
pub fn sentinels_from_file(
	fs: &dyn TranscriptFs,
	file: &str,
	size: usize,
) -> Result<Vec<LocalTranscriptSentinel>, String> {
	let length = SENTINEL_BYTES.min(size);
	let mut out = Vec::new();
	for offset in sentinel_offsets(size) {
		let bytes = fs.read_range_sync(file, offset, length)?;
		out.push(LocalTranscriptSentinel { offset, bytes });
	}
	Ok(out)
}

/// Read a file byte range.
pub fn read_file_range(
	fs: &dyn TranscriptFs,
	file: &str,
	offset: usize,
	length: usize,
) -> Result<Vec<u8>, String> {
	fs.read_range_sync(file, offset, length)
}

/// Mutable viewer state snapshot (non-UI fields mirrored from TS class).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranscriptViewerState {
	pub notice: Option<String>,
	pub expanded: bool,
	pub local_state: Option<LocalTranscriptState>,
	pub local_unavailable: String,
	pub remote_bytes: u64,
	pub remote_fetch_in_flight: bool,
	pub remote_token: u64,
	pub remote_unavailable: bool,
	pub remote_error: String,
	pub has_remote_data: bool,
	pub model: Option<String>,
	pub disposed: bool,
	pub local_load_in_flight: bool,
	pub idle_polls: u32,
	pub initial_entry_id: Option<String>,
}

impl TranscriptViewerState {
	#[must_use]
	pub fn new(initial_entry_id: Option<String>) -> Self {
		Self {
			initial_entry_id,
			..Default::default()
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct MemFs {
		data: Vec<u8>,
	}
	impl TranscriptFs for MemFs {
		fn read_file_sync(&self, _path: &str) -> Result<Vec<u8>, String> {
			Ok(self.data.clone())
		}
		fn stat(&self, _path: &str) -> Result<TranscriptStat, String> {
			Ok(TranscriptStat { size: self.data.len() as u64, ..Default::default() })
		}
	}

	#[test]
	fn sentinel_offsets_and_buffer_sentinels() {
		assert!(sentinel_offsets(0).is_empty());
		assert_eq!(sentinel_offsets(10), vec![0, 1, 6].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
		let buf = b"abcdefghijklmnopqrstuvwxyz".to_vec();
		let s = sentinels_from_buffer(&buf);
		assert!(!s.is_empty());
		assert_eq!(s.first().unwrap().offset, 0);
		assert!(s.iter().all(|x| !x.bytes.is_empty()));
	}

	#[test]
	fn read_range_and_file_sentinels() {
		let fs = MemFs { data: b"0123456789abcdef".to_vec() };
		let r = read_file_range(&fs, "x", 4, 4).unwrap();
		assert_eq!(r, b"4567");
		let s = sentinels_from_file(&fs, "x", 16).unwrap();
		assert_eq!(s.len(), sentinel_offsets(16).len());
	}

	#[test]
	fn status_labels_and_state_new() {
		assert_eq!(status_badge_label(AgentStatus::Running), "running");
		assert_eq!(status_tone(AgentStatus::Aborted), "error");
		let st = TranscriptViewerState::new(Some("e1".into()));
		assert_eq!(st.initial_entry_id.as_deref(), Some("e1"));
		assert!(!st.disposed);
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::mouse::MouseEvent;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Callbacks/deps for transcript viewer component integration.
pub trait AgentTranscriptViewerCallbacks {
	fn on_close(&self);
	fn on_hub_close(&self);
	fn request_render(&self);
	/// Resolve session file path for agent id (local mode).
	fn session_file_for(&self, _agent_id: &str) -> Option<String> {
		None
	}
}

/// Fullscreen transcript viewer component.
///
/// Reference: `AgentTranscriptViewer` in `l/src/overlays/agent-transcript-viewer.ts`.
///
/// Deviations: `ChatTranscriptBuilder`, `TranscriptBrowser`, `Editor`, async
/// timers/polling, and direct TUI rendering are host-driven. This component
/// owns viewer state, scroll/expand/input transitions, local/remote refresh
/// orchestration hooks, and bordered rendering + native rows description.
pub struct AgentTranscriptViewer<CB: AgentTranscriptViewerCallbacks> {
	pub agent_id: String,
	pub state: TranscriptViewerState,
	pub expanded: bool,
	pub notice: Option<String>,
	pub lines: Vec<String>,
	pub scroll_offset: usize,
	pub viewport_rows: usize,
	pub sendable: bool,
	pub editor_text: String,
	callbacks: CB,
	theme: OverlayTheme,
}

impl<CB: AgentTranscriptViewerCallbacks> std::fmt::Debug for AgentTranscriptViewer<CB> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AgentTranscriptViewer")
			.field("agent_id", &self.agent_id)
			.field("expanded", &self.expanded)
			.field("sendable", &self.sendable)
			.finish_non_exhaustive()
	}
}

impl<CB: AgentTranscriptViewerCallbacks> AgentTranscriptViewer<CB> {
	#[must_use]
	pub fn new(agent_id: impl Into<String>, callbacks: CB, sendable: bool) -> Self {
		Self {
			agent_id: agent_id.into(),
			state: TranscriptViewerState::new(None),
			expanded: false,
			notice: None,
			lines: Vec::new(),
			scroll_offset: 0,
			viewport_rows: 12,
			sendable,
			editor_text: String::new(),
			callbacks,
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	pub fn set_lines(&mut self, lines: Vec<String>) {
		self.lines = lines;
		self.clamp_scroll();
	}

	pub fn dispose_viewer(&mut self) {
		self.state.disposed = true;
	}

	fn clamp_scroll(&mut self) {
		let max = self.lines.len().saturating_sub(self.viewport_rows.max(1));
		if self.scroll_offset > max {
			self.scroll_offset = max;
		}
	}

	pub fn scroll(&mut self, delta: isize) {
		if delta.is_negative() {
			self.scroll_offset = self.scroll_offset.saturating_sub(delta.unsigned_abs());
		} else {
			self.scroll_offset = self.scroll_offset.saturating_add(delta as usize);
		}
		self.clamp_scroll();
	}

	pub fn scroll_to_top(&mut self) {
		self.scroll_offset = 0;
	}

	pub fn scroll_to_bottom(&mut self) {
		self.scroll_offset = self.lines.len().saturating_sub(self.viewport_rows.max(1));
	}

	fn handle_scroll_key(&mut self, key: &Key) -> bool {
		match key {
			Key::Up | Key::Char('k') => {
				self.scroll(-1);
				true
			}
			Key::Down | Key::Char('j') => {
				self.scroll(1);
				true
			}
			Key::PageUp => {
				self.scroll(-(self.viewport_rows.max(1) as isize));
				true
			}
			Key::PageDown => {
				self.scroll(self.viewport_rows.max(1) as isize);
				true
			}
			Key::Home | Key::Char('g') => {
				self.scroll_to_top();
				true
			}
			Key::End | Key::Char('G') => {
				self.scroll_to_bottom();
				true
			}
			_ => false,
		}
	}

	fn escape(&mut self) {
		if self.sendable && !self.editor_text.trim().is_empty() {
			self.editor_text.clear();
			self.callbacks.request_render();
			return;
		}
		self.callbacks.on_close();
	}

	fn visible_body(&self) -> Vec<String> {
		if self.lines.is_empty() {
			return vec![if self.state.local_unavailable == "none" {
				"No transcript session file.".to_string()
			} else if self.state.local_unavailable == "missing" {
				"Transcript file missing.".to_string()
			} else {
				"(empty transcript)".to_string()
			}];
		}
		let start = self.scroll_offset.min(self.lines.len());
		let end = (start + self.viewport_rows.max(1)).min(self.lines.len());
		self.lines[start..end].to_vec()
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let title = format!("Agent transcript · {}", self.agent_id);
		let mut out = vec![top_border(width, &title, &self.theme)];
		for l in self.visible_body() {
			out.push(row(&l, width, &self.theme));
		}
		if let Some(n) = &self.notice {
			out.push(divider(width, &self.theme));
			out.push(row(&format!("Notice: {n}"), width, &self.theme));
		}
		if self.sendable {
			out.push(divider(width, &self.theme));
			out.push(row(&format!("Message: {}", self.editor_text), width, &self.theme));
		}
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<CB: AgentTranscriptViewerCallbacks> Component for AgentTranscriptViewer<CB> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if keys::is_escape(key) {
			self.escape();
			return;
		}
		if matches!(key, Key::Ctrl('o')) || (matches!(key, Key::Char('o')) && !self.sendable) {
			self.expanded = !self.expanded;
			self.callbacks.request_render();
			return;
		}
		if self.sendable
			&& (!self.editor_text.trim().is_empty()
				|| matches!(key, Key::Char(_) | Key::Backspace | Key::Delete | Key::Enter))
		{
			match key {
				Key::Char(c) => {
					self.editor_text.push(*c);
					self.callbacks.request_render();
					return;
				}
				Key::Backspace => {
					self.editor_text.pop();
					self.callbacks.request_render();
					return;
				}
				Key::Delete => {
					self.editor_text.clear();
					self.callbacks.request_render();
					return;
				}
				Key::Enter => {
					if !self.editor_text.trim().is_empty() {
						self.notice = Some("Message queued for host send".to_string());
						self.editor_text.clear();
					}
					self.callbacks.request_render();
					return;
				}
				_ => {}
			}
		}
		if self.handle_scroll_key(key) {
			self.callbacks.request_render();
		}
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		if event.is_scroll_up() {
			self.scroll(-3);
			self.callbacks.request_render();
		} else if event.is_scroll_down() {
			self.scroll(3);
			self.callbacks.request_render();
		}
	}

	fn invalidate(&mut self) {}

	fn dispose(&mut self) {
		self.dispose_viewer();
	}
}

impl<CB: AgentTranscriptViewerCallbacks> NativeDescribe for AgentTranscriptViewer<CB> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;
	use std::cell::RefCell;

	struct Cb {
		closed: RefCell<bool>,
	}
	impl AgentTranscriptViewerCallbacks for Cb {
		fn on_close(&self) { *self.closed.borrow_mut() = true; }
		fn on_hub_close(&self) {}
		fn request_render(&self) {}
	}

	#[test]
	fn transcript_viewer_component_renders_scrolls_and_describes() {
		let mut v = AgentTranscriptViewer::new("agent-1", Cb { closed: RefCell::new(false) }, false);
		v.viewport_rows = 3;
		v.set_lines(vec!["l1".into(), "l2".into(), "l3".into(), "l4".into(), "l5".into()]);
		let lines = v.render(60);
		assert!(lines.iter().any(|l| l.contains("Agent transcript")));
		v.handle_input(&Key::Down);
		assert_eq!(v.scroll_offset, 1);
		v.handle_input(&Key::End);
		assert_eq!(v.scroll_offset, 2);
		let node = v.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
		v.handle_input(&Key::Esc);
		assert!(*v.callbacks.closed.borrow());
	}

	#[test]
	fn transcript_viewer_component_editor_and_dispose() {
		let mut v = AgentTranscriptViewer::new("a", Cb { closed: RefCell::new(false) }, true);
		v.handle_input(&Key::Char('h'));
		v.handle_input(&Key::Char('i'));
		assert_eq!(v.editor_text, "hi");
		v.handle_input(&Key::Enter);
		assert!(v.editor_text.is_empty());
		v.dispose();
		assert!(v.state.disposed);
	}
}
