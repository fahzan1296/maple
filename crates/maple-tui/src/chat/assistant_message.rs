//! One assistant reply: prose, reasoning, images, errors, and turn usage.
//!
//! Reference: `l/src/chat/assistant-message.ts`.
//!
//! Streaming content stays mutable until the provider finalizes it, because a
//! later delta can revise earlier Markdown. Native rendering is one column of
//! `md` nodes (or a folded thinking section) under `omp.assistant`, with the
//! turn-ending error frame and the usage row after the answer.
//!
//! Deviations:
//! - No widgets, timers, or incremental repaint plumbing: the thinking pulse,
//!   the streaming tok/s gauge, Kitty PNG conversion, and the stable-row ledger
//!   are drop-in host concerns. `thinking_frame`/`streaming_rate` are accepted
//!   as plain inputs so the same heads can be described.
//! - Table charts and SVG figure lifting are not wired here; text blocks draw
//!   as plain Markdown (the crate's Markdown already renders charts).
//! - Extension text renderers are stored but not invoked: `AssistantTextRenderer`
//!   is a JS extension surface.
//! - The crate has no `retry_recovery.attempt` count in scope, so callers pass
//!   `retry_attempt` where the reference reads it.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::chat::cache_invalidation_marker::CacheInvalidation;
use crate::chat::messages::{AssistantContent, AssistantMessage, ImageContent};
use crate::chat::served_model_marker::ServedModelMismatch;
use crate::chat::thinking_display::{
	canonicalize_message, format_thinking_for_display, has_displayable_thinking,
};
use crate::chat::transcript_render_helpers::{
	resolve_assistant_error_presentation, AssistantErrorPresentation,
};
use crate::chrome::error_block::format_error_block;
use crate::component::Component;
use crate::components::markdown::Markdown;
use crate::native::describe::{col, keyed, md, node, span, text as ntext, NativeDescribe};
use crate::native::node::{
	DescribeContext, NativeNode, NodeKind, NodeText, Props, Span, Tone,
};
use crate::overlays::usage_row::{format_turn_usage, TurnUsageSummary};
use crate::render::render_utils::expand_key_hint;
use crate::theme::theme_class::Theme;
use crate::theme::tui_adapters::get_markdown_theme;

/// Max wrapped rows of a turn-ending provider error rendered inline. Bounds a
/// proxy 502 whose body is a whole HTML page; a long single line still wraps
/// instead of being cut at a fixed column.
pub const MAX_TRANSCRIPT_ERROR_ROWS: usize = 8;

/// The streaming "thinking" starburst frames.
pub const THINKING_DOTS_FRAMES: [&str; 8] = ["✻", "✼", "❉", "❊", "✺", "✹", "✸", "✶"];

/// When a thinking block was first seen streaming and when it stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThinkingClock {
	/// Clock milliseconds at the first live frame.
	pub start: i64,
	/// Clock milliseconds when it stopped; `None` while live.
	pub end: Option<i64>,
	/// Provider tokens seen.
	pub tokens: u64,
}

/// "Thought for 12s", or "Thought" when it was never seen streaming.
pub fn thought_label(clock: Option<&ThinkingClock>) -> String {
	let Some(clock) = clock else {
		return "Thought".to_string();
	};
	let Some(end) = clock.end else {
		return "Thought".to_string();
	};
	let ms = (end - clock.start).max(0);
	if ms < 60_000 {
		format!("Thought for {}s", ((ms + 500) / 1000).max(1))
	} else {
		format!("Thought for {}m {}s", ms / 60_000, (ms % 60_000) / 1000)
	}
}

/// A stable part of the block's finished content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StablePart {
	/// Reasoning text.
	Thinking(String),
	/// Answer text.
	Text(String),
	/// A spacer between parts.
	Spacer,
}

/// Whether `current` extends `previous` part-wise (only the last part grows).
pub fn is_snapshot_extension(previous: &[StablePart], current: &[StablePart]) -> bool {
	if previous.len() > current.len() {
		return false;
	}
	for (index, before) in previous.iter().enumerate() {
		let after = &current[index];
		match (before, after) {
			(StablePart::Spacer, _) | (_, StablePart::Spacer) => continue,
			(StablePart::Thinking(before), StablePart::Thinking(after)) => {
				let is_last = index + 1 == previous.len();
				if is_last {
					if !after.starts_with(before) {
						return false;
					}
				} else if before != after {
					return false;
				}
			}
			(StablePart::Text(before), StablePart::Text(after)) => {
				let is_last = index + 1 == previous.len();
				if is_last {
					if !after.starts_with(before) {
						return false;
					}
				} else if before != after {
					return false;
				}
			}
			_ => return false,
		}
	}
	true
}

/// The display text and visibility of a thinking block.
pub fn resolve_thinking_display(thinking: &str, prose_only: bool) -> (String, bool) {
	let formatted = format_thinking_for_display(thinking, prose_only);
	let visible = has_displayable_thinking(Some(thinking), Some(&formatted));
	(formatted.trim().to_string(), visible)
}

/// Drop the closing border a renderer draws for a source that ends inside an
/// open fence: that row is not part of the message yet.
pub fn trim_fabricated_closer(rows: &[String], ends_inside_open_fence: bool) -> Vec<String> {
	if !ends_inside_open_fence {
		return rows.to_vec();
	}
	let mut last = rows.len();
	while last > 0 && rows[last - 1].trim().is_empty() {
		last -= 1;
	}
	if last > 0 {
		rows[..last - 1].to_vec()
	} else {
		rows.to_vec()
	}
}

/// Parse a `#rrggbb` color.
fn parse_hex(color: &str) -> Option<(u8, u8, u8)> {
	let color = color.strip_prefix('#')?;
	if color.len() != 6 {
		return None;
	}
	let r = u8::from_str_radix(&color[0..2], 16).ok()?;
	let g = u8::from_str_radix(&color[2..4], 16).ok()?;
	let b = u8::from_str_radix(&color[4..6], 16).ok()?;
	Some((r, g, b))
}

/// Linear blend between two `#rrggbb` colors; `t` is clamped to `0..=1`.
pub fn lerp_hex(from: &str, to: &str, t: f64) -> String {
	let t = t.clamp(0.0, 1.0);
	let (Some(from), Some(to)) = (parse_hex(from), parse_hex(to)) else {
		return from.to_string();
	};
	let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
	format!("#{:02x}{:02x}{:02x}", mix(from.0, to.0), mix(from.1, to.1), mix(from.2, to.2))
}

/// Flatten an assistant message's displayable parts.
pub fn display_parts(message: &AssistantMessage, prose_only: bool) -> Vec<StablePart> {
	let mut parts = Vec::new();
	for content in &message.content {
		match content {
			AssistantContent::Text(text) => {
				if !canonicalize_message(Some(&text.text)).is_empty() {
					parts.push(StablePart::Text(text.text.clone()));
				}
			}
			AssistantContent::Thinking(thinking) => {
				if thinking.narration == Some(true) {
					if !canonicalize_message(Some(&thinking.thinking)).is_empty() {
						parts.push(StablePart::Text(thinking.thinking.clone()));
					}
					continue;
				}
				let (display, visible) = resolve_thinking_display(&thinking.thinking, prose_only);
				if visible {
					parts.push(StablePart::Thinking(display));
				}
			}
			_ => {}
		}
	}
	parts
}

/// Whether a message carries any visible answer text.
pub fn answered(message: &AssistantMessage) -> bool {
	message.content.iter().any(|content| match content {
		AssistantContent::Text(text) => !canonicalize_message(Some(&text.text)).is_empty(),
		_ => false,
	})
}

/// Render a reply: prose, reasoning, and its error frame.
pub struct AssistantMessageComponent {
	theme: Theme,
	message: Option<AssistantMessage>,
	show_images: bool,
	show_tool_result_images: bool,
	show_table_charts: bool,
	hide_thinking_block: bool,
	prose_only_thinking: bool,
	expand_thinking_blocks: bool,
	mid_stream_publication: bool,
	finalized: bool,
	error_pinned: bool,
	error_expanded: bool,
	rewound: bool,
	expanded: bool,
	block_version: u64,
	turn_usage: Option<TurnUsageSummary>,
	cache_invalidation: Option<CacheInvalidation>,
	served_model_mismatch: Option<ServedModelMismatch>,
	tool_images: HashMap<String, Vec<ImageContent>>,
	tool_media_paths: HashMap<String, String>,
	link_targets: HashMap<String, String>,
	text_color_transform: Option<Rc<dyn Fn(&str) -> String>>,
	reaction: Option<String>,
	thinking_clock: HashMap<usize, ThinkingClock>,
	thinking_collapsed: HashMap<usize, bool>,
	thinking_tokens: u64,
	streaming_rate: f64,
	stable_rows: Vec<String>,
	stable_parts: Vec<StablePart>,
	text_renderers: usize,
	last_transient: bool,
	persistence_key: Option<String>,
}

impl AssistantMessageComponent {
	/// An empty reply.
	pub fn new(theme: Theme) -> Self {
		AssistantMessageComponent {
			theme,
			message: None,
			show_images: true,
			show_tool_result_images: true,
			show_table_charts: true,
			hide_thinking_block: false,
			prose_only_thinking: false,
			expand_thinking_blocks: false,
			mid_stream_publication: true,
			finalized: false,
			error_pinned: false,
			error_expanded: false,
			rewound: false,
			expanded: false,
			block_version: 0,
			turn_usage: None,
			cache_invalidation: None,
			served_model_mismatch: None,
			tool_images: HashMap::new(),
			tool_media_paths: HashMap::new(),
			link_targets: HashMap::new(),
			text_color_transform: None,
			reaction: None,
			thinking_clock: HashMap::new(),
			thinking_collapsed: HashMap::new(),
			thinking_tokens: 0,
			streaming_rate: 0.0,
			stable_rows: Vec::new(),
			stable_parts: Vec::new(),
			text_renderers: 0,
			last_transient: false,
			persistence_key: None,
		}
	}

	/// The message this component draws.
	pub fn message(&self) -> Option<&AssistantMessage> {
		self.message.as_ref()
	}

	/// Replace the message and bump the block version.
	pub fn update_content(&mut self, message: AssistantMessage, transient: bool) {
		self.message = Some(message);
		self.last_transient = transient;
		self.block_version += 1;
		self.stable_rows.clear();
	}

	/// The text transform applied to prose.
	pub fn set_text_color_transform(&mut self, transform: Option<Box<dyn Fn(&str) -> String>>) {
		self.text_color_transform = transform.map(Rc::from);
	}

	/// Install resolved destinations for model-authored prose links.
	pub fn set_link_targets(&mut self, targets: HashMap<String, String>) {
		self.link_targets = targets;
	}

	/// The reaction lifted from the reply's opening emoji.
	pub fn reaction(&self) -> Option<&str> {
		self.reaction.as_deref()
	}

	/// Take a reaction lifted from the reply's opening emoji.
	pub fn set_reaction(&mut self, reaction: Option<String>) {
		self.reaction = reaction;
	}

	/// Note the cache invalidation this reply proved.
	pub fn set_cache_invalidation(&mut self, info: Option<CacheInvalidation>) {
		if self.cache_invalidation == info {
			return;
		}
		self.cache_invalidation = info;
		self.block_version += 1;
	}

	/// The cache invalidation marker's window.
	pub fn cache_invalidation(&self) -> Option<CacheInvalidation> {
		self.cache_invalidation
	}

	/// The turn's totals when this answer ends it.
	pub fn set_turn_usage(&mut self, summary: Option<TurnUsageSummary>) {
		if self.turn_usage == summary {
			return;
		}
		self.turn_usage = summary;
		self.block_version += 1;
	}

	/// The turn's totals.
	pub fn turn_usage(&self) -> Option<&TurnUsageSummary> {
		self.turn_usage.as_ref()
	}

	/// Note that a different model served the turn.
	pub fn set_served_model_mismatch(&mut self, info: Option<ServedModelMismatch>) {
		self.served_model_mismatch = info;
		self.block_version += 1;
	}

	/// The served-model mismatch, when one was reported.
	pub fn served_model_mismatch(&self) -> Option<&ServedModelMismatch> {
		self.served_model_mismatch.as_ref()
	}

	/// Hide or show finished thinking blocks.
	pub fn set_hide_thinking_block(&mut self, hide: bool) {
		if self.hide_thinking_block == hide {
			return;
		}
		self.hide_thinking_block = hide;
		self.thinking_collapsed.clear();
		self.block_version += 1;
	}

	/// Allow or stop mid-stream publication.
	pub fn set_mid_stream_publication(&mut self, allowed: bool) {
		self.mid_stream_publication = allowed;
	}

	/// Whether mid-stream publication is allowed.
	pub fn mid_stream_publication(&self) -> bool {
		self.mid_stream_publication
	}

	/// Render only prose-shaped reasoning.
	pub fn set_prose_only_thinking(&mut self, prose_only: bool) {
		if self.prose_only_thinking == prose_only {
			return;
		}
		self.prose_only_thinking = prose_only;
		self.block_version += 1;
	}

	/// Keep finished thinking blocks expanded.
	pub fn set_expand_thinking_blocks(&mut self, expand: bool) {
		if self.expand_thinking_blocks == expand {
			return;
		}
		self.expand_thinking_blocks = expand;
		self.block_version += 1;
	}

	/// Suppress the inline error while the same error sits in the banner.
	pub fn set_error_pinned(&mut self, pinned: bool) {
		if self.error_pinned == pinned {
			return;
		}
		self.error_pinned = pinned;
		self.block_version += 1;
	}

	/// Whether the inline error is suppressed.
	pub fn error_pinned(&self) -> bool {
		self.error_pinned
	}

	/// Expand or collapse the reply's thinking and its error body.
	pub fn set_expanded(&mut self, expanded: bool) {
		self.expanded = expanded;
		self.error_expanded = expanded;
		self.block_version += 1;
	}

	/// Whether the reply is expanded.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Whether the block may retire as history.
	pub fn is_transcript_block_finalized(&self) -> bool {
		self.finalized
	}

	/// Whether the block still holds images or a live figure: nothing does
	/// here, so a reply is never pending.
	pub fn is_transcript_block_pending(&self) -> bool {
		false
	}

	/// A TTSR rule aborted this text; the turn re-streams below it.
	pub fn mark_rewound(&mut self) {
		self.rewound = true;
		self.block_version += 1;
	}

	/// Whether the reply was rewound.
	pub fn rewound(&self) -> bool {
		self.rewound
	}

	/// The block's mutation counter.
	pub fn transcript_block_version(&self) -> u64 {
		self.block_version
	}

	/// Seal the block as final history.
	pub fn mark_transcript_block_finalized(&mut self) {
		if self.finalized {
			return;
		}
		self.finalized = true;
		self.block_version += 1;
	}

	/// Take the extension text renderers; they are counted, not invoked.
	pub fn set_text_renderers(&mut self, count: usize) {
		self.text_renderers = count;
	}

	/// Note how many extension text renderers are installed.
	pub fn text_renderers(&self) -> usize {
		self.text_renderers
	}

	/// Apply retry-recovery bookkeeping from a rebuilt message.
	pub fn apply_retry_recovery(&mut self, recovery: Option<crate::chat::messages::AssistantRetryRecovery>) {
		let Some(message) = self.message.as_mut() else {
			return;
		};
		if message.retry_recovery == recovery {
			return;
		}
		message.retry_recovery = recovery;
		self.block_version += 1;
	}

	/// Re-render only when `timestamp` matches this reply.
	pub fn rerender_message(&mut self, timestamp: i64) -> bool {
		if self.message.as_ref().map(|message| message.timestamp) != Some(timestamp) {
			return false;
		}
		self.block_version += 1;
		true
	}

	/// The key the session persists this reply under.
	pub fn message_persistence_key(&self) -> Option<&str> {
		self.persistence_key.as_deref()
	}

	/// Set the persistence key.
	pub fn set_persistence_key(&mut self, key: Option<String>) {
		self.persistence_key = key;
	}

	/// Show or hide images.
	pub fn set_images_visible(&mut self, visible: bool) {
		self.show_images = visible;
		self.block_version += 1;
	}

	/// Whether images are shown.
	pub fn images_visible(&self) -> bool {
		self.show_images
	}

	/// Draw charts under numeric tables.
	pub fn set_table_charts_visible(&mut self, visible: bool) {
		self.show_table_charts = visible;
		self.block_version += 1;
	}

	/// Whether charts are drawn.
	pub fn table_charts_visible(&self) -> bool {
		self.show_table_charts
	}

	/// Show or hide images inside tool results.
	pub fn set_tool_result_images_visible(&mut self, visible: bool) {
		self.show_tool_result_images = visible;
		self.block_version += 1;
	}

	/// Attach late tool-result images.
	pub fn set_tool_result_images(&mut self, tool_call_id: &str, images: Vec<ImageContent>, media_path: Option<&str>) {
		if images.is_empty() {
			self.tool_images.remove(tool_call_id);
		} else {
			self.tool_images.insert(tool_call_id.to_string(), images);
		}
		match media_path {
			Some(path) => {
				self.tool_media_paths.insert(tool_call_id.to_string(), path.to_string());
			}
			None => {
				self.tool_media_paths.remove(tool_call_id);
			}
		}
		self.block_version += 1;
	}

	/// Tool-result images attached to the reply.
	pub fn tool_result_images(&self) -> &HashMap<String, Vec<ImageContent>> {
		&self.tool_images
	}

	/// Record the provider tokens a live thinking block reports.
	pub fn set_thinking_progress(&mut self, tokens: u64, rate: f64) {
		self.thinking_tokens = tokens;
		self.streaming_rate = rate;
	}

	/// Note that a thinking block started or stopped streaming.
	pub fn set_thinking_clock(&mut self, index: usize, clock: ThinkingClock) {
		self.thinking_clock.insert(index, clock);
	}

	/// Toggle a thinking section as the terminal did.
	pub fn toggle_thinking(&mut self, index: usize) {
		let collapsed = self.thinking_collapsed.entry(index).or_insert(
			self.hide_thinking_block || !self.expand_thinking_blocks,
		);
		*collapsed = !*collapsed;
		self.block_version += 1;
	}

	/// The published stable rows.
	pub fn transcript_stable_rows(&self) -> &[String] {
		&self.stable_rows
	}

	/// Drop the published stable rows.
	pub fn reset_transcript_stable_rows(&mut self) {
		self.stable_rows.clear();
	}

	/// Publish the reply's prose as stable rows and return them.
	pub fn publish_stable_rows(&mut self, width: u16) -> Vec<String> {
		let rows = self.prose_rows(width, usize::MAX);
		let parts = self.message.as_ref().map(|message| display_parts(message, self.prose_only_thinking)).unwrap_or_default();
		if is_snapshot_extension(&self.stable_parts, &parts) {
			self.stable_parts = parts;
			self.stable_rows = rows.clone();
		}
		rows
	}

	/// The published rows, or the first `count` of a fresh render.
	pub fn render_transcript_stable_rows(&mut self, count: usize, width: u16) -> Vec<String> {
		if self.stable_rows.len() >= count {
			return self.stable_rows[..count].to_vec();
		}
		let rows = self.publish_stable_rows(width);
		rows[..count.min(rows.len())].to_vec()
	}

	/// The emergency row for a block torn down mid-stream.
	pub fn render_transcript_block_emergency_row(&self, width: u16) -> Option<String> {
		self.prose_rows(width, 1).into_iter().next()
	}

	/// Paint a prose block's plain runs.
	///
	/// An explicitly installed transform wins (live-command contract); otherwise
	/// the optional `assistantMessageText` token paints prose only, leaving md*
	/// roles their own colors (`proseFg` in the reference). An unset token keeps
	/// the terminal default foreground byte-for-byte.
	fn apply_prose_paint(&self, markdown: &mut Markdown) {
		markdown.set_theme(get_markdown_theme(&self.theme, true));
		if let Some(transform) = self.text_color_transform.clone() {
			markdown.set_text_transform(Some(Box::new(move |text: &str| transform(text))));
			markdown.set_prose_transform(None);
		} else if self.theme.has_color("assistantMessageText") {
			let theme = self.theme.clone();
			markdown.set_text_transform(None);
			markdown.set_prose_transform(Some(Box::new(move |text: &str| {
				theme
					.fg("assistantMessageText", text)
					.unwrap_or_else(|_| text.to_string())
			})));
		} else {
			markdown.set_text_transform(None);
			markdown.set_prose_transform(None);
		}
	}

	fn prose_rows(&self, width: u16, limit: usize) -> Vec<String> {
		let Some(message) = self.message.as_ref() else {
			return Vec::new();
		};
		let mut rows = Vec::new();
		for content in &message.content {
			if let AssistantContent::Text(text) = content {
				if canonicalize_message(Some(&text.text)).is_empty() {
					continue;
				}
				let mut markdown = Markdown::new();
				self.apply_prose_paint(&mut markdown);
				markdown.set_text(text.text.clone());
				rows.extend(markdown.render(width));
				if rows.len() >= limit {
					rows.truncate(limit);
					return rows;
				}
			}
		}
		rows
	}

	/// The turn-ending error's native/terminal rows.
	pub fn error_rows(&self, width: u16) -> Vec<String> {
		let Some(message) = self.message.as_ref() else {
			return Vec::new();
		};
		if self.error_pinned {
			return Vec::new();
		}
		match resolve_assistant_error_presentation(message, 1) {
			AssistantErrorPresentation::None => Vec::new(),
			AssistantErrorPresentation::Full { text, .. } => {
				let max_rows = if self.error_expanded {
					usize::MAX
				} else {
					MAX_TRANSCRIPT_ERROR_ROWS
				};
				let hint = expand_key_hint();
				let error_style = |line: &str, index: usize| {
					if index == 0 {
						self.theme
							.fg("error", &format!("Error: {line}"))
							.unwrap_or_else(|_| line.to_string())
					} else {
						self.theme.fg("error", line).unwrap_or_else(|_| line.to_string())
					}
				};
				let dim = |line: &str| self.theme.fg("dim", line).unwrap_or_else(|_| line.to_string());
				let block = format_error_block(&text, width as usize, max_rows, error_style, dim, &hint);
				block.lines().map(str::to_string).collect()
			}
			AssistantErrorPresentation::CompactRecovered { text, .. } => {
				let line = self
					.theme
					.fg("dim", &format!("↺ {text}"))
					.unwrap_or_else(|_| text);
				vec![line]
			}
		}
	}

	fn thinking_rows(&self, index: usize, text: &str, live: bool, width: u16) -> Vec<String> {
		let collapsed = self
			.thinking_collapsed
			.get(&index)
			.copied()
			.unwrap_or(self.hide_thinking_block || (!live && !self.expand_thinking_blocks));
		let head = if live {
			let frame = THINKING_DOTS_FRAMES
				[(self.block_version as usize) % THINKING_DOTS_FRAMES.len()];
			let mut head = format!("{frame} Thinking…");
			if self.streaming_rate >= 0.05 {
				head.push_str(&format!(" {:.0} tok/s", self.streaming_rate));
			}
			let painted = self.theme.fg("muted", &head).unwrap_or(head);
			painted
		} else {
			let label = thought_label(self.thinking_clock.get(&index));
			let title = if self.thinking_tokens > 0 {
				Some(format!("{} tokens", self.thinking_tokens))
			} else {
				None
			};
			let mut head = label;
			if let Some(title) = title {
				head.push_str(&format!(" ({title})"));
			}
			self.theme.fg("muted", &head).unwrap_or(head)
		};
		let mut rows = vec![head];
		if !collapsed {
			let mut markdown = Markdown::new();
			markdown.set_theme(get_markdown_theme(&self.theme, true));
			markdown.set_text(text);
			rows.extend(markdown.render(width));
		}
		rows
	}

	/// The thinking indexes of the message.
	pub fn thinking_indexes(&self) -> Vec<usize> {
		let Some(message) = self.message.as_ref() else {
			return Vec::new();
		};
		message
			.content
			.iter()
			.enumerate()
			.filter_map(|(index, content)| match content {
				AssistantContent::Thinking(_) => Some(index),
				_ => None,
			})
			.collect()
	}
}

impl Component for AssistantMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let Some(message) = self.message.clone() else {
			return Vec::new();
		};
		let mut rows = Vec::new();
		for (index, content) in message.content.iter().enumerate() {
			match content {
				AssistantContent::Text(text) => {
					if canonicalize_message(Some(&text.text)).is_empty() {
						continue;
					}
					if !rows.is_empty() {
						rows.push(String::new());
					}
					let mut markdown = Markdown::new();
					self.apply_prose_paint(&mut markdown);
					markdown.set_text(text.text.clone());
					rows.extend(markdown.render(width));
				}
				AssistantContent::Thinking(thinking) => {
					if thinking.narration == Some(true) {
						if !canonicalize_message(Some(&thinking.thinking)).is_empty() {
							if !rows.is_empty() {
								rows.push(String::new());
							}
							let mut markdown = Markdown::new();
							self.apply_prose_paint(&mut markdown);
							markdown.set_text(thinking.thinking.clone());
							rows.extend(markdown.render(width));
						}
						continue;
					}
					let live = !self.finalized && self.last_transient;
					let (display, visible) =
						resolve_thinking_display(&thinking.thinking, self.prose_only_thinking);
					if !visible {
						continue;
					}
					if self.hide_thinking_block && !live {
						continue;
					}
					if !rows.is_empty() {
						rows.push(String::new());
					}
					rows.extend(self.thinking_rows(index, &display, live, width));
				}
				_ => {}
			}
		}
		let error_rows = self.error_rows(width);
		if !error_rows.is_empty() {
			rows.push(String::new());
			rows.extend(error_rows);
		}
		if self.rewound {
			let badge = self
				.theme
				.fg("muted", "↺ rewound")
				.unwrap_or_else(|_| "↺ rewound".to_string());
			rows.push(badge);
		}
		if let Some(usage) = &self.turn_usage {
			let (text, _title) = format_turn_usage(usage);
			let line = self
				.theme
				.fg("dim", &format!("⧉ · {text}"))
				.unwrap_or(text);
			rows.push(line);
		}
		rows
	}

	fn invalidate(&mut self) {
		self.block_version += 1;
	}
}

impl crate::native::describe::NativeDescribe for AssistantMessageComponent {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let mut children = Vec::new();
		let mut answered_text = false;
		if let Some(message) = self.message.as_ref() {
			for (index, content) in message.content.iter().enumerate() {
				match content {
					AssistantContent::Text(text) => {
						if canonicalize_message(Some(&text.text)).is_empty() {
							continue;
						}
						answered_text = true;
						let source = match &self.text_color_transform {
							Some(transform) => transform(&text.text),
							None => text.text.clone(),
						};
						children.push(keyed(md(source, Props::new()), format!("t{index}")));
					}
					AssistantContent::Thinking(thinking) => {
						let live = !self.finalized && self.last_transient;
						let (display, visible) =
							resolve_thinking_display(&thinking.thinking, self.prose_only_thinking);
						if !visible || (self.hide_thinking_block && !live) {
							continue;
						}
						let collapsed = self.thinking_collapsed.get(&index).copied().unwrap_or(
							self.hide_thinking_block || (!live && !self.expand_thinking_blocks),
						);
						let head_text = if live {
							format!("{} Thinking…", THINKING_DOTS_FRAMES[0])
						} else {
							thought_label(self.thinking_clock.get(&index))
						};
						let mut head_props = Props::new();
						head_props.set_str("gap", "sm");
						head_props.set_str("role", "omp.assistant.thinking.head");
						let head = node(
							NodeKind::Row,
							head_props,
							vec![ntext(
								NodeText::Spans(vec![span(head_text, Some("muted"))]),
								Props::new(),
							)],
							Some("head"),
						);
						let mut section_props = Props::new();
						section_props.set_str(
							"role",
							if live { "omp.thinking.live" } else { "omp.thinking" },
						);
						section_props.set_bool("collapsible", true);
						section_props.set_bool("collapsed", collapsed);
						if let Some(clock) = self.thinking_clock.get(&index) {
							if let Some(end) = clock.end {
								section_props.set_i64("took", (end - clock.start).max(0));
							}
						}
						children.push(keyed(
							node(
								NodeKind::Section,
								section_props,
								vec![head, keyed(md(display, Props::new()), "body".to_string())],
								None,
							),
							format!("k{index}"),
						));
					}
					AssistantContent::Image(image) if self.show_images && !image.data.is_empty() => {
						children.push(keyed(
							node(
								NodeKind::Image,
								{
									let mut props = Props::new();
									props.set_str("mime", image.mime_type.clone());
									props
								},
								Vec::new(),
								None,
							),
							format!("i{index}"),
						));
					}
					_ => {}
				}
			}
			if let Some(error) = self.describe_error(message) {
				children.push(error);
			}
		}
		if self.rewound {
			let mut props = Props::new();
			props.set_str("text", "↺ rewound");
			props.set_str("tone", "muted");
			props.set_str("role", "omp.assistant.rewound-tag");
			children.push(keyed(node(NodeKind::Badge, props, Vec::new(), None), "rewound".to_string()));
		}
		if let Some(usage) = &self.turn_usage {
			let (text, title) = format_turn_usage(usage);
			let mut props = Props::new();
			props.set_str("role", "omp.turn.usage");
			props.set_str("gap", "xs");
			props.set_str("align", "center");
			props.set_str("title", title);
			let mut line = Vec::new();
			if answered_text {
				let mut copy = Props::new();
				copy.set_str("name", "copy");
				copy.set_str("role", "omp.turn.copy");
				copy.set_str("title", "Copy answer");
				copy.set_str("aria", "Copy answer");
				line.push(keyed(node(NodeKind::Icon, copy, Vec::new(), None), "copy".to_string()));
			}
			line.push(ntext(NodeText::Plain(text), Props::new()));
			children.push(keyed(node(NodeKind::Row, props, line, None), "turn-usage".to_string()));
		}
		let mut props = Props::new();
		props.set_str(
			"role",
			if self.rewound { "omp.assistant.rewound" } else { "omp.assistant" },
		);
		col(children, props)
	}
}

impl AssistantMessageComponent {
	fn describe_error(&self, message: &AssistantMessage) -> Option<NativeNode> {
		if self.error_pinned {
			return None;
		}
		let presentation = resolve_assistant_error_presentation(message, 1);
		match presentation {
			AssistantErrorPresentation::None => None,
			AssistantErrorPresentation::CompactRecovered { text, .. } => {
				let mut props = Props::new();
				props.set_str("role", "omp.assistant.recovered");
				props.set_tone(Tone::Muted);
				Some(node(
					NodeKind::Section,
					props,
					vec![ntext(
						NodeText::Spans(vec![span(text, Some("dim"))]),
						Props::new(),
					)],
					None,
				))
			}
			AssistantErrorPresentation::Full { text, .. } => {
				let mut props = Props::new();
				props.set_str("role", "omp.assistant.error");
				props.set_tone(Tone::Error);
				props.set_str("frame", "error");
				props.set_bool("collapsible", false);
				let mut body = md(text.clone(), Props::new());
				if !self.error_expanded && text.lines().count() > MAX_TRANSCRIPT_ERROR_ROWS {
					body.props.set_i64("preview", MAX_TRANSCRIPT_ERROR_ROWS as i64);
					props.set_bool("collapsible", true);
					props.set_bool("collapsed", true);
				}
				Some(node(NodeKind::Card, props, vec![keyed(body, "body".to_string())], None))
			}
		}
	}

	/// The ids of tool calls whose images the reply carries.
	pub fn tool_image_call_ids(&self) -> HashSet<&str> {
		self.tool_images.keys().map(String::as_str).collect()
	}

	/// Spans for the reply's native head, if a host wants one.
	pub fn head_spans(&self) -> Vec<Span> {
		let mut spans = Vec::new();
		if let Some(reaction) = &self.reaction {
			spans.push(span(format!("{reaction} "), None));
		}
		spans
	}

	/// The resolved link target map.
	pub fn link_targets(&self) -> &HashMap<String, String> {
		&self.link_targets
	}

	/// The thinking frames, for hosts that animate in place.
	pub fn thinking_frames(&self) -> &'static [&'static str] {
		&THINKING_DOTS_FRAMES
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::messages::{StopReason, TextContent, ThinkingContent};
	use crate::native::describe::NativeDescribe;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use crate::theme::color::ColorMode;

	fn theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").unwrap(),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.unwrap()
	}

	fn message(content: Vec<AssistantContent>) -> AssistantMessage {
		AssistantMessage {
			content,
			stop_reason: StopReason::Stop,
			timestamp: 7,
			..AssistantMessage::default()
		}
	}

	fn plain(rows: &[String]) -> String {
		rows.iter()
			.map(|row| crate::ansi_scan::strip_ansi(row))
			.collect::<Vec<_>>()
			.join("\n")
	}

	#[test]
	fn thought_label_formats_seconds_and_minutes() {
		assert_eq!(thought_label(None), "Thought");
		let clock = ThinkingClock {
			start: 0,
			end: Some(12_300),
			tokens: 5,
		};
		assert_eq!(thought_label(Some(&clock)), "Thought for 12s");
		let long = ThinkingClock {
			start: 0,
			end: Some(125_000),
			tokens: 1,
		};
		assert_eq!(thought_label(Some(&long)), "Thought for 2m 5s");
	}

	#[test]
	fn snapshot_extension_only_grows_the_last_part() {
		let a = vec![StablePart::Thinking("one".to_string()), StablePart::Text("hi".to_string())];
		let grown = vec![StablePart::Thinking("one".to_string()), StablePart::Text("hi there".to_string())];
		let changed = vec![StablePart::Thinking("two".to_string()), StablePart::Text("hi".to_string())];
		assert!(is_snapshot_extension(&a, &grown));
		assert!(!is_snapshot_extension(&a, &changed));
		assert!(!is_snapshot_extension(&grown, &a));
	}

	#[test]
	fn hex_blend_clamps_and_rounds() {
		assert_eq!(lerp_hex("#000000", "#ffffff", 0.0), "#000000");
		assert_eq!(lerp_hex("#000000", "#ffffff", 1.0), "#ffffff");
		assert_eq!(lerp_hex("#000000", "#ffffff", 0.5), "#808080");
		assert_eq!(lerp_hex("nope", "#ffffff", 0.5), "nope");
	}

	#[test]
	fn trim_fabricated_closer_drops_the_closing_row() {
		let rows = vec!["text".to_string(), "└───".to_string(), String::new()];
		assert_eq!(trim_fabricated_closer(&rows, true), vec!["text".to_string()]);
		assert_eq!(trim_fabricated_closer(&rows, false).len(), 3);
	}

	#[test]
	fn render_draws_prose_and_hides_finished_thinking() {
		let mut component = AssistantMessageComponent::new(theme());
		component.update_content(
			message(vec![
				AssistantContent::Thinking(ThinkingContent {
					thinking: "weighing options".to_string(),
					..ThinkingContent::default()
				}),
				AssistantContent::Text(TextContent {
					text: "Use a guard clause.".to_string(),
					..TextContent::default()
				}),
			]),
			false,
		);
		component.mark_transcript_block_finalized();
		let rows = component.render(80);
		let text = plain(&rows);
		assert!(text.contains("Use a guard clause."), "{text}");
		assert!(!text.contains("weighing options"), "finished thinking stays folded: {text}");
		assert!(text.contains("Thought"), "{text}");

		component.set_expand_thinking_blocks(true);
		let expanded = plain(&component.render(80));
		assert!(expanded.contains("weighing options"), "{expanded}");
	}

	#[test]
	fn errors_render_capped_and_pinned_errors_stay_hidden() {
		let mut component = AssistantMessageComponent::new(theme());
		let long = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
		component.update_content(
			AssistantMessage {
				content: vec![],
				stop_reason: StopReason::Error,
				error_message: Some(long),
				..AssistantMessage::default()
			},
			false,
		);
		let capped = plain(&component.error_rows(60));
		assert!(capped.contains("Error: line 0"), "{capped}");
		assert!(capped.contains("more line"), "{capped}");

		component.set_expanded(true);
		let full = plain(&component.error_rows(60));
		assert!(full.contains("line 19"), "{full}");

		component.set_error_pinned(true);
		assert!(component.error_rows(60).is_empty());
	}

	#[test]
	fn native_column_reports_the_assistant_role_and_usage() {
		let mut component = AssistantMessageComponent::new(theme());
		component.update_content(
			message(vec![AssistantContent::Text(TextContent {
				text: "answer".to_string(),
				..TextContent::default()
			})]),
			false,
		);
		component.set_turn_usage(Some(TurnUsageSummary {
			requests: 1,
			total_tokens: 19_000,
			output: 400,
			..TurnUsageSummary::default()
		}));
		let described = component.describe_native(&DescribeContext::new(80));
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.props.get_str("role"), Some("omp.assistant"));
		assert!(described
			.children
			.iter()
			.any(|child| child.props.get_str("role") == Some("omp.turn.usage")));
	}

	#[test]
	fn stable_rows_publish_only_extending_snapshots() {
		let mut component = AssistantMessageComponent::new(theme());
		component.update_content(
			message(vec![AssistantContent::Text(TextContent {
				text: "first".to_string(),
				..TextContent::default()
			})]),
			false,
		);
		let rows = component.publish_stable_rows(70);
		assert!(!rows.is_empty());
		component.update_content(
			message(vec![AssistantContent::Text(TextContent {
				text: "first and second".to_string(),
				..TextContent::default()
			})]),
			false,
		);
		let grown = component.render_transcript_stable_rows(1, 70);
		assert_eq!(grown.len(), 1);
	}
}
