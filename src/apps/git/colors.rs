//! Truecolor helpers for the git TUI, derived from the active theme.
//!
//! Reference: `l/src/apps/git/colors.ts`. Surface tints mix theme colors toward
//! the theme's own canvas, and filled pill buttons pick their label contrast from
//! the button color's luminance.
//!
//! Deviation: the reference reads the global `theme` singleton; this port takes
//! the resolved values through [`GitPalette`], built once from a
//! [`maple_tui::theme::Theme`]. No global theme state exists in maple-tui.

use maple_tui::theme::color::{color_luma, color_to_ansi, mix_hex, parse_hex, ColorValue, ColorMode};
use maple_tui::theme::Theme;

const BLACK: &str = "#000000";
const WHITE: &str = "#ffffff";

/// Theme colors the git chrome needs, resolved once from a [`Theme`].
#[derive(Clone, Debug)]
pub struct GitPalette {
    /// Theme canvas: the surface diff tints and pills blend toward.
    pub canvas: String,
    /// Theme default text color, used as the "bright" mix pole.
    pub text: String,
    /// Theme accent, used by the active toggle chip.
    pub accent: String,
    /// True when the theme sits on a dark surface.
    pub dark: bool,
}

impl GitPalette {
    /// Resolve the palette from a theme's `statusLineBg`, `text` and `accent`.
    pub fn from_theme(theme: &Theme) -> Self {
        let dark = !theme.is_light();
        let canvas = theme
            .get_bg_hex("statusLineBg")
            .ok()
            .filter(|hex| is_six_digit_hex(hex))
            .unwrap_or_else(|| {
                if dark {
                    BLACK.to_string()
                } else {
                    WHITE.to_string()
                }
            });
        Self {
            canvas,
            text: theme.get_color_hex("text").unwrap_or_default(),
            accent: theme.get_color_hex("accent").unwrap_or_default(),
            dark,
        }
    }
}

fn is_six_digit_hex(hex: &str) -> bool {
    hex.len() == 7 && hex.starts_with('#') && hex[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Decode a hex color into RGB channels.
pub fn hex_channels(hex: &str) -> Option<(u8, u8, u8)> {
    parse_hex(hex)
}

/// Encode a truecolor background escape sequence.
pub fn bg_ansi_hex(hex: &str) -> Result<String, String> {
    Ok(color_to_ansi(hex, ColorMode::TrueColor)?.replace("\x1b[38;", "\x1b[48;"))
}

/// Encode a truecolor foreground escape sequence.
pub fn fg_ansi_hex(hex: &str) -> Result<String, String> {
    color_to_ansi(hex, ColorMode::TrueColor)
}

/// Perceptual luminance of a hex color (0..1).
pub fn luminance(hex: &str) -> Option<f64> {
    color_luma(&ColorValue::Hex(hex.to_string()))
}

/// Pick a label color that stays readable on `fill`.
fn label_hex(fill: &str) -> String {
    match luminance(fill) {
        Some(value) if value > 0.5 => mix_hex(fill, BLACK, 0.82),
        _ => mix_hex(fill, WHITE, 0.92),
    }
}

/// Re-assert a background after any reset inside syntax-highlighted text so a
/// row tint survives token boundaries emitted by the highlighter.
pub fn with_bg(text: &str, bg: &str) -> String {
    let reasserted = text
        .replace("\x1b[0m", &format!("\x1b[0m{bg}"))
        .replace("\x1b[49m", bg);
    format!("{bg}{reasserted}")
}

/// Filled pill button with half-block end caps (`▐ label ▌`).
///
/// Visible width is `label.length + 2`.
pub fn pill(label: &str, hex: &str, palette: &GitPalette, options: PillOptions) -> String {
    let fill = if options.selected {
        mix_hex(hex, &palette.text, 0.22)
    } else if options.dim {
        mix_hex(hex, &palette.canvas, 0.55)
    } else {
        hex.to_string()
    };
    let label_hex = label_hex(&fill);
    let fg = fg_ansi_hex(&fill).unwrap_or_default();
    let bg = bg_ansi_hex(&fill).unwrap_or_default();
    let label_fg = fg_ansi_hex(&label_hex).unwrap_or_default();
    format!("{fg}\u{2590}{bg}{label_fg}{label}\x1b[0m{fg}\u{258c}\x1b[0m")
}

/// Pill styling choices.
#[derive(Clone, Copy, Debug, Default)]
pub struct PillOptions {
    pub selected: bool,
    pub dim: bool,
}

/// Flat filled chip with a contrast-computed label.
pub fn chip_fill(label: &str, hex: &str) -> String {
    let label_hex = label_hex(hex);
    let bg = bg_ansi_hex(hex).unwrap_or_default();
    let fg = fg_ansi_hex(&label_hex).unwrap_or_default();
    format!("{bg}{fg}{label}\x1b[0m")
}

/// Selection-row background: a canvas-adjacent surface so every theme foreground
/// stays readable on it. `dim` renders a fainter band for unfocused panes.
pub fn selection_bg_ansi(palette: &GitPalette, dim: bool) -> String {
    let mix = if dim { 0.08 } else { 0.14 };
    let hex = mix_hex(&palette.canvas, &palette.text, mix);
    bg_ansi_hex(&hex).unwrap_or_default()
}

/// Tinted chip: faint fill of a theme color with the full color as label.
pub fn tint_chip(label: &str, hex: &str, palette: &GitPalette) -> String {
    let bg = bg_ansi_hex(&mix_hex(&palette.canvas, hex, 0.18)).unwrap_or_default();
    let fg = fg_ansi_hex(hex).unwrap_or_default();
    format!("{bg}{fg}{label}\x1b[0m")
}

/// Subtle toggle chip: accent fill when active, neutral surface otherwise.
pub fn soft_pill(label: &str, palette: &GitPalette, active: bool) -> String {
    if active {
        return chip_fill(label, &palette.accent);
    }
    let bg = bg_ansi_hex(&mix_hex(&palette.canvas, &palette.text, 0.1)).unwrap_or_default();
    let fg = fg_ansi_hex(&mix_hex(&palette.canvas, &palette.text, 0.62)).unwrap_or_default();
    format!("{bg}{fg}{label}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::*;
    use maple_tui::theme::{builtin, create_theme, CreateThemeOptions};

    fn palette(name: &str) -> GitPalette {
        let json = builtin::builtin_theme_json(name).expect("builtin theme");
        let theme =
            create_theme(json, CreateThemeOptions { mode: Some(ColorMode::TrueColor), ..Default::default() })
                .expect("theme");
        GitPalette::from_theme(&theme)
    }

    #[test]
    fn channels_and_luminance_read_hex() {
        assert_eq!(hex_channels("#ff0000"), Some((255, 0, 0)));
        assert_eq!(hex_channels("#000000"), Some((0, 0, 0)));
        assert_eq!(hex_channels("nope"), None);
        assert!((luminance("#ffffff").unwrap() - luminance("#000000").unwrap()).abs() > 0.5);
        assert!(luminance("nope").is_none());
    }

    #[test]
    fn ansi_escapes_use_truecolor() {
        assert_eq!(fg_ansi_hex("#ff0000").as_deref(), Ok("\x1b[38;2;255;0;0m"));
        assert_eq!(bg_ansi_hex("#ff0000").as_deref(), Ok("\x1b[48;2;255;0;0m"));
        assert!(fg_ansi_hex("nope").is_err());
    }

    #[test]
    fn palettes_follow_theme_surface() {
        assert!(palette("dark").dark);
        assert!(!palette("light").dark);
        assert!(is_six_digit_hex(&palette("dark").canvas));
        assert!(!palette("dark").text.is_empty());
    }

    #[test]
    fn with_bg_reasserts_after_resets() {
        let tinted = with_bg("a\x1b[0mb\x1b[49mc", "\x1b[48;2;1;2;3m");
        assert_eq!(
            tinted,
            "\x1b[48;2;1;2;3ma\x1b[0m\x1b[48;2;1;2;3mb\x1b[48;2;1;2;3mc"
        );
    }

    #[test]
    fn pills_caps_and_chips_carry_label_contrast() {
        let palette = palette("dark");
        let plain = pill("run", "#ffffff", &palette, PillOptions::default());
        assert!(plain.contains('\u{2590}'));
        assert!(plain.contains("\u{258c}\x1b[0m"));
        assert!(plain.contains("run"));

        let selected = pill("run", "#ffffff", &palette, PillOptions { selected: true, ..PillOptions::default() });
        let dim = pill("run", "#ffffff", &palette, PillOptions { dim: true, ..PillOptions::default() });
        assert_ne!(selected, dim);
        assert_ne!(selected, plain);

        let chip = chip_fill("ok", "#ffffff");
        assert!(chip.contains("ok"));
        assert!(chip.ends_with("\x1b[0m"));
    }

    #[test]
    fn selection_and_tint_chips_mix_toward_canvas() {
        let palette = palette("dark");
        let normal = selection_bg_ansi(&palette, false);
        let dim = selection_bg_ansi(&palette, true);
        assert_ne!(normal, dim);
        assert!(normal.starts_with("\x1b[48;"));

        let chip = tint_chip("new", "#ff0000", &palette);
        assert!(chip.contains("new"));
        assert!(chip.ends_with("\x1b[0m"));
    }

    #[test]
    fn soft_pill_uses_accent_only_when_active() {
        let palette = palette("dark");
        assert_eq!(soft_pill("on", &palette, true), chip_fill("on", &palette.accent));
        assert_ne!(soft_pill("off", &palette, false), soft_pill("on", &palette, true));
    }
}
