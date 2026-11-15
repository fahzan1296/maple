//! Color values, ANSI conversion, variable resolution and color math.
//!
//! Ported from `l/src/theme/color.ts` plus the color helpers it re-exports from
//! `@oh-my-pi/pi-utils/color`.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

use crate::theme::schema::ColorMap;

/// SGR reset for the foreground color only, leaving other attributes intact.
pub const FG_RESET: &str = "\x1b[39m";
/// SGR reset for the background color only, leaving other attributes intact.
pub const BG_RESET: &str = "\x1b[49m";

/// Theme color depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorMode {
	/// 24-bit RGB passthrough.
	TrueColor,
	/// 256-color palette indices.
	Color256,
}

impl ColorMode {
	/// The JSON/config name (`"truecolor"` or `"256color"`).
	pub fn as_str(self) -> &'static str {
		match self {
			ColorMode::TrueColor => "truecolor",
			ColorMode::Color256 => "256color",
		}
	}
}

impl Default for ColorMode {
	fn default() -> Self {
		ColorMode::Color256
	}
}

/// A theme color value: a hex/var-ref string or a 256-color palette index.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorValue {
	/// 256-color palette index.
	Index(i64),
	/// Hex string (`#rgb`, `#rrggbb`, `#rrggbbaa`), `""` (terminal default), or
	/// a named `vars` reference.
	Hex(String),
}

impl ColorValue {
	/// The string form, when this is a [`ColorValue::Hex`].
	pub fn as_str(&self) -> Option<&str> {
		match self {
			ColorValue::Hex(s) => Some(s),
			ColorValue::Index(_) => None,
		}
	}

	/// The palette index, when this is a [`ColorValue::Index`].
	pub fn as_index(&self) -> Option<i64> {
		match self {
			ColorValue::Index(n) => Some(*n),
			ColorValue::Hex(_) => None,
		}
	}

	/// True for the empty string (terminal default color).
	pub fn is_empty(&self) -> bool {
		matches!(self, ColorValue::Hex(s) if s.is_empty())
	}
}

impl<'de> Deserialize<'de> for ColorValue {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		let value = serde_json::Value::deserialize(deserializer)?;
		match value {
			serde_json::Value::Number(n) => n
				.as_i64()
				.map(ColorValue::Index)
				.ok_or_else(|| serde::de::Error::custom("expected an integer palette index")),
			serde_json::Value::String(s) => Ok(ColorValue::Hex(s)),
			other => Err(serde::de::Error::custom(format!(
				"expected a string or number, found {}",
				other
			))),
		}
	}
}

/// Resolve theme color depth. Windows Terminal (`WT_SESSION`) gets truecolor;
/// everything else is treated as 256-color. No terminal probing is performed.
pub fn detect_color_mode() -> ColorMode {
	detect_color_mode_from_env(&std::env::vars().collect())
}

/// [`detect_color_mode`] against an explicit environment map (testable).
pub fn detect_color_mode_from_env(env: &HashMap<String, String>) -> ColorMode {
	match env.get("WT_SESSION") {
		Some(value) if !value.is_empty() => ColorMode::TrueColor,
		_ => ColorMode::Color256,
	}
}

/// Convert a theme color to foreground SGR at the requested depth.
///
/// Truecolor passes the hex through as `38;2;R;G;B`. The 256-color mode picks
/// the nearest xterm palette entry by squared RGB distance over the full
/// 256-entry palette (a close approximation of `Bun.color(..., "ansi-256")`).
pub fn color_to_ansi(color: &str, mode: ColorMode) -> Result<String, String> {
	let rgb = parse_hex(color)
		.ok_or_else(|| format!("Invalid color value: {}", color))?;
	match mode {
		ColorMode::TrueColor => Ok(format!("\x1b[38;2;{};{};{}m", rgb.0, rgb.1, rgb.2)),
		ColorMode::Color256 => Ok(format!("\x1b[38;5;{}m", nearest_palette_index(rgb))),
	}
}

/// Foreground SGR for a theme color value.
pub fn fg_ansi(color: &ColorValue, mode: ColorMode) -> Result<String, String> {
	match color {
		ColorValue::Hex(s) if s.is_empty() => Ok(FG_RESET.to_string()),
		ColorValue::Index(n) => Ok(format!("\x1b[38;5;{}m", n)),
		ColorValue::Hex(s) => color_to_ansi(s, mode),
	}
}

/// Background SGR for a theme color value.
pub fn bg_ansi(color: &ColorValue, mode: ColorMode) -> Result<String, String> {
	match color {
		ColorValue::Hex(s) if s.is_empty() => Ok(BG_RESET.to_string()),
		ColorValue::Index(n) => Ok(format!("\x1b[48;5;{}m", n)),
		ColorValue::Hex(s) => {
			let ansi = color_to_ansi(s, mode)?;
			Ok(ansi.replace("\x1b[38;", "\x1b[48;"))
		}
	}
}

/// Resolve `value` through `vars`; numbers, `""` and `#…` are returned as-is.
pub fn resolve_var_refs(value: &ColorValue, vars: &ColorMap) -> Result<ColorValue, String> {
	resolve_var_refs_inner(value, vars, &mut Vec::new())
}

fn resolve_var_refs_inner(
	value: &ColorValue,
	vars: &ColorMap,
	visited: &mut Vec<String>,
) -> Result<ColorValue, String> {
	let name = match value {
		ColorValue::Index(_) => return Ok(value.clone()),
		ColorValue::Hex(s) if s.is_empty() || s.starts_with('#') => return Ok(value.clone()),
		ColorValue::Hex(s) => s.clone(),
	};
	if visited.iter().any(|v| *v == name) {
		return Err(format!("Circular variable reference detected: {}", name));
	}
	let next = match vars.get(&name) {
		Some(next) => next.clone(),
		None => return Err(format!("Variable reference not found: {}", name)),
	};
	visited.push(name);
	resolve_var_refs_inner(&next, vars, visited)
}

/// Resolve every entry of `colors` through `vars`.
pub fn resolve_theme_colors(colors: &ColorMap, vars: &ColorMap) -> Result<ColorMap, String> {
	let mut resolved = ColorMap::new();
	for (key, value) in colors.iter() {
		let value = resolve_var_refs(value, vars)?;
		resolved.insert(key.to_string(), value);
	}
	Ok(resolved)
}

/// True when a `color_luma` value belongs to a light background; `None` reads
/// as dark.
pub fn is_light_luma(luma: Option<f64>) -> bool {
	matches!(luma, Some(l) if l > 0.5)
}

/// True when `surface` is a light background.
pub fn is_light_surface(surface: Option<&ColorValue>) -> bool {
	is_light_luma(surface.and_then(color_luma))
}

/// Resolve a theme color value to a CSS hex string. `""` becomes the guessed
/// terminal foreground for the theme's light/dark classification.
pub fn resolve_to_hex(value: &ColorValue, is_light: bool) -> Result<String, String> {
	match value {
		ColorValue::Index(n) => ansi256_to_hex(*n),
		ColorValue::Hex(s) if s.is_empty() => Ok(if is_light { "#000000" } else { "#e5e5e7" }.to_string()),
		ColorValue::Hex(s) => Ok(s.clone()),
	}
}

/// Blend two theme color values in sRGB: `t` = 0 gives `a`, 1 gives `b`.
pub fn mix_theme_colors(
	a: &ColorValue,
	b: &ColorValue,
	t: f64,
	is_light: bool,
) -> Result<String, String> {
	let hex_a = normalize_hex(&resolve_to_hex(a, is_light)?)?;
	let hex_b = normalize_hex(&resolve_to_hex(b, is_light)?)?;
	Ok(mix_hex(&hex_a, &hex_b, t))
}

/// Convert a 256-color index to a hex string.
///
/// Indices 0–15 use the conventional xterm base palette, 16–231 the 6×6×6
/// cube, and 232–255 the grayscale ramp.
pub fn ansi256_to_hex(index: i64) -> Result<String, String> {
	palette_to_rgb(index).map(rgb_to_hex).ok_or_else(|| format!("Invalid palette index: {}", index))
}

/// Conventional xterm RGB for the 16 base ANSI colors.
const ANSI_16: [(u8, u8, u8); 16] = [
	(0, 0, 0),
	(128, 0, 0),
	(0, 128, 0),
	(128, 128, 0),
	(0, 0, 128),
	(128, 0, 128),
	(0, 128, 128),
	(192, 192, 192),
	(128, 128, 128),
	(255, 0, 0),
	(0, 255, 0),
	(255, 255, 0),
	(0, 0, 255),
	(255, 0, 255),
	(0, 255, 255),
	(255, 255, 255),
];

/// The 6×6×6 color-cube channel steps.
const CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Parse a 256-color palette index (0–255) to RGB.
pub fn palette_to_rgb(index: i64) -> Option<(u8, u8, u8)> {
	if index < 0 || index > 255 {
		return None;
	}
	let index = index as u32;
	if index < 16 {
		return Some(ANSI_16[index as usize]);
	}
	if index < 232 {
		let n = index - 16;
		let step = |v: u32| CUBE_STEPS[(v % 6) as usize];
		return Some((step(n / 36), step(n / 6), step(n)));
	}
	let gray = 8 + (index - 232) * 10;
	Some((gray as u8, gray as u8, gray as u8))
}

/// Nearest xterm-256 palette index for an RGB triple (squared distance).
fn nearest_palette_index(rgb: (u8, u8, u8)) -> u8 {
	let (r, g, b) = (rgb.0 as i32, rgb.1 as i32, rgb.2 as i32);
	let mut best = 0u8;
	let mut best_distance = i32::MAX;
	for index in 0..=255u32 {
		let (pr, pg, pb) = palette_to_rgb(index as i64).expect("index in range");
		let dr = r - pr as i32;
		let dg = g - pg as i32;
		let db = b - pb as i32;
		let distance = dr * dr + dg * dg + db * db;
		if distance < best_distance {
			best_distance = distance;
			best = index as u8;
		}
	}
	best
}

/// Parse `#rgb`, `#rrggbb` or `#rrggbbaa` (case-insensitive) to RGB.
///
/// Byte-wise parsing keeps malformed multi-byte input from panicking on a
/// non-ASCII index; non-hex characters yield `None`.
pub fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
	let h = hex.strip_prefix('#')?;
	let bytes = h.as_bytes();
	if bytes.len() != 3 && bytes.len() != 6 && bytes.len() != 8 {
		return None;
	}
	if !bytes.iter().all(|byte| byte.is_ascii_hexdigit()) {
		return None;
	}
	let nibble = |index: usize| (bytes[index] as char).to_digit(16).map(|digit| digit as u8);
	if bytes.len() == 3 {
		let r = nibble(0)?;
		let g = nibble(1)?;
		let b = nibble(2)?;
		return Some((r * 17, g * 17, b * 17));
	}
	let pair = |index: usize| -> Option<u8> { Some(nibble(index * 2)? * 16 + nibble(index * 2 + 1)?) };
	Some((pair(0)?, pair(1)?, pair(2)?))
}

/// Normalize a resolved color to `#rrggbb`, rejecting malformed hex.
fn normalize_hex(value: &str) -> Result<String, String> {
	parse_hex(value)
		.map(rgb_to_hex)
		.ok_or_else(|| format!("Invalid color value: {}", value))
}

/// Convert RGB to a lowercase `#rrggbb` string.
pub fn rgb_to_hex(rgb: (u8, u8, u8)) -> String {
	format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// Linear sRGB blend of two hex colors: `t` = 0 gives `a`, 1 gives `b`.
pub fn mix_hex(a: &str, b: &str, t: f64) -> String {
	let (ca, cb) = match (parse_hex(a), parse_hex(b)) {
		(Some(ca), Some(cb)) => (ca, cb),
		_ => return String::new(),
	};
	let blend = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8;
	rgb_to_hex((blend(ca.0, cb.0), blend(ca.1, cb.1), blend(ca.2, cb.2)))
}

/// Parse a theme color value to RGB; `#rrggbbaa` is rejected (matches
/// `pi-utils/color.ts`, which only accepts lengths 4 and 7).
fn to_rgb(value: &ColorValue) -> Option<(u8, u8, u8)> {
	match value {
		ColorValue::Index(n) => palette_to_rgb(*n),
		ColorValue::Hex(s) => {
			if s.len() != 4 && s.len() != 7 {
				return None;
			}
			parse_hex(s)
		}
	}
}

/// Perceptual luma (gamma-encoded BT.709 weights over raw sRGB), 0..1.
pub fn color_luma(value: &ColorValue) -> Option<f64> {
	to_rgb(value).map(|(r, g, b)| (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0)
}

/// WCAG 2.x relative luminance (BT.709 weights over linearized sRGB), 0..1.
pub fn relative_luminance(value: &ColorValue) -> Option<f64> {
	to_rgb(value).map(|(r, g, b)| {
		0.2126 * linearize_channel(r) + 0.7152 * linearize_channel(g) + 0.0722 * linearize_channel(b)
	})
}

/// Gamma-decode a single 0..255 sRGB channel to linear 0..1.
pub fn linearize_channel(channel: u8) -> f64 {
	let c = channel as f64 / 255.0;
	if c <= 0.04045 {
		c / 12.92
	} else {
		((c + 0.055) / 1.055).powf(2.4)
	}
}

/// HSV adjustment: `h` is additive degrees, `s`/`v` are multipliers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HsvAdjustment {
	/// Hue shift in degrees (additive).
	pub h: Option<f64>,
	/// Saturation multiplier.
	pub s: Option<f64>,
	/// Value/brightness multiplier.
	pub v: Option<f64>,
}

/// Convert RGB to HSV (`h` 0–360, `s`/`v` 0–1).
pub fn rgb_to_hsv(rgb: (u8, u8, u8)) -> (f64, f64, f64) {
	let r = rgb.0 as f64 / 255.0;
	let g = rgb.1 as f64 / 255.0;
	let b = rgb.2 as f64 / 255.0;
	let max = r.max(g).max(b);
	let min = r.min(g).min(b);
	let d = max - min;
	let h = if d == 0.0 {
		0.0
	} else if max == r {
		((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
	} else if max == g {
		((b - r) / d + 2.0) / 6.0
	} else {
		((r - g) / d + 4.0) / 6.0
	};
	(h * 360.0, if max == 0.0 { 0.0 } else { d / max }, max)
}

/// Convert HSV (`h` 0–360, `s`/`v` 0–1) to RGB.
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
	let h = ((h % 360.0) + 360.0) % 360.0;
	let i = (h / 60.0).floor();
	let f = h / 60.0 - i;
	let p = v * (1.0 - s);
	let q = v * (1.0 - f * s);
	let t = v * (1.0 - (1.0 - f) * s);
	let (r, g, b) = match (i as i64).rem_euclid(6) {
		0 => (v, t, p),
		1 => (q, v, p),
		2 => (p, v, t),
		3 => (p, q, v),
		4 => (t, p, v),
		_ => (v, p, q),
	};
	(
		(r * 255.0).round().clamp(0.0, 255.0) as u8,
		(g * 255.0).round().clamp(0.0, 255.0) as u8,
		(b * 255.0).round().clamp(0.0, 255.0) as u8,
	)
}

/// Adjust HSV components of a hex color.
pub fn adjust_hsv(hex: &str, adj: HsvAdjustment) -> String {
	let Some(rgb) = parse_hex(hex) else {
		return hex.to_string();
	};
	let (mut h, mut s, mut v) = rgb_to_hsv(rgb);
	if let Some(degrees) = adj.h {
		h = (h + degrees) % 360.0;
		if h < 0.0 {
			h += 360.0;
		}
	}
	if let Some(multiplier) = adj.s {
		s = (s * multiplier).clamp(0.0, 1.0);
	}
	if let Some(multiplier) = adj.v {
		v = (v * multiplier).clamp(0.0, 1.0);
	}
	rgb_to_hex(hsv_to_rgb(h, s, v))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hex_parsing_round_trips() {
		assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
		assert_eq!(parse_hex("#FfFfFf"), Some((255, 255, 255)));
		assert_eq!(parse_hex("#123456"), Some((0x12, 0x34, 0x56)));
		assert_eq!(parse_hex("#12345678"), Some((0x12, 0x34, 0x56)));
		assert_eq!(parse_hex("123456"), None);
		assert_eq!(parse_hex("#12"), None);
		assert_eq!(parse_hex("#ggg"), None);
		assert_eq!(rgb_to_hex((0x12, 0x34, 0x56)), "#123456");
	}

	#[test]
	fn truecolor_passthrough() {
		assert_eq!(
			color_to_ansi("#123456", ColorMode::TrueColor).unwrap(),
			"\x1b[38;2;18;52;86m"
		);
		assert_eq!(
			color_to_ansi("#ffffff", ColorMode::TrueColor).unwrap(),
			"\x1b[38;2;255;255;255m"
		);
		assert!(color_to_ansi("accent", ColorMode::TrueColor).is_err());
	}

	#[test]
	fn ansi256_approximates() {
		assert_eq!(color_to_ansi("#000000", ColorMode::Color256).unwrap(), "\x1b[38;5;0m");
		assert_eq!(color_to_ansi("#ff0000", ColorMode::Color256).unwrap(), "\x1b[38;5;9m");
		assert_eq!(color_to_ansi("#ffffff", ColorMode::Color256).unwrap(), "\x1b[38;5;15m");
		assert_eq!(color_to_ansi("#00ff00", ColorMode::Color256).unwrap(), "\x1b[38;5;10m");
	}

	#[test]
	fn ansi256_palette_endpoints() {
		assert_eq!(ansi256_to_hex(0).unwrap(), "#000000");
		assert_eq!(ansi256_to_hex(15).unwrap(), "#ffffff");
		assert_eq!(ansi256_to_hex(16).unwrap(), "#000000");
		assert_eq!(ansi256_to_hex(231).unwrap(), "#ffffff");
		assert_eq!(ansi256_to_hex(244).unwrap(), "#808080");
		assert_eq!(ansi256_to_hex(255).unwrap(), "#eeeeee");
		assert!(ansi256_to_hex(256).is_err());
		assert!(ansi256_to_hex(-1).is_err());
	}

	#[test]
	fn fg_and_bg_ansi_wrap_palette_indices() {
		assert_eq!(fg_ansi(&ColorValue::Index(244), ColorMode::TrueColor).unwrap(), "\x1b[38;5;244m");
		assert_eq!(bg_ansi(&ColorValue::Index(244), ColorMode::TrueColor).unwrap(), "\x1b[48;5;244m");
		assert_eq!(fg_ansi(&ColorValue::Hex(String::new()), ColorMode::TrueColor).unwrap(), FG_RESET);
		assert_eq!(bg_ansi(&ColorValue::Hex(String::new()), ColorMode::TrueColor).unwrap(), BG_RESET);
		assert_eq!(
			bg_ansi(&ColorValue::Hex("#123456".into()), ColorMode::TrueColor).unwrap(),
			"\x1b[48;2;18;52;86m"
		);
	}

	#[test]
	fn var_resolution_handles_missing_and_circular() {
		let mut vars = ColorMap::new();
		vars.insert("accent".into(), ColorValue::Hex("#febc38".into()));
		vars.insert("a".into(), ColorValue::Hex("b".into()));
		vars.insert("b".into(), ColorValue::Hex("a".into()));
		assert_eq!(
			resolve_var_refs(&ColorValue::Hex("accent".into()), &vars).unwrap(),
			ColorValue::Hex("#febc38".into())
		);
		assert_eq!(
			resolve_var_refs(&ColorValue::Hex("#abc".into()), &vars).unwrap(),
			ColorValue::Hex("#abc".into())
		);
		assert_eq!(
			resolve_var_refs(&ColorValue::Hex(String::new()), &vars).unwrap(),
			ColorValue::Hex(String::new())
		);
		assert_eq!(
			resolve_var_refs(&ColorValue::Index(7), &vars).unwrap(),
			ColorValue::Index(7)
		);
		assert_eq!(
			resolve_var_refs(&ColorValue::Hex("missing".into()), &vars).unwrap_err(),
			"Variable reference not found: missing"
		);
		assert_eq!(
			resolve_var_refs(&ColorValue::Hex("a".into()), &vars).unwrap_err(),
			"Circular variable reference detected: a"
		);
	}

	#[test]
	fn resolve_theme_colors_resolves_each_entry() {
		let mut vars = ColorMap::new();
		vars.insert("brand".into(), ColorValue::Hex("#febc38".into()));
		let mut colors = ColorMap::new();
		colors.insert("accent".into(), ColorValue::Hex("brand".into()));
		colors.insert("text".into(), ColorValue::Hex(String::new()));
		let resolved = resolve_theme_colors(&colors, &vars).unwrap();
		assert_eq!(resolved.get("accent"), Some(&ColorValue::Hex("#febc38".into())));
		assert_eq!(resolved.get("text"), Some(&ColorValue::Hex(String::new())));
	}

	#[test]
	fn resolve_to_hex_maps_empty_to_classification() {
		assert_eq!(resolve_to_hex(&ColorValue::Hex(String::new()), false).unwrap(), "#e5e5e7");
		assert_eq!(resolve_to_hex(&ColorValue::Hex(String::new()), true).unwrap(), "#000000");
		assert_eq!(resolve_to_hex(&ColorValue::Index(244), false).unwrap(), "#808080");
		assert_eq!(resolve_to_hex(&ColorValue::Hex("#abc".into()), false).unwrap(), "#abc");
	}

	#[test]
	fn mix_ratio_is_exact() {
		assert_eq!(mix_hex("#000000", "#ffffff", 0.0), "#000000");
		assert_eq!(mix_hex("#000000", "#ffffff", 1.0), "#ffffff");
		assert_eq!(mix_hex("#000000", "#ffffff", 0.5), "#808080");
		assert_eq!(mix_hex("#000000", "#ff0000", 0.25), "#400000");
		assert_eq!(
			mix_theme_colors(
				&ColorValue::Hex("#000000".into()),
				&ColorValue::Hex("#ffffff".into()),
				0.5,
				false
			)
			.unwrap(),
			"#808080"
		);
		assert!(mix_theme_colors(
			&ColorValue::Hex("nope".into()),
			&ColorValue::Hex("#ffffff".into()),
			0.5,
			false
		)
		.is_err());
	}

	#[test]
	fn luma_classifies_light_and_dark() {
		assert!(is_light_luma(color_luma(&ColorValue::Hex("#ffffff".into()))));
		assert!(!is_light_luma(color_luma(&ColorValue::Hex("#000000".into()))));
		assert!(is_light_surface(Some(&ColorValue::Hex("#ffffff".into()))));
		assert!(!is_light_surface(Some(&ColorValue::Hex("#000000".into()))));
		assert!(!is_light_surface(None));
		assert!(color_luma(&ColorValue::Hex(String::new())).is_none());
		assert!(relative_luminance(&ColorValue::Hex("#ffffff".into())).unwrap() > 0.99);
	}

	#[test]
	fn colorblind_adjustment_shifts_green_toward_blue() {
		let adjustment = HsvAdjustment { h: Some(60.0), s: Some(0.71), v: None };
		// Upstream `adjustHsv` doc vector.
		assert_eq!(adjust_hsv("#00ff88", adjustment), "#4a9eff");
		assert_eq!(adjust_hsv("#89d281", adjustment), "#98d2cc");
	}

	#[test]
	fn color_mode_follows_wt_session() {
		let mut env = HashMap::new();
		assert_eq!(detect_color_mode_from_env(&env), ColorMode::Color256);
		env.insert("WT_SESSION".into(), "1".into());
		assert_eq!(detect_color_mode_from_env(&env), ColorMode::TrueColor);
		env.insert("WT_SESSION".into(), String::new());
		assert_eq!(detect_color_mode_from_env(&env), ColorMode::Color256);
	}
}
