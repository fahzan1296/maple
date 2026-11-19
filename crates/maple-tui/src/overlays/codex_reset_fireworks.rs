//! Codex quota-reset fireworks detection and canvas helpers.
//!
//! Reference: `l/src/overlays/codex-reset-fireworks.ts`.
//!
//! Deviations:
//! - UI component/overlay host (`CodexResetFireworksComponent`, TUI, native
//!   describe/events, timers, theme styling) is not ported; this module ports
//!   exported snapshot/event types, detection logic, frame/banner/star/canvas
//!   drawing helpers and renderable frame model.
//! - ANSI/theme colors are represented by symbolic color names; host applies
//!   actual styling. Grapheme/visible-width handling is approximated by chars.

pub const FRAME_INTERVAL_MS: u64 = 85;
pub const FRAME_COUNT: usize = 34;

/// Active Codex account fields retained between status refreshes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodexResetUsageSevenDay {
	pub percent: f64,
	pub resets_at: Option<i64>,
	pub tier: Option<String>,
	pub plan: Option<String>,
}

/// Usage snapshot for one account.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodexResetUsageSnapshot {
	pub observed_at: Option<i64>,
	pub seven_day: Option<CodexResetUsageSevenDay>,
	pub saved_resets: Option<i64>,
}

/// Detected Codex quota event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodexResetFireworksEvent {
	UnscheduledWeeklyReset,
	SavedResetBanked { added: i64, available: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireworkColor {
	Cyan,
	Dim,
	Gold,
	Green,
	Pink,
	Violet,
	White,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasCell {
	pub glyph: String,
	pub color: FireworkColor,
	pub priority: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FireworkBurst {
	pub x: f64,
	pub y: f64,
	pub start: i32,
	pub color: FireworkColor,
}

pub const BURSTS: &[FireworkBurst] = &[
	FireworkBurst { x: 0.17, y: 0.46, start: 5, color: FireworkColor::Pink },
	FireworkBurst { x: 0.48, y: 0.20, start: 9, color: FireworkColor::Cyan },
	FireworkBurst { x: 0.78, y: 0.42, start: 13, color: FireworkColor::Gold },
	FireworkBurst { x: 0.31, y: 0.24, start: 17, color: FireworkColor::Violet },
	FireworkBurst { x: 0.65, y: 0.28, start: 21, color: FireworkColor::Green },
	FireworkBurst { x: 0.88, y: 0.18, start: 25, color: FireworkColor::Pink },
];

/// Compare consecutive reports for one Codex account.
#[must_use]
pub fn detect_codex_reset_fireworks(
	previous: &CodexResetUsageSnapshot,
	current: &CodexResetUsageSnapshot,
) -> Option<CodexResetFireworksEvent> {
	let previous_saved = previous.saved_resets;
	let current_saved = current.saved_resets;
	if let Some(prev) = previous_saved {
		match current_saved {
			None => {
				if prev > 0 {
					return None;
				}
			}
			Some(cur) => {
				if cur > prev {
					return Some(CodexResetFireworksEvent::SavedResetBanked {
						added: cur - prev,
						available: cur,
					});
				}
				if cur < prev {
					return None;
				}
			}
		}
	}

	let (prev7, cur7) = match (&previous.seven_day, &current.seven_day) {
		(Some(a), Some(b)) => (a, b),
		_ => return None,
	};
	if prev7.tier != cur7.tier || prev7.plan != cur7.plan {
		return None;
	}
	let prev_pct = (prev7.percent.max(0.0).min(100.0)).round() as i64;
	let cur_pct = (cur7.percent.max(0.0).min(100.0)).round() as i64;
	if prev_pct == 0 || cur_pct >= prev_pct {
		return None;
	}

	let scheduled = prev7.resets_at;
	let next = cur7.resets_at;
	let observed = current.observed_at;
	let (Some(scheduled), Some(next), Some(observed)) = (scheduled, next, observed) else {
		return None;
	};
	if next <= scheduled || observed >= scheduled {
		return None;
	}
	Some(CodexResetFireworksEvent::UnscheduledWeeklyReset)
}

pub type Canvas = Vec<Vec<Option<CanvasCell>>>;

#[must_use]
pub fn new_canvas(width: usize, height: usize) -> Canvas {
	vec![vec![None; width]; height]
}

pub fn set_cell(canvas: &mut [Vec<Option<CanvasCell>>], x: usize, y: usize, glyph: &str, color: FireworkColor, priority: i32) {
	let Some(row) = canvas.get_mut(y) else { return };
	if x >= row.len() {
		return;
	}
	let replace = row[x].as_ref().map(|c| priority >= c.priority).unwrap_or(true);
	if replace {
		row[x] = Some(CanvasCell {
			glyph: glyph.to_string(),
			color,
			priority,
		});
	}
}

pub fn draw_text(canvas: &mut [Vec<Option<CanvasCell>>], x: usize, y: usize, text: &str, color: FireworkColor, priority: i32) {
	let mut col = x;
	for ch in text.chars() {
		let w = 1usize;
		set_cell(canvas, col, y, &ch.to_string(), color, priority);
		col = col.saturating_add(w);
	}
}

pub fn draw_banner(canvas: &mut [Vec<Option<CanvasCell>>], left: usize, art_width: usize, height: usize, event: &CodexResetFireworksEvent) {
	if height < 3 || art_width < 8 {
		return;
	}
	let panel_width = 62usize.min(art_width);
	let panel_left = left + (art_width.saturating_sub(panel_width)) / 2;
	let top = height - 3;
	let inner_width = panel_width.saturating_sub(2);
	let title_text = match event {
		CodexResetFireworksEvent::UnscheduledWeeklyReset => " O P E N A I   R E S E T ",
		CodexResetFireworksEvent::SavedResetBanked { .. } => " S A V E D   R E S E T ",
	};
	let subtitle_text = match event {
		CodexResetFireworksEvent::UnscheduledWeeklyReset => "Weekly usage cleared early · Esc to return".to_string(),
		CodexResetFireworksEvent::SavedResetBanked { added: 1, available } => {
			format!("New reset banked · {available} available · Esc to return")
		}
		CodexResetFireworksEvent::SavedResetBanked { added, available } => {
			format!("{added} resets banked · {available} available · Esc to return")
		}
	};
	let title = truncate_chars(title_text, inner_width);
	let subtitle = truncate_chars(&subtitle_text, inner_width);
	let title_offset = inner_width.saturating_sub(char_len(&title)) / 2;
	let subtitle_offset = inner_width.saturating_sub(char_len(&subtitle)) / 2;
	draw_text(canvas, panel_left, top, &format!("╭{}╮", "─".repeat(inner_width)), FireworkColor::Violet, 20);
	draw_text(canvas, panel_left + 1 + title_offset, top, &title, FireworkColor::Gold, 21);
	draw_text(canvas, panel_left, top + 1, &format!("│{}│", " ".repeat(inner_width)), FireworkColor::Violet, 20);
	draw_text(canvas, panel_left + 1 + subtitle_offset, top + 1, &subtitle, FireworkColor::Cyan, 21);
	draw_text(canvas, panel_left, top + 2, &format!("╰{}╯", "─".repeat(inner_width)), FireworkColor::Violet, 20);
}

pub fn draw_stars(canvas: &mut [Vec<Option<CanvasCell>>], left: usize, art_width: usize, sky_height: usize, frame: usize) {
	if sky_height == 0 || art_width == 0 {
		return;
	}
	let count = (art_width / 3).max(5).min(26);
	for index in 0..count {
		let x = left + ((index * 37 + 11) % art_width);
		let y = (index * 7 + 2) % sky_height;
		let bright = (index + (frame / 3)) % 5 == 0;
		set_cell(canvas, x, y, if bright { "+" } else { "." }, if bright { FireworkColor::White } else { FireworkColor::Dim }, if bright { 2 } else { 1 });
	}
}

fn char_len(s: &str) -> usize { s.chars().count() }
fn truncate_chars(s: &str, max: usize) -> String {
	let mut out = String::new();
	for (i, ch) in s.chars().enumerate() {
		if i >= max { break; }
		out.push(ch);
	}
	out
}

/// Render a simple fireworks frame as lines of glyphs (host may color by cell metadata).
#[must_use]
pub fn render_frame_lines(event: &CodexResetFireworksEvent, width: usize, height: usize, frame: usize) -> Vec<String> {
	let w = width.max(8);
	let h = height.max(3);
	let mut canvas = new_canvas(w, h);
	draw_stars(&mut canvas, 0, w, h.saturating_sub(3), frame);
	draw_banner(&mut canvas, 0, w, h, event);
	canvas
		.into_iter()
		.map(|row| {
			row.into_iter()
				.map(|c| c.map(|c| c.glyph).unwrap_or_else(|| " ".to_string()))
				.collect::<String>()
		})
		.collect()
}

/// Stateful frame progression helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FireworksAnimation {
	pub event: CodexResetFireworksEvent,
	pub frame: usize,
	pub finished: bool,
}

impl FireworksAnimation {
	#[must_use]
	pub fn new(event: CodexResetFireworksEvent) -> Self {
		Self { event, frame: 0, finished: false }
	}
	pub fn tick(&mut self) {
		if self.finished { return; }
		self.frame = self.frame.saturating_add(1);
		if self.frame >= FRAME_COUNT {
			self.finished = true;
		}
	}
	#[must_use]
	pub fn render(&self, width: usize, height: usize) -> Vec<String> {
		render_frame_lines(&self.event, width, height, self.frame)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn snap7(pct: f64, resets_at: Option<i64>, tier: &str, plan: &str) -> CodexResetUsageSnapshot {
		CodexResetUsageSnapshot {
			observed_at: Some(100),
			seven_day: Some(CodexResetUsageSevenDay {
				percent: pct,
				resets_at,
				tier: Some(tier.into()),
				plan: Some(plan.into()),
			}),
			saved_resets: None,
		}
	}

	#[test]
	fn detect_saved_reset_banked_and_decrease_suppresses() {
		let prev = CodexResetUsageSnapshot { saved_resets: Some(1), ..Default::default() };
		let cur = CodexResetUsageSnapshot { saved_resets: Some(3), ..Default::default() };
		assert_eq!(detect_codex_reset_fireworks(&prev, &cur), Some(CodexResetFireworksEvent::SavedResetBanked { added: 2, available: 3 }));
		let dec = CodexResetUsageSnapshot { saved_resets: Some(0), ..Default::default() };
		assert_eq!(detect_codex_reset_fireworks(&prev, &dec), None);
		let gone = CodexResetUsageSnapshot { saved_resets: None, ..Default::default() };
		assert_eq!(detect_codex_reset_fireworks(&prev, &gone), None);
	}

	#[test]
	fn detect_unscheduled_weekly_reset_conditions() {
		let prev = snap7(80.0, Some(500), "pro", "p");
		let mut cur = snap7(20.0, Some(900), "pro", "p");
		assert_eq!(detect_codex_reset_fireworks(&prev, &cur), Some(CodexResetFireworksEvent::UnscheduledWeeklyReset));
		cur.observed_at = Some(600);
		assert_eq!(detect_codex_reset_fireworks(&prev, &cur), None);
		cur.observed_at = Some(100);
		cur.seven_day.as_mut().unwrap().tier = Some("team".into());
		assert_eq!(detect_codex_reset_fireworks(&prev, &cur), None);
	}

	#[test]
	fn canvas_banner_and_stars_draw() {
		let mut c = new_canvas(40, 8);
		draw_stars(&mut c, 0, 40, 5, 6);
		assert!(c.iter().flatten().filter_map(|x| x.as_ref()).any(|cell| cell.glyph == "." || cell.glyph == "+"));
		draw_banner(&mut c, 0, 40, 8, &CodexResetFireworksEvent::UnscheduledWeeklyReset);
		let joined = render_frame_lines(&CodexResetFireworksEvent::SavedResetBanked { added: 1, available: 2 }, 50, 8, 0).join("\n");
		assert!(joined.contains("New reset banked"));
	}

	#[test]
	fn animation_progresses_and_finishes() {
		let mut a = FireworksAnimation::new(CodexResetFireworksEvent::UnscheduledWeeklyReset);
		for _ in 0..FRAME_COUNT { a.tick(); }
		assert!(a.finished);
		assert_eq!(a.frame, FRAME_COUNT);
		let lines = a.render(30, 6);
		assert_eq!(lines.len(), 6);
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::component::Component;
use crate::keys::{self, Key};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Callbacks for fireworks overlay dismissal.
pub trait CodexResetFireworksCallbacks {
	fn on_close(&self);
	fn request_render(&self) {}
}

/// Fireworks overlay component shown on Codex quota-reset events.
///
/// Reference: `CodexResetFireworksComponent` in `l/src/overlays/codex-reset-fireworks.ts`.
///
/// Deviations: overlay handle/timers and terminal animation scheduling are host-
/// driven; this component advances frames via `tick()`/`handle_input` and renders
/// glyph frames plus native rows description.
pub struct CodexResetFireworksComponent<C: CodexResetFireworksCallbacks> {
	pub animation: FireworksAnimation,
	callbacks: C,
}

impl<C: CodexResetFireworksCallbacks> std::fmt::Debug for CodexResetFireworksComponent<C> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("CodexResetFireworksComponent")
			.field("frame", &self.animation.frame)
			.field("finished", &self.animation.finished)
			.finish_non_exhaustive()
	}
}

impl<C: CodexResetFireworksCallbacks> CodexResetFireworksComponent<C> {
	#[must_use]
	pub fn new(event: CodexResetFireworksEvent, callbacks: C) -> Self {
		Self {
			animation: FireworksAnimation::new(event),
			callbacks,
		}
	}

	#[must_use]
	pub fn finished(&self) -> bool {
		self.animation.finished
	}

	pub fn tick(&mut self) {
		self.animation.tick();
		if self.animation.finished {
			self.callbacks.request_render();
		}
	}
}

impl<C: CodexResetFireworksCallbacks> Component for CodexResetFireworksComponent<C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.animation.render(width.max(20) as usize, 10)
	}

	fn handle_input(&mut self, key: &Key) {
		if keys::is_escape(key) || matches!(key, Key::Enter) || self.animation.finished {
			self.callbacks.on_close();
		} else {
			self.tick();
			self.callbacks.request_render();
		}
	}

	fn invalidate(&mut self) {}

	fn dispose(&mut self) {
		self.animation.finished = true;
	}
}

impl<C: CodexResetFireworksCallbacks> NativeDescribe for CodexResetFireworksComponent<C> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.animation.render(cx.cols.max(20) as usize, 10);
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("frame", serde_json::Value::from(self.animation.frame as i64));
		props.insert("finished", serde_json::Value::Bool(self.animation.finished));
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

	struct Cb { closed: RefCell<bool> }
	impl CodexResetFireworksCallbacks for Cb {
		fn on_close(&self) { *self.closed.borrow_mut() = true; }
		fn request_render(&self) {}
	}

	#[test]
	fn codex_reset_fireworks_component_renders_ticks_and_closes() {
		let mut c = CodexResetFireworksComponent::new(
			CodexResetFireworksEvent::UnscheduledWeeklyReset,
			Cb { closed: RefCell::new(false) },
		);
		let lines = c.render(50);
		assert!(!lines.is_empty());
		c.tick();
		assert!(c.animation.frame >= 1);
		c.handle_input(&Key::Esc);
		assert!(*c.callbacks.closed.borrow());
		let node = c.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
