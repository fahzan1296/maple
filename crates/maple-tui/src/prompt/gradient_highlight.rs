//! Reference: `l/src/prompt/gradient-highlight.ts`.
//!
//! A declarative gradient highlighter for editor keywords: every standalone
//! match of a word-bounded regex is painted with a smooth HSL gradient, using
//! only zero-width SGR escapes so the visible width is unchanged.
//!
//! Deviations:
//! - `Bun.color("hsl(…)", format)` is replaced by local HSL→RGB conversion plus
//!   truecolor/256 SGR formatting.
//! - The palette is resolved for an explicit [`ColorMode`] at construction
//!   instead of being memoized per active theme mode: Rust passes state
//!   explicitly (see [`crate::theme::color::ColorMode`]).
//! - Gradient positions count characters, not UTF-16 code units.

use std::sync::Arc;

use regex::Regex;

use crate::prompt::markdown_prose::mask_non_prose;
use crate::theme::color::{ColorMode, FG_RESET};

/// A gradient keyword highlighter.
///
/// `reset_to` is the SGR foreground sequence re-emitted after each painted
/// keyword so surrounding text keeps its color; it defaults to a plain
/// foreground reset. `phase` in `[0, 1)` rotates the gradient stops cyclically;
/// pass time-derived values to animate a shimmer, `0` for the static palette.
pub type KeywordHighlighter = Arc<dyn Fn(&str, &str, f64) -> String + Send + Sync>;

/// Declarative spec for [`create_gradient_highlighter`].
#[derive(Clone)]
pub struct GradientHighlightSpec {
	/// Literal substring whose absence skips the boundary regex on most lines.
	pub probe: String,
	/// Word-bounded match regex walked over the masked text.
	pub highlight: Regex,
	/// Number of color stops swept across the gradient.
	pub stops: usize,
	/// Maps a normalized position `t` in `[0, 1)` to an HSL hue in degrees.
	pub hue: Arc<dyn Fn(f64) -> f64 + Send + Sync>,
	/// HSL saturation percentage. Default 90.
	pub saturation: Option<f64>,
	/// Maps a normalized position `t` in `[0, 1)` to an HSL lightness
	/// percentage. Default: 62 everywhere.
	pub lightness: Option<Arc<dyn Fn(f64) -> f64 + Send + Sync>>,
}

impl GradientHighlightSpec {
	/// A spec with the reference defaults (`saturation` 90, `lightness` 62).
	pub fn new(
		probe: impl Into<String>,
		highlight: Regex,
		stops: usize,
		hue: impl Fn(f64) -> f64 + Send + Sync + 'static,
	) -> Self {
		Self {
			probe: probe.into(),
			highlight,
			stops,
			hue: Arc::new(hue),
			saturation: None,
			lightness: None,
		}
	}
}

/// HSL (hue degrees, saturation %, lightness %) to 8-bit RGB.
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (u8, u8, u8) {
	let h = hue.rem_euclid(360.0) / 360.0;
	let s = (saturation / 100.0).clamp(0.0, 1.0);
	let l = (lightness / 100.0).clamp(0.0, 1.0);
	if s == 0.0 {
		let value = (l * 255.0).round() as u8;
		return (value, value, value);
	}
	let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
	let p = 2.0 * l - q;
	let channel = |mut t: f64| -> u8 {
		if t < 0.0 {
			t += 1.0;
		}
		if t > 1.0 {
			t -= 1.0;
		}
		let value = if t < 1.0 / 6.0 {
			p + (q - p) * 6.0 * t
		} else if t < 0.5 {
			q
		} else if t < 2.0 / 3.0 {
			p + (q - p) * (2.0 / 3.0 - t) * 6.0
		} else {
			p
		};
		(value * 255.0).round().clamp(0.0, 255.0) as u8
	};
	(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

/// Nearest xterm-256 palette index for an RGB color.
fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
	const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
	let quantize = |value: u8| -> (u8, u16) {
		let mut best = 0u8;
		let mut best_distance = u16::MAX;
		for (index, level) in LEVELS.iter().enumerate() {
			let distance = (value as i16 - *level as i16).unsigned_abs();
			if distance < best_distance {
				best_distance = distance;
				best = index as u8;
			}
		}
		(best, best_distance)
	};
	let (ri, rd) = quantize(r);
	let (gi, gd) = quantize(g);
	let (bi, bd) = quantize(b);
	let cube_distance = rd as u32 + gd as u32 + bd as u32;
	let cube = 16 + 36 * ri as u16 + 6 * gi as u16 + bi as u16;

	// Grayscale ramp 232..=255 holds 8 + 10n.
	let average = (r as u16 + g as u16 + b as u16) / 3;
	let gray_index = ((average as i16 - 8).max(0) as u32 + 5) / 10;
	let gray_index = gray_index.min(23) as u16;
	let gray_value = (8 + 10 * gray_index) as i16;
	let gray_distance = (r as i16 - gray_value).unsigned_abs() as u32
		+ (g as i16 - gray_value).unsigned_abs() as u32
		+ (b as i16 - gray_value).unsigned_abs() as u32;

	if gray_distance < cube_distance {
		232 + gray_index as u8
	} else {
		cube as u8
	}
}

/// SGR foreground escape for a color, in the requested mode.
fn sgr_foreground(r: u8, g: u8, b: u8, mode: ColorMode) -> String {
	match mode {
		ColorMode::TrueColor => format!("\x1b[38;2;{r};{g};{b}m"),
		ColorMode::Color256 => format!("\x1b[38;5;{}m", rgb_to_ansi256(r, g, b)),
	}
}

/// A built highlighter: palette fixed for one color mode.
pub struct GradientHighlighter {
	probe: String,
	highlight: Regex,
	palette: Vec<String>,
}

impl GradientHighlighter {
	/// Paint `text` with the palette, resetting to [`FG_RESET`] after each match.
	pub fn highlight(&self, text: &str) -> String {
		self.highlight_with(text, FG_RESET, 0.0)
	}

	/// Paint `text`, restoring `reset_to` after each painted keyword.
	pub fn highlight_with(&self, text: &str, reset_to: &str, phase: f64) -> String {
		self.highlight_owned(text, reset_to, phase)
	}

	fn highlight_owned(&self, text: &str, reset_to: &str, phase: f64) -> String {
		if !text.contains(&self.probe) {
			return text.to_string();
		}
		// Wrap phase into [0, 1) so negative inputs and values >= 1 stay well-defined.
		let wrapped_phase = phase.rem_euclid(1.0);
		// Match against a code/markup-masked copy so keywords inside code spans,
		// fenced blocks, or XML sections never paint; indices still address `text`.
		let masked = mask_non_prose(text);
		let mut out = String::with_capacity(text.len());
		let mut last = 0usize;
		for found in self.highlight.find_iter(&masked) {
			out.push_str(&text[last..found.start()]);
			out.push_str(&self.paint(&text[found.start()..found.end()], reset_to, wrapped_phase));
			last = found.end();
		}
		out.push_str(&text[last..]);
		out
	}

	/// Paint each character of `word` with the next gradient stop, restoring
	/// `reset_to` after. `phase` cyclically rotates the palette index so
	/// successive renders with increasing phase shimmer; `0` is static.
	fn paint(&self, word: &str, reset_to: &str, phase: f64) -> String {
		let stops = &self.palette;
		let m = stops.len();
		if m == 0 {
			return word.to_string();
		}
		let chars: Vec<char> = word.chars().collect();
		let n = chars.len();
		let mut out = String::new();
		let mut prev = "";
		for (index, ch) in chars.iter().enumerate() {
			let t = (index as f64 / n as f64 + phase) % 1.0;
			let color = &stops[(t * m as f64).floor() as usize % m];
			// Coalesce consecutive characters that resolve to the same stop.
			if color != prev {
				out.push_str(color);
				prev = color;
			}
			out.push(*ch);
		}
		out.push_str(reset_to);
		out
	}
}

/// Build a stateless highlighter that paints each standalone match of the
/// spec's regex with a smooth HSL gradient for editor display.
pub fn create_gradient_highlighter(spec: GradientHighlightSpec, mode: ColorMode) -> GradientHighlighter {
	let saturation = spec.saturation.unwrap_or(90.0);
	let lightness = spec.lightness.clone();
	let palette = (0..spec.stops)
		.map(|index| {
			let t = index as f64 / spec.stops as f64;
			let hue = (spec.hue)(t).round();
			let light = lightness.as_ref().map(|f| f(t)).unwrap_or(62.0).round();
			let (r, g, b) = hsl_to_rgb(hue, saturation, light);
			sgr_foreground(r, g, b, mode)
		})
		.collect();
	GradientHighlighter {
		probe: spec.probe,
		highlight: spec.highlight,
		palette,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn highlighter() -> GradientHighlighter {
		create_gradient_highlighter(
			GradientHighlightSpec::new("think", Regex::new(r"\bthink\b").expect("regex"), 4, |t| 260.0 * t),
			ColorMode::TrueColor,
		)
	}

	#[test]
	fn leaves_text_without_probe_untouched() {
		let h = highlighter();
		assert_eq!(h.highlight("nothing here"), "nothing here");
	}

	#[test]
	fn paints_matches_and_restores_reset() {
		let h = highlighter();
		let painted = h.highlight("a think b");
		assert!(painted.starts_with("a \x1b[38;2;"));
		assert!(painted.ends_with("\x1b[39m b"));
		assert_eq!(crate::ansi_scan::strip_ansi(&painted), "a think b");
	}

	#[test]
	fn skips_matches_inside_code() {
		let h = highlighter();
		let painted = h.highlight("```\nthink\n```");
		assert_eq!(painted, "```\nthink\n```");
		assert!(!painted.contains("\x1b[38;2;"));
	}

	#[test]
	fn phases_wrap_into_unit_range() {
		let h = highlighter();
		let cases = [-0.5, 0.0, 0.25, 1.5];
		for phase in cases {
			let painted = h.highlight_with("think", FG_RESET, phase);
			assert_eq!(crate::ansi_scan::strip_ansi(&painted), "think");
			assert!(painted.contains("\x1b[38;2;"), "phase {phase}");
		}
	}

	#[test]
	fn palette_matches_color_mode() {
		let spec = GradientHighlightSpec::new("x", Regex::new("x").expect("regex"), 2, |_| 0.0);
		let truecolor = create_gradient_highlighter(spec.clone(), ColorMode::TrueColor);
		assert!(truecolor.highlight("x").contains("\x1b[38;2;"));
		let palette = create_gradient_highlighter(spec, ColorMode::Color256);
		assert!(palette.highlight("x").contains("\x1b[38;5;"));
	}

	#[test]
	fn hsl_and_palette_land_in_range() {
		let cases = [(0.0, 90.0, 50.0), (120.0, 0.0, 0.0), (240.0, 100.0, 100.0)];
		for (h, s, l) in cases {
			let (r, g, b) = hsl_to_rgb(h, s, l);
			assert!(rgb_to_ansi256(r, g, b) >= 16, "h {h} s {s} l {l}");
		}
		let (r, g, b) = hsl_to_rgb(0.0, 100.0, 50.0);
		assert_eq!((r, g, b), (255, 0, 0));
		let (r, g, b) = hsl_to_rgb(120.0, 100.0, 50.0);
		assert_eq!((r, g, b), (0, 255, 0));
	}
}
