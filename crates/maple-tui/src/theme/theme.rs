//! Process-global theme manager: current-theme handle, change notifications,
//! epoch counter, custom-theme watcher and HTML export helpers.
//!
//! Reference: `l/src/theme/theme.ts`.
//!
//! Deviations:
//! - The TS module exposes `theme` as a live binding; Rust callers read
//!   [`theme`] and mirror assignments with [`set_theme_instance`] or
//!   [`bind_theme`].
//! - OSC 11 probing and the macOS appearance reprobe fallback
//!   (`startMacOSAppearanceReprobeFallback`) stay in the terminal layer; this
//!   module consumes [`on_terminal_appearance_change`] and falls back to
//!   `COLORFGBG` and then dark.
//! - `initTheme(enableWatcher)` is split: [`init_theme`] /
//!   [`init_theme_sync`] load, [`start_theme_watcher`] watches.
//! - The watcher polls a content hash on a background thread instead of using
//!   `fs.watch`, with the same 100 ms debounce and stale-name guard.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::builtin::is_builtin_theme;
use super::color::{ansi256_to_hex, is_light_surface, resolve_theme_colors, resolve_var_refs};
use super::loader::{
	create_theme, get_custom_themes_dir, load_theme_json_sync, parse_theme_json,
	CreateThemeOptions,
};
use super::schema::{ColorMap, ColorValue, ThemeJson};
use super::theme_class::Theme;
use super::symbols::SymbolPreset;

/// Called after any theme-affecting change. Mirrors the TS `ThemeChangeEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThemeChangeEvent {
	/// Preview/presentation-only changes should repaint live UI without
	/// replacing native scrollback.
	pub ephemeral: bool,
}

/// Detected terminal appearance, as the terminal layer or `COLORFGBG` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
	/// Dark background.
	Dark,
	/// Light background.
	Light,
}

static CURRENT_THEME: Mutex<Option<Theme>> = Mutex::new(None);
static CURRENT_THEME_NAME: Mutex<Option<String>> = Mutex::new(None);
static THEME_EPOCH: AtomicU64 = AtomicU64::new(0);
static THEME_CHANGE_CALLBACK: Mutex<Option<Arc<dyn Fn(ThemeChangeEvent) + Send + Sync>>> =
	Mutex::new(None);
static THEME_BINDINGS: Mutex<Vec<Arc<dyn Fn(Option<Theme>) + Send + Sync>>> = Mutex::new(Vec::new());
static TERMINAL_APPEARANCE: Mutex<Option<Appearance>> = Mutex::new(None);
static AUTO_DETECTED_THEME: AtomicBool = AtomicBool::new(false);
static AUTO_DARK_THEME: std::sync::LazyLock<Mutex<String>> =
	std::sync::LazyLock::new(|| Mutex::new("dark".to_string()));
static AUTO_LIGHT_THEME: std::sync::LazyLock<Mutex<String>> =
	std::sync::LazyLock::new(|| Mutex::new("light".to_string()));
static SYMBOL_PRESET_OVERRIDE: Mutex<Option<SymbolPreset>> = Mutex::new(None);
static NATIVE_SYMBOL_PRESET: Mutex<Option<SymbolPreset>> = Mutex::new(None);
static COLOR_BLIND_MODE: AtomicBool = AtomicBool::new(false);
static BACKGROUND_FALLBACK_LISTENERS: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>> =
	Mutex::new(Vec::new());
static WATCHER: Mutex<Option<ThemeWatcher>> = Mutex::new(None);

/// The active theme, or `None` before initialization.
pub fn theme() -> Option<Theme> {
	CURRENT_THEME.lock().expect("theme lock").clone()
}

/// The name of the currently active theme (`"<in-memory>"` for a directly
/// assigned instance).
pub fn get_current_theme_name() -> Option<String> {
	CURRENT_THEME_NAME.lock().expect("theme name lock").clone()
}

/// Appearance detected from authoritative sources, defaulting to dark.
///
/// Mirrors the TS tiers: terminal-reported appearance, then `COLORFGBG`, then
/// dark.
pub fn detect_terminal_background() -> Appearance {
	if let Some(appearance) = *TERMINAL_APPEARANCE.lock().expect("appearance lock") {
		return appearance;
	}
	if let Some(colorfgbg) = crate::env_lookup("COLORFGBG") {
		let colorfgbg = colorfgbg.to_string_lossy();
		let parts: Vec<&str> = colorfgbg.split(';').collect();
		if let Some(background) = parts.get(1).and_then(|part| part.parse::<i32>().ok()) {
			return if background < 8 {
				Appearance::Dark
			} else {
				Appearance::Light
			};
		}
	}
	Appearance::Dark
}

/// Auto-detected theme name for the current appearance.
pub fn default_theme_name() -> String {
	match detect_terminal_background() {
		Appearance::Dark => AUTO_DARK_THEME.lock().expect("auto dark lock").clone(),
		Appearance::Light => AUTO_LIGHT_THEME.lock().expect("auto light lock").clone(),
	}
}

fn assign_theme(value: Theme) {
	*CURRENT_THEME.lock().expect("theme lock") = Some(value.clone());
	let bindings: Vec<_> = THEME_BINDINGS
		.lock()
		.expect("theme binding lock")
		.clone();
	for binding in bindings {
		binding(Some(value.clone()));
	}
}

/// Replace the active theme with an in-memory instance, stopping the watcher
/// and marking the theme name `"<in-memory>"`.
pub fn set_theme_instance(value: Theme) {
	AUTO_DETECTED_THEME.store(false, Ordering::SeqCst);
	*CURRENT_THEME_NAME.lock().expect("theme name lock") = Some("<in-memory>".to_string());
	stop_theme_watcher();
	assign_theme(value);
	notify_theme_change(ThemeChangeEvent { ephemeral: true });
}

/// Mirror assignments into a binding: invoked once with the current theme and
/// again after every theme assignment.
pub fn bind_theme(binding: impl Fn(Option<Theme>) + Send + Sync + 'static) -> ThemeBinding {
	let callback: Arc<dyn Fn(Option<Theme>) + Send + Sync> = Arc::new(binding);
	THEME_BINDINGS
		.lock()
		.expect("theme binding lock")
		.push(callback.clone());
	callback(theme());
	ThemeBinding {
		callback: Some(callback),
	}
}

/// Handle returned by [`bind_theme`]; dropping it stops the mirroring.
pub struct ThemeBinding {
	callback: Option<Arc<dyn Fn(Option<Theme>) + Send + Sync>>,
}

impl ThemeBinding {
	/// Stop mirroring.
	pub fn unbind(mut self) {
		self.clear();
	}

	fn clear(&mut self) {
		let Some(callback) = self.callback.take() else {
			return;
		};
		THEME_BINDINGS
			.lock()
			.expect("theme binding lock")
			.retain(|current| !Arc::ptr_eq(current, &callback));
	}
}

impl Drop for ThemeBinding {
	fn drop(&mut self) {
		self.clear();
	}
}

fn theme_options() -> CreateThemeOptions {
	let native = *NATIVE_SYMBOL_PRESET.lock().expect("native preset lock");
	let preset = native.or(*SYMBOL_PRESET_OVERRIDE.lock().expect("preset lock"));
	CreateThemeOptions {
		mode: None,
		symbol_preset_override: preset,
		color_blind_mode: Some(COLOR_BLIND_MODE.load(Ordering::SeqCst)),
	}
}

fn load_with_options(name: &str) -> Result<Theme, String> {
	let json = load_theme_json_sync(name)?;
	create_theme(&json, theme_options())
}

/// Initialize the default theme synchronously, recording auto-detection
/// preferences. Falls back to the dark theme when the detected name fails.
pub fn init_theme_sync(
	symbol_preset: Option<SymbolPreset>,
	color_blind: bool,
	dark_theme: Option<&str>,
	light_theme: Option<&str>,
) {
	AUTO_DETECTED_THEME.store(true, Ordering::SeqCst);
	if let Some(name) = dark_theme {
		*AUTO_DARK_THEME.lock().expect("auto dark lock") = name.to_string();
	}
	if let Some(name) = light_theme {
		*AUTO_LIGHT_THEME.lock().expect("auto light lock") = name.to_string();
	}
	*SYMBOL_PRESET_OVERRIDE.lock().expect("preset lock") = symbol_preset;
	COLOR_BLIND_MODE.store(color_blind, Ordering::SeqCst);

	let name = default_theme_name();
	match load_with_options(&name) {
		Ok(theme) => {
			*CURRENT_THEME_NAME.lock().expect("theme name lock") = Some(name);
			assign_theme(theme);
		}
		Err(_) => {
			*CURRENT_THEME_NAME.lock().expect("theme name lock") = Some("dark".to_string());
			if let Ok(fallback) = load_with_options("dark") {
				assign_theme(fallback);
			}
		}
	}
	notify_theme_change(ThemeChangeEvent::default());
}

/// Initialize the active theme by explicit name. Unlike `initTheme` this never
/// starts the watcher; call [`start_theme_watcher`] for that.
pub fn init_theme(name: &str) -> Result<(), String> {
	apply_theme(name, ThemeChangeEvent::default())
}

/// Ensure the module-local theme is initialized before synchronous component
/// work.
pub fn ensure_theme_sync() {
	if theme().is_none() {
		init_theme_sync(None, false, None, None);
	}
}

/// Async TS `ensureTheme` equivalent: initialize when absent.
pub fn ensure_theme() {
	ensure_theme_sync();
}

/// Switch themes and notify listeners. An invalid name falls back to `dark`.
pub fn set_theme(name: &str) -> Result<(), String> {
	AUTO_DETECTED_THEME.store(false, Ordering::SeqCst);
	match apply_theme(name, ThemeChangeEvent::default()) {
		Ok(()) => Ok(()),
		Err(error) => {
			*CURRENT_THEME_NAME.lock().expect("theme name lock") = Some("dark".to_string());
			if let Ok(fallback) = load_with_options("dark") {
				assign_theme(fallback);
			}
			notify_theme_change(ThemeChangeEvent::default());
			Err(error)
		}
	}
}

/// Switch themes for a preview: listeners get `ephemeral = true`.
pub fn preview_theme(name: &str) -> Result<(), String> {
	apply_theme(
		name,
		ThemeChangeEvent {
			ephemeral: true,
		},
	)
}

/// Enable auto-detection mode, switching to the appropriate dark/light theme.
pub fn enable_auto_theme(event: ThemeChangeEvent) {
	AUTO_DETECTED_THEME.store(true, Ordering::SeqCst);
	reevaluate_auto_theme(None, event);
}

/// Update one auto-detection mapping and re-evaluate when auto mode is active.
pub fn set_auto_theme_mapping(appearance: Appearance, theme_name: &str) {
	match appearance {
		Appearance::Dark => {
			*AUTO_DARK_THEME.lock().expect("auto dark lock") = theme_name.to_string();
		}
		Appearance::Light => {
			*AUTO_LIGHT_THEME.lock().expect("auto light lock") = theme_name.to_string();
		}
	}
	reevaluate_auto_theme(None, ThemeChangeEvent::default());
}

/// Called when the terminal detects a dark/light appearance change.
pub fn on_terminal_appearance_change(appearance: Appearance, event: ThemeChangeEvent) {
	let mut current = TERMINAL_APPEARANCE.lock().expect("appearance lock");
	if *current == Some(appearance) {
		return;
	}
	*current = Some(appearance);
	drop(current);
	reevaluate_auto_theme(Some(appearance), event);
}

fn reevaluate_auto_theme(appearance: Option<Appearance>, event: ThemeChangeEvent) {
	if !AUTO_DETECTED_THEME.load(Ordering::SeqCst) {
		return;
	}
	let resolved = match appearance {
		Some(Appearance::Dark) => AUTO_DARK_THEME.lock().expect("auto dark lock").clone(),
		Some(Appearance::Light) => AUTO_LIGHT_THEME.lock().expect("auto light lock").clone(),
		None => default_theme_name(),
	};
	if get_current_theme_name().as_deref() == Some(resolved.as_str()) {
		return;
	}
	if let Ok(theme) = load_with_options(&resolved) {
		*CURRENT_THEME_NAME.lock().expect("theme name lock") = Some(resolved);
		assign_theme(theme);
		notify_theme_change(event);
	}
}

/// Set the symbol preset override and rebuild the active theme.
pub fn set_symbol_preset(preset: Option<SymbolPreset>) -> Result<(), String> {
	*SYMBOL_PRESET_OVERRIDE.lock().expect("preset lock") = preset;
	reload_current_theme()
}

/// The current symbol preset override.
pub fn get_symbol_preset_override() -> Option<SymbolPreset> {
	*SYMBOL_PRESET_OVERRIDE.lock().expect("preset lock")
}

/// Force `preset` while a native surface is live; returns whether the active
/// theme was rebuilt.
pub fn set_native_symbol_preset(preset: Option<SymbolPreset>) -> bool {
	{
		let mut native = NATIVE_SYMBOL_PRESET.lock().expect("native preset lock");
		if *native == preset {
			return false;
		}
		*native = preset;
	}
	if get_current_theme_name().is_none() || theme().is_none() {
		return false;
	}
	reload_current_theme().is_ok()
}

/// Set color-blind mode and rebuild the active theme.
pub fn set_color_blind_mode(enabled: bool) -> Result<(), String> {
	COLOR_BLIND_MODE.store(enabled, Ordering::SeqCst);
	reload_current_theme()
}

/// The current color-blind mode setting.
pub fn get_color_blind_mode() -> bool {
	COLOR_BLIND_MODE.load(Ordering::SeqCst)
}

fn reload_current_theme() -> Result<(), String> {
	let Some(name) = get_current_theme_name() else {
		return Ok(());
	};
	let theme = load_with_options(&name)?;
	assign_theme(theme);
	notify_theme_change(ThemeChangeEvent { ephemeral: true });
	Ok(())
}

/// Returns unstyled `text` before [`init_theme`] assigns a global theme; use
/// only for early-render paths.
pub fn fg_or_plain(color: &str, text: &str) -> String {
	match theme() {
		None => text.to_string(),
		Some(theme) => theme.fg(color, text).unwrap_or_else(|_| text.to_string()),
	}
}

/// Monotonic counter bumped on any theme-affecting change that should
/// invalidate cached renders.
pub fn get_theme_epoch() -> u64 {
	THEME_EPOCH.load(Ordering::SeqCst)
}

/// Register the theme-change listener, replacing any previous one.
pub fn on_theme_change(
	callback: impl Fn(ThemeChangeEvent) + Send + Sync + 'static,
) -> ThemeChangeSubscription {
	let callback: Arc<dyn Fn(ThemeChangeEvent) + Send + Sync> = Arc::new(callback);
	*THEME_CHANGE_CALLBACK.lock().expect("theme callback lock") = Some(callback.clone());
	ThemeChangeSubscription { callback }
}

/// Handle returned by [`on_theme_change`]; dropping it clears the listener.
pub struct ThemeChangeSubscription {
	callback: Arc<dyn Fn(ThemeChangeEvent) + Send + Sync>,
}

impl Drop for ThemeChangeSubscription {
	fn drop(&mut self) {
		let mut slot = THEME_CHANGE_CALLBACK.lock().expect("theme callback lock");
		let is_current = slot
			.as_ref()
			.is_some_and(|current| Arc::ptr_eq(current, &self.callback));
		if is_current {
			*slot = None;
		}
	}
}

/// Bump the theme epoch and notify the registered theme-change listener.
pub fn notify_theme_change(event: ThemeChangeEvent) {
	THEME_EPOCH.fetch_add(1, Ordering::SeqCst);
	let callback = THEME_CHANGE_CALLBACK
		.lock()
		.expect("theme callback lock")
		.clone();
	if let Some(callback) = callback {
		callback(event);
	}
}

fn apply_theme(name: &str, event: ThemeChangeEvent) -> Result<(), String> {
	let theme = load_with_options(name)?;
	*CURRENT_THEME_NAME.lock().expect("theme name lock") = Some(name.to_string());
	assign_theme(theme);
	notify_theme_change(event);
	Ok(())
}

/// Subscribe to native background-fallback appearance changes.
pub fn on_terminal_background_fallback_change(
	callback: impl Fn() + Send + Sync + 'static,
) -> BackgroundFallbackSubscription {
	let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(callback);
	BACKGROUND_FALLBACK_LISTENERS
		.lock()
		.expect("background listener lock")
		.push(callback.clone());
	BackgroundFallbackSubscription {
		callback: Some(callback),
	}
}

/// Handle returned by [`on_terminal_background_fallback_change`].
pub struct BackgroundFallbackSubscription {
	callback: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Drop for BackgroundFallbackSubscription {
	fn drop(&mut self) {
		let Some(callback) = self.callback.take() else {
			return;
		};
		BACKGROUND_FALLBACK_LISTENERS
			.lock()
			.expect("background listener lock")
			.retain(|current| !Arc::ptr_eq(current, &callback));
	}
}

/// Notify background-fallback listeners (the host calls this after a native
/// appearance report); consumers must re-check the effective appearance.
pub fn notify_terminal_background_fallback() {
	let listeners: Vec<_> = BACKGROUND_FALLBACK_LISTENERS
		.lock()
		.expect("background listener lock")
		.clone();
	for listener in listeners {
		listener();
	}
}

// ============================================================================
// Custom-theme watcher
// ============================================================================

/// Background reloader for the active custom theme file.
pub struct ThemeWatcher {
	stop: Arc<AtomicBool>,
	handle: Option<JoinHandle<()>>,
}

impl Drop for ThemeWatcher {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::SeqCst);
		if let Some(handle) = self.handle.take() {
			let _ = handle.join();
		}
	}
}

/// Watch the active theme when it is a custom (non-built-in) one.
pub fn start_theme_watcher() {
	stop_theme_watcher();
	let Some(name) = get_current_theme_name() else {
		return;
	};
	start_theme_watcher_at(&get_custom_themes_dir(), &name);
}

/// Watch `dir` for changes to `<name>.json`; returns whether watching started.
pub fn start_theme_watcher_at(dir: &Path, name: &str) -> bool {
	stop_theme_watcher();
	if is_builtin_theme(name) {
		return false;
	}
	let file = dir.join(format!("{name}.json"));
	if !file.exists() {
		return false;
	}
	let watched_name = name.to_string();
	let stop = Arc::new(AtomicBool::new(false));
	let thread_stop = stop.clone();
	let watched_file = file.clone();
	let handle = thread::spawn(move || {
		let mut last = file_hash(&watched_file);
		while !thread_stop.load(Ordering::SeqCst) {
			thread::sleep(Duration::from_millis(100));
			if thread_stop.load(Ordering::SeqCst) {
				break;
			}
			let Some(hash) = file_hash(&watched_file) else {
				// Keep the last successfully loaded theme while the file is
				// temporarily missing.
				continue;
			};
			if last == Some(hash) {
				continue;
			}
			last = Some(hash);
			if get_current_theme_name().as_deref() != Some(watched_name.as_str()) {
				break;
			}
			let Ok(content) = fs::read_to_string(&watched_file) else {
				continue;
			};
			let Ok(json) = parse_theme_json(&watched_name, &content) else {
				continue;
			};
			let Ok(loaded) = create_theme(&json, theme_options()) else {
				continue;
			};
			assign_theme(loaded);
			notify_theme_change(ThemeChangeEvent { ephemeral: true });
		}
	});
	*WATCHER.lock().expect("watcher lock") = Some(ThemeWatcher {
		stop,
		handle: Some(handle),
	});
	true
}

fn file_hash(path: &Path) -> Option<u64> {
	use std::hash::{Hash, Hasher};
	let content = fs::read(path).ok()?;
	let mut hasher = std::collections::hash_map::DefaultHasher::new();
	content.hash(&mut hasher);
	Some(hasher.finish())
}

/// Stop the watcher, clearing the recorded terminal appearance.
pub fn stop_theme_watcher() {
	let watcher = WATCHER.lock().expect("watcher lock").take();
	drop(watcher);
	*TERMINAL_APPEARANCE.lock().expect("appearance lock") = None;
}

// ============================================================================
// HTML export helpers
// ============================================================================

fn is_light_theme_json(json: &ThemeJson) -> bool {
	let Some(value) = json.colors.get("statusLineBg") else {
		return false;
	};
	match resolve_var_refs(value, &json.vars) {
		Ok(resolved) => is_light_surface(Some(&resolved)),
		Err(_) => false,
	}
}

fn get_html_default_text_for_surface(surface: Option<&ColorValue>) -> String {
	if is_light_surface(surface) {
		"#000000".to_string()
	} else {
		"#e5e5e7".to_string()
	}
}

fn resolve_export_value(value: Option<&ColorValue>, vars: &ColorMap) -> Option<String> {
	let value = value?;
	match value {
		ColorValue::Index(index) => ansi256_to_hex(*index).ok(),
		ColorValue::Hex(text) => {
			if text.is_empty() || text.starts_with('#') {
				return Some(text.clone());
			}
			let var_name = text.strip_prefix('$').unwrap_or(text);
			let Some(resolved) = vars.get(var_name).cloned() else {
				// Unknown var: the raw value is already a CSS color.
				return Some(text.clone());
			};
			match resolved {
				ColorValue::Index(index) => ansi256_to_hex(index).ok(),
				ColorValue::Hex(hex) => Some(hex),
			}
		}
	}
}

fn resolve_theme_export_colors(json: &ThemeJson) -> BTreeMap<String, String> {
	let mut out = BTreeMap::new();
	let Some(export) = json.export.as_ref() else {
		return out;
	};
	for (key, value) in [
		("pageBg", export.page_bg.as_ref()),
		("cardBg", export.card_bg.as_ref()),
		("infoBg", export.info_bg.as_ref()),
	] {
		if let Some(resolved) = resolve_export_value(value, &json.vars) {
			out.insert(key.to_string(), resolved);
		}
	}
	out
}

/// Resolved theme colors as CSS-compatible hex strings, keyed by color token.
pub fn get_resolved_theme_colors(theme_name: Option<&str>) -> Result<BTreeMap<String, String>, String> {
	let name = theme_name.map_or_else(default_theme_name, str::to_string);
	let json = load_theme_json_sync(&name)?;
	let export_colors = resolve_theme_export_colors(&json);
	let resolved = resolve_theme_colors(&json.colors, &json.vars)?;

	// Empty foreground tokens use the terminal default; in HTML that default
	// must contrast the export surface, not the TUI status line.
	let default_text = get_html_default_text_for_surface(
		export_colors
			.get("cardBg")
			.map(|hex| ColorValue::Hex(hex.clone()))
			.or_else(|| export_colors.get("pageBg").map(|hex| ColorValue::Hex(hex.clone())))
			.as_ref()
			.or_else(|| resolved.get("userMessageBg")),
	);

	let mut colors = BTreeMap::new();
	for (key, value) in resolved.iter() {
		let css = match value {
			ColorValue::Index(index) => ansi256_to_hex(*index)?,
			ColorValue::Hex(text) if text.is_empty() => default_text.clone(),
			ColorValue::Hex(text) => text.clone(),
		};
		colors.insert(key.to_string(), css);
	}
	Ok(colors)
}

/// Check whether a theme is light by the luminance of its status-line
/// background.
pub fn is_light_theme(theme_name: Option<&str>) -> bool {
	let name = theme_name.map_or_else(|| "dark".to_string(), str::to_string);
	match load_theme_json_sync(&name) {
		Ok(json) => is_light_theme_json(&json),
		Err(_) => false,
	}
}

/// Explicit export colors from theme JSON; absent keys are omitted.
pub fn get_theme_export_colors(theme_name: Option<&str>) -> BTreeMap<String, String> {
	let name = theme_name.map_or_else(default_theme_name, str::to_string);
	match load_theme_json_sync(&name) {
		Ok(json) => resolve_theme_export_colors(&json),
		Err(_) => BTreeMap::new(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::AtomicU32;
	use std::sync::Arc;

	/// Theme state is process-global; serialize the tests that mutate it.
	static TEST_LOCK: Mutex<()> = Mutex::new(());

	fn reset(name: &str) {
		set_theme(name).expect("theme loads");
	}

	#[test]
	fn theme_handle_tracks_name_and_instance() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("light");
		assert_eq!(get_current_theme_name().as_deref(), Some("light"));
		let current = theme().expect("theme installed");
		assert!(current.has_color("accent"));
		ensure_theme_sync();
		assert_eq!(get_current_theme_name().as_deref(), Some("light"));
	}

	#[test]
	fn epoch_and_listener_fire_on_changes() {
		let _guard = TEST_LOCK.lock().unwrap();
		let seen = Arc::new(Mutex::new(Vec::new()));
		let sink = seen.clone();
		let subscription = on_theme_change(move |event| {
			sink.lock().unwrap().push(event.ephemeral);
		});
		let before = get_theme_epoch();
		reset("dark");
		preview_theme("light").unwrap();
		let after = get_theme_epoch();
		assert!(after >= before + 2);
		assert_eq!(seen.lock().unwrap().as_slice(), &[false, true]);
		drop(subscription);
		reset("dark");
		assert_eq!(seen.lock().unwrap().len(), 2);
	}

	#[test]
	fn bindings_mirror_assignments() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("dark");
		let count = Arc::new(AtomicU32::new(0));
		let sink = count.clone();
		let binding = bind_theme(move |_| {
			sink.fetch_add(1, Ordering::SeqCst);
		});
		assert_eq!(count.load(Ordering::SeqCst), 1);
		set_theme_instance(theme().unwrap());
		assert_eq!(count.load(Ordering::SeqCst), 2);
		binding.unbind();
		set_theme_instance(theme().unwrap());
		assert_eq!(count.load(Ordering::SeqCst), 2);
	}

	#[test]
	fn fg_or_plain_styles_only_after_init() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("dark");
		let styled = fg_or_plain("accent", "hi");
		assert_ne!(styled, "hi");
		assert!(styled.ends_with("\u{1b}[39m"));
	}

	#[test]
	fn light_and_dark_classification_matches_status_line() {
		assert!(!is_light_theme(None));
		assert!(is_light_theme(Some("light")));
		assert!(!is_light_theme(Some("dark")));
		assert!(!is_light_theme(Some("missing-theme")));
	}

	#[test]
	fn resolved_colors_are_css_hex() {
		let colors = get_resolved_theme_colors(Some("dark")).expect("dark resolves");
		assert!(colors.contains_key("accent"));
		assert!(colors["accent"].starts_with('#'));
		assert!(colors.contains_key("text"));
		assert!(colors["text"].starts_with('#'));
	}

	#[test]
	fn export_colors_resolve_surfaces() {
		let dark = get_theme_export_colors(Some("dark"));
		assert_eq!(dark.get("pageBg").map(String::as_str), Some("#18181e"));
		assert_eq!(dark.get("cardBg").map(String::as_str), Some("#1e1e24"));
		let light = get_theme_export_colors(Some("light"));
		assert_eq!(light.get("pageBg").map(String::as_str), Some("#f8f8f8"));
	}

	#[test]
	fn auto_theme_follows_terminal_appearance() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("dark");
		on_terminal_appearance_change(Appearance::Dark, ThemeChangeEvent::default());
		enable_auto_theme(ThemeChangeEvent::default());
		on_terminal_appearance_change(Appearance::Light, ThemeChangeEvent::default());
		assert_eq!(get_current_theme_name().as_deref(), Some("light"));
		on_terminal_appearance_change(Appearance::Dark, ThemeChangeEvent::default());
		assert_eq!(get_current_theme_name().as_deref(), Some("dark"));
		stop_theme_watcher();
	}

	#[test]
	fn auto_mapping_redirects_the_appearance() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("dark");
		on_terminal_appearance_change(Appearance::Dark, ThemeChangeEvent::default());
		enable_auto_theme(ThemeChangeEvent::default());
		set_auto_theme_mapping(Appearance::Light, "dark");
		on_terminal_appearance_change(Appearance::Light, ThemeChangeEvent::default());
		assert_eq!(get_current_theme_name().as_deref(), Some("dark"));
		set_auto_theme_mapping(Appearance::Light, "light");
		stop_theme_watcher();
	}

	#[test]
	fn background_fallback_listeners_detach() {
		let count = Arc::new(AtomicU32::new(0));
		let sink = count.clone();
		let subscription = on_terminal_background_fallback_change(move || {
			sink.fetch_add(1, Ordering::SeqCst);
		});
		notify_terminal_background_fallback();
		assert_eq!(count.load(Ordering::SeqCst), 1);
		drop(subscription);
		notify_terminal_background_fallback();
		assert_eq!(count.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn color_blind_and_symbol_presets_rebuild_the_theme() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("dark");
		assert!(!get_color_blind_mode());
		set_color_blind_mode(true).expect("dark rebuilds");
		assert!(get_color_blind_mode());
		set_symbol_preset(Some(SymbolPreset::Ascii)).expect("ascii rebuilds");
		assert_eq!(get_symbol_preset_override(), Some(SymbolPreset::Ascii));
		assert!(set_native_symbol_preset(Some(SymbolPreset::Unicode)));
		assert!(!set_native_symbol_preset(Some(SymbolPreset::Unicode)));
		set_native_symbol_preset(None);
		set_symbol_preset(None).unwrap();
		set_color_blind_mode(false).unwrap();
	}

	#[test]
	fn in_memory_instance_is_named_and_notifies() {
		let _guard = TEST_LOCK.lock().unwrap();
		reset("dark");
		let events = Arc::new(Mutex::new(Vec::new()));
		let sink = events.clone();
		let subscription = on_theme_change(move |event| sink.lock().unwrap().push(event.ephemeral));
		set_theme_instance(crate::theme::default_theme());
		assert_eq!(get_current_theme_name().as_deref(), Some("<in-memory>"));
		assert_eq!(events.lock().unwrap().as_slice(), &[true]);
		drop(subscription);
		reset("dark");
	}

	fn watcher_theme_json(name: &str) -> String {
		let dark = super::super::builtin::builtin_theme_source("dark").expect("dark source");
		dark.replacen("\"name\": \"dark\"", &format!("\"name\": \"{name}\""), 1)
	}

	#[test]
	fn watcher_reloads_changed_theme_file() {
		let _guard = TEST_LOCK.lock().unwrap();
		let dir = std::env::temp_dir().join(format!("maple-theme-watch-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let name = "watch-custom";
		let file = dir.join(format!("{name}.json"));
		fs::write(&file, watcher_theme_json(name)).unwrap();

		reset("dark");
		*CURRENT_THEME_NAME.lock().unwrap() = Some(name.to_string());
		assert!(start_theme_watcher_at(&dir, name));
		let before = get_theme_epoch();

		thread::sleep(Duration::from_millis(50));
		let mut changed = watcher_theme_json(name);
		changed = changed.replacen("\"accent\": \"#febc38\"", "\"accent\": \"#123456\"", 1);
		fs::write(&file, changed).unwrap();

		let deadline = std::time::Instant::now() + Duration::from_secs(3);
		while get_theme_epoch() == before && std::time::Instant::now() < deadline {
			thread::sleep(Duration::from_millis(50));
		}
		stop_theme_watcher();
		assert!(get_theme_epoch() > before);
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn builtin_themes_are_never_watched() {
		let _guard = TEST_LOCK.lock().unwrap();
		assert!(!start_theme_watcher_at(&std::env::temp_dir(), "dark"));
	}
}
