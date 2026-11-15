//! Theme loading: JSON parsing/validation, discovery and [`create_theme`].
//!
//! Ported from `l/src/theme/loader.ts`.
//!
//! # Divergences
//!
//! - The custom-theme directory is `PI_TUI_THEMES_DIR` or `~/.pi/tui/themes`,
//!   not upstream's `~/.omp/agent/themes` (no `dirs` crate is available).
//! - [`load_theme_json`] and [`load_theme_json_sync`] are the same synchronous
//!   operation; this port has no async runtime.
//! - JSON parse errors surface `serde_json`'s message instead of JavaScript's
//!   `SyntaxError: …` text.
//! - `get_available_themes*` sort by UTF-8 code unit rather than
//!   `localeCompare`.

use std::path::PathBuf;

use crate::theme::builtin;
use crate::theme::color::{
	adjust_hsv, detect_color_mode, is_light_surface, mix_theme_colors, resolve_theme_colors,
	resolve_var_refs, ColorMode, ColorValue, HsvAdjustment,
};
use crate::theme::schema::{validate_theme_json, ColorMap, ThemeJson};
use crate::theme::symbols::{normalize_spinner_frames_override, SymbolPreset};
use crate::theme::theme_class::Theme;

/// HSV adjustment applied to `toolDiffAdded` in colorblind mode, shifting green
/// toward blue for red-green colorblindness.
pub const COLORBLIND_ADJUSTMENT: HsvAdjustment = HsvAdjustment {
	h: Some(60.0),
	s: Some(0.71),
	v: None,
};

/// Share of `mdLink` in the derived `mdExtensionLink`; the rest is `text`.
pub const EXTENSION_LINK_TINT: f64 = 0.45;

/// The exact background-token key set; every other resolved color is a
/// foreground token.
const BG_COLOR_KEYS: &[&str] = &[
	"selectedBg",
	"userMessageBg",
	"customMessageBg",
	"toolPendingBg",
	"toolSuccessBg",
	"toolErrorBg",
	"statusLineBg",
];

/// Options for [`create_theme`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreateThemeOptions {
	/// Override the detected color mode.
	pub mode: Option<ColorMode>,
	/// Override the theme's symbol preset.
	pub symbol_preset_override: Option<SymbolPreset>,
	/// Shift `toolDiffAdded` hue for red-green colorblindness.
	pub color_blind_mode: Option<bool>,
}

/// A discoverable theme: name plus an optional custom file path.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeInfo {
	/// Theme name.
	pub name: String,
	/// Custom file path; `None` for embedded themes.
	pub path: Option<PathBuf>,
}

/// The custom-theme directory: `$PI_TUI_THEMES_DIR` when set, else
/// `~/.pi/tui/themes`.
pub fn get_custom_themes_dir() -> PathBuf {
	if let Some(dir) = std::env::var_os("PI_TUI_THEMES_DIR") {
		if !dir.is_empty() {
			return PathBuf::from(dir);
		}
	}
	match std::env::var_os("HOME") {
		Some(home) if !home.is_empty() => PathBuf::from(home).join(".pi/tui/themes"),
		_ => PathBuf::from(".pi/tui/themes"),
	}
}

/// Parse and validate theme JSON.
///
/// Parse failures are reported as `Failed to parse theme <name>: <error>`;
/// validation failures use the `Invalid theme "<name>":` envelope with the
/// missing-token list and the underlying validation error.
pub fn parse_theme_json(name: &str, content: &str) -> Result<ThemeJson, String> {
	let value: serde_json::Value = serde_json::from_str(content)
		.map_err(|error| format!("Failed to parse theme {}: {}", name, error))?;
	if let Err(message) = validate_theme_json(&value) {
		return Err(invalid_theme_message(name, &message));
	}
	serde_json::from_value(value).map_err(|error| {
		format!(
			"Invalid theme \"{}\":\n\nValidation error:\n  - {}",
			name, error
		)
	})
}

/// Build the user-facing validation envelope, mirroring `parseThemeJson`.
fn invalid_theme_message(name: &str, message: &str) -> String {
	let mut out = format!("Invalid theme \"{}\":\n", name);
	let missing = parse_missing_keys(message);
	if !missing.is_empty() {
		out.push_str("\nMissing required color tokens:\n");
		out.push_str(
			&missing
				.iter()
				.map(|key| format!("  - {}", key))
				.collect::<Vec<_>>()
				.join("\n"),
		);
		out.push_str("\n\nPlease add these colors to your theme's \"colors\" object.");
		out.push_str("\nSee the built-in themes (dark.json, light.json) for reference values.");
	}
	out.push_str(&format!("\n\nValidation error:\n  - {}", message));
	out
}

/// Extract the comma-separated key list from a `missing keys: …` message.
fn parse_missing_keys(message: &str) -> Vec<String> {
	let needle = "missing keys: ";
	let Some(position) = message.to_ascii_lowercase().find(needle) else {
		return Vec::new();
	};
	let rest = &message[position + needle.len()..];
	rest.split('\n')
		.next()
		.unwrap_or("")
		.split(',')
		.map(str::trim)
		.filter(|key| !key.is_empty())
		.map(str::to_string)
		.collect()
}

/// Load a theme definition: an embedded theme by name, else
/// `<custom-themes-dir>/<name>.json`.
pub fn load_theme_json(name: &str) -> Result<ThemeJson, String> {
	load_theme_json_sync(name)
}

/// Synchronous variant of [`load_theme_json`] for latency-sensitive first paint.
pub fn load_theme_json_sync(name: &str) -> Result<ThemeJson, String> {
	if let Some(theme) = builtin::builtin_theme_json(name) {
		return Ok(theme.clone());
	}
	let path = get_custom_themes_dir().join(format!("{}.json", name));
	match std::fs::read_to_string(&path) {
		Ok(content) => parse_theme_json(name, &content),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
			Err(format!("Theme not found: {}", name))
		}
		Err(error) => Err(format!("Failed to read theme {}: {}", name, error)),
	}
}

/// Every available theme name: embedded names plus `*.json` basenames in the
/// custom-theme directory, deduplicated and sorted by code unit.
pub fn get_available_themes() -> Vec<String> {
	let mut names: Vec<String> = builtin::builtin_theme_names()
		.iter()
		.map(|name| (*name).to_string())
		.collect();
	if let Ok(entries) = std::fs::read_dir(get_custom_themes_dir()) {
		for entry in entries.flatten() {
			let file_name = entry.file_name();
			if let Some(stem) = file_name.to_str().and_then(|file| file.strip_suffix(".json")) {
				names.push(stem.to_string());
			}
		}
	}
	names.sort();
	names.dedup();
	names
}

/// Available themes with their custom file paths; a custom name shadowing an
/// embedded theme is skipped. Sorted by name (code unit, not `localeCompare`).
pub fn get_available_themes_with_paths() -> Vec<ThemeInfo> {
	let mut result: Vec<ThemeInfo> = builtin::builtin_theme_names()
		.iter()
		.map(|name| ThemeInfo {
			name: (*name).to_string(),
			path: None,
		})
		.collect();
	if let Ok(entries) = std::fs::read_dir(get_custom_themes_dir()) {
		for entry in entries.flatten() {
			let file_name = entry.file_name();
			let Some(stem) = file_name.to_str().and_then(|file| file.strip_suffix(".json")) else {
				continue;
			};
			if result.iter().any(|theme| theme.name == stem) {
				continue;
			}
			result.push(ThemeInfo {
				name: stem.to_string(),
				path: Some(entry.path()),
			});
		}
	}
	result.sort_by(|a, b| a.name.cmp(&b.name));
	result
}

/// A blend input as the theme intends it: `""` becomes the theme's
/// `terminal.foreground`, and an index below 16 its `terminal.ansi` slot when
/// the theme declares one.
fn intended_color(
	value: &ColorValue,
	terminal_foreground: Option<&ColorValue>,
	ansi_palette: Option<&[ColorValue]>,
) -> ColorValue {
	let color = if value.is_empty() {
		terminal_foreground.unwrap_or(value)
	} else {
		value
	};
	if let ColorValue::Index(index) = color {
		if *index < 16 {
			if let Some(resolved) = ansi_palette.and_then(|palette| palette.get(*index as usize)) {
				return resolved.clone();
			}
		}
	}
	color.clone()
}

/// Build a resolved [`Theme`] from a theme definition, implementing the exact
/// `createTheme` pipeline: var resolution, colorblind adjustment, ANSI palette
/// resolution, `mdExtensionLink` derivation, fg/bg splitting and symbol setup.
pub fn create_theme(theme_json: &ThemeJson, options: CreateThemeOptions) -> Result<Theme, String> {
	let mode = options.mode.unwrap_or_else(detect_color_mode);
	let mut resolved = resolve_theme_colors(&theme_json.colors, &theme_json.vars)?;

	if options.color_blind_mode == Some(true) {
		if let Some(ColorValue::Hex(added)) = resolved.get("toolDiffAdded") {
			if added.starts_with('#') {
				let adjusted = adjust_hsv(added, COLORBLIND_ADJUSTMENT);
				resolved.set("toolDiffAdded", ColorValue::Hex(adjusted));
			}
		}
	}

	let ansi_palette: Option<Vec<ColorValue>> = match theme_json
		.terminal
		.as_ref()
		.and_then(|terminal| terminal.ansi.as_ref())
	{
		Some(ansi) => {
			let mut resolved_palette = Vec::with_capacity(ansi.len());
			for color in ansi {
				resolved_palette.push(resolve_var_refs(color, &theme_json.vars)?);
			}
			Some(resolved_palette)
		}
		None => None,
	};

	// A theme without `mdExtensionLink` gets a muted link tint, so links an
	// extension handles in place stay distinct from links that open a URL.
	if resolved.get("mdExtensionLink").is_none() {
		let terminal_foreground = match theme_json
			.terminal
			.as_ref()
			.and_then(|terminal| terminal.foreground.as_ref())
		{
			Some(foreground) => Some(resolve_var_refs(foreground, &theme_json.vars)?),
			None => None,
		};
		let text = resolved
			.get("text")
			.cloned()
			.unwrap_or(ColorValue::Hex(String::new()));
		let md_link = resolved
			.get("mdLink")
			.cloned()
			.unwrap_or(ColorValue::Hex(String::new()));
		let text = intended_color(&text, terminal_foreground.as_ref(), ansi_palette.as_deref());
		let md_link = intended_color(&md_link, terminal_foreground.as_ref(), ansi_palette.as_deref());
		let mixed = mix_theme_colors(
			&text,
			&md_link,
			EXTENSION_LINK_TINT,
			is_light_surface(resolved.get("statusLineBg")),
		)?;
		resolved.set("mdExtensionLink", ColorValue::Hex(mixed));
	}

	let mut fg_colors = ColorMap::new();
	let mut bg_colors = ColorMap::new();
	for (key, value) in resolved.iter() {
		if BG_COLOR_KEYS.contains(&key) {
			bg_colors.insert(key.to_string(), value.clone());
		} else {
			fg_colors.insert(key.to_string(), value.clone());
		}
	}

	let symbol_preset = options
		.symbol_preset_override
		.or_else(|| theme_json.symbols.as_ref().and_then(|symbols| symbols.preset))
		.unwrap_or(SymbolPreset::Unicode);
	let symbol_overrides = theme_json
		.symbols
		.as_ref()
		.and_then(|symbols| symbols.overrides.as_ref())
		.cloned()
		.unwrap_or_default();
	let spinner_frames_overrides = normalize_spinner_frames_override(
		theme_json
			.symbols
			.as_ref()
			.and_then(|symbols| symbols.spinner_frames.as_ref()),
	);

	Theme::new(
		&fg_colors,
		&bg_colors,
		mode,
		symbol_preset,
		&symbol_overrides,
		&spinner_frames_overrides,
		ansi_palette.as_deref().unwrap_or_default(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::color::{mix_hex, ColorMode};
	use crate::theme::schema::AnsiColor;

	#[test]
	fn builtin_dark_and_light_create() {
		let dark = create_theme(
			builtin::builtin_theme_json("dark").unwrap(),
			CreateThemeOptions { mode: Some(ColorMode::TrueColor), ..CreateThemeOptions::default() },
		)
		.unwrap();
		assert_eq!(dark.get_color_hex("mdHeading").unwrap(), "#febc38");
		assert_eq!(dark.get_color_hex("accent").unwrap(), "#febc38");
		assert_eq!(dark.get_bg_hex("statusLineBg").unwrap(), "#121212");
		assert_eq!(dark.get_bg_hex("selectedBg").unwrap(), "#31363f");
		assert_eq!(dark.ansi_fg(AnsiColor::Black, "x"), "\x1b[38;2;0;0;0mx\x1b[39m");
		assert_eq!(dark.ansi_fg(AnsiColor::BrightWhite, "x"), "\x1b[38;2;229;229;229mx\x1b[39m");
		assert!(!dark.is_light());

		let light = create_theme(
			builtin::builtin_theme_json("light").unwrap(),
			CreateThemeOptions { mode: Some(ColorMode::TrueColor), ..CreateThemeOptions::default() },
		)
		.unwrap();
		assert_eq!(light.get_color_hex("mdHeading").unwrap(), "#9a7326");
		assert_eq!(light.get_color_hex("text").unwrap(), "#000000");
		assert_eq!(light.get_bg_hex("statusLineBg").unwrap(), "#e0e0e0");
		assert!(light.is_light());
	}

	#[test]
	fn colorblind_mode_adjusts_tool_diff_added() {
		let json = builtin::builtin_theme_json("dark").unwrap();
		let plain = create_theme(json, CreateThemeOptions::default()).unwrap();
		assert_eq!(plain.get_color_hex("toolDiffAdded").unwrap(), "#89d281");
		let adjusted = create_theme(
			json,
			CreateThemeOptions {
				color_blind_mode: Some(true),
				..CreateThemeOptions::default()
			},
		)
		.unwrap();
		assert_eq!(
			adjusted.get_color_hex("toolDiffAdded").unwrap(),
			adjust_hsv("#89d281", COLORBLIND_ADJUSTMENT)
		);
		assert_ne!(
			adjusted.get_color_hex("toolDiffAdded").unwrap(),
			plain.get_color_hex("toolDiffAdded").unwrap()
		);
	}

	#[test]
	fn md_extension_link_is_derived_from_text_and_md_link() {
		let json = builtin::builtin_theme_json("dark").unwrap();
		let theme = create_theme(json, CreateThemeOptions::default()).unwrap();
		let expected = mix_hex("#cccccc", "#0088fa", EXTENSION_LINK_TINT);
		assert_eq!(theme.get_color_hex("mdExtensionLink").unwrap(), expected);
	}

	#[test]
	fn explicit_md_extension_link_is_preserved() {
		let mut json = builtin::builtin_theme_json("dark").unwrap().clone();
		json.colors.set("mdExtensionLink", ColorValue::Hex("#123456".into()));
		let theme = create_theme(&json, CreateThemeOptions::default()).unwrap();
		assert_eq!(theme.get_color_hex("mdExtensionLink").unwrap(), "#123456");
	}

	#[test]
	fn symbol_override_and_spinner_override_apply() {
		let mut json = builtin::builtin_theme_json("dark").unwrap().clone();
		let mut overrides = std::collections::HashMap::new();
		overrides.insert("sep.dot".to_string(), "!".to_string());
		json.symbols = Some(crate::theme::schema::SymbolsJson {
			preset: Some(SymbolPreset::Ascii),
			overrides: Some(overrides),
			spinner_frames: Some(crate::theme::symbols::SpinnerFramesOverride::Frames(vec!["z".into()])),
		});
		let theme = create_theme(
			&json,
			CreateThemeOptions {
				symbol_preset_override: Some(SymbolPreset::Unicode),
				..CreateThemeOptions::default()
			},
		)
		.unwrap();
		assert_eq!(theme.get_symbol_preset(), SymbolPreset::Unicode);
		assert_eq!(theme.sep().dot, "!");
		assert_eq!(theme.spinner_frames().len(), 1);
		assert_eq!(theme.spinner_frames()[0], "z");
	}

	#[test]
	fn mode_override_is_honored() {
		let json = builtin::builtin_theme_json("dark").unwrap();
		let theme = create_theme(
			json,
			CreateThemeOptions {
				mode: Some(ColorMode::Color256),
				..CreateThemeOptions::default()
			},
		)
		.unwrap();
		assert_eq!(theme.get_color_mode(), ColorMode::Color256);
		assert_eq!(theme.fg("accent", "x").unwrap(), "\x1b[38;5;215mx\x1b[39m");
	}

	#[test]
	fn invalid_json_is_an_error() {
		let error = parse_theme_json("broken", "{ not json").unwrap_err();
		assert!(error.starts_with("Failed to parse theme broken:"), "{error}");
	}

	#[test]
	fn missing_tokens_produce_the_documented_envelope() {
		let error = parse_theme_json("x", r##"{"name":"x","colors":{"accent":"#fff"}}"##).unwrap_err();
		assert!(error.starts_with("Invalid theme \"x\":\n"), "{error}");
		assert!(error.contains("\nMissing required color tokens:\n"), "{error}");
		assert!(error.contains("  - border"), "{error}");
		assert!(error.contains("Please add these colors to your theme's \"colors\" object."), "{error}");
		assert!(error.contains("See the built-in themes (dark.json, light.json) for reference values."), "{error}");
		assert!(error.contains("\n\nValidation error:\n  - missing keys: "), "{error}");
	}

	#[test]
	fn valid_custom_json_parses() {
		let mut colors = serde_json::Map::new();
		for key in crate::theme::schema::REQUIRED_THEME_COLOR_KEYS {
			colors.insert((*key).to_string(), serde_json::Value::String("#ffffff".into()));
		}
		let content = serde_json::json!({"name": "custom", "colors": colors}).to_string();
		let parsed = parse_theme_json("custom", &content).unwrap();
		assert_eq!(parsed.name, "custom");
	}

	#[test]
	fn intended_color_resolves_empty_and_low_indices() {
		let palette = vec![ColorValue::Hex("#010203".into())];
		assert_eq!(
			intended_color(
				&ColorValue::Hex(String::new()),
				Some(&ColorValue::Hex("#cccccc".into())),
				Some(palette.as_slice())
			),
			ColorValue::Hex("#cccccc".into())
		);
		assert_eq!(
			intended_color(&ColorValue::Index(0), None, Some(palette.as_slice())),
			ColorValue::Hex("#010203".into())
		);
		assert_eq!(
			intended_color(&ColorValue::Index(20), None, Some(palette.as_slice())),
			ColorValue::Index(20)
		);
	}

	#[test]
	fn missing_key_parser_handles_case_and_line_end() {
		assert_eq!(
			parse_missing_keys("missing keys: a, b,c"),
			vec!["a".to_string(), "b".to_string(), "c".to_string()]
		);
		assert_eq!(parse_missing_keys("Missing Keys: a\nother"), vec!["a".to_string()]);
		assert!(parse_missing_keys("something else").is_empty());
	}

	#[test]
	fn custom_themes_dir_prefers_env() {
		// SAFETY: tests run single-threaded for this process-local env probe.
		std::env::set_var("PI_TUI_THEMES_DIR", "/tmp/maple-themes-test");
		assert_eq!(get_custom_themes_dir(), PathBuf::from("/tmp/maple-themes-test"));
		std::env::remove_var("PI_TUI_THEMES_DIR");
	}
}
