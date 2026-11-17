//! Copy selector data model and pure helpers.
//!
//! Reference: `l/src/overlays/copy-selector.ts`.
//!
//! Deviations:
//! - `CopySelectorComponent` (fullscreen/navigator UI, transcript builder/
//!   browser rendering, selection/descend behavior, clipboard/open integration,
//!   theme/highlighting/native picker) is not ported. This module ports
//!   exported types/constants and pure text extraction/block/timeline helpers.
//! - Transcript/session message types are abstracted to lightweight structs and
//!   JSON-like views where `@oh-my-pi/pi-coding-agent` types are unavailable.
//! - `turnPreview`/native node builders and theme separators are represented as
//!   plain data; host renders previews.
//! - `extractBlocks/extractLinks/commandFromToolCall/userMessageLabel` from
//!   other modules are reimplemented minimally where needed.

pub const EARLIER_TURNS_KEY: &str = "earlier";
pub const PREVIEW_BLOCK_LINES: usize = 400;
pub const BLOCK_PREVIEW_LINES: usize = 12;

/// Plain one-line label and styling role for a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnSummary {
	pub label: String,
	pub role: String,
}

/// Minimal transcript entry view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranscriptEntry {
	pub id: String,
	pub timestamp: Option<i64>,
	pub role: String,
	pub content: String,
	pub tool_name: Option<String>,
	pub command: Option<String>,
	pub code: Option<String>,
	pub custom_type: Option<String>,
	pub is_user_turn: bool,
}

/// Outline target for a turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutlineTarget {
	pub turn_id: String,
	pub entries: Vec<TranscriptEntry>,
	pub is_user_turn: bool,
}

/// Where picked content came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyPickSource {
	pub entry_id: String,
	pub role: String,
}

/// Block kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyBlockKind {
	Code,
	Quote,
	Command,
	Link,
	Text,
}

/// One copyable inner block of a transcript turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyBlock {
	pub label: String,
	pub content: String,
	pub entry_id: String,
	pub kind: Option<CopyBlockKind>,
	pub language: Option<String>,
	pub href: Option<String>,
}

/// Timeline column definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineColumn {
	pub id: &'static str,
	pub format: &'static str,
	pub priority: u8,
}

pub const TIMELINE_COLUMNS: &[TimelineColumn] = &[TimelineColumn { id: "at", format: "dim", priority: 1 }];

/// Timeline item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineItem {
	pub id: String,
	pub label: String,
	pub node: String,
	pub role: Option<String>,
	pub at: Option<String>,
}

/// Memoized timeline items.
#[derive(Clone, Debug, Default)]
pub struct TimelineItems {
	targets_len: usize,
	items: Vec<TimelineItem>,
}

impl TimelineItems {
	#[must_use]
	pub fn of(&mut self, targets: &[OutlineTarget]) -> &[TimelineItem] {
		// cheap invalidation by length+ids hash not tracked; recompute always for correctness
		self.items = targets.iter().map(timeline_item).collect();
		self.targets_len = targets.len();
		&self.items
	}
}

/// Argument keys naming what a tool call works on.
pub const TOOL_TARGET_KEYS: &[&str] = &["command", "pattern", "path", "file_path", "query", "url", "code"];

/// First non-blank line, whitespace collapsed.
#[must_use]
pub fn first_line(source: &str) -> String {
	let line = source
		.lines()
		.find(|p| !p.trim().is_empty())
		.unwrap_or("");
	line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Local HH:MM from timestamp ms; empty if invalid.
#[must_use]
pub fn entry_time_ms(timestamp_ms: Option<i64>) -> String {
	let Some(ms) = timestamp_ms else { return String::new() };
	if !(0..=4102444800000).contains(&ms) { // rough sanity
		return String::new();
	}
	let secs = (ms / 1000).rem_euclid(86400);
	let h = secs / 3600;
	let m = (secs % 3600) / 60;
	format!("{h:02}:{m:02}")
}

/// Turn summary for an entry.
#[must_use]
pub fn turn_summary(entry: &TranscriptEntry) -> TurnSummary {
	match entry.role.as_str() {
		"user" => TurnSummary { label: first_line(&entry.content), role: "omp.user".into() },
		"assistant" => {
			let label = first_line(&entry.content);
			TurnSummary {
				label: if label.is_empty() { "thinking".into() } else { label },
				role: "omp.assistant".into(),
			}
		}
		"toolResult" => TurnSummary {
			label: format!("{} result", entry.tool_name.as_deref().unwrap_or("tool")),
			role: format!("omp.tool.{}", entry.tool_name.as_deref().unwrap_or("tool")),
		},
		"bashExecution" => TurnSummary {
			label: format!("$ {}", first_line(entry.command.as_deref().unwrap_or(&entry.content))),
			role: "omp.tool.bash".into(),
		},
		"pythonExecution" => TurnSummary {
			label: first_line(entry.code.as_deref().unwrap_or(&entry.content)),
			role: "omp.tool.eval".into(),
		},
		"compactionSummary" => TurnSummary { label: "Compaction summary".into(), role: "omp.summary".into() },
		"branchSummary" => TurnSummary { label: "Branch summary".into(), role: "omp.summary".into() },
		"custom" | "hookMessage" => TurnSummary {
			label: {
				let t = first_line(&entry.content);
				if t.is_empty() { entry.custom_type.clone().unwrap_or_else(|| "custom".into()) } else { t }
			},
			role: "omp.custom".into(),
		},
		_ => TurnSummary { label: entry.id.clone(), role: "omp.message".into() },
	}
}

/// One-line tool call label.
#[must_use]
pub fn tool_call_label(name: &str, args: Option<&std::collections::BTreeMap<String, String>>) -> String {
	if let Some(args) = args {
		for key in TOOL_TARGET_KEYS {
			if let Some(value) = args.get(*key) {
				if !value.trim().is_empty() {
					return format!("{name} {}", first_line(value));
				}
			}
		}
	}
	name.to_string()
}

/// Timeline item for an outline target.
#[must_use]
pub fn timeline_item(target: &OutlineTarget) -> TimelineItem {
	let Some(entry) = target.entries.first() else {
		return TimelineItem { id: target.turn_id.clone(), label: String::new(), node: "marker".into(), role: None, at: None };
	};
	let summary = turn_summary(entry);
	let label = if summary.label.is_empty() { "(empty)".into() } else { summary.label.clone() };
	if target.is_user_turn {
		return TimelineItem {
			id: target.turn_id.clone(),
			label,
			node: "user".into(),
			role: None,
			at: {
				let t = entry_time_ms(entry.timestamp);
				if t.is_empty() { None } else { Some(t) }
			},
		};
	}
	match entry.role.as_str() {
		"assistant" => TimelineItem { id: target.turn_id.clone(), label, node: "assistant".into(), role: None, at: None },
		"toolResult" | "bashExecution" | "pythonExecution" => TimelineItem {
			id: target.turn_id.clone(),
			label,
			node: "tool".into(),
			role: Some(summary.role),
			at: None,
		},
		_ => TimelineItem { id: target.turn_id.clone(), label, node: "marker".into(), role: None, at: None },
	}
}

/// Block caption.
#[must_use]
pub fn block_caption(block: &CopyBlock) -> String {
	if block.href.is_some() || block.kind == Some(CopyBlockKind::Quote) {
		return block.label.clone();
	}
	let lines = block.content.lines().count().max(1);
	let kind = if block.kind == Some(CopyBlockKind::Code) {
		block.language.clone().unwrap_or_else(|| "code".into())
	} else {
		block.label.clone()
	};
	let suffix = if lines == 1 { "" } else { "s" };
	format!("{kind} · {lines} line{suffix}")
}

/// Extract code/quote blocks from markdown-like text (minimal).
#[must_use]
pub fn extract_blocks(text: &str) -> Vec<(bool, Option<String>, String)> {
	// (is_code, lang, content)
	let mut out = Vec::new();
	let mut in_code = false;
	let mut lang: Option<String> = None;
	let mut buf = String::new();
	for line in text.lines() {
		if let Some(rest) = line.strip_prefix("```") {
			if in_code {
				out.push((true, lang.take(), buf.clone()));
				buf.clear();
				in_code = false;
			} else {
				in_code = true;
				let l = rest.trim();
				lang = if l.is_empty() { None } else { Some(l.to_string()) };
			}
			continue;
		}
		if in_code {
			if !buf.is_empty() { buf.push('\n'); }
			buf.push_str(line);
			continue;
		}
		if line.trim_start().starts_with('>') {
			let q = line.trim_start().trim_start_matches('>').trim_start();
			out.push((false, None, q.to_string()));
		}
	}
	if in_code {
		out.push((true, lang, buf));
	}
	out
}

/// Extract links (minimal URL scan).
#[must_use]
pub fn extract_links(text: &str) -> Vec<(String, String)> {
	// (label/text, href) - simplified: raw URLs
	let mut out = Vec::new();
	for token in text.split_whitespace() {
		let t = token.trim_matches(|c: char| "(),[]<>\"'".contains(c));
		if t.starts_with("http://") || t.starts_with("https://") {
			out.push((t.to_string(), t.to_string()));
		}
	}
	out
}

/// Collect inner blocks from entries.
#[must_use]
pub fn collect_blocks(entries: &[TranscriptEntry]) -> Vec<CopyBlock> {
	let mut blocks = Vec::new();
	for entry in entries {
		match entry.role.as_str() {
			"user" | "assistant" | "custom" | "hookMessage" => {
				for (is_code, lang, content) in extract_blocks(&entry.content) {
					if is_code {
						blocks.push(CopyBlock {
							label: lang.as_ref().map(|l| format!("{l} code")).unwrap_or_else(|| "code".into()),
							content,
							entry_id: entry.id.clone(),
							kind: Some(CopyBlockKind::Code),
							language: lang,
							href: None,
						});
					} else {
						blocks.push(CopyBlock {
							label: "quote".into(),
							content,
							entry_id: entry.id.clone(),
							kind: Some(CopyBlockKind::Quote),
							language: None,
							href: None,
						});
					}
				}
				for (label, href) in extract_links(&entry.content) {
					blocks.push(CopyBlock {
						label: if label == href { "link".into() } else { format!("link · {label}") },
						content: href.clone(),
						entry_id: entry.id.clone(),
						kind: Some(CopyBlockKind::Link),
						language: None,
						href: Some(href),
					});
				}
			}
			"toolResult" => {
				if !entry.content.trim().is_empty() {
					blocks.push(CopyBlock {
						label: format!("{} result", entry.tool_name.as_deref().unwrap_or("tool")),
						content: entry.content.clone(),
						entry_id: entry.id.clone(),
						kind: Some(CopyBlockKind::Text),
						language: None,
						href: None,
					});
				}
			}
			"bashExecution" => {
				if let Some(cmd) = &entry.command {
					blocks.push(CopyBlock { label: "command".into(), content: cmd.clone(), entry_id: entry.id.clone(), kind: Some(CopyBlockKind::Command), language: Some("bash".into()), href: None });
				}
				if !entry.content.trim().is_empty() {
					blocks.push(CopyBlock { label: "output".into(), content: entry.content.clone(), entry_id: entry.id.clone(), kind: Some(CopyBlockKind::Text), language: None, href: None });
				}
			}
			"pythonExecution" => {
				if let Some(code) = &entry.code {
					blocks.push(CopyBlock { label: "eval code".into(), content: code.clone(), entry_id: entry.id.clone(), kind: Some(CopyBlockKind::Command), language: Some("python".into()), href: None });
				}
				if !entry.content.trim().is_empty() {
					blocks.push(CopyBlock { label: "output".into(), content: entry.content.clone(), entry_id: entry.id.clone(), kind: Some(CopyBlockKind::Text), language: None, href: None });
				}
			}
			_ => {}
		}
	}
	blocks
}

/// Clipboard payload for a whole turn.
#[must_use]
pub fn target_copy(target: &OutlineTarget, blocks: &[CopyBlock]) -> (String, String) {
	if let Some(first) = target.entries.first() {
		if !first.content.trim().is_empty() && (first.role == "user" || first.role == "assistant") {
			return (first.content.clone(), "turn".to_string());
		}
	}
	if let Some(b) = blocks.first() {
		return (b.content.clone(), b.label.clone());
	}
	(String::new(), "empty".to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn first_line_and_time() {
		assert_eq!(first_line("\n  hi \nthere"), "hi");
		assert_eq!(entry_time_ms(Some(0)), "00:00");
		assert_eq!(entry_time_ms(None), "");
	}

	#[test]
	fn turn_summary_and_timeline_item() {
		let e = TranscriptEntry { id: "1".into(), role: "user".into(), content: "hello\nworld".into(), is_user_turn: true, timestamp: Some(3661_000), ..Default::default() };
		let s = turn_summary(&e);
		assert_eq!(s.label, "hello");
		assert_eq!(s.role, "omp.user");
		let t = OutlineTarget { turn_id: "1".into(), entries: vec![e], is_user_turn: true };
		let item = timeline_item(&t);
		assert_eq!(item.node, "user");
		assert_eq!(item.at.as_deref(), Some("01:01"));
	}

	#[test]
	fn block_extract_collect_and_caption() {
		let text = "```rs\nfn a(){}\n```\n> quote\nhttps://x.y";
		let e = TranscriptEntry { id: "e".into(), role: "assistant".into(), content: text.into(), ..Default::default() };
		let blocks = collect_blocks(&[e]);
		assert!(blocks.iter().any(|b| b.kind == Some(CopyBlockKind::Code) && b.language.as_deref() == Some("rs")));
		assert!(blocks.iter().any(|b| b.kind == Some(CopyBlockKind::Quote)));
		assert!(blocks.iter().any(|b| b.href.is_some()));
		let cap = block_caption(&CopyBlock { label: "code".into(), content: "a\nb".into(), kind: Some(CopyBlockKind::Code), language: Some("rust".into()), ..Default::default() });
		assert!(cap.contains("rust"));
		assert!(cap.contains("2 lines"));
	}

	#[test]
	fn target_copy_prefers_turn_content() {
		let t = OutlineTarget { turn_id: "t".into(), entries: vec![TranscriptEntry { id: "1".into(), role: "user".into(), content: "msg".into(), ..Default::default() }], is_user_turn: true };
		let (c, l) = target_copy(&t, &[]);
		assert_eq!(c, "msg");
		assert_eq!(l, "turn");
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::mouse::{MouseEvent, MouseKind};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Callbacks for copy selector.
pub trait CopySelectorCallbacks {
	fn on_copy(&self, content: String, label: String);
	fn on_cancel(&self);
	fn request_render(&self) {}
}

/// Picker to select a transcript block/turn to copy.
///
/// Reference: `CopySelectorComponent` in `l/src/overlays/copy-selector.ts`.
///
/// Deviations: rich select-list rendering, mouse routing, clipboard API, and
/// timeline styling are simplified to state-driven list selection and copy/cancel.
pub struct CopySelectorComponent<CB: CopySelectorCallbacks> {
	pub targets: Vec<OutlineTarget>,
	pub blocks: Vec<CopyBlock>,
	pub index: usize,
	callbacks: CB,
	theme: OverlayTheme,
}

impl<CB: CopySelectorCallbacks> std::fmt::Debug for CopySelectorComponent<CB> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("CopySelectorComponent")
			.field("targets", &self.targets.len())
			.field("index", &self.index)
			.finish_non_exhaustive()
	}
}

impl<CB: CopySelectorCallbacks> CopySelectorComponent<CB> {
	#[must_use]
	pub fn new(targets: Vec<OutlineTarget>, callbacks: CB) -> Self {
		let blocks = targets
			.iter()
			.flat_map(|t| collect_blocks(&t.entries))
			.collect();
		Self { targets, blocks, index: 0, callbacks, theme: OverlayTheme::default() }
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	fn items(&self) -> Vec<(String, String)> {
		let mut items = Vec::new();
		for t in &self.targets {
			let (c, l) = target_copy(t, &collect_blocks(&t.entries));
			items.push((l, c));
		}
		for b in &self.blocks {
			items.push((block_caption(b), b.content.clone()));
		}
		if items.is_empty() {
			items.push(("empty".to_string(), String::new()));
		}
		items
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let items = self.items();
		let mut out = vec![top_border(width, "Copy", &self.theme)];
		for (i, (label, _)) in items.iter().enumerate().take(20) {
			let cursor = if i == self.index { "›" } else { " " };
			out.push(row(&format!("{cursor} {label}"), width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		out.push(row("↑/↓ select · enter copy · esc cancel", width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<CB: CopySelectorCallbacks> Component for CopySelectorComponent<CB> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		let len = self.items().len().max(1);
		match key {
			Key::Esc => self.callbacks.on_cancel(),
			Key::Up => self.index = self.index.saturating_sub(1),
			Key::Down => self.index = (self.index + 1).min(len - 1),
			Key::Enter => {
				let items = self.items();
				if let Some((label, content)) = items.get(self.index).cloned() {
					self.callbacks.on_copy(content, label);
				}
			}
			_ => {}
		}
		self.callbacks.request_render();
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		match event.kind {
			MouseKind::ScrollUp => self.index = self.index.saturating_sub(1),
			MouseKind::ScrollDown => {
				let len = self.items().len().max(1);
				self.index = (self.index + 1).min(len - 1);
			}
			_ => {}
		}
		self.callbacks.request_render();
	}
}

impl<CB: CopySelectorCallbacks> NativeDescribe for CopySelectorComponent<CB> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("index", serde_json::Value::from(self.index as i64));
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

	struct Cb { copied: RefCell<bool>, cancelled: RefCell<bool> }
	impl CopySelectorCallbacks for Cb {
		fn on_copy(&self, _content: String, _label: String) { *self.copied.borrow_mut() = true; }
		fn on_cancel(&self) { *self.cancelled.borrow_mut() = true; }
		fn request_render(&self) {}
	}

	#[test]
	fn copy_selector_component_renders_and_copies() {
		let t = OutlineTarget { turn_id: "t".into(), entries: vec![TranscriptEntry { id: "1".into(), role: "user".into(), content: "msg".into(), ..Default::default() }], is_user_turn: true };
		let mut c = CopySelectorComponent::new(vec![t], Cb { copied: RefCell::new(false), cancelled: RefCell::new(false) });
		let lines = c.render(50);
		assert!(lines.iter().any(|l| l.contains("Copy")));
		c.handle_input(&Key::Enter);
		assert!(*c.callbacks.copied.borrow());
		let node = c.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
