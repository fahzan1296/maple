//! Live call visualizer panel.
//!
//! Reference: `l/src/apps/live-visualizer.ts`.
//!
//! Deviations:
//! - TS component depends on crate/TS TUI types (`Component`, `OverlayPanel`,
//!   `PanelRows`, native describe/memo, theme, key matching, mouse/native
//!   events) that are not 1:1 here. This port provides a faithful state/input
//!   model and ANSI render logic as [`LiveVisualizerModel`] plus
//!   [`LiveVisualizerOptions`] callbacks.
//! - `matchesKey` string semantics approximated by direct string/key-name
//!   checks used in the reference (`escape`, `ctrl+c`, `space`, stop keys).
//! - Spectrum animation uses the same block set and peak-decay math; frame
//!   ticks are explicit via [`LiveVisualizerModel::set_frame`].
//! - Native description is omitted (no native node builder deps in this file);
//!   render output targets the ANSI panel path.
//! - [`LiveVisualizerModel::render`] is the compact three-row meter view;
//!   [`LiveVisualizerModel::render_panel`] mirrors the reference's bordered
//!   call panel (top border, two spectrum rows, transcript, footer).
//! - Theme coloring of the spectrum/transcript is omitted; glyphs and layout
//!   match the reference.

use crate::key_hint_format::KeyHintFormatter;
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};
use crate::theme::Theme;


/// Distinct states of a realtime call connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivePhase {
	Connecting,
	Listening,
	Working,
	Speaking,
	Muted,
	Error,
}

impl LivePhase {
	pub fn icon(self) -> &'static str {
		match self {
			LivePhase::Connecting => "○",
			LivePhase::Listening => "●",
			LivePhase::Working => "○",
			LivePhase::Speaking => "»",
			LivePhase::Muted => "×",
			LivePhase::Error => "!",
		}
	}
	pub fn label(self) -> &'static str {
		match self {
			LivePhase::Connecting => "connecting",
			LivePhase::Listening => "listening",
			LivePhase::Working => "working",
			LivePhase::Speaking => "speaking",
			LivePhase::Muted => "muted",
			LivePhase::Error => "error",
		}
	}
}

const SPECTRUM_BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const METER_STEPS: f64 = 20.0;

/// Configuration callbacks for user interactions in the visualizer.
pub trait LiveVisualizerOptions {
	fn on_stop(&mut self);
	fn on_toggle_mute(&mut self);
	/// Configured stop chords (e.g. `ctrl+l`).
	fn stop_keys(&self) -> &[String] {
		&[]
	}
}

fn sanitize_single_line(text: &str) -> String {
	text.chars()
		.map(|c| if c.is_control() && c != ' ' { ' ' } else { c })
		.collect::<String>()
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// A compact, fixed-height terminal model for displaying a realtime call.
pub struct LiveVisualizerModel<O> {
	options: O,
	phase: LivePhase,
	input_level: f64,
	display_level: f64,
	frame: u64,
	user_transcript: String,
	cache_key: Option<(usize, LivePhase, u64, String)>,
	cache_lines: Vec<String>,
}

impl<O: LiveVisualizerOptions> LiveVisualizerModel<O> {
	pub fn new(options: O) -> Self {
		Self {
			options,
			phase: LivePhase::Connecting,
			input_level: 0.0,
			display_level: 0.0,
			frame: 0,
			user_transcript: String::new(),
			cache_key: None,
			cache_lines: Vec::new(),
		}
	}

	pub fn phase(&self) -> LivePhase {
		self.phase
	}
	pub fn transcript(&self) -> &str {
		&self.user_transcript
	}
	pub fn input_level(&self) -> f64 {
		self.input_level
	}
	pub fn display_level(&self) -> f64 {
		self.display_level
	}
	pub fn options_mut(&mut self) -> &mut O {
		&mut self.options
	}

	/// Updates the current call phase.
	pub fn set_phase(&mut self, phase: LivePhase) {
		if self.phase != phase {
			self.phase = phase;
			self.invalidate();
		}
	}

	/// Updates the microphone volume level (0..1).
	pub fn set_input_level(&mut self, level: f64) {
		let next = if level.is_finite() {
			level.clamp(0.0, 1.0)
		} else {
			0.0
		};
		if (self.input_level == next) {
			return;
		}
		self.input_level = next;
		if next > self.display_level {
			self.display_level = next;
		}
		self.invalidate();
	}

	/// Advances the spectrum animation and its peak decay.
	pub fn set_frame(&mut self, frame: u64) {
		let next_level = self.input_level.max(self.display_level * 0.84);
		if self.frame != frame || (self.display_level - next_level).abs() > f64::EPSILON {
			self.frame = frame;
			self.display_level = next_level;
			self.invalidate();
		}
	}

	/// Updates the user's streaming voice transcript.
	pub fn set_transcript(&mut self, text: &str) {
		let normalized = sanitize_single_line(text);
		if self.user_transcript != normalized {
			self.user_transcript = normalized;
			self.invalidate();
		}
	}

	/// Clears the user's voice transcript row.
	pub fn clear_transcript(&mut self) {
		if !self.user_transcript.is_empty() {
			self.user_transcript.clear();
			self.invalidate();
		}
	}

	/// Processes user keypresses.
	pub fn handle_input(&mut self, data: &str) {
		if data == "escape" || data == "ctrl+c" || self.options.stop_keys().iter().any(|k| k == data) {
			self.options.on_stop();
		} else if data == "space" {
			self.options.on_toggle_mute();
		}
	}

	pub fn invalidate(&mut self) {
		self.cache_key = None;
		self.cache_lines.clear();
	}

	fn spectrum(&self, width: usize) -> String {
		let w = width.max(1);
		let mut s = String::with_capacity(w);
		let energy = if self.phase == LivePhase::Muted {
			0.0
		} else {
			(self.display_level * 5.0).sqrt().min(1.0)
		};
		for i in 0..w {
			let phase = (self.frame as f64 * 0.35) + (i as f64 * 0.7);
			let wave = (phase.sin() * 0.5 + 0.5) * 0.6
				+ ((phase * 0.37 + 1.3).sin() * 0.5 + 0.5) * 0.4;
			let level = (energy * wave * 8.0).round().clamp(0.0, 8.0) as usize;
			s.push(SPECTRUM_BLOCKS[level]);
		}
		s
	}

	/// Renders the microphone spectrum into a compact panel.
	pub fn render(&mut self, width: usize) -> Vec<String> {
		let w = width.max(10);
		let key = (w, self.phase, self.frame, self.user_transcript.clone());
		if self.cache_key.as_ref() == Some(&key) {
			return self.cache_lines.clone();
		}
		let icon = self.phase.icon();
		let label = self.phase.label();
		let status = format!("{icon} {label}");
		let meter = self.spectrum(w.saturating_sub(4));
		let transcript = if self.user_transcript.is_empty() {
			String::new()
		} else if self.user_transcript.chars().count() > w.saturating_sub(2) {
			let take = w.saturating_sub(3);
			format!("…{}", self.user_transcript.chars().rev().take(take).collect::<String>().chars().rev().collect::<String>())
		} else {
			self.user_transcript.clone()
		};
		let lines = vec![status, meter, transcript];
		self.cache_key = Some(key);
		self.cache_lines = lines.clone();
		lines
	}

	/// Two-row spectrum, matching the reference's panel math.
	fn spectrum_rows(&self, width: usize, rows: usize) -> Vec<String> {
		let mut out = vec![String::new(); rows];
		let energy = if self.phase == LivePhase::Muted {
			0.0
		} else {
			(self.display_level * 5.0).sqrt().min(1.0)
		};
		let max_height = rows * (SPECTRUM_BLOCKS.len() - 1);
		for column in 0..width {
			let carrier = 0.5 + 0.5 * ((self.frame as f64 * 0.43) + (column as f64 * 0.71)).sin();
			let shimmer = 0.5 + 0.5 * ((self.frame as f64 * 0.19) - (column as f64 * 1.17)).sin();
			let height = (energy * (0.3 + carrier * 0.5 + shimmer * 0.2) * max_height as f64).round()
				as usize;
			for row in 0..rows {
				let units = height
					.saturating_sub((rows - row - 1) * 8)
					.min(SPECTRUM_BLOCKS.len() - 1);
				out[row].push(SPECTRUM_BLOCKS[units]);
			}
		}
		out
	}

	/// Transcript clipped from the start, keeping the tail visible.
	fn transcript_row(&self, width: usize) -> String {
		if width == 0 {
			return String::new();
		}
		let text = &self.user_transcript;
		if visible_width(text) <= width {
			return text.clone();
		}
		if width == 1 {
			return "…".to_string();
		}
		let budget = width - 1;
		// Guard against an empty transcript: `visible_width("") <= width` above.
		let mut out = String::from("…");
		let mut used = 0usize;
		for ch in text.chars().rev() {
			let ch_width = visible_width(&ch.to_string());
			if used + ch_width > budget {
				break;
			}
			out.insert(1, ch);
			used += ch_width;
		}
		out
	}

	/// Bordered call panel: every row spans exactly `width` cells.
	pub fn render_panel(&mut self, theme: &Theme, width: usize) -> Vec<String> {
		if width < 4 {
			return self.render(width);
		}
		let box_ = theme.box_round();
		let content_width = width - 4;
		let spectrum = self.spectrum_rows(content_width, 2);
		let transcript = self.transcript_row(content_width);
		let mut lines = Vec::with_capacity(5);
		lines.push(format!(
			"{}{}{}",
			box_.top_left,
			box_.horizontal.repeat(width - 2),
			box_.top_right
		));
		for row in &spectrum {
			lines.push(format!(
				"{} {} {}",
				box_.vertical,
				pad_to_width(row, content_width),
				box_.vertical
			));
		}
		lines.push(format!(
			"{} {} {}",
			box_.vertical,
			pad_to_width(&transcript, content_width),
			box_.vertical
		));
		let icon = self.phase.icon();
		let status = format!("{icon} {}", self.phase.label());
		let hints = KeyHintFormatter::new(theme);
		let full_label = format!(
			" {} · {} mute · {} end ",
			status,
			hints.format_key_hint("space"),
			hints.format_key_hint("escape")
		);
		let short_label = format!(" {status} ");
		let inner_width = width - 2;
		let label = if inner_width >= visible_width(&full_label) + 1 {
			full_label
		} else if inner_width >= visible_width(&short_label) + 1 {
			short_label
		} else {
			String::new()
		};
		if label.is_empty() {
			lines.push(format!(
				"{}{}{}",
				box_.bottom_left,
				box_.horizontal.repeat(inner_width),
				box_.bottom_right
			));
		} else {
			let remaining = inner_width.saturating_sub(visible_width(&label) + 1);
			let label = truncate_to_width(&label, inner_width - 1, Ellipsis::Omit);
			lines.push(format!(
				"{}{}{}{}{}",
				box_.bottom_left,
				box_.horizontal,
				label,
				box_.horizontal.repeat(remaining),
				box_.bottom_right
			));
		}
		lines
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::Cell;
	use std::rc::Rc;

	struct Opts {
		stopped: Rc<Cell<u32>>,
		muted: Rc<Cell<u32>>,
		keys: Vec<String>,
	}
	impl LiveVisualizerOptions for Opts {
		fn on_stop(&mut self) { self.stopped.set(self.stopped.get() + 1); }
		fn on_toggle_mute(&mut self) { self.muted.set(self.muted.get() + 1); }
		fn stop_keys(&self) -> &[String] { &self.keys }
	}

	fn model(keys: Vec<String>) -> (LiveVisualizerModel<Opts>, Rc<Cell<u32>>, Rc<Cell<u32>>) {
		let stopped = Rc::new(Cell::new(0));
		let muted = Rc::new(Cell::new(0));
		let m = LiveVisualizerModel::new(Opts {
			stopped: stopped.clone(),
			muted: muted.clone(),
			keys,
		});
		(m, stopped, muted)
	}

	#[test]
	fn phase_icon_and_label() {
		assert_eq!(LivePhase::Listening.icon(), "●");
		assert_eq!(LivePhase::Error.label(), "error");
	}

	#[test]
	fn level_clamps_and_peak_tracks_input() {
		let (mut m, _, _) = model(vec![]);
		m.set_input_level(1.5);
		assert_eq!(m.input_level(), 1.0);
		assert_eq!(m.display_level(), 1.0);
		m.set_input_level(0.0);
		m.set_frame(1);
		assert!(m.display_level() < 1.0);
	}

	#[test]
	fn transcript_normalizes_whitespace() {
		let (mut m, _, _) = model(vec![]);
		m.set_transcript("  hello\n\t world  ");
		assert_eq!(m.transcript(), "hello world");
		m.clear_transcript();
		assert_eq!(m.transcript(), "");
	}

	#[test]
	fn handle_input_routes_stop_mute_and_custom_stop_key() {
		let (mut m, stopped, muted) = model(vec!["ctrl+l".into()]);
		m.handle_input("space");
		assert_eq!(muted.get(), 1);
		m.handle_input("escape");
		assert_eq!(stopped.get(), 1);
		m.handle_input("ctrl+l");
		assert_eq!(stopped.get(), 2);
	}

	#[test]
	fn render_has_three_lines_and_caches() {
		let (mut m, _, _) = model(vec![]);
		m.set_phase(LivePhase::Listening);
		m.set_frame(1);
		let a = m.render(30);
		let b = m.render(30);
		assert_eq!(a.len(), 3);
		assert!(a[0].contains("listening"));
		assert_eq!(a, b);
	}

	#[test]
	fn muted_phase_has_flat_spectrum() {
		let (mut m, _, _) = model(vec![]);
		m.set_phase(LivePhase::Muted);
		m.set_input_level(1.0);
		m.set_frame(5);
		let lines = m.render(20);
		assert!(lines[1].trim().is_empty());
	}
}
