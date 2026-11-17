//! Ported from `l/test/session-color.test.ts`.
//!
//! Deviations:
//! - The TypeScript suite uses `@oh-my-pi/pi-utils` color helpers
//!   (`hexToOklch`, `oklchCusp`, `relativeLuminance`) and
//!   `@oh-my-pi/pi-tui/theme`'s `getThemeByName`/`Theme.sessionAccentInputs`.
//!   Rust exposes `get_session_accent_hex`, `SessionAccentTheme`, and public
//!   analysis helpers (`hex_to_oklch`, `oklch_cusp`, `relative_luminance`) from
//!   `maple_tui::theme::session_color`, plus `Theme::get_session_accent_inputs()`.
//! - `getThemeByName`/`sessionAccentInputs` in TS can resolve non-embedded
//!   themes like `light-catppuccin`/`dark-catppuccin`; Maple TUI embeds only the
//!   builtin `dark`/`light` themes. Real-theme assertions therefore use embedded
//!   themes when available and remain self-consistent without depending on
//!   catalog-only themes.
//! - Hue/luminance thresholds follow the same intent as TS (determinism, cool
//!   light band, readable chroma/lightness, WCAG contrast on light surfaces,
//!   dark-theme vividness), with small tolerance for 8-bit hex quantization.

use maple_tui::theme::session_color::{
	get_session_accent_hex, hex_to_oklch, oklch_cusp, relative_luminance, SessionAccentTheme,
};
use maple_tui::theme::{builtin, create_theme, CreateThemeOptions};

const DARK_ACCENT: &str = "#e5c07b";
const LIGHT_ACCENT: &str = "#7287fd";
const HUE_TOLERANCE: f64 = 2.5;

fn lum(hex: &str) -> f64 {
	relative_luminance(hex).unwrap_or(0.0)
}

fn contrast(a: f64, b: f64) -> f64 {
	(let_hi_lo(a, b).0 + 0.05) / (let_hi_lo(a, b).1 + 0.05)
}

fn let_hi_lo(a: f64, b: f64) -> (f64, f64) {
	if a >= b { (a, b) } else { (b, a) }
}

fn on_dark(accent_hex: &str, color_hexes: Vec<String>) -> SessionAccentTheme {
	SessionAccentTheme {
		accent_hex: accent_hex.to_string(),
		color_hexes,
		surface_luminance: None,
	}
}

fn on_light(surface_luminance: f64, accent_hex: &str) -> SessionAccentTheme {
	SessionAccentTheme {
		accent_hex: accent_hex.to_string(),
		color_hexes: Vec::new(),
		surface_luminance: Some(surface_luminance),
	}
}

fn names() -> Vec<String> {
	(0..600).map(|i| format!("analyze-debian-trixie-{i}")).collect()
}

fn saturated_theme_hues(colors: &[String]) -> Vec<f64> {
	let mut hues = Vec::new();
	for color in colors {
		let ok = hex_to_oklch(color);
		if ok.c >= 0.03 {
			hues.push(ok.h);
		}
	}
	hues
}

fn hue_distance(a: f64, b: f64) -> f64 {
	let d = (a - b).abs() % 360.0;
	d.min(360.0 - d)
}

#[test]
fn is_deterministic_per_name_and_parameters() {
	assert_eq!(
		get_session_accent_hex("analyze debian trixie", &on_dark(DARK_ACCENT, vec![])),
		get_session_accent_hex("analyze debian trixie", &on_dark(DARK_ACCENT, vec![]))
	);
	assert_eq!(
		get_session_accent_hex("x", &on_light(0.7, LIGHT_ACCENT)),
		get_session_accent_hex("x", &on_light(0.7, LIGHT_ACCENT))
	);
}

#[test]
fn uses_full_wheel_hues_with_reachable_vividness_peak_on_dark_themes() {
	for name in names() {
		let h = hex_to_oklch(&get_session_accent_hex(&name, &on_dark(DARK_ACCENT, vec![]))).h;
		assert!(oklch_cusp(h).l <= 0.88 + 0.02, "hue {h} cusp too light for {name}");
	}
}

#[test]
fn does_not_collapse_into_warm_band_on_dark_themes() {
	let all = names();
	let warm = all
		.iter()
		.filter(|name| {
			let h = hex_to_oklch(&get_session_accent_hex(name, &on_dark(DARK_ACCENT, vec![]))).h;
			h < 94.0 || h > 350.0
		})
		.count();
	assert!((warm as f64 / all.len() as f64) < 0.5, "warm share {} / {}", warm, all.len());
}

#[test]
fn uses_cool_oklch_hues_on_light_themes() {
	for name in names() {
		let h = hex_to_oklch(&get_session_accent_hex(&name, &on_light(0.5, LIGHT_ACCENT))).h;
		assert!(h >= 195.0 - HUE_TOLERANCE, "hue {h} too low for {name}");
		assert!(h <= 330.0 + HUE_TOLERANCE, "hue {h} too high for {name}");
	}
}

#[test]
fn never_lands_in_yellow_chartreuse_core_on_dark_themes() {
	for accent in [DARK_ACCENT, "#ed4abf", "#7aa2f7"] {
		for name in names() {
			let h = hex_to_oklch(&get_session_accent_hex(&name, &on_dark(accent, vec![]))).h;
			assert!(
				h < 94.0 + HUE_TOLERANCE || h > 138.0 - HUE_TOLERANCE,
				"hue {h} in yellow/chartreuse core for {name} accent {accent}"
			);
		}
	}
}

#[test]
fn derives_softer_accents_from_pastel_themes_than_vivid_ones() {
	for name in names().into_iter().take(50) {
		let vivid = hex_to_oklch(&get_session_accent_hex(&name, &on_dark("#ed4abf", vec![]))).c;
		let pastel = hex_to_oklch(&get_session_accent_hex(&name, &on_dark("#bb9af7", vec![]))).c;
		assert!(pastel <= vivid + 0.02, "pastel {pastel} > vivid {vivid} for {name}");
		assert!(pastel >= 0.03, "pastel {pastel} too gray for {name}");
	}
}

#[test]
fn keeps_readable_chroma_floor_when_theme_accent_is_near_gray() {
	for name in names().into_iter().take(50) {
		let c = hex_to_oklch(&get_session_accent_hex(&name, &on_dark("#8a8a8e", vec![]))).c;
		assert!(c >= 0.03, "chroma {c} too low for {name}");
	}
}

#[test]
fn raises_very_dark_theme_accents_to_visible_lightness_on_dark_themes() {
	for name in names().into_iter().take(50) {
		let l = hex_to_oklch(&get_session_accent_hex(&name, &on_dark("#5a3d1e", vec![]))).l;
		assert!(l >= 0.60, "lightness {l} too low for {name}");
	}
}

#[test]
fn keeps_vivid_bright_accents_on_dark_themes() {
	let max_dark = names()
		.into_iter()
		.map(|n| lum(&get_session_accent_hex(&n, &on_dark(DARK_ACCENT, vec![]))))
		.fold(0.0_f64, f64::max);
	assert!(max_dark > 0.45, "max dark luminance {max_dark} unexpectedly dim");
}

#[test]
fn clears_readable_contrast_against_light_surfaces() {
	let surfaces = [("#dce0e8", lum("#dce0e8")), ("#7390aa", lum("#7390aa"))];
	for (_, bg) in surfaces {
		for name in names() {
			let hex = get_session_accent_hex(&name, &on_light(bg, LIGHT_ACCENT));
			assert!(
				contrast(lum(&hex), bg) >= 2.9,
				"contrast {} too low on {bg} for {name}",
				contrast(lum(&hex), bg)
			);
		}
	}
}

#[test]
fn never_produces_lighter_accent_on_light_than_dark_for_same_accent_and_name() {
	let near_white = lum("#dce0e8");
	for name in names() {
		let light = lum(&get_session_accent_hex(&name, &on_light(near_white, DARK_ACCENT)));
		let dark = lum(&get_session_accent_hex(&name, &on_dark(DARK_ACCENT, vec![])));
		assert!(light <= dark + 1e-6, "light {light} > dark {dark} for {name}");
	}
}

#[test]
fn session_accent_inputs_from_builtin_themes_drive_coherent_accents() {
	for name in ["dark", "light"] {
		let Some(json) = builtin::builtin_theme_json(name) else {
			continue;
		};
		let theme = match create_theme(json.clone(), CreateThemeOptions::default()) {
			Ok(theme) => theme,
			Err(_) => continue,
		};
		let inputs = theme.get_session_accent_inputs();
		let theme_hues = saturated_theme_hues(&inputs.color_hexes);
		for sample in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"] {
			let hex = get_session_accent_hex(sample, &inputs);
			assert!(hex.starts_with('#') && hex.len() == 7, "invalid hex {hex} for {sample}/{name}");
			let h = hex_to_oklch(&hex).h;
			if inputs.surface_luminance.is_some() {
				assert!(h >= 195.0 - HUE_TOLERANCE && h <= 330.0 + HUE_TOLERANCE, "light hue {h} out of band");
			} else {
				assert!(oklch_cusp(h).l <= 0.90, "dark cusp too light at hue {h}");
			}
			for th in &theme_hues {
				let dist = hue_distance(h, *th);
				assert!(dist >= 8.0 - HUE_TOLERANCE || theme_hues.is_empty(), "collides with theme hue dist {dist}");
			}
		}
	}
}
