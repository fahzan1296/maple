//! Reference: `l/src/prompt/welcome.ts`.
//!
//! The session welcome banner: the gradient logo lockup with the version (and
//! background self-update activity) plus the tip of the session, the weighted
//! tip picker, the animated rainbow `NEW!` tag, and the intro gradient sweep.
//!
//! Deviations:
//! - There is no global theme in this crate: rendering takes a [`WelcomeEnv`]
//!   (theme, key formatter, keybindings, terminal capabilities).
//! - The intro runs off caller-driven frames: [`WelcomeComponent::play_intro`]
//!   marks the start and the caller repaints; there is no interval timer and no
//!   render callback to retarget.
//! - The native (`describe`) card is not ported: its lockup uses a terminal
//!   builtin image name with no crate equivalent; the caller renders the
//!   terminal banner.
//! - `data/tips.txt` is this port's own tip table, not the reference's file.

use std::sync::LazyLock;
use std::time::Instant;

use regex::Regex;

use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::render::hyperlink::{is_hyperlink_enabled, url_hyperlink};

use crate::render::render_utils::{padding, replace_tabs};
use crate::text_shape::{visible_width, wrap_text_with_ansi};
use crate::theme::theme_class::Theme;

/// tips.txt lines embedded at build time; blanks dropped.
static TIP_LINES: LazyLock<Vec<String>> = LazyLock::new(|| {
	include_str!("data/tips.txt")
		.lines()
		.map(|line| line.trim().to_string())
		.filter(|line| !line.is_empty())
		.collect()
});

/// Leading marker for a tip that pitches the native surface: shown only while
/// no native surface is live.
pub static NO_TERN_TIP_MARKER: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^\[NO-TERN\]\s*").expect("no-tern marker"));

/// Tips for the classic renderer: every tip, marker stripped.
pub static TIPS: LazyLock<Vec<String>> = LazyLock::new(|| {
	TIP_LINES
		.iter()
		.map(|line| NO_TERN_TIP_MARKER.replace(line, "").into_owned())
		.collect()
});

/// Tips for a live native surface: marked tips dropped.
pub static NATIVE_TIPS: LazyLock<Vec<String>> = LazyLock::new(|| {
	TIP_LINES
		.iter()
		.filter(|line| !NO_TERN_TIP_MARKER.is_match(line))
		.cloned()
		.collect()
});

/// Trailing marker that flags a tip as a "what's new" callout.
pub static NEW_TIP_MARKER: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\s*\[NEW\]\s*$").expect("new tip marker"));

/// Visible text rendered in place of [`NEW_TIP_MARKER`].
pub const NEW_TAG_TEXT: &str = "NEW!";

/// Milliseconds for one full hue rotation of the rainbow "NEW!" tag.
pub const NEW_GLOW_PERIOD_MS: f64 = 1500.0;

/// Selection weight for `[NEW]` tips; ordinary tips weigh 1.
pub const NEW_TIP_WEIGHT: usize = 4;

/// Pick a tip from `tips`, biased toward `[NEW]` tips; `r` is a uniform sample
/// in `[0, 1)`. Empty string when `tips` is empty.
pub fn pick_weighted_tip(tips: &[String], r: f64) -> String {
	if tips.is_empty() {
		return String::new();
	}
	let weights: Vec<usize> = tips
		.iter()
		.map(|tip| if NEW_TIP_MARKER.is_match(tip) { NEW_TIP_WEIGHT } else { 1 })
		.collect();
	let total: usize = weights.iter().sum();
	let mut acc = r * total as f64;
	for (index, weight) in weights.iter().enumerate() {
		acc -= *weight as f64;
		if acc < 0.0 {
			return tips[index].clone();
		}
	}
	tips.last().cloned().unwrap_or_default()
}

/// How ANSI colors are encoded for the rainbow tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorEncoding {
	TrueColor,
	Color256,
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (u8, u8, u8) {
	let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
	let h = hue / 60.0;
	let x = c * (1.0 - (h % 2.0 - 1.0).abs());
	let (r, g, b) = match h {
		h if h < 1.0 => (c, x, 0.0),
		h if h < 2.0 => (x, c, 0.0),
		h if h < 3.0 => (0.0, c, x),
		h if h < 4.0 => (0.0, x, c),
		h if h < 5.0 => (x, 0.0, c),
		_ => (c, 0.0, x),
	};
	let m = lightness - c / 2.0;
	let to_byte = |value: f64| ((value + m) * 255.0).round().clamp(0.0, 255.0) as u8;
	(to_byte(r), to_byte(g), to_byte(b))
}

/// Paint each glyph of [`NEW_TAG_TEXT`] on a moving HSL rainbow. Successive
/// renders with increasing phase shimmer; a fixed phase yields a still rainbow.
pub fn render_new_tag(phase: f64, encoding: ColorEncoding) -> String {
	let wrapped = phase.rem_euclid(1.0);
	let chars: Vec<char> = NEW_TAG_TEXT.chars().collect();
	let mut out = String::from("\x1b[1m");
	let mut prev = String::new();
	for (index, ch) in chars.iter().enumerate() {
		let hue = ((index as f64 / chars.len() as f64 + wrapped) % 1.0 * 360.0).round();
		let (r, g, b) = hsl_to_rgb(hue, 0.95, 0.60);
		let color = match encoding {
			ColorEncoding::TrueColor => format!("\x1b[38;2;{r};{g};{b}m"),
			ColorEncoding::Color256 => {
				// Nearest 6x6x6 cube entry, matching the reference's ramp fallback.
				let quantize = |value: u8| {
					let index = ((value as f64 / 255.0 * 5.0).round() as u8).min(5);
					index
				};
				let code = 16 + 36 * quantize(r) + 6 * quantize(g) + quantize(b);
				format!("\x1b[38;5;{code}m")
			}
		};
		if color != prev {
			out.push_str(&color);
			prev = color;
		}
		out.push(*ch);
	}
	out.push_str("\x1b[0m");
	out
}

/// Key placeholders in tips.txt: `{key:shift+tab}`, `{keys:up,down}`,
/// `{tap:left}`, `{action:tui.editor.undo}`.
pub static TIP_KEY_PLACEHOLDER: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\{(key|keys|tap|action):([^}]+)\}").expect("tip key placeholder"));

/// Markdown-style links in tips.txt: `[Tern](https://scl.so/tern)`.
pub static TIP_LINK: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^\s)]+)\)").expect("tip link"));

fn is_key_name(key: &str) -> bool {
	let parts: Vec<&str> = key.split('+').collect();
	parts.iter().enumerate().all(|(index, part)| {
		!part.is_empty()
			&& (index == parts.len() - 1 || matches!(*part, "ctrl" | "shift" | "alt" | "super"))
	})
}

/// Expand tip key placeholders through the key formatter; malformed ones stay
/// verbatim.
pub fn expand_tip_keys(tip: &str, formatter: &KeyHintFormatter<'_>, keybindings: &KeybindingsManager) -> String {
	TIP_KEY_PLACEHOLDER
		.replace_all(tip, |caps: &regex::Captures<'_>| {
			let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
			let kind = caps.get(1).map(|m| m.as_str()).unwrap_or("");
			let value = caps.get(2).map(|m| m.as_str()).unwrap_or("");
			if kind == "action" {
				let keys = keybindings.get_keys(value);
				return if keys.is_empty() {
					whole.to_string()
				} else {
					let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
					formatter.format_key_hints(&refs)
				};
			}
			let keys: Vec<&str> = value.split(',').collect();
			if !keys.iter().all(|key| is_key_name(key)) {
				return whole.to_string();
			}
			if kind == "keys" {
				return formatter.format_key_hints(&keys);
			}
			let Some(key) = keys.first() else {
				return whole.to_string();
			};
			if kind == "tap" {
				formatter.format_double_tap(key)
			} else {
				formatter.format_key_hint(key)
			}
		})
		.into_owned()
}

/// Tip links as underlined OSC 8 labels, or `label (url)` where the terminal
/// cannot link.
pub fn link_tip_text(tip: &str, theme: &Theme) -> String {
	let linkable = is_hyperlink_enabled();
	TIP_LINK
		.replace_all(tip, |caps: &regex::Captures<'_>| {
			let label = caps.get(1).map(|m| m.as_str()).unwrap_or("");
			let url = caps.get(2).map(|m| m.as_str()).unwrap_or("");
			if linkable {
				url_hyperlink(url, &theme.underline(label))
			} else {
				format!("{label} ({url})")
			}
		})
		.into_owned()
}

/// Inputs the banner render needs from the app.
pub struct WelcomeEnv<'a> {
	pub theme: &'a Theme,
	pub formatter: &'a KeyHintFormatter<'a>,
	pub keybindings: &'a KeybindingsManager,
	pub true_color: bool,
	pub native_rendering: bool,
}

/// The welcome tip as lines of at most `width` columns: `Tip:` and the body
/// wrapped together so the banner can center each line. `vec![]` when `width`
/// leaves no room for a useful line.
pub fn render_welcome_tip(tip: &str, width: usize, phase: f64, env: &WelcomeEnv<'_>) -> Vec<String> {
	let label = "Tip: ";
	if width < visible_width(label) + 8 {
		return Vec::new();
	}

	let is_new = NEW_TIP_MARKER.is_match(tip);
	let expanded = if is_new {
		NEW_TIP_MARKER.replace(tip, "").into_owned()
	} else {
		tip.to_string()
	};
	let body = link_tip_text(&expand_tip_keys(&expanded, env.formatter, env.keybindings), env.theme);

	let wrapped: Vec<String> = wrap_text_with_ansi(&replace_tabs(&format!("{label}{body}")), width)
		.into_iter()
		.map(|line| line.trim_end().to_string())
		.collect();
	if wrapped.is_empty() {
		return Vec::new();
	}

	let styled_label = env
		.theme
		.fg("customMessageLabel", label)
		.unwrap_or_else(|_| label.to_string());
	let lines: Vec<String> = wrapped
		.iter()
		.enumerate()
		.map(|(index, line)| {
			let styled = if index == 0 {
				match line.strip_prefix(label) {
					Some(rest) => format!(
						"{styled_label}{}",
						env.theme.fg("muted", rest).unwrap_or_else(|_| rest.to_string())
					),
					None => env.theme.fg("muted", line).unwrap_or_else(|_| line.clone()),
				}
			} else {
				env.theme.fg("muted", line).unwrap_or_else(|_| line.clone())
			};
			env.theme.italic(&styled)
		})
		.collect();


	if is_new {
		let encoding = if env.true_color {
			ColorEncoding::TrueColor
		} else {
			ColorEncoding::Color256
		};
		let tag = render_new_tag(phase, encoding);
		let tag_width = 1 + visible_width(NEW_TAG_TEXT);
		let mut lines = lines;
		if let Some(last) = lines.last_mut() {
			if visible_width(last) + tag_width <= width {
				last.push(' ');
				last.push_str(&tag);
				return lines;
			}
		}
		lines.push(tag);
		return lines;
	}
	lines
}

/// Background self-update activity shown after the version label.
#[derive(Clone, Debug, PartialEq)]
pub enum VersionUpdate {
	/// A newer `version` is downloading; `fraction` once bytes are counted.
	Updating { version: String, fraction: Option<f64> },
	/// The next launch starts the update.
	Ready { version: String },
	/// The attempt stopped.
	Failed { version: String },
}

impl VersionUpdate {
	pub fn version(&self) -> &str {
		match self {
			VersionUpdate::Updating { version, .. }
			| VersionUpdate::Ready { version }
			| VersionUpdate::Failed { version } => version,
		}
	}
}

/// Block-grid brand mark shared by the welcome and setup surfaces.
pub const PI_LOGO: [&str; 5] = [
	"████████████",
	"   ██  ██   ",
	"   ██  ██   ",
	"   ▒▒  ██   ",
	"       ██   ",
];

/// Columns of [`PI_LOGO`].
pub fn logo_width() -> usize {
	PI_LOGO.iter().map(|row| row.chars().count()).max().unwrap_or(0)
}

/// The wordmark in half-blocks, set beside [`PI_LOGO`] from its second row.
pub const WORDMARK: [&str; 3] = ["▄▀▀▄ █▀▄▀▄ █▀▀▄", "▀▄▄▀ █ █ █ █▄▄▀", "           █"];

/// Columns of [`WORDMARK`].
pub fn wordmark_width() -> usize {
	WORDMARK.iter().map(|row| row.chars().count()).max().unwrap_or(0)
}

/// Columns between the logo and the wordmark.
pub const LOCKUP_GAP: usize = 4;

/// Lockup row holding the version: below the wordmark, which starts on the
/// logo's second row.
pub const VERSION_ROW: usize = WORDMARK.len() + 1;

/// Cells of the download bar in a [`VersionUpdate`] indicator.
pub const UPDATE_BAR_WIDTH: usize = 10;

/// Widest a welcome tip wraps, so a long one stays a centered paragraph.
pub const TIP_MEASURE: usize = 72;

/// Narrowest terminal that still shows the tip.
pub const TIP_MIN_COLUMNS: usize = 50;

/// Total length of the intro animation.
pub const INTRO_MS: f64 = 3000.0;
/// Number of full gradient rotations the sweep performs before settling.
pub const INTRO_SWEEPS: f64 = 2.5;
/// Number of times the shine highlight crosses the diagonal across the intro.
pub const INTRO_SHINE_TRAVERSALS: f64 = 3.0;

fn update_percent(fraction: f64) -> String {
	format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).floor())
}

fn update_tone(update: &VersionUpdate) -> &'static str {
	match update {
		VersionUpdate::Ready { .. } => "success",
		VersionUpdate::Failed { .. } => "warning",
		VersionUpdate::Updating { .. } => "accent",
	}
}

fn progress_bar(fraction: f64, width: usize, theme: &Theme) -> String {
	let filled = theme.progress().filled.clone();
	let empty = theme.progress().empty.clone();
	let filled_cells = ((fraction.clamp(0.0, 1.0) * width as f64).floor() as usize).min(width);
	let mut out = String::new();
	for cell in 0..width {
		if cell < filled_cells {
			out.push_str(&theme.fg("accent", &filled).unwrap_or_else(|_| filled.clone()));
		} else {
			out.push_str(&theme.fg("dim", &empty).unwrap_or_else(|_| empty.clone()));
		}
	}
	out
}

/// Terminal forms of a [`VersionUpdate`] indicator, widest first.
fn version_update_labels(update: &VersionUpdate, theme: &Theme) -> Vec<String> {
	let tone = update_tone(update);
	let head = theme
		.fg(tone, &format!("→ v{}", update.version()))
		.unwrap_or_else(|_| format!("→ v{}", update.version()));
	let (detail, compact) = match update {
		VersionUpdate::Updating { fraction: None, .. } => (
			theme.fg("muted", "updating…").unwrap_or_else(|_| "updating…".to_string()),
			head.clone(),
		),
		VersionUpdate::Updating {
			fraction: Some(fraction),
			..
		} => {
			let percent = theme
				.fg("accent", &update_percent(*fraction))
				.unwrap_or_else(|_| update_percent(*fraction));
			(
				format!("{} {percent}", progress_bar(*fraction, UPDATE_BAR_WIDTH, theme)),
				format!("{head} {percent}"),
			)
		}
		VersionUpdate::Ready { .. } => (
			theme
				.fg("success", &format!("{} next launch", theme.symbol("status.success")))
				.unwrap_or_default(),
			head.clone(),
		),
		VersionUpdate::Failed { .. } => (
			theme
				.fg("warning", &format!("{} update failed", theme.symbol("status.warning")))
				.unwrap_or_default(),
			head.clone(),
		),
	};
	vec![format!("{head} {detail}"), compact]
}

/// Append the widest [`version_update_labels`] form that fits after the
/// lockup's version label, keeping the right margin column clear.
fn append_version_update(lines: &mut [String], update: &VersionUpdate, term_width: usize, theme: &Theme) {
	let Some(base) = lines.get(VERSION_ROW).cloned() else {
		return;
	};
	let room = term_width.saturating_sub(2 + visible_width(&base));
	if let Some(label) = version_update_labels(update, theme)
		.into_iter()
		.find(|form| visible_width(form) <= room)
	{
		lines[VERSION_ROW] = format!("{base} {label}");
	}
}

/// Logo frame `logo` with the wordmark beside it and `version` (styled) under
/// the wordmark.
fn lockup(logo: &[String], version: &str, theme: &Theme) -> Vec<String> {
	let mut beside: Vec<String> = vec![String::new()];
	beside.extend(
		WORDMARK
			.iter()
			.map(|row| theme.bold(&theme.fg("text", row).unwrap_or_else(|_| row.to_string()))),
	);
	beside.push(version.to_string());
	logo.iter()
		.enumerate()
		.map(|(index, row)| format!("{row}{}{}", padding(LOCKUP_GAP), beside.get(index).cloned().unwrap_or_default()))
		.collect()
}

/// `lines` indented as one block whose widest line is centered in `width`
/// columns.
pub fn center_block(lines: &[String], width: usize) -> Vec<String> {
	let widest = lines.iter().map(|line| visible_width(line)).max().unwrap_or(0);
	let indent = padding(width.saturating_sub(widest) / 2);
	lines.iter().map(|line| format!("{indent}{line}")).collect()
}

/// Multi-stop palette for the diagonal gradient.
pub const GRADIENT_STOPS: [(u8, u8, u8); 3] = [(248, 79, 204), (147, 98, 244), (0, 219, 228)];

/// 256-color ramp fallback when truecolor isn't available.
pub const GRADIENT_RAMP_256: [u8; 7] = [206, 170, 134, 99, 69, 74, 44];

/// Half-width of the shine highlight band, in gradient-t units.
pub const SHINE_HALF_WIDTH: f64 = 0.18;

/// Shine overlay configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShineConfig {
	/// Overall opacity of the shine overlay, in [0, 1].
	pub strength: f64,
	/// Center of the shine band along the diagonal, in [0, 1].
	pub pos: f64,
}

/// Resolve the gradient SGR foreground escape for a normalized position `t`
/// along the diagonal, compositing the optional sliding shine highlight.
pub fn gradient_escape(t: f64, shine: Option<ShineConfig>, true_color: bool) -> String {
	let shine_strength = shine.map(|shine| shine.strength).filter(|strength| *strength > 0.0);
	let shine_pos = shine.map(|shine| shine.pos).unwrap_or(0.0);
	let intensity = |t: f64| {
		let Some(strength) = shine_strength else {
			return 0.0;
		};
		(1.0 - (t - shine_pos).abs() / SHINE_HALF_WIDTH).max(0.0) * strength
	};
	if true_color {
		let stops = GRADIENT_STOPS;
		let seg = t * (stops.len() - 1) as f64;
		let index = ((seg.floor() as usize).min(stops.len() - 2)).min(stops.len() - 1);
		let fraction = seg - index as f64;
		let a = stops[index];
		let b = stops[(index + 1).min(stops.len() - 1)];
		let mut r = a.0 as f64 + (b.0 as f64 - a.0 as f64) * fraction;
		let mut g = a.1 as f64 + (b.1 as f64 - a.1 as f64) * fraction;
		let mut bl = a.2 as f64 + (b.2 as f64 - a.2 as f64) * fraction;
		let amount = intensity(t);
		if amount > 0.0 {
			r += (255.0 - r) * amount;
			g += (255.0 - g) * amount;
			bl += (255.0 - bl) * amount;
		}
		return format!(
			"\x1b[38;2;{};{};{}m",
			r.round(),
			g.round(),
			bl.round()
		);
	}
	let ramp = GRADIENT_RAMP_256;
	let mut index = ((t * (ramp.len() - 1) as f64 + 0.5).floor().max(0.0) as usize).min(ramp.len() - 1);
	if intensity(t) > 0.5 {
		index = ramp.len() - 1;
	}
	format!("\x1b[38;5;{}m", ramp[index])
}

/// Apply a multi-stop diagonal gradient (top-left → bottom-right) plus an
/// optional sliding shine band across multi-line art. `phase` (0..1) shifts the
/// gradient along the diagonal, wrapping at 1.
pub fn gradient_logo(lines: &[String], phase: f64, shine: Option<ShineConfig>, true_color: bool) -> Vec<String> {
	let reset = "\x1b[0m";
	let rows = lines.len();
	let cols = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0);
	let x_span = (cols.max(1) - 1) as f64;
	let y_span = (rows.max(1) - 1) as f64;
	let normalized_phase = phase.rem_euclid(1.0);
	lines
		.iter()
		.enumerate()
		.map(|(y, line)| {
			let mut result = String::new();
			for (x, ch) in line.chars().enumerate() {
				if ch == ' ' {
					result.push(ch);
					continue;
				}
				let base = (x as f64 / x_span + y as f64 / y_span) / 2.0;
				let t = if normalized_phase == 0.0 {
					base
				} else {
					(base + normalized_phase) % 1.0
				};
				result.push_str(&gradient_escape(t, shine, true_color));
				result.push(ch);
				result.push_str(reset);
			}
			result
		})
		.collect()
}

/// Logo frame for a normalized intro progress in [0, 1): ease-out cubic, the
/// gradient sweeping backward, and a fading shine highlight.
pub fn intro_logo_frame(progress: f64, true_color: bool) -> Vec<String> {
	let eased = 1.0 - (1.0 - progress).powi(3);
	let phase = (((1.0 - eased) * INTRO_SWEEPS) % 1.0 + 1.0) % 1.0;
	let shine_pos = ((progress * INTRO_SHINE_TRAVERSALS) % 1.0 + 1.0) % 1.0;
	let shine_strength = (1.0 - eased).powf(1.5);
	let lines: Vec<String> = PI_LOGO.iter().map(|row| row.to_string()).collect();
	gradient_logo(
		&lines,
		phase,
		Some(ShineConfig {
			strength: shine_strength,
			pos: shine_pos,
		}),
		true_color,
	)
}

/// Resting gradient frame, cached for re-renders outside the intro.
static REST_FRAME: LazyLock<Vec<String>> = LazyLock::new(|| {
	let lines: Vec<String> = PI_LOGO.iter().map(|row| row.to_string()).collect();
	gradient_logo(&lines, 0.0, None, true)
});

/// The session's welcome banner.
pub struct WelcomeComponent {
	version: String,
	version_update: Option<VersionUpdate>,
	nag_roll: Option<f64>,
	tip_roll: Option<f64>,
	anim_start: Option<Instant>,
	cached_width: Option<usize>,
	cached_lines: Option<Vec<String>>,
}

impl WelcomeComponent {
	pub fn new(version: impl Into<String>) -> Self {
		WelcomeComponent {
			version: version.into(),
			version_update: None,
			nag_roll: None,
			tip_roll: None,
			anim_start: None,
			cached_width: None,
			cached_lines: None,
		}
	}

	/// The tip of the session, latched once so it is stable across renders.
	pub fn tip(&mut self, env: &WelcomeEnv<'_>) -> Option<String> {
		let nag_roll = *self.nag_roll.get_or_insert_with(rand_unit);
		let tip_roll = *self.tip_roll.get_or_insert_with(rand_unit);
		if env.theme.get_symbol_preset() == crate::theme::symbols::SymbolPreset::Unicode && nag_roll < 0.1 {
			return Some("Please use nerdfont 😭.".to_string());
		}
		let pool = if env.native_rendering { &*NATIVE_TIPS } else { &*TIPS };
		let tip = pick_weighted_tip(pool, tip_roll);
		(!tip.is_empty()).then_some(tip)
	}

	/// Drop cached rows after a data or settings change.
	pub fn invalidate(&mut self) {
		self.cached_width = None;
		self.cached_lines = None;
	}

	/// The intro keeps the welcome block mutable; settling lets it retire.
	pub fn is_transcript_block_finalized(&self) -> bool {
		self.anim_start.is_none()
	}

	/// Start (or restart) the one-shot intro; the caller repaints until
	/// [`WelcomeComponent::stop_intro`].
	pub fn play_intro(&mut self, env: &WelcomeEnv<'_>) {
		self.stop_intro();
		// A native surface shows the settled card right away.
		if env.native_rendering {
			return;
		}
		self.anim_start = Some(Instant::now());
	}

	/// Stop the intro immediately and settle on the resting frame.
	pub fn stop_intro(&mut self) {
		if self.anim_start.take().is_some() {
			self.invalidate();
		}
	}

	/// Whether the intro is still animating.
	pub fn intro_running(&self) -> bool {
		self.anim_start.is_some()
	}

	pub fn set_version(&mut self, version: impl Into<String>) {
		self.version = version.into();
		self.invalidate();
	}

	pub fn version(&self) -> &str {
		&self.version
	}

	/// Show (or with `None`, clear) background self-update activity after the
	/// version label.
	pub fn set_version_update(&mut self, update: Option<VersionUpdate>) {
		self.version_update = update;
		self.invalidate();
	}

	pub fn version_update(&self) -> Option<&VersionUpdate> {
		self.version_update.as_ref()
	}

	fn current_logo_frame(&self, now: Instant, true_color: bool) -> Vec<String> {
		let Some(start) = self.anim_start else {
			return (*REST_FRAME).clone();
		};
		let elapsed = now.saturating_duration_since(start).as_secs_f64() * 1000.0;
		if elapsed >= INTRO_MS {
			return (*REST_FRAME).clone();
		}
		intro_logo_frame(elapsed / INTRO_MS, true_color)
	}

	/// Render the banner: centered lockup with the version (and update
	/// indicator), then the tip of the session.
	pub fn render(&mut self, term_width: usize, env: &WelcomeEnv<'_>, now: Instant) -> Vec<String> {
		let animating = self.anim_start.is_some();
		if !animating {
			if let (Some(cached), Some(width)) = (&self.cached_lines, self.cached_width) {
				if width == term_width {
					return cached.clone();
				}
			}
		}
		let lines = self.render_lines(term_width, env, now);
		if animating {
			self.cached_lines = None;
			self.cached_width = None;
		} else {
			self.cached_lines = Some(lines.clone());
			self.cached_width = Some(term_width);
		}
		lines
	}

	fn render_lines(&mut self, term_width: usize, env: &WelcomeEnv<'_>, now: Instant) -> Vec<String> {
		// Content keeps a column clear on each side; everything centers in the full width.
		let room = term_width.saturating_sub(2);
		if room < 4 {
			return Vec::new();
		}
		let logo = self.current_logo_frame(now, env.true_color);
		let version = env
			.theme
			.fg("dim", &format!("v{}", self.version))
			.unwrap_or_else(|_| format!("v{}", self.version));
		let lockup_width = logo_width() + LOCKUP_GAP + wordmark_width().max(visible_width(&version));
		let has_lockup = room >= lockup_width;
		let art = if has_lockup {
			lockup(&logo, &version, env.theme)
		} else if room >= logo_width() {
			logo
		} else {
			return center_block(&[], term_width);
		};
		let mut lines = center_block(&art, term_width);
		if has_lockup {
			if let Some(update) = self.version_update.clone() {
				append_version_update(&mut lines, &update, term_width, env.theme);
			}
		}
		let tip = if term_width >= TIP_MIN_COLUMNS {
			self.render_tip(room, env)
		} else {
			Vec::new()
		};
		if !tip.is_empty() {
			lines.push(String::new());
			for line in tip {
				lines.extend(center_block(&[line], term_width));
			}
		}
		lines
	}

	/// The tip wrapped to [`TIP_MEASURE`] columns (fewer when `room` is
	/// narrower).
	fn render_tip(&mut self, room: usize, env: &WelcomeEnv<'_>) -> Vec<String> {
		let Some(tip) = self.tip(env) else {
			return Vec::new();
		};
		let phase = if NEW_TIP_MARKER.is_match(&tip) {
			now_phase()
		} else {
			0.0
		};
		render_welcome_tip(&tip, room.min(TIP_MEASURE), phase, env)
	}
}

fn rand_unit() -> f64 {
	use std::time::{SystemTime, UNIX_EPOCH};
	// A deterministic-enough sample; tip selection only needs stability per
	// component, not statistical quality.
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.subsec_nanos())
		.unwrap_or(0);
	(nanos % 1_000_000) as f64 / 1_000_000.0
}

/// Rainbow phase for the "NEW!" tag, advancing with wall time.
fn now_phase() -> f64 {
	use std::time::{SystemTime, UNIX_EPOCH};
	let millis = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_millis() as f64)
		.unwrap_or(0.0);
	millis / NEW_GLOW_PERIOD_MS
}


#[cfg(test)]
mod tests {
	use super::*;
	use crate::key_hint_format::KeyHintFormatter;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};

	fn theme() -> Theme {
		let json = builtin_theme_json("dark").expect("builtin theme");
		create_theme(json, CreateThemeOptions::default()).expect("theme")
	}

	#[test]
	fn weighted_tips_bias_new_ones() {
		let tips = vec!["plain".to_string(), "shiny [NEW]".to_string()];
		// Weights 1:4; a plain sample below 0.2 lands on the first tip.
		assert_eq!(pick_weighted_tip(&tips, 0.0), "plain");
		assert_eq!(pick_weighted_tip(&tips, 0.19), "plain");
		assert_eq!(pick_weighted_tip(&tips, 0.2), "shiny [NEW]");
		assert_eq!(pick_weighted_tip(&tips, 0.99), "shiny [NEW]");
		assert_eq!(pick_weighted_tip(&[], 0.5), "");
	}

	#[test]
	fn own_tip_table_has_classic_and_native_pools() {
		assert!(TIPS.len() >= 8);
		assert_eq!(TIPS.len(), TIP_LINES.len());
		assert!(NATIVE_TIPS.len() < TIPS.len());
	}

	#[test]
	fn tip_needs_room() {
		let theme = theme();
		let keybindings = KeybindingsManager::new();
		let formatter = KeyHintFormatter::new(&theme);
		let env = WelcomeEnv {
			theme: &theme,
			formatter: &formatter,
			keybindings: &keybindings,
			true_color: true,
			native_rendering: false,
		};
		assert!(render_welcome_tip("hello", 12, 0.0, &env).is_empty());
		let lines = render_welcome_tip("hello", 40, 0.0, &env);
		assert_eq!(lines.len(), 1);
		assert!(lines[0].contains("Tip:"));
	}

	#[test]
	fn malformed_key_placeholders_stay_verbatim() {
		let theme = theme();
		let keybindings = KeybindingsManager::new();
		let formatter = KeyHintFormatter::new(&theme);
		assert_eq!(
			expand_tip_keys("{key:ctrl+}", &formatter, &keybindings),
			"{key:ctrl+}"
		);
		assert_eq!(
			expand_tip_keys("{keys:ctrl+}", &formatter, &keybindings),
			"{keys:ctrl+}"
		);
		let shift_tab = expand_tip_keys("{key:shift+tab}", &formatter, &keybindings);
		assert!(shift_tab.to_lowercase().contains("tab"));
		assert!(!shift_tab.contains('{'));
		let arrows = expand_tip_keys("{keys:up,down}", &formatter, &keybindings);
		assert!(arrows.contains('/'));
		assert!(!expand_tip_keys("{action:tui.editor.undo}", &formatter, &keybindings).is_empty());
	}

	#[test]
	fn tip_links_fall_back_to_plain_when_unlinked() {
		let theme = theme();
		let text = link_tip_text("[Tern](https://scl.so/tern)", &theme);
		assert!(text.contains("Tern"));
		assert!(text.contains("https://scl.so/tern"));
	}

	#[test]
	fn gradient_escape_encodes_both_modes() {
		let truecolor = gradient_escape(0.5, None, true);
		assert!(truecolor.starts_with("\x1b[38;2;"));
		let ansi256 = gradient_escape(0.5, None, false);
		assert!(ansi256.starts_with("\x1b[38;5;"));
		let shined = gradient_escape(0.5, Some(ShineConfig { strength: 1.0, pos: 0.5 }), true);
		assert!(shined.starts_with("\x1b[38;2;"));
	}

	#[test]
	fn gradient_logo_skips_blank_cells_and_wraps_phase() {
		let lines = vec!["ab cd".to_string()];
		let painted = gradient_logo(&lines, 0.0, None, true);
		assert_eq!(visible_width(&painted[0]), 5);
		assert!(painted[0].contains(' '));
		let wrapped = gradient_logo(&lines, -0.25, None, false);
		assert_eq!(visible_width(&wrapped[0]), 5);
	}

	#[test]
	fn intro_frame_settles_into_the_resting_frame() {
		let frame = intro_logo_frame(1.0, true);
		assert_eq!(frame.len(), PI_LOGO.len());
		let resting = gradient_logo(
			&PI_LOGO.iter().map(|row| row.to_string()).collect::<Vec<_>>(),
			0.0,
			None,
			true,
		);
		assert_eq!(frame, resting);
	}

	#[test]
	fn banner_renders_lockup_and_tip_once_wide_enough() {
		let theme = theme();
		let keybindings = KeybindingsManager::new();
		let formatter = KeyHintFormatter::new(&theme);
		let env = WelcomeEnv {
			theme: &theme,
			formatter: &formatter,
			keybindings: &keybindings,
			true_color: true,
			native_rendering: false,
		};
		let mut welcome = WelcomeComponent::new("1.2.3");
		// The nag roll is latched on first render; force the plain pool by
		// accepting whichever tip it picked.
		let lines = welcome.render(80, &env, Instant::now());
		assert!(lines.len() >= PI_LOGO.len());
		assert!(welcome.is_transcript_block_finalized());
		assert!(welcome.render(3, &env, Instant::now()).is_empty());

		welcome.set_version_update(Some(VersionUpdate::Ready {
			version: "1.3.0".to_string(),
		}));
		let lines = welcome.render(120, &env, Instant::now());
		assert!(lines.len() >= PI_LOGO.len());
	}

	#[test]
	fn new_tag_shimmers_between_phases() {
		let a = render_new_tag(0.0, ColorEncoding::TrueColor);
		let b = render_new_tag(0.5, ColorEncoding::TrueColor);
		assert_ne!(a, b);
		assert!(a.contains("NEW!"));
		assert!(render_new_tag(0.0, ColorEncoding::Color256).starts_with("\x1b[1m\x1b[38;5;"));
		assert_eq!(render_new_tag(1.5, ColorEncoding::TrueColor), render_new_tag(0.5, ColorEncoding::TrueColor));
	}
}
