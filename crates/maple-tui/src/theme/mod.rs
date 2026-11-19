//! Theme system: JSON schema, color math, symbol presets, built-in themes and
//! the resolved [`Theme`] used by every renderer.
//!
//! Ported from the reference `l/src/theme/` directory. The module is split the
//! same way:
//!
//! - [`schema`] — color token key sets and the serde `ThemeJson` shape.
//! - [`color`] — color values, ANSI conversion, var resolution and color math.
//! - [`symbols`] — the unicode/nerd/ascii symbol vocabulary and spinner frames.
//! - [`builtin`] — the embedded `dark` and `light` themes.
//! - [`loader`] — parsing, validation, discovery and [`loader::create_theme`].
//! - [`theme_class`] — the resolved [`Theme`] with per-token ANSI/hex caches.
//! - [`theme`] — the process-global current theme, watcher and export helpers.
//! - [`shimmer`] — the animated three-tier shimmer sweep engine.

//! # Key representation
//!
//! Theme colors are addressed by their JSON token string (`"accent"`,
//! `"mdHeading`, …) rather than by a generated enum. The TS API is
//! `fg(color: ThemeColor, …)` where `ThemeColor` is a string union, and the
//! theme JSON itself contains at least one key (`link` in `dark.json`) that is
//! *not* a `ThemeColor`; string keys keep that behavior exact and let callers
//! pass [`schema::ThemeColor::Accent`] (or any `&str`) interchangeably.

pub mod builtin;
pub mod active_symbols;
pub mod mermaid_cache;
pub mod tui_adapters;
pub mod shimmer;
pub mod theme;

pub mod color;
pub mod loader;
pub mod schema;
pub mod symbols;
pub mod session_color;
pub mod theme_class;

pub use color::{ColorMode, ColorValue};
pub use loader::{create_theme, load_theme_json, load_theme_json_sync, CreateThemeOptions};
pub use schema::{ThemeBg, ThemeColor, ThemeJson};
pub use symbols::{SymbolPreset, SymbolMap};
pub use theme_class::{SessionAccentTheme, Theme};
pub use shimmer::{
	describe_shimmer, set_shimmer_mode, shimmer_enabled, shimmer_mode, shimmer_segments,
	shimmer_segments_at, shimmer_text, ShimmerDescription, ShimmerMode, ShimmerPalette,
	ShimmerSegment, ShimmerSpan, ShimmerTier, DEFAULT_SHIMMER_PALETTE,
};
pub use theme::{
	bind_theme, detect_terminal_background, enable_auto_theme, ensure_theme, ensure_theme_sync,
	fg_or_plain, get_color_blind_mode, get_current_theme_name, get_resolved_theme_colors,
	get_symbol_preset_override, get_theme_epoch, get_theme_export_colors, init_theme,
	init_theme_sync, is_light_theme, notify_terminal_background_fallback, notify_theme_change,
	on_terminal_appearance_change, on_terminal_background_fallback_change, on_theme_change,
	preview_theme, set_auto_theme_mapping, set_color_blind_mode, set_native_symbol_preset,
	set_symbol_preset, set_theme, set_theme_instance, start_theme_watcher,
	start_theme_watcher_at, stop_theme_watcher, theme, Appearance,
	BackgroundFallbackSubscription, ThemeBinding, ThemeChangeEvent, ThemeChangeSubscription,
	ThemeWatcher,
};

/// The built-in dark theme, for renderers constructed without a host theme.
///
/// Reference components read the process-global theme; crate components take
/// one by field or setter, and this is the default they start from.
pub fn default_theme() -> Theme {
	let json = builtin::builtin_theme_json("dark").expect("built-in dark theme is embedded");
	create_theme(json, CreateThemeOptions::default()).expect("built-in dark theme resolves")
}
