//! Status-line label color helpers for the Git TUI.
//!
//! Reference: `l/src/apps/git/colors.ts`.
//!
//! The Git status line renders small filled labels (`pill`, `chipFill`) and
//! tinted chips whose contrast text is derived from the fill color. These
//! helpers port that math against [`Theme`] instead of a module-level global.

use crate::theme::color::{color_luma, color_to_ansi, mix_hex, parse_hex, ColorMode, ColorValue};
use crate::theme::theme_class::Theme;

/// Split a `#rgb`/`#rrggbb` string into its channels.
pub fn hex_channels(hex: &str) -> Option<(u8, u8, u8)> {
	parse_hex(hex)
}

/// Truecolor background SGR sequence for a hex color.
pub fn bg_ansi_hex(hex: &str) -> String {
	match color_to_ansi(hex, ColorMode::TrueColor) {
		Ok(ansi) => ansi.replace("\x1b[38;", "\x1b[48;"),
		Err(_) => String::new(),
	}
}

/// Truecolor foreground SGR sequence for a hex color.
pub fn fg_ansi_hex(hex: &str) -> String {
	color_to_ansi(hex, ColorMode::TrueColor).unwrap_or_default()
}

/// Whether the active theme's status line is dark.
pub fn is_dark(theme: &Theme) -> bool {
	!theme.is_light()
}

fn is_six_digit_hex(value: &str) -> bool {
	let bytes = value.as_bytes();
	bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

fn dark_fallback(theme: &Theme) -> String {
	if is_dark(theme) {
		"#000000".to_string()
	} else {
		"#ffffff".to_string()
	}
}

/// Status-line canvas hex, validated; falls back to black/white.
pub fn canvas_hex(theme: &Theme) -> String {
	let candidate = theme.get_bg_hex("statusLineBg").unwrap_or_default();
	if is_six_digit_hex(&candidate) {
		return candidate;
	}
	dark_fallback(theme)
}

/// Text hex from the theme; empty when unavailable.
pub fn text_hex(theme: &Theme) -> String {
	theme.get_color_hex("text").unwrap_or_default()
}

/// Perceptual luma (0..1) of a hex color; `0.0` for malformed input.
pub fn luminance(hex: &str) -> f64 {
	color_luma(&ColorValue::Hex(hex.to_string())).unwrap_or(0.0)
}

/// Wrap text so every reset restores `bg` instead of the terminal default.
pub fn with_bg(text: &str, bg: &str) -> String {
	text.replace("\x1b[0m", &format!("\x1b[0m{bg}")).replace("\x1b[49m", bg)
}

/// Text color that stays readable on `fill`: darkened light fills, lightened
/// dark fills.
fn label_hex(fill: &str) -> String {
	if luminance(fill) > 0.5 {
		mix_hex(fill, "#000000", 0.82)
	} else {
		mix_hex(fill, "#ffffff", 0.92)
	}
}

/// Options for [`pill`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PillOptions {
	/// Selected pills fill with a blend toward the text color.
	pub selected: bool,
	/// Dimmed pills blend toward the canvas.
	pub dim: bool,
}

/// Render a single rounded label cell using vertical half-block glyphs.
pub fn pill(theme: &Theme, label: &str, hex: &str, options: PillOptions) -> String {
	let text = text_hex(theme);
	let canvas = canvas_hex(theme);
	let fill = if options.selected {
		mix_hex(hex, &text, 0.22)
	} else if options.dim {
		mix_hex(hex, &canvas, 0.55)
	} else {
		hex.to_string()
	};
	let label_color = label_hex(&fill);
	format!(
		"{}▐{}{}{}\x1b[0m{}▌\x1b[0m",
		fg_ansi_hex(&fill),
		bg_ansi_hex(&fill),
		fg_ansi_hex(&label_color),
		label,
		fg_ansi_hex(&fill)
	)
}

/// Render a square-cornered chip: filled background plus contrasting label.
pub fn chip_fill(label: &str, hex: &str) -> String {
	let label_color = label_hex(hex);
	format!("{}{}{}\x1b[0m", bg_ansi_hex(hex), fg_ansi_hex(&label_color), label)
}

/// Background SGR for the selection region.
pub fn selection_bg_ansi(theme: &Theme, dim: bool) -> String {
	let canvas = canvas_hex(theme);
	let text = text_hex(theme);
	let mix = if dim { 0.08 } else { 0.14 };
	bg_ansi_hex(&mix_hex(&canvas, &text, mix))
}

/// Render a tinted chip: low-alpha fill of `hex` over the canvas.
pub fn tint_chip(theme: &Theme, label: &str, hex: &str) -> String {
	let canvas = canvas_hex(theme);
	let bg = mix_hex(&canvas, hex, 0.18);
	format!("{}{}{}\x1b[0m", bg_ansi_hex(&bg), fg_ansi_hex(hex), label)
}

/// Options for [`soft_pill`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoftPillOptions {
	/// Active soft pills use the theme accent as a solid chip.
	pub active: bool,
}

/// Render an outlined (non-filled) label used for counts and placeholders.
pub fn soft_pill(theme: &Theme, label: &str, options: SoftPillOptions) -> String {
	if options.active {
		let accent = theme.get_color_hex("accent").unwrap_or_default();
		return chip_fill(label, &accent);
	}
	let canvas = canvas_hex(theme);
	let text = text_hex(theme);
	let bg = mix_hex(&canvas, &text, 0.1);
	let fg = mix_hex(&canvas, &text, 0.62);
	format!("{}{}{}\x1b[0m", bg_ansi_hex(&bg), fg_ansi_hex(&fg), label)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::default_theme;

	fn theme() -> Theme {
		default_theme()
	}

	#[test]
	fn hex_channels_parses_short_and_long_hex() {
		assert_eq!(hex_channels("#ff0000"), Some((255, 0, 0)));
		assert_eq!(hex_channels("#fff"), Some((255, 255, 255)));
		assert_eq!(hex_channels("nope"), None);
	}

	#[test]
	fn ansi_helpers_emit_truecolor_sgr() {
		assert_eq!(fg_ansi_hex("#ff0000"), "\x1b[38;2;255;0;0m");
		assert_eq!(bg_ansi_hex("#ff0000"), "\x1b[48;2;255;0;0m");
		assert_eq!(fg_ansi_hex("bad"), "");
	}

	#[test]
	fn luminance_tracks_perceived_brightness() {
		assert_eq!(luminance("#000000"), 0.0);
		assert!(luminance("#ffffff") > 0.9);
		assert!(luminance("#ffffff") > luminance("#808080"));
	}

	#[test]
	fn with_bg_restores_background_after_resets() {
		assert_eq!(with_bg("a\x1b[0mb", "#112233"), "#112233a\x1b[0m#112233b");
		assert_eq!(with_bg("a\x1b[49mb", "#112233"), "a#112233b");
	}

	#[test]
	fn canvas_is_validated_and_text_comes_from_the_theme() {
		let theme = theme();
		let canvas = canvas_hex(&theme);
		assert!(is_six_digit_hex(&canvas), "canvas was {canvas}");
		assert!(!text_hex(&theme).is_empty());
		assert!(is_dark(&theme()));
	}

	#[test]
	fn pill_renders_half_blocks_with_a_contrasting_label() {
		let rendered = pill(&theme(), "main", "#2e7d32", PillOptions::default());
		assert!(rendered.contains('▐'));
		assert!(rendered.contains('▌'));
		assert!(rendered.contains("main"));
		assert!(rendered.starts_with("\x1b[38;2;"));
	}

	#[test]
	fn chip_fill_uses_the_supplied_fill() {
		let rendered = chip_fill("M", "#1565c0");
		assert!(rendered.contains("M"));
		assert!(rendered.starts_with("\x1b[48;2;21;101;192m"));
	}

	#[test]
	fn selection_background_blends_canvas_and_text() {
		let normal = selection_bg_ansi(&theme(), false);
		let dim = selection_bg_ansi(&theme(), true);
		assert_ne!(normal, dim);
		assert!(normal.starts_with("\x1b[48;2;"));
	}

	#[test]
	fn tint_chip_and_soft_pill_render_labels() {
		let theme = theme();
		let tinted = tint_chip(&theme, "2", "#f9a825");
		assert!(tinted.contains("2"));
		assert!(tinted.starts_with("\x1b[48;2;"));
		assert!(soft_pill(&theme, "idle", SoftPillOptions::default()).contains("idle"));
		assert!(soft_pill(&theme, "active", SoftPillOptions { active: true }).contains("active"));
	}
}
