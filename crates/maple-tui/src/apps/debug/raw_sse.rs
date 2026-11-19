//! Scrollable raw SSE viewer for the debug menu.
//!
//! Reference: `l/src/apps/debug/raw-sse.ts`.
//!
//! Deviations:
//! - TS uses `Scrollable`, `Markdown`, `matchesKey`, theme, clipboard, logger,
//!   and the TUI component tree. This port provides a data/view model
//!   ([`RawSseViewerModel`]) and plain-text render helpers over
//!   [`crate::apps::debug::raw_sse_buffer::RawSseBuffer`].
//! - Copy action is represented as a returned [`RawSseAction::Copy(String)`]
//!   instead of invoking clipboard/notify side effects.
//! - Styling/markdown rendering omitted; lines are the underlying buffer text.

use crate::apps::debug::raw_sse_buffer::RawSseBuffer;

/// `data:` payloads longer than this are pretty-printed.
///
/// Reference: `PRETTY_PRINT_DATA_THRESHOLD` in `l/src/apps/debug/raw-sse.ts`.
pub const PRETTY_PRINT_DATA_THRESHOLD: usize = 100;

/// Expand wide JSON `data:` payloads into multi-line indented `data:` entries.
///
/// Reference: `expandPrettyDataLines` in `l/src/apps/debug/raw-sse.ts`. A
/// `data: <json>` line longer than [`PRETTY_PRINT_DATA_THRESHOLD`] whose body
/// parses as JSON is replaced by `JSON.stringify(parsed, null, 2)` with each
/// line re-prefixed by `data: `; multi-line `data:` is still valid SSE. Short
/// lines, non-`data:` lines, non-JSON bodies and invalid JSON pass through
/// unchanged.
pub fn expand_pretty_data_lines(raw: &[String]) -> Vec<String> {
	let mut out: Vec<String> = Vec::new();
	for line in raw {
		if !line.starts_with("data: ") || line.encode_utf16().count() <= PRETTY_PRINT_DATA_THRESHOLD {
			out.push(line.clone());
			continue;
		}
		let body = &line["data: ".len()..];
		let trimmed = body.trim();
		if trimmed.is_empty() || (!trimmed.starts_with('{') && !trimmed.starts_with('[')) {
			out.push(line.clone());
			continue;
		}
		let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
			Ok(parsed) => parsed,
			Err(_) => {
				out.push(line.clone());
				continue;
			}
		};
		let pretty = match serde_json::to_string_pretty(&parsed) {
			Ok(pretty) => pretty,
			Err(_) => {
				out.push(line.clone());
				continue;
			}
		};
		for pretty_line in pretty.split('\n') {
			out.push(format!("data: {pretty_line}"));
		}
	}
	out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawSseAction {
	None,
	ScrollUp(u16),
	ScrollDown(u16),
	PageUp,
	PageDown,
	ScrollHome,
	ScrollEnd,
	Copy(String),
}

/// Viewer model wrapping a raw SSE buffer with scroll/selection state.
#[derive(Clone, Debug)]
pub struct RawSseViewerModel<'a> {
	buffer: &'a RawSseBuffer,
	scroll_offset_lines: usize,
	follow_tail: bool,
}

impl<'a> RawSseViewerModel<'a> {
	pub fn new(buffer: &'a RawSseBuffer) -> Self {
		Self { buffer, scroll_offset_lines: 0, follow_tail: true }
	}

	pub fn scroll_offset_lines(&self) -> usize { self.scroll_offset_lines }
	pub fn follow_tail(&self) -> bool { self.follow_tail }

	/// Total display rows: wide `data:` payloads count once per expanded line,
	/// so scroll offsets and follow-tail stay in rendered-line units.
	pub fn total_lines(&self) -> usize { self.expanded_lines().len() }
	fn expanded_lines(&self) -> Vec<String> {
		expand_pretty_data_lines(self.buffer.lines())
	}
	pub fn max_offset(&self, viewport_lines: usize) -> usize {
		self.total_lines().saturating_sub(viewport_lines.max(1))
	}

	pub fn clamp(&mut self, viewport_lines: usize) {
		let max = self.max_offset(viewport_lines);
		if self.follow_tail || self.scroll_offset_lines > max {
			self.scroll_offset_lines = max;
		}
	}

	pub fn scroll_up(&mut self, n: usize, viewport_lines: usize) {
		self.follow_tail = false;
		self.scroll_offset_lines = self.scroll_offset_lines.saturating_sub(n);
		self.clamp(viewport_lines);
	}

	pub fn scroll_down(&mut self, n: usize, viewport_lines: usize) {
		self.scroll_offset_lines = self.scroll_offset_lines.saturating_add(n);
		let max = self.max_offset(viewport_lines);
		if self.scroll_offset_lines >= max {
			self.scroll_offset_lines = max;
			self.follow_tail = true;
		}
	}

	pub fn page_up(&mut self, viewport_lines: usize) {
		let step = viewport_lines.saturating_sub(1).max(1);
		self.scroll_up(step, viewport_lines);
	}
	pub fn page_down(&mut self, viewport_lines: usize) {
		let step = viewport_lines.saturating_sub(1).max(1);
		self.scroll_down(step, viewport_lines);
	}
	pub fn scroll_home(&mut self, viewport_lines: usize) {
		self.follow_tail = false;
		self.scroll_offset_lines = 0;
		self.clamp(viewport_lines);
	}
	pub fn scroll_end(&mut self, viewport_lines: usize) {
		self.follow_tail = true;
		self.scroll_offset_lines = self.max_offset(viewport_lines);
	}

	pub fn handle_input(&mut self, data: &str, viewport_lines: usize) -> RawSseAction {
		match data {
			"up" | "k" => { self.scroll_up(1, viewport_lines); RawSseAction::ScrollUp(1) }
			"down" | "j" => { self.scroll_down(1, viewport_lines); RawSseAction::ScrollDown(1) }
			"pageUp" => { self.page_up(viewport_lines); RawSseAction::PageUp }
			"pageDown" => { self.page_down(viewport_lines); RawSseAction::PageDown }
			"home" | "g" => { self.scroll_home(viewport_lines); RawSseAction::ScrollHome }
			"end" | "G" => { self.scroll_end(viewport_lines); RawSseAction::ScrollEnd }
			"c" | "y" => RawSseAction::Copy(self.copy_text()),
			_ => RawSseAction::None,
		}
	}

	/// Visible formatted lines for the viewport.
	pub fn render_lines(&mut self, viewport_lines: usize) -> Vec<String> {
		self.clamp(viewport_lines);
		let lines = self.expanded_lines();
		let start = self.scroll_offset_lines;
		let end = (start + viewport_lines.max(1)).min(lines.len());
		lines[start..end].to_vec()
	}

	pub fn copy_text(&self) -> String {
		self.buffer.lines().join("\n")
	}

}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::apps::debug::raw_sse_buffer::RawSseBuffer;

	fn filled(n: usize) -> RawSseBuffer {
		let mut b = RawSseBuffer::new();
		for i in 0..n { b.push_line(&format!("line-{i}")); }
		b
	}

	#[test]
	fn follows_tail_and_scrolls() {
		let b = filled(20);
		let mut v = RawSseViewerModel::new(&b);
		assert!(v.follow_tail());
		let lines = v.render_lines(5);
		assert_eq!(lines.len(), 5);
		assert_eq!(lines[4], "line-19");
		v.scroll_up(3, 5);
		assert!(!v.follow_tail());
		v.scroll_end(5);
		assert!(v.follow_tail());
		assert_eq!(v.scroll_offset_lines(), 15);
	}

	#[test]
	fn paging_and_home_end() {
		let b = filled(30);
		let mut v = RawSseViewerModel::new(&b);
		v.scroll_home(10);
		assert_eq!(v.scroll_offset_lines(), 0);
		v.page_down(10);
		assert!(v.scroll_offset_lines() > 0);
		v.page_up(10);
		assert_eq!(v.scroll_offset_lines(), 0);
		v.scroll_end(10);
		assert_eq!(v.scroll_offset_lines(), 20);
	}

	#[test]
	fn copy_and_input() {
		let b = filled(3);
		let mut v = RawSseViewerModel::new(&b);
		let a = v.handle_input("c", 2);
		match a {
			RawSseAction::Copy(t) => assert!(t.contains("line-0") && t.contains("line-2")),
			_ => panic!("expected copy"),
		}
		assert_eq!(v.handle_input("x", 2), RawSseAction::None);
	}

	#[test]
	fn render_clamps_to_bounds() {
		let b = filled(2);
		let mut v = RawSseViewerModel::new(&b);
		let lines = v.render_lines(10);
		assert_eq!(lines, vec!["line-0", "line-1"]);
	}

	#[test]
	fn expands_wide_json_data_lines_and_passes_short_lines_through() {
		let wide = format!("data: {}", serde_json::json!({
			"type": "response.output_item.done",
			"item": {
				"type": "message",
				"id": "msg_1234567890",
				"role": "assistant",
				"status": "completed",
				"content": [{ "type": "output_text", "text": "Hello there, this is a long enough message" }],
			},
		}));
		let input = vec![": ws ← response.output_item.done".to_string(), wide.clone()];
		let out = expand_pretty_data_lines(&input);
		assert_eq!(out[0], input[0]);
		assert!(out.len() > 2);
		assert!(out[1..].iter().all(|line| line.starts_with("data: ")));
		let rejoined: String = out[1..].iter().map(|line| &line["data: ".len()..]).collect::<Vec<_>>().join("\n");
		assert_eq!(serde_json::from_str::<serde_json::Value>(&rejoined).unwrap(), serde_json::from_str::<serde_json::Value>(&wide["data: ".len()..]).unwrap());

		let short = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}".to_string();
		assert_eq!(expand_pretty_data_lines(&[short.clone()]), vec![short]);
	}

	#[test]
	fn falls_back_for_non_json_and_invalid_payloads() {
		let non_json = format!("data: {}", "x".repeat(200));
		assert_eq!(expand_pretty_data_lines(&[non_json.clone()]), vec![non_json]);
		let broken = format!("data: {{\"unterminated\":\"{}", "x".repeat(200));
		assert_eq!(expand_pretty_data_lines(&[broken.clone()]), vec![broken]);
		let wide_comment = format!(": {}", "x".repeat(300));
		assert_eq!(expand_pretty_data_lines(&[wide_comment.clone()]), vec![wide_comment]);
	}

	#[test]
	fn render_lines_expands_wide_data_payloads() {
		let mut b = RawSseBuffer::new();
		b.push_line(": ws ← response.output_item.done");
		b.push_line(&format!(
			"data: {}",
			serde_json::json!({
				"type": "response.output_item.done",
				"item": {
					"type": "message",
					"id": "msg_1234567890",
					"role": "assistant",
					"status": "completed",
					"content": [{ "type": "output_text", "text": "Hello there, this is a long enough message" }],
				},
			})
		));
		b.push_line("data: {\"event\":\"done\",\"index\":1}");
		let mut v = RawSseViewerModel::new(&b);
		let lines = v.render_lines(100);
		assert!(lines.len() > b.lines().len(), "{lines:?}");
		assert!(lines.iter().any(|line| line.contains("\"type\": \"response.output_item.done\"")), "{lines:?}");
		assert_eq!(v.total_lines(), lines.len());
		assert_eq!(v.scroll_offset_lines(), 0);
	}
}
