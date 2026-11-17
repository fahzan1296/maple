//! Reference: `l/src/setup/scenes/splash.ts`.
//!
//! Animated setup splash: the brand mark under a diagonal gradient with a
//! sliding shine, rising out of a rippling gradient-lit water surface beneath a
//! twinkling starfield, plus the native lockup description.
//!
//! Deviations from the TypeScript original:
//! - `theme` is passed in explicitly (`&Theme`) instead of the module singleton.
//! - `PI_LOGO`, `APP_NAME`, `VERSION`, `gradientLogo`, `gradientEscape`,
//!   `lockupNode` and `ShineConfig` have no crate equivalent, so they are defined
//!   here. `APP_NAME` is `"maple"` and `VERSION` is the crate version.
//! - The water field's size-keyed cache is recomputed per frame; the output is
//!   identical and the whole render is already O(width × height).

use crate::key_hint_format::KeyHintFormatter;
use crate::native::describe::{col, kbd, keyed, node, row, span, text};
use crate::native::node::{NodeKind, NodeText, Props};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};
use crate::theme::color::{detect_color_mode, ColorMode};
use crate::theme::theme_class::Theme;

/// Total length of the splash animation, in milliseconds.
pub const SETUP_SPLASH_MS: f64 = 2600.0;
/// Render cadence of the splash animation.
pub const SETUP_TICK_MS: f64 = 33.0;

/// Application name. The reference reads `APP_NAME` from `pi-utils`.
pub const APP_NAME: &str = "maple";
/// Application version. The reference reads `VERSION` from `pi-utils`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Block-grid brand mark shared by the welcome and setup surfaces.
pub const PI_LOGO: [&str; 5] = [
	"████████████",
	"   ██  ██   ",
	"   ██  ██   ",
	"   ▒▒  ██   ",
	"       ██   ",
];

/// Brand mark at 2x: every glyph doubled horizontally, every row doubled
/// vertically.
fn large_logo() -> Vec<String> {
	let mut out = Vec::with_capacity(PI_LOGO.len() * 2);
	for line in PI_LOGO {
		let mut wide = String::with_capacity(line.chars().count() * 2);
		for character in line.chars() {
			if character == ' ' {
				wide.push_str("  ");
			} else {
				wide.push(character);
				wide.push(character);
			}
		}
		out.push(wide.clone());
		out.push(wide);
	}
	out
}

/// Columns of the widest logo line.
fn logo_width(lines: &[String]) -> usize {
	lines
		.iter()
		.map(|line| line.chars().count())
		.max()
		.unwrap_or(0)
}

const RESET: &str = "\x1b[0m";

/// Full scene needs comfortable room; below this we drop to a centered mark.
const MIN_SCENE_WIDTH: usize = 56;
const MIN_SCENE_HEIGHT: usize = 22;

/// Multi-stop palette for the diagonal gradient.
const GRADIENT_STOPS: [(i64, i64, i64); 3] = [
	(248, 79, 204),  // oklch(0.7 0.24 340)
	(147, 98, 244),  // oklch(0.62 0.21 295)
	(0, 219, 228),   // oklch(0.81 0.14 200)
];

/// 256-color ramp fallback when truecolor isn't available.
const GRADIENT_RAMP_256: [u8; 7] = [206, 170, 134, 99, 69, 74, 44];

/// Half-width of the shine highlight band, expressed in gradient-t units.
const SHINE_HALF_WIDTH: f64 = 0.18;

/// Overall opacity and center of the sliding shine highlight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShineConfig {
	/// Overall opacity of the shine overlay, in `[0, 1]`.
	pub strength: f64,
	/// Center of the shine band along the diagonal, in `[0, 1]`.
	pub pos: f64,
}

pub(crate) fn fg(theme: &Theme, color: &str, text: &str) -> String {
	theme.fg(color, text).unwrap_or_else(|_| text.to_string())
}

/// Resolve the gradient SGR foreground escape for a normalized position `t`
/// (0..1) along the diagonal, compositing the optional sliding shine highlight.
pub fn gradient_escape(t: f64, shine: Option<ShineConfig>) -> String {
	let shine_strength = match shine {
		Some(config) if config.strength > 0.0 => config.strength,
		_ => 0.0,
	};
	let shine_pos = shine.map(|config| config.pos).unwrap_or(0.0);
	if detect_color_mode() == ColorMode::TrueColor {
		let stops = GRADIENT_STOPS;
		let seg = t * (stops.len() - 1) as f64;
		let i = (stops.len() - 2).min(seg.floor() as usize);
		let f = seg - i as f64;
		let (a, b) = (stops[i], stops[i + 1]);
		let mut r = a.0 as f64 + (b.0 - a.0) as f64 * f;
		let mut g = a.1 as f64 + (b.1 - a.1) as f64 * f;
		let mut bl = a.2 as f64 + (b.2 - a.2) as f64 * f;
		if shine_strength > 0.0 {
			let dist = (t - shine_pos).abs();
			let intensity = (1.0 - dist / SHINE_HALF_WIDTH).max(0.0) * shine_strength;
			if intensity > 0.0 {
				r += (255.0 - r) * intensity;
				g += (255.0 - g) * intensity;
				bl += (255.0 - bl) * intensity;
			}
		}
		return format!(
			"\x1b[38;2;{};{};{}m",
			r.round() as i64,
			g.round() as i64,
			bl.round() as i64
		);
	}
	let ramp = GRADIENT_RAMP_256;
	let mut idx = (ramp.len() - 1)
		.min(((t * (ramp.len() - 1) as f64 + 0.5).floor()).max(0.0) as usize);
	if shine_strength > 0.0 {
		let dist = (t - shine_pos).abs();
		let intensity = (1.0 - dist / SHINE_HALF_WIDTH).max(0.0) * shine_strength;
		// Promote to the brightest ramp slot when the shine band peaks here.
		if intensity > 0.5 {
			idx = ramp.len() - 1;
		}
	}
	format!("\x1b[38;5;{}m", ramp[idx])
}

/// Apply a multi-stop diagonal gradient (top-left → bottom-right) plus an
/// optional sliding shine band across multi-line art.
pub fn gradient_logo(lines: &[impl AsRef<str>], phase: f64, shine: Option<ShineConfig>) -> Vec<String> {
	let rows = lines.len();
	let cols = lines
		.iter()
		.map(|line| line.as_ref().chars().count())
		.max()
		.unwrap_or(0);
	let x_span = (cols.saturating_sub(1)).max(1) as f64;
	let y_span = (rows.saturating_sub(1)).max(1) as f64;
	let normalized_phase = ((phase % 1.0) + 1.0) % 1.0;
	lines
		.iter()
		.enumerate()
		.map(|(y, line)| {
			let mut result = String::new();
			for (x, character) in line.as_ref().chars().enumerate() {
				if character == ' ' {
					result.push(character);
					continue;
				}
				// SVG's (0,0) → (1,1) gradient projects both normalized axes
				// equally: top-right and bottom-left land on the purple midpoint.
				let base = (x as f64 / x_span + y as f64 / y_span) / 2.0;
				let t = if normalized_phase == 0.0 {
					base
				} else {
					(base + normalized_phase) % 1.0
				};
				result.push_str(&gradient_escape(t, shine));
				result.push(character);
				result.push_str(RESET);
			}
			result
		})
		.collect()
}

/// Skip affordance; built at render time so it follows the live symbol preset.
fn skip_hint(theme: &Theme) -> String {
	format!(
		"press {} to skip",
		KeyHintFormatter::new(theme).format_key_hint("enter")
	)
}

fn star_at(theme: &Theme, x: i64, y: i64, frame: i64) -> String {
	let hash = (x.wrapping_mul(73856093)) ^ (y.wrapping_mul(19349663)) ^ (frame.wrapping_mul(83492791));
	let bucket = hash.unsigned_abs() % 97;
	match bucket {
		0 => fg(theme, "accent", "✦"),
		1 => fg(theme, "muted", "·"),
		_ => " ".to_string(),
	}
}

/// The faint twinkling starfield behind the brand mark.
pub fn render_starfield(theme: &Theme, width: usize, height: usize, frame: i64) -> Vec<String> {
	let star_frame = frame >> 3;
	(0..height)
		.map(|y| {
			(0..width)
				.map(|x| star_at(theme, x as i64, y as i64, star_frame))
				.collect()
		})
		.collect()
}

/// Continuous diagonal gradient position (bottom-left → top-right) across the
/// whole screen.
fn screen_gradient_t(x: i64, y: i64, width: i64, height: i64, phase: f64) -> f64 {
	let span = (width + height - 1).max(1) as f64;
	let base = (x as f64 + (height - 1 - y) as f64) / span;
	(((base + phase) % 1.0) + 1.0) % 1.0
}

/// Twinkling sparkle for the upper "sky". Returns a styled glyph, or `None` for
/// empty space.
fn sky_glyph(theme: &Theme, x: i64, y: i64, frame: i64) -> Option<String> {
	let hash = (x.wrapping_mul(73856093)) ^ (y.wrapping_mul(19349663)) ^ (frame.wrapping_mul(83492791));
	let bucket = hash.unsigned_abs() % 150;
	match bucket {
		0 => Some(fg(theme, "accent", "✦")),
		1 => Some(fg(theme, "border", "✧")),
		2 => Some(fg(theme, "border", "·")),
		_ => None,
	}
}

/// Static value-jitter in `[0,1)` that softens the water's threshold banding.
fn water_jitter(x: i64, y: i64) -> f64 {
	let mut h: i32 = ((x as i32).wrapping_mul(374761393))
		.wrapping_add((y as i32).wrapping_mul(668265263));
	let unsigned = h as u32;
	h = (unsigned ^ (unsigned >> 13)) as i32;
	h = h.wrapping_mul(1274126177);
	h ^= ((h as u32) >> 16) as i32;
	(h as u32) as f64 / 4_294_967_296.0
}

/// Time-invariant terms of the rippling water for one screen size: three
/// travelling sine waves interfere, then a radial edge falloff and a downward
/// fade concentrate the ripples beneath the mark and dissolve them toward the
/// edges/bottom.
struct WaterField {
	width: usize,
	water_top: usize,
	radial: Vec<f64>,
	diagonal: Vec<f64>,
	cross: Vec<f64>,
	jitter: Vec<f64>,
	edge: Vec<f64>,
	fade: Vec<f64>,
}

fn water_field(width: usize, height: usize, cx: i64, water_top: usize) -> WaterField {
	let water_height = (height.saturating_sub(water_top)).max(1) as f64;
	let rows = height.saturating_sub(water_top);
	let mut radial = vec![0.0; rows * width];
	let mut diagonal = vec![0.0; rows * width];
	let mut cross = vec![0.0; rows * width];
	let mut jitter = vec![0.0; rows * width];
	let mut edge = vec![0.0; width];
	let mut fade = vec![0.0; rows];
	for x in 0..width {
		edge[x] = (1.0 - ((x as i64 - cx).abs() as f64) / (width as f64 * 0.5))
			.max(0.0)
			.powf(0.7);
	}
	for row in 0..rows {
		let y = water_top + row;
		let dy = (y - water_top) as f64;
		fade[row] = (1.0 - (dy / water_height) * 0.55).max(0.0);
		for x in 0..width {
			let i = row * width + x;
			let dx = (x as i64 - cx) as f64 / 2.0;
			radial[i] = (dx * dx + dy * dy).sqrt() * 0.55;
			diagonal[i] = x as f64 * 0.22 + y as f64 * 0.45;
			cross[i] = dx.abs() * 0.8 + dy * 0.5;
			jitter[i] = (water_jitter(x as i64, y as i64) - 0.5) * 0.06;
		}
	}
	WaterField {
		width,
		water_top,
		radial,
		diagonal,
		cross,
		jitter,
		edge,
		fade,
	}
}

/// Water glyph for an amplitude, lightest → heaviest density ramp; `None` below
/// the floor.
fn water_char(amp: f64) -> Option<&'static str> {
	if amp > 0.62 {
		Some("█")
	} else if amp > 0.5 {
		Some("▓")
	} else if amp > 0.36 {
		Some("▒")
	} else if amp > 0.24 {
		Some("░")
	} else {
		None
	}
}

/// Center a line in a field of `width` columns, truncating when too wide.
pub(crate) fn center_line(line: &str, width: usize) -> String {
	let line_width = visible_width(line);
	if line_width >= width {
		return truncate_to_width(line, width, Ellipsis::Unicode);
	}
	let left = (width - line_width) / 2;
	let mut out = " ".repeat(left);
	out.push_str(line);
	out.push_str(&" ".repeat(width - left - line_width));
	out
}

/// Animated setup splash. `elapsed_ms` is milliseconds since the phase began.
pub fn render_setup_splash(theme: &Theme, width: usize, height: usize, elapsed_ms: f64) -> Vec<String> {
	let w = width.max(1);
	let h = height.max(1);
	let progress = (elapsed_ms / SETUP_SPLASH_MS).clamp(0.0, 1.0);
	let phase = progress * 1.8;
	let shine = ShineConfig {
		pos: (progress * 2.5) % 1.0,
		strength: (1.0 - progress * 0.35).max(0.0),
	};

	if w < MIN_SCENE_WIDTH || h < MIN_SCENE_HEIGHT {
		return render_compact_splash(theme, w, h, phase, shine);
	}

	let frame = (elapsed_ms / SETUP_TICK_MS).floor() as i64;
	let cx = (w / 2) as i64;
	let surface_time = frame as f64 * 0.13;

	let mut cells: Vec<Vec<String>> = vec![vec![" ".to_string(); w]; h];
	let mut put = |x: i64, y: i64, glyph: String| {
		if y >= 0 && y < h as i64 && x >= 0 && x < w as i64 {
			cells[y as usize][x as usize] = glyph;
		}
	};

	// Every cell on a diagonal shares one gradient position, so resolve each
	// escape once per frame instead of once per cell.
	let diagonals = w + h - 1;
	let mut escapes: Vec<Option<String>> = vec![None; diagonals];
	let mut gradient = |x: i64, y: i64| -> String {
		let d = (x + (h as i64 - 1 - y)) as usize;
		if let Some(escape) = &escapes[d] {
			return escape.clone();
		}
		let escape = gradient_escape(screen_gradient_t(x, y, w as i64, h as i64, phase), Some(shine));
		escapes[d] = Some(escape.clone());
		escape
	};

	let logo = large_logo();
	let logo_width = logo_width(&logo);
	let hx = (w - logo_width) / 2;
	let hy = 2.max(h * 16 / 100);
	let water_top = hy + logo.len();

	// 1. rippling water surface (shares the screen-wide gradient with the mark).
	let field = water_field(w, h, cx, water_top);
	let t_diagonal = surface_time * 0.7;
	let t_cross = surface_time * 1.4;
	for y in water_top..h {
		let row = y - water_top;
		let fade = field.fade[row];
		for x in 0..w {
			let i = row * w + x;
			let wave = 0.5 * (field.radial[i] - surface_time).sin()
				+ 0.3 * (field.diagonal[i] - t_diagonal).sin()
				+ 0.2 * (field.cross[i] - t_cross).sin();
			let level = 0.5 + 0.5 * wave;
			if let Some(character) = water_char(level * field.edge[x] * fade + field.jitter[i]) {
				put(x as i64, y as i64, format!("{}{}{}", gradient(x as i64, y as i64), character, RESET));
			}
		}
	}
	// 2. twinkling starfield in the sky above the water.
	for y in 0..water_top.saturating_sub(1) {
		for x in 0..w {
			if let Some(star) = sky_glyph(theme, x as i64, y as i64, frame >> 3) {
				put(x as i64, y as i64, star);
			}
		}
	}
	// 3. hero — the brand mark with the live gradient + shine sweep.
	for (row, line) in logo.iter().enumerate() {
		for (col, character) in line.chars().enumerate() {
			let x = hx + col;
			let y = hy + row;
			if character != ' ' && y < h && x < w {
				put(x as i64, y as i64, format!("{}{}{}", gradient(x as i64, y as i64), character, RESET));
			}
		}
	}
	// 4. skip hint on a cleared strip at the bottom so it stays legible.
	let hint = skip_hint(theme);
	let hint_width = visible_width(&hint);
	let hint_start = (w - hint_width) / 2;
	let hint_row = h - 1;
	for x in (hint_start as i64 - 1)..=(hint_start + hint_width) as i64 {
		put(x, hint_row as i64, " ".to_string());
	}
	for (offset, character) in hint.chars().enumerate() {
		let painted = if character == ' ' {
			" ".to_string()
		} else {
			fg(theme, "dim", &character.to_string())
		};
		put(hint_start as i64 + offset as i64, hint_row as i64, painted);
	}

	cells.into_iter().map(|row| row.concat()).collect()
}

/// The native splash: the brand lockup centered and a quiet Enter hint pinned to
/// the bottom. The water and starfield are cell paintings with no semantic
/// counterpart. A click anywhere sends `skip`.
pub fn describe_setup_splash() -> crate::native::node::NativeNode {
	let mut grow = Props::new();
	grow.insert("grow", 1.into());
	let mut actions = serde_json::Map::new();
	actions.insert("click".to_string(), "skip".into());
	let mut root = Props::new();
	root.set_str("align", "center");
	root.insert("grow", 1.into());
	root.set_str("role", "omp.setup.splash");
	root.insert("actions", actions.into());

	col(
		vec![
			node(NodeKind::Spacer, grow.clone(), Vec::new(), Some("top")),
			lockup_node(),
			node(NodeKind::Spacer, grow.clone(), Vec::new(), Some("bottom")),
			keyed(
				row(
					vec![
						text(NodeText::Spans(vec![span("Press", Some("muted"))]), Props::new()),
						kbd("enter", None),
						text(NodeText::Spans(vec![span("to begin", Some("muted"))]), Props::new()),
					],
					{
						let mut props = Props::new();
						props.set_str("align", "center");
						props.set_str("gap", "xs");
						props.set_str("role", "omp.setup.hint");
						props
					},
				),
				"hint",
			),
		],
		root,
	)
}

/// The brand lockup: the builtin mark beside the wordmark and version.
fn lockup_node() -> crate::native::node::NativeNode {
	let mut image = Props::new();
	image.set_str("builtin", "omp");
	image.set_str("alt", APP_NAME);
	image.set_i64("w", 128);
	image.set_str("role", "omp.welcome.logo");
	let wordmark = {
		let mut props = Props::new();
		props.set_str("wrap", "none");
		props.set_str("role", "omp.welcome.wordmark");
		keyed(
			text(
				NodeText::Spans(vec![span(APP_NAME, Some("strong"))]),
				props,
			),
			"omp.welcome.wordmark",
		)
	};
	let version = {
		let mut props = Props::new();
		props.set_str("wrap", "none");
		props.set_str("role", "omp.welcome.version");
		keyed(
			text(
				NodeText::Spans(vec![span(format!("v{VERSION}"), Some("dim mono"))]),
				props,
			),
			"omp.welcome.version",
		)
	};
	let mark = {
		let mut props = Props::new();
		props.set_str("role", "omp.welcome.mark");
		keyed(col(vec![wordmark, version], props), "mark")
	};
	let mut lockup = Props::new();
	lockup.set_str("align", "center");
	lockup.set_str("gap", "md");
	lockup.set_str("role", "omp.welcome.lockup");
	keyed(
		row(
			vec![node(NodeKind::Image, image, Vec::new(), Some("logo")), mark],
			lockup,
		),
		"lockup",
	)
}

/// Centered fallback for windows too small to hold the full scene.
fn render_compact_splash(
	theme: &Theme,
	width: usize,
	height: usize,
	phase: f64,
	shine: ShineConfig,
) -> Vec<String> {
	let art: Vec<String> = if height >= 14 {
		large_logo()
	} else {
		PI_LOGO.iter().map(|line| (*line).to_string()).collect()
	};
	let mut content = gradient_logo(&art, phase, Some(shine));
	content.push(String::new());
	content.push(theme.bold(APP_NAME));
	let start = (height.saturating_sub(content.len())) / 2;
	let mut lines = Vec::with_capacity(height);
	for y in 0..height {
		let item = content.get(y.wrapping_sub(start));
		lines.push(if width > 0 {
			pad_to_width(&center_line(item.map(String::as_str).unwrap_or(""), width), width)
		} else {
			String::new()
		});
	}
	if height > 2 {
		lines[height - 2] = if width > 0 {
			pad_to_width(&center_line(&fg(theme, "dim", &skip_hint(theme)), width), width)
		} else {
			String::new()
		};
	}
	lines
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").expect("dark theme"),
			CreateThemeOptions::default(),
		)
		.expect("theme")
	}

	#[test]
	fn starfield_is_full_size_and_deterministic() {
		let theme = test_theme();
		let first = render_starfield(&theme, 12, 5, 8);
		let second = render_starfield(&theme, 12, 5, 8);
		assert_eq!(first.len(), 5);
		assert!(first.iter().all(|line| line.chars().count() == 12));
		assert_eq!(first, second);
	}

	#[test]
	fn gradient_escape_is_truecolor_at_the_ends() {
		assert!(gradient_escape(0.0, None).starts_with("\x1b[38;2;"));
		assert!(gradient_escape(1.0, None).starts_with("\x1b[38;2;"));
	}

	#[test]
	fn gradient_logo_keeps_spaces_and_sizes_rows() {
		let rows = gradient_logo(&PI_LOGO, 0.0, None);
		assert_eq!(rows.len(), PI_LOGO.len());
		assert_eq!(visible_width(&rows[0]), PI_LOGO[0].chars().count());
	}

	#[test]
	fn splash_falls_back_when_narrow() {
		let theme = test_theme();
		let lines = render_setup_splash(&theme, 20, 10, 0.0);
		assert_eq!(lines.len(), 10);
		assert!(lines.iter().all(|line| visible_width(line) <= 20));
	}

	#[test]
	fn splash_fills_the_screen() {
		let theme = test_theme();
		let lines = render_setup_splash(&theme, 80, 24, 100.0);
		assert_eq!(lines.len(), 24);
		assert!(lines.iter().all(|line| visible_width(line) <= 80));
	}

	#[test]
	fn native_splash_carries_skip_action() {
		let node = describe_setup_splash();
		assert_eq!(node.kind, NodeKind::Col);
		assert_eq!(node.props.get_str("role"), Some("omp.setup.splash"));
		assert_eq!(
			node.props
				.get("actions")
				.and_then(|value| value.get("click"))
				.and_then(|value| value.as_str()),
			Some("skip")
		);
	}
}
