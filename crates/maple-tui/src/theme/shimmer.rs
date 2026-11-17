//! Shimmer sweep engine: three-tier intensity band over text, in classic
//! sweep or K.I.T.T. scanner mode.
//!
//! Reference: `l/src/theme/shimmer.ts`.
//!
//! Deviations:
//! - The clock is a parameter ([`shimmer_segments_at`] takes seconds); the
//!   runtime entry points read [`SystemTime`] once.
//! - The TS palette cache keyed by theme identity is omitted: Rust compiles
//!   three ANSI pairs per segment per frame, which is cheaper than the cache's
//!   bookkeeping.
//! - No TSP `native` rendering branch and no `describeShimmer` node builder;
//!   [`describe_shimmer`] returns the host-neutral description the terminal
//!   layer would serialize.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::theme::color::FG_RESET;
use crate::theme::theme_class::Theme;

/// Band/head travel speed in cells per second.
const SHIMMER_SPEED_CELLS_PER_S: f64 = 30.0;

const CLASSIC_PADDING: f64 = 10.0;
const CLASSIC_BAND_HALF_WIDTH: f64 = 6.0;

const KITT_HEAD_HALF: f64 = 0.6;
const KITT_TRAIL_LEN: f64 = 7.0;

const TIER_HIGH: f64 = 0.65;
const TIER_MID: f64 = 0.22;

const BOLD_OPEN: &str = "\x1b[1m";
const BOLD_CLOSE: &str = "\x1b[22m";

/// Sweep style for animated shimmer text; `Disabled` renders every tier as the
/// mid color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShimmerMode {
	/// Cosine bump sweeping left → right with edge padding.
	Classic,
	/// Knight Rider ping-pong head with a decaying trail.
	Kitt,
	/// No animation; all text uses the mid tier.
	Disabled,
}

impl ShimmerMode {
	fn code(self) -> u8 {
		match self {
			ShimmerMode::Classic => 0,
			ShimmerMode::Kitt => 1,
			ShimmerMode::Disabled => 2,
		}
	}

	fn from_code(code: u8) -> Self {
		match code {
			1 => ShimmerMode::Kitt,
			2 => ShimmerMode::Disabled,
			_ => ShimmerMode::Classic,
		}
	}
}

static ACTIVE_MODE: AtomicU8 = AtomicU8::new(0);

/// Select the shimmer sweep style. The host pushes its `display.shimmer`
/// preference here.
pub fn set_shimmer_mode(mode: ShimmerMode) {
	ACTIVE_MODE.store(mode.code(), Ordering::Relaxed);
}

/// The active shimmer sweep style.
pub fn shimmer_mode() -> ShimmerMode {
	ShimmerMode::from_code(ACTIVE_MODE.load(Ordering::Relaxed))
}

/// Whether shimmer animations are active (any mode other than `Disabled`).
pub fn shimmer_enabled() -> bool {
	shimmer_mode() != ShimmerMode::Disabled
}

/// A palette tier: a theme color token or a raw ANSI sequence.
#[derive(Clone, Debug, PartialEq)]
pub enum ShimmerTier {
	/// Theme color token, resolved against the theme at render time.
	Token(&'static str),
	/// Raw SGR foreground sequence.
	Ansi(String),
}

impl ShimmerTier {
	fn ansi(&self, theme: &Theme) -> String {
		match self {
			ShimmerTier::Token(token) => theme.get_fg_ansi(*token).unwrap_or_default(),
			ShimmerTier::Ansi(ansi) => ansi.clone(),
		}
	}
}

/// Three-tier color stack a shimmer character cycles through as the band
/// sweeps.
#[derive(Clone, Debug, PartialEq)]
pub struct ShimmerPalette {
	/// Color for chars outside / at the edge of the band (intensity < ~0.22).
	pub low: ShimmerTier,
	/// Color for chars approaching the crest (~0.22 ≤ intensity < ~0.65).
	pub mid: ShimmerTier,
	/// Color at the band's crest (intensity ≥ ~0.65).
	pub high: ShimmerTier,
	/// Whether to bold the crest tier.
	pub bold: bool,
}

impl Default for ShimmerPalette {
	fn default() -> Self {
		DEFAULT_SHIMMER_PALETTE.clone()
	}
}

/// One run of text that shares a palette inside a larger shimmer sweep.
#[derive(Clone, Copy, Debug)]
pub struct ShimmerSegment<'a> {
	/// The text run.
	pub text: &'a str,
	/// Palette for this run; `None` uses the default palette.
	pub palette: Option<&'a ShimmerPalette>,
}

impl<'a> ShimmerSegment<'a> {
	/// A segment using this run's own palette.
	pub fn new(text: &'a str, palette: &'a ShimmerPalette) -> Self {
		Self {
			text,
			palette: Some(palette),
		}
	}

	/// A segment using the default palette.
	pub fn plain(text: &'a str) -> Self {
		Self {
			text,
			palette: None,
		}
	}

	fn palette(&self) -> &ShimmerPalette {
		self.palette.unwrap_or(&DEFAULT_SHIMMER_PALETTE)
	}
}

/// The default three-tier palette: `dim` → `muted` → bold `accent`.
pub static DEFAULT_SHIMMER_PALETTE: ShimmerPalette = ShimmerPalette {
	low: ShimmerTier::Token("dim"),
	mid: ShimmerTier::Token("muted"),
	high: ShimmerTier::Token("accent"),
	bold: true,
};

/// One described span: a text run plus its mid-tier color token, or none when
/// the run's mid tier is raw ANSI (the terminal theme decides).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShimmerSpan {
	/// The text run.
	pub text: String,
	/// Theme color token for the run, when the mid tier is a token.
	pub style: Option<String>,
}

/// Host-neutral form of the TS `shimmer` native node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShimmerDescription {
	/// Text runs, in order.
	pub spans: Vec<ShimmerSpan>,
	/// Active sweep style.
	pub mode: ShimmerMode,
	/// Palette tokens the terminal should animate with; raw ANSI tiers are
	/// omitted.
	pub low: Option<String>,
	/// Mid-tier token.
	pub mid: Option<String>,
	/// Crest-tier token.
	pub high: Option<String>,
}

fn token(tier: &ShimmerTier) -> Option<String> {
	match tier {
		ShimmerTier::Token(name) => Some((*name).to_string()),
		ShimmerTier::Ansi(_) => None,
	}
}

/// Describe shimmering segments for a terminal that clocks the sweep itself.
pub fn describe_shimmer(segments: &[ShimmerSegment]) -> ShimmerDescription {
	let spans: Vec<ShimmerSpan> = segments
		.iter()
		.filter(|segment| !segment.text.is_empty())
		.map(|segment| ShimmerSpan {
			text: segment.text.to_string(),
			style: token(&segment.palette().mid),
		})
		.collect();
	let palette = segments
		.first()
		.map_or(&DEFAULT_SHIMMER_PALETTE, |segment| segment.palette());
	ShimmerDescription {
		spans,
		mode: shimmer_mode(),
		low: token(&palette.low),
		mid: token(&palette.mid),
		high: token(&palette.high),
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tier {
	Low,
	Mid,
	High,
}

struct TierSeq {
	open: String,
	close: String,
}

struct CompiledPalette {
	low: TierSeq,
	mid: TierSeq,
	high: TierSeq,
}

fn compile(theme: &Theme, palette: &ShimmerPalette) -> CompiledPalette {
	let low = palette.low.ansi(theme);
	let mid = palette.mid.ansi(theme);
	let high_color = palette.high.ansi(theme);
	let (high_open, high_close) = if palette.bold {
		(format!("{BOLD_OPEN}{high_color}"), format!("{BOLD_CLOSE}{FG_RESET}"))
	} else {
		(high_color, FG_RESET.to_string())
	};
	CompiledPalette {
		low: TierSeq {
			open: low,
			close: FG_RESET.to_string(),
		},
		mid: TierSeq {
			open: mid,
			close: FG_RESET.to_string(),
		},
		high: TierSeq {
			open: high_open,
			close: high_close,
		},
	}
}

impl CompiledPalette {
	fn seq(&self, tier: Tier) -> &TierSeq {
		match tier {
			Tier::Low => &self.low,
			Tier::Mid => &self.mid,
			Tier::High => &self.high,
		}
	}
}

/// Smooth cosine bump sweeping left → right with edge padding.
fn classic_intensity(index: f64, position: f64) -> f64 {
	let dist = (index + CLASSIC_PADDING - position).abs();
	if dist >= CLASSIC_BAND_HALF_WIDTH {
		return 0.0;
	}
	0.5 * (1.0 + (std::f64::consts::PI * dist / CLASSIC_BAND_HALF_WIDTH).cos())
}

/// Knight Rider scanner: a single bright head ping-pongs with a quadratic
/// trailing decay. No leading glow.
fn kitt_intensity(index: f64, head: f64, going_right: bool) -> f64 {
	let delta = index - head;
	if delta.abs() <= KITT_HEAD_HALF {
		return 1.0;
	}
	let behind = if going_right { -delta } else { delta };
	if behind <= KITT_HEAD_HALF {
		return 0.0;
	}
	let t = (behind - KITT_HEAD_HALF) / KITT_TRAIL_LEN;
	if t >= 1.0 {
		return 0.0;
	}
	let f = 1.0 - t;
	f * f
}

fn tier_for(intensity: f64) -> Tier {
	if intensity >= TIER_HIGH {
		Tier::High
	} else if intensity >= TIER_MID {
		Tier::Mid
	} else {
		Tier::Low
	}
}

/// Apply a shimmer sweep across segments, treating them as one continuous
/// string for band positioning. Each segment can supply its own palette.
pub fn shimmer_segments(segments: &[ShimmerSegment], theme: &Theme) -> String {
	shimmer_segments_at(segments, theme, now_secs())
}

/// [`shimmer_segments`] with an injected clock, so frames are reproducible.
pub fn shimmer_segments_at(segments: &[ShimmerSegment], theme: &Theme, now_secs: f64) -> String {
	let mode = shimmer_mode();

	// Disabled: no animation or code-point scan. Preserve the all-empty result,
	// but include empty segments' ANSI pairs when any segment has text.
	if mode == ShimmerMode::Disabled {
		if segments.iter().all(|segment| segment.text.is_empty()) {
			return String::new();
		}
		let mut out = String::new();
		for segment in segments {
			let seq = compile(theme, segment.palette());
			out.push_str(&seq.mid.open);
			out.push_str(segment.text);
			out.push_str(&seq.mid.close);
		}
		return out;
	}

	let total: usize = segments
		.iter()
		.map(|segment| segment.text.chars().count())
		.sum();
	if total == 0 {
		return String::new();
	}
	let total = total as f64;

	// Fixed velocity keeps the per-frame step independent of message length.
	let travel = now_secs * SHIMMER_SPEED_CELLS_PER_S;
	let is_kitt = mode == ShimmerMode::Kitt;
	let mut going_right = true;
	let (position, band_lo, band_hi) = if is_kitt {
		let range = total - 1.0;
		if range <= 0.0 {
			(0.0, -KITT_HEAD_HALF - KITT_TRAIL_LEN, KITT_HEAD_HALF)
		} else {
			let cycle_cells = 2.0 * range;
			let sweep = travel % cycle_cells;
			going_right = sweep < range;
			let position = if going_right { sweep } else { cycle_cells - sweep };
			let (lo, hi) = if going_right {
				(
					position - KITT_HEAD_HALF - KITT_TRAIL_LEN,
					position + KITT_HEAD_HALF,
				)
			} else {
				(
					position - KITT_HEAD_HALF,
					position + KITT_HEAD_HALF + KITT_TRAIL_LEN,
				)
			};
			(position, lo, hi)
		}
	} else {
		let position = travel % (total + CLASSIC_PADDING * 2.0);
		(
			position,
			position - CLASSIC_PADDING - CLASSIC_BAND_HALF_WIDTH,
			position - CLASSIC_PADDING + CLASSIC_BAND_HALF_WIDTH,
		)
	};

	let mut out = String::new();
	let mut index: f64 = 0.0;
	for segment in segments {
		let compiled = compile(theme, segment.palette());
		let mut run_tier: Option<Tier> = None;
		let mut run_start = 0;
		let mut run_end = 0;
		for (offset, _) in segment.text.char_indices() {
			let tier = if index < band_lo || index > band_hi {
				Tier::Low
			} else if is_kitt {
				tier_for(kitt_intensity(index, position, going_right))
			} else {
				tier_for(classic_intensity(index, position))
			};
			if run_tier != Some(tier) {
				if let Some(previous) = run_tier {
					if run_end > run_start {
						let seq = compiled.seq(previous);
						out.push_str(&seq.open);
						out.push_str(&segment.text[run_start..run_end]);
						out.push_str(&seq.close);
					}
				}
				run_tier = Some(tier);
				run_start = offset;
			}
			run_end = offset + segment.text[offset..].chars().next().map_or(0, char::len_utf8);
			index += 1.0;
		}
		if let Some(tier) = run_tier {
			if run_end > run_start {
				let seq = compiled.seq(tier);
				out.push_str(&seq.open);
				out.push_str(&segment.text[run_start..run_end]);
				out.push_str(&seq.close);
			}
		}
	}
	out
}

/// Apply a shimmer sweep to a single run.
pub fn shimmer_text(text: &str, theme: &Theme, palette: Option<&ShimmerPalette>) -> String {
	shimmer_segments(&[ShimmerSegment { text, palette }], theme)
}

fn now_secs() -> f64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::default_theme;

	/// Position the classic band crest exactly at `index`.
	fn now_for_index(index: usize) -> f64 {
		(index as f64 + CLASSIC_PADDING) / SHIMMER_SPEED_CELLS_PER_S
	}

	/// The active mode is process-global; serialize tests that change it.
	static MODE_LOCK: Mutex<()> = Mutex::new(());

	#[test]
	fn mode_round_trips() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Kitt);
		assert_eq!(shimmer_mode(), ShimmerMode::Kitt);
		assert!(shimmer_enabled());
		set_shimmer_mode(ShimmerMode::Disabled);
		assert!(!shimmer_enabled());
		set_shimmer_mode(ShimmerMode::Classic);
		assert!(shimmer_enabled());
	}

	#[test]
	fn intensity_profiles_peak_at_the_band() {
		assert!((classic_intensity(10.0, 20.0) - 1.0).abs() < 1e-9);
		assert!(classic_intensity(10.0, 20.0 + CLASSIC_BAND_HALF_WIDTH) <= 1e-9);
		assert!(classic_intensity(10.0, 60.0) == 0.0);
		assert!((kitt_intensity(10.0, 10.4, true) - 1.0).abs() < 1e-9);
		// Behind the right-moving head lights up, ahead of it does not.
		assert!(kitt_intensity(8.0, 10.0, true) > 0.0);
		assert_eq!(kitt_intensity(12.0, 10.0, true), 0.0);
		// Mirror image when moving left.
		assert!(kitt_intensity(12.0, 10.0, false) > 0.0);
		assert_eq!(kitt_intensity(8.0, 10.0, false), 0.0);
		// Trail decays to zero.
		assert_eq!(kitt_intensity(10.0 - KITT_HEAD_HALF - KITT_TRAIL_LEN, 10.0, true), 0.0);
	}

	#[test]
	fn tier_thresholds_match_reference() {
		assert_eq!(tier_for(TIER_HIGH), Tier::High);
		assert_eq!(tier_for(TIER_MID), Tier::Mid);
		assert_eq!(tier_for(0.0), Tier::Low);
	}

	#[test]
	fn classic_sweep_marks_the_crest_bold() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Classic);
		let theme = default_theme();
		let palette = ShimmerPalette::default();
		let text = "hello shimmer";
		let out = shimmer_segments_at(&[ShimmerSegment::new(text, &palette)], &theme, now_for_index(3));
		let accent = theme.get_fg_ansi("accent").unwrap();
		assert!(out.contains(&format!("{BOLD_OPEN}{accent}")));
		assert!(out.contains("hello"));
		assert!(out.contains("shimmer"));
		assert!(out.ends_with(FG_RESET));
		// Long prefix far from the band stays in the low tier.
		let long = "0123456789abcdefghijklmnopqrstuvwxyz";
		let out = shimmer_segments_at(&[ShimmerSegment::new(long, &palette)], &theme, now_for_index(30));
		let dim = theme.get_fg_ansi("dim").unwrap();
		assert!(out.starts_with(&format!("{dim}0123")));
	}

	#[test]
	fn kitt_mode_animates_the_described_node() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Kitt);
		assert_eq!(describe_shimmer(&[ShimmerSegment::plain("ab")]).mode, ShimmerMode::Kitt);
		set_shimmer_mode(ShimmerMode::Classic);
	}

	#[test]
	fn disabled_mode_uses_the_mid_tier_for_every_segment() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Disabled);
		let theme = default_theme();
		let out = shimmer_segments_at(&[ShimmerSegment::plain("ab"), ShimmerSegment::plain("")], &theme, 1.0);
		let muted = theme.get_fg_ansi("muted").unwrap();
		assert_eq!(
			out,
			format!("{muted}ab{FG_RESET}{muted}{FG_RESET}")
		);
		assert_eq!(
			shimmer_segments_at(&[ShimmerSegment::plain("")], &theme, 1.0),
			""
		);
		set_shimmer_mode(ShimmerMode::Classic);
	}

	#[test]
	fn empty_and_codepoint_positions_are_stable() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Classic);
		let theme = default_theme();
		assert_eq!(shimmer_segments_at(&[], &theme, 0.0), "");
		assert_eq!(
			shimmer_segments_at(&[ShimmerSegment::plain("")], &theme, 0.0),
			""
		);
		let palette = ShimmerPalette::default();
		let out = shimmer_segments_at(&[ShimmerSegment::new("a😀b", &palette)], &theme, now_for_index(1));
		assert!(out.contains("😀"));
		assert!(out.matches(FG_RESET).count() >= 1);
	}

	#[test]
	fn segments_share_one_continuous_band() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Classic);
		let theme = default_theme();
		let palette = ShimmerPalette::default();
		let segments = [
			ShimmerSegment::new("abc", &palette),
			ShimmerSegment::new("def", &palette),
		];
		let joined = shimmer_segments_at(&segments, &theme, now_for_index(2));
		let single = shimmer_segments_at(&[ShimmerSegment::new("abcdef", &palette)], &theme, now_for_index(2));
		assert_eq!(joined, single);
	}

	#[test]
	fn describe_reports_tokens_and_drops_raw_ansi() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Classic);
		let custom = ShimmerPalette {
			low: ShimmerTier::Ansi("\x1b[31m".to_string()),
			mid: ShimmerTier::Token("muted"),
			high: ShimmerTier::Token("accent"),
			bold: false,
		};
		let description = describe_shimmer(&[
			ShimmerSegment::new("ab", &custom),
			ShimmerSegment::plain(""),
			ShimmerSegment::plain("cd"),
		]);
		assert_eq!(description.mode, ShimmerMode::Classic);
		assert_eq!(description.spans.len(), 2);
		assert_eq!(description.spans[0].text, "ab");
		assert_eq!(description.spans[0].style.as_deref(), Some("muted"));
		assert_eq!(description.spans[1].text, "cd");
		assert_eq!(description.low, None);
		assert_eq!(description.high.as_deref(), Some("accent"));
	}

	#[test]
	fn shimmer_text_renders_a_single_run() {
		let _guard = MODE_LOCK.lock().unwrap();
		set_shimmer_mode(ShimmerMode::Classic);
		let theme = default_theme();
		let out = shimmer_text("abc", &theme, None);
		assert_eq!(strip_ansi(&out), "abc");
		assert!(out.ends_with(FG_RESET));
	}

	fn strip_ansi(text: &str) -> String {
		let mut out = String::new();
		let mut chars = text.chars();
		while let Some(c) = chars.next() {
			if c == '\u{1b}' {
				for next in chars.by_ref() {
					if next == 'm' {
						break;
					}
				}
			} else {
				out.push(c);
			}
		}
		out
	}
}
