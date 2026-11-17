//! Session accent derivation.
//! Reference: `l/src/theme/session-color.ts`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::theme::theme_class::SessionAccentTheme;

/// Derive a stable 32-bit hash from a string using djb2.
pub fn name_to_hash(name: &str) -> u32 {
	let mut hash: u32 = 5381;
	for b in name.as_bytes() {
		hash = hash.wrapping_shl(5).wrapping_add(hash) ^ (*b as u32);
	}
	hash
}

/// Fallback cusp-chroma fraction when the theme accent is unparseable or
/// near-gray: sits at each hue's gamut cusp at 70% of its peak chroma,
/// matching a typical vivid accent.
pub const FALLBACK_CUSP_CHROMA_FRACTION: f64 = 0.7;

/// Chroma bounds for the carried theme-accent chroma.
pub const MIN_CHROMA: f64 = 0.05;
pub const MAX_CHROMA: f64 = 0.21;

/// Lightness bounds on dark themes so a carried accent lightness stays visible on dark surfaces.
pub const DARK_MIN_LIGHTNESS: f64 = 0.65;
pub const DARK_MAX_LIGHTNESS: f64 = 0.88;

/// Minimum contrast ratio (WCAG AA large text) between a light-theme accent and its surface.
pub const ACCENT_MIN_CONTRAST: f64 = 3.0;

/// Largest relative luminance an accent may have while still meeting
/// [`ACCENT_MIN_CONTRAST`] against a surface of the given luminance.
pub fn accent_luminance_cap(surface_luminance: f64) -> f64 {
	((surface_luminance + 0.05) / ACCENT_MIN_CONTRAST - 0.05).max(0.0)
}

/// Minimum angular distance in hue degrees from any theme color to avoid visual collision.
pub const MIN_HUE_DISTANCE: f64 = 10.0;
/// OKLCH chroma threshold below which hue is meaningless (near-gray).
pub const MIN_CHROMA_FOR_HUE: f64 = 0.03;

/// Inclusive hue interval [start, end] in OKLCH degrees.
pub type HueInterval = (f64, f64);

/// Parse OKLCH hue (0-360) from a hex color string.
/// Returns `None` for near-gray colors where hue is not meaningful.
pub fn hex_to_hue(hex: &str) -> Option<f64> {
	let o = hex_to_oklch(hex);
	if !o.h.is_finite() || o.c < MIN_CHROMA_FOR_HUE {
		return None;
	}
	Some(normalize_hue(o.h))
}

/// Number of integer hue positions across all intervals.
pub fn arc_length(intervals: &[HueInterval]) -> usize {
	let mut n: i64 = 0;
	for &(a, b) in intervals {
		n += (b.round() as i64 - a.round() as i64 + 1).max(0);
	}
	n.max(0) as usize
}

/// Map an arc position (`0..arc_length-1`) to its hue degree.
pub fn arc_to_hue(intervals: &[HueInterval], pos: usize) -> f64 {
	let mut p = pos as i64;
	for &(a, b) in intervals {
		let ai = a.round() as i64;
		let bi = b.round() as i64;
		let n = (bi - ai + 1).max(0);
		if p < n {
			return (ai + p) as f64;
		}
		p -= n;
	}
	intervals.last().map(|&(_, b)| b).unwrap_or(0.0)
}

/// Find a hue at least [`MIN_HUE_DISTANCE`] from all occupied hues,
/// walked outward in arc positions so it never leaves the admissible arc.
/// Returns the hue at `pos` unchanged if no safe hue exists within the arc.
pub fn find_safe_hue(pos: usize, occupied: &[f64], intervals: &[HueInterval]) -> f64 {
	let total = arc_length(intervals);
	if total == 0 {
		return 0.0;
	}
	let hue_at = |p: i64| -> f64 {
		let clamped = p.clamp(0, total as i64 - 1) as usize;
		arc_to_hue(intervals, clamped)
	};
	let target = hue_at(pos as i64);
	if occupied.is_empty() {
		return target;
	}
	if occupied.iter().all(|&h| hue_distance(target, h) >= MIN_HUE_DISTANCE) {
		return target;
	}
	for d in 1..(total as i64) {
		for dir in [1i64, -1i64] {
			let candidate = hue_at(pos as i64 + d * dir);
			if occupied.iter().all(|&h| hue_distance(candidate, h) >= MIN_HUE_DISTANCE) {
				return candidate;
			}
		}
	}
	target
}

static DARK_HUE_INTERVALS: LazyLock<Vec<HueInterval>> = LazyLock::new(|| {
	let mut intervals: Vec<HueInterval> = Vec::new();
	let mut start: Option<i32> = None;
	for h in 0..=360 {
		let ok = h < 360 && oklch_cusp(h as f64).l <= DARK_MAX_LIGHTNESS;
		if ok {
			if start.is_none() {
				start = Some(h);
			}
		} else if let Some(s) = start.take() {
			intervals.push((s as f64, (h - 1) as f64));
		}
	}
	intervals
});

fn dark_hue_intervals() -> &'static [HueInterval] {
	DARK_HUE_INTERVALS.as_slice()
}

static LIGHT_HUE_INTERVALS: LazyLock<Vec<HueInterval>> = LazyLock::new(|| vec![(195.0, 330.0)]);

fn light_hue_intervals() -> &'static [HueInterval] {
	LIGHT_HUE_INTERVALS.as_slice()
}

/// Dark-theme admissible hue intervals.
pub fn dark_hue_intervals_const() -> &'static [HueInterval] {
	dark_hue_intervals()
}

/// Light-theme admissible hue intervals.
pub fn light_hue_intervals_const() -> &'static [HueInterval] {
	light_hue_intervals()
}

/// Derive a stable CSS hex accent color from a session name and the active theme.
pub fn get_session_accent_hex(name: &str, theme: &SessionAccentTheme) -> String {
	let key = format!(
		"{}\0{}\0{:?}\0{}",
		name,
		theme.accent_hex,
		theme.surface_luminance,
		theme.color_hexes.join(",")
	);
	if let Some(cached) = ACCENT_HEX_CACHE.lock().ok().and_then(|m| m.get(&key).cloned()) {
		return cached;
	}
	let hex = compute_session_accent_hex(name, theme);
	if let Ok(mut m) = ACCENT_HEX_CACHE.lock() {
		if m.len() >= ACCENT_CACHE_LIMIT {
			m.clear();
		}
		m.insert(key, hex.clone());
	}
	hex
}

const ACCENT_CACHE_LIMIT: usize = 256;

static ACCENT_HEX_CACHE: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static ACCENT_ANSI_CACHE: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn compute_session_accent_hex(name: &str, theme: &SessionAccentTheme) -> String {
	let is_dark = theme.surface_luminance.is_none();
	let intervals = if is_dark { dark_hue_intervals() } else { light_hue_intervals() };
	let total = arc_length(intervals);
	if total == 0 {
		return theme.accent_hex.clone();
	}
	let pos = (name_to_hash(name) as usize) % total;
	let mut theme_hues: Vec<f64> = Vec::new();
	for hex in &theme.color_hexes {
		if let Some(h) = hex_to_hue(hex) {
			theme_hues.push(h);
		}
	}
	let target_hue = find_safe_hue(pos, &theme_hues, intervals);

	let accent = hex_to_oklch_inner(&theme.accent_hex);
	let usable = accent.l.is_finite()
		&& accent.c.is_finite()
		&& accent.h.is_finite()
		&& accent.c >= MIN_CHROMA_FOR_HUE;
	let target = oklch_cusp_inner(target_hue);
	let (lightness, chroma_fraction) = if usable {
		let source = oklch_cusp_inner(normalize_hue(accent.h));
		let l = if accent.l <= source.l {
			if source.l == 0.0 { target.l } else { (accent.l / source.l) * target.l }
		} else if (1.0 - source.l) == 0.0 {
			target.l
		} else {
			target.l + ((accent.l - source.l) / (1.0 - source.l)) * (1.0 - target.l)
		};
		let cf = if source.c == 0.0 { FALLBACK_CUSP_CHROMA_FRACTION } else { accent.c / source.c };
		(l, cf)
	} else {
		(target.l, FALLBACK_CUSP_CHROMA_FRACTION)
	};
	let chroma = (chroma_fraction * target.c).clamp(MIN_CHROMA, MAX_CHROMA);

	if let Some(surface) = theme.surface_luminance {
		let cap = accent_luminance_cap(surface);
		let top = oklch_to_hex(Oklch { l: lightness, c: chroma, h: target_hue });
		if relative_luminance_hex(&top).unwrap_or(0.0) <= cap {
			return top;
		}
		let mut lo = 0.0f64;
		let mut hi = lightness;
		for _ in 0..20 {
			let mid = (lo + hi) / 2.0;
			let hex = oklch_to_hex(Oklch { l: mid, c: chroma, h: target_hue });
			if relative_luminance_hex(&hex).unwrap_or(0.0) > cap {
				hi = mid;
			} else {
				lo = mid;
			}
		}
		return oklch_to_hex(Oklch { l: lo, c: chroma, h: target_hue });
	}

	let l = lightness.clamp(DARK_MIN_LIGHTNESS, DARK_MAX_LIGHTNESS);
	oklch_to_hex(Oklch { l, c: chroma, h: target_hue })
}

/// Convert a hex accent color to an ANSI foreground escape sequence.
/// Returns `None` if `hex` is empty or invalid.
pub fn get_session_accent_ansi(hex: Option<&str>) -> Option<String> {
	let hex = hex?;
	if hex.is_empty() {
		return None;
	}
	let true_color = truecolor_capable();
	let key = if true_color { hex.to_string() } else { format!("256:{hex}") };
	if let Ok(m) = ACCENT_ANSI_CACHE.lock() {
		if let Some(v) = m.get(&key) {
			return Some(v.clone());
		}
	}
	let ansi = if true_color { hex_to_ansi_16m(hex) } else { hex_to_ansi_256(hex) }?;
	if let Ok(mut m) = ACCENT_ANSI_CACHE.lock() {
		if m.len() >= ACCENT_CACHE_LIMIT {
			m.clear();
		}
		m.insert(key, ansi.clone());
	}
	Some(ansi)
}

fn truecolor_capable() -> bool {
	match std::env::var("COLORTERM") {
		Ok(v) => {
			let v = v.to_lowercase();
			v.contains("truecolor") || v.contains("24bit")
		}
		Err(_) => false,
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Oklch {
	l: f64,
	c: f64,
	h: f64,
}

fn normalize_hue(h: f64) -> f64 {
	let mut h = h % 360.0;
	if h < 0.0 {
		h += 360.0;
	}
	h
}

fn hue_distance(a: f64, b: f64) -> f64 {
	let d = (normalize_hue(a) - normalize_hue(b)).abs();
	d.min(360.0 - d)
}

fn hex_to_rgb(hex: &str) -> Option<(f64, f64, f64)> {
	let s = hex.trim().trim_start_matches('#');
	if s.len() != 6 {
		return None;
	}
	let r = u8::from_str_radix(&s[0..2], 16).ok()? as f64 / 255.0;
	let g = u8::from_str_radix(&s[2..4], 16).ok()? as f64 / 255.0;
	let b = u8::from_str_radix(&s[4..6], 16).ok()? as f64 / 255.0;
	Some((r, g, b))
}

fn rgb_to_hex(r: f64, g: f64, b: f64) -> String {
	fn c(v: f64) -> u8 {
		(v.clamp(0.0, 1.0) * 255.0).round() as u8
	}
	format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

fn srgb_to_linear(c: f64) -> f64 {
	if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f64) -> f64 {
	if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn hex_to_oklch_inner(hex: &str) -> Oklch {
	let Some((r, g, b)) = hex_to_rgb(hex) else {
		return Oklch { l: 0.0, c: 0.0, h: 0.0 };
	};
	let rl = srgb_to_linear(r);
	let gl = srgb_to_linear(g);
	let bl = srgb_to_linear(b);
	let l = 0.4122214708 * rl + 0.5363325363 * gl + 0.0514459929 * bl;
	let m = 0.2119034982 * rl + 0.6806995451 * gl + 0.1073969566 * bl;
	let s = 0.0883024619 * rl + 0.2817188376 * gl + 0.6299787005 * bl;
	let l_ = l.cbrt();
	let m_ = m.cbrt();
	let s_ = s.cbrt();
	let L = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
	let a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
	let b2 = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;
	let c = (a * a + b2 * b2).sqrt();
	let h = if c < 1e-7 { 0.0 } else { b2.atan2(a).to_degrees() };
	Oklch { l: L, c, h: normalize_hue(h) }
}

// (hex_to_oklch_inner is used internally; public API exposed below)

fn oklch_to_hex(ok: Oklch) -> String {
	let hrad = ok.h.to_radians();
	let a = ok.c * hrad.cos();
	let b = ok.c * hrad.sin();
	let l_ = ok.l + 0.3963377774 * a + 0.2158037573 * b;
	let m_ = ok.l - 0.1055613458 * a - 0.0638541728 * b;
	let s_ = ok.l - 0.0894841775 * a - 1.2914855480 * b;
	let l = l_ * l_ * l_;
	let m = m_ * m_ * m_;
	let s = s_ * s_ * s_;
	let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
	let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
	let b2 = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
	rgb_to_hex(linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b2))
}

fn relative_luminance_rgb(r: f64, g: f64, b: f64) -> f64 {
	0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

fn relative_luminance_hex(hex: &str) -> Option<f64> {
	let (r, g, b) = hex_to_rgb(hex)?;
	Some(relative_luminance_rgb(r, g, b))
}

#[derive(Clone, Copy, Debug)]
struct Cusp {
	l: f64,
	c: f64,
}

fn oklch_cusp_inner(h_deg: f64) -> Cusp {
	let mut best = Cusp { l: 0.7, c: 0.0 };
	for i in 0..=100 {
		let l = i as f64 / 100.0;
		if let Some(cmax) = max_chroma_at(l, h_deg) {
			if cmax >= best.c {
				best = Cusp { l, c: cmax };
			}
		}
	}
	best
}

// (oklch_cusp_inner is used internally; public API exposed below)

fn max_chroma_at(l: f64, h_deg: f64) -> Option<f64> {
	let mut lo = 0.0f64;
	let mut hi = 0.4f64;
	for _ in 0..20 {
		let mid = (lo + hi) / 2.0;
		if in_gamut(Oklch { l, c: mid, h: h_deg }) {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	if in_gamut(Oklch { l, c: lo, h: h_deg }) { Some(lo) } else { None }
}

fn in_gamut(ok: Oklch) -> bool {
	let hrad = ok.h.to_radians();
	let a = ok.c * hrad.cos();
	let b = ok.c * hrad.sin();
	let l_ = ok.l + 0.3963377774 * a + 0.2158037573 * b;
	let m_ = ok.l - 0.1055613458 * a - 0.0638541728 * b;
	let s_ = ok.l - 0.0894841775 * a - 1.2914855480 * b;
	let l = l_ * l_ * l_;
	let m = m_ * m_ * m_;
	let s = s_ * s_ * s_;
	let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
	let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
	let b2 = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
	let eps = -0.0005;
	let hi = 1.0005;
	r >= eps && r <= hi && g >= eps && g <= hi && b2 >= eps && b2 <= hi
}

fn hex_to_ansi_16m(hex: &str) -> Option<String> {
	let (r, g, b) = hex_to_rgb(hex)?;
	let ri = (r * 255.0).round() as u8;
	let gi = (g * 255.0).round() as u8;
	let bi = (b * 255.0).round() as u8;
	Some(format!("\x1b[38;2;{ri};{gi};{bi}m"))
}

fn hex_to_ansi_256(hex: &str) -> Option<String> {
	let (r, g, b) = hex_to_rgb(hex)?;
	let ri = (r * 255.0).round() as i32;
	let gi = (g * 255.0).round() as i32;
	let bi = (b * 255.0).round() as i32;
	let idx = if (ri - gi).abs() < 8 && (gi - bi).abs() < 8 {
		if ri < 8 { 16 } else if ri > 248 { 231 } else { 232 + ((ri - 8) * 24 / 247) }
	} else {
		16 + 36 * (ri * 5 / 255) + 6 * (gi * 5 / 255) + (bi * 5 / 255)
	};
	Some(format!("\x1b[38;5;{idx}m"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn djb2_hash_stable() {
		assert_eq!(name_to_hash(""), 5381);
		assert_eq!(name_to_hash("session"), name_to_hash("session"));
		assert_ne!(name_to_hash("a"), name_to_hash("b"));
	}

	#[test]
	fn arc_helpers_basic() {
		let ints = vec![(0.0, 2.0), (10.0, 11.0)];
		assert_eq!(arc_length(&ints), 5);
		assert_eq!(arc_to_hue(&ints, 0) as i32, 0);
		assert_eq!(arc_to_hue(&ints, 3) as i32, 10);
	}

	#[test]
	fn hex_hue_gray_none() {
		assert!(hex_to_hue("#808080").is_none());
	}

	#[test]
	fn accent_hex_is_hex() {
		let theme = SessionAccentTheme {
			accent_hex: "#ff6688".to_string(),
			color_hexes: vec!["#00ff00".to_string()],
			surface_luminance: None,
		};
		let h = get_session_accent_hex("s1", &theme);
		assert!(h.starts_with('#') && h.len() == 7);
	}

	#[test]
	fn accent_ansi_some() {
		let a = get_session_accent_ansi(Some("#ff0000"));
		assert!(a.is_some());
	}
}
/// OKLCH color components exposed for session-accent analysis/tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionOklch {
	/// Lightness.
	pub l: f64,
	/// Chroma.
	pub c: f64,
	/// Hue in degrees.
	pub h: f64,
}

/// Gamut cusp for a hue: peak chroma and its lightness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionOklchCusp {
	/// Cusp lightness.
	pub l: f64,
	/// Cusp chroma.
	pub c: f64,
}

/// Parse a `#rrggbb` color into OKLCH (public helper for tests/tools).
#[must_use]
pub fn hex_to_oklch(hex: &str) -> SessionOklch {
	let ok = hex_to_oklch_inner(hex);
	SessionOklch { l: ok.l, c: ok.c, h: ok.h }
}

/// Approximate sRGB gamut cusp for `hue_deg`.
#[must_use]
pub fn oklch_cusp(hue_deg: f64) -> SessionOklchCusp {
	let cusp = oklch_cusp_inner(hue_deg);
	SessionOklchCusp { l: cusp.l, c: cusp.c }
}

/// Relative luminance of a `#rrggbb` color.
#[must_use]
pub fn relative_luminance(hex: &str) -> Option<f64> {
	relative_luminance_hex(hex)
}

