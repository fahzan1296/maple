//! Reference: `l/src/prompt/composer-cache.ts`.
//!
//! Speculative composer state for the next first frame: the resolved theme and
//! composer preferences plus the status-bar inputs the last session persisted.
//!
//! Each row is one JSON payload keyed by project (the resolved cwd) and kind.
//! Settings-derived kinds (theme/composer preferences, status-bar inputs) are
//! also written under the empty project, so a folder that never ran the app
//! still paints with the user's theme and status bar.
//!
//! ```text
//! entries (project TEXT, kind TEXT, value TEXT JSON, PRIMARY KEY (project, kind))
//! ```
//!
//! Deviations:
//! - The reference stores rows in SQLite (`~/.omp/agent/cache/composer.db`). This
//!   crate has no SQLite driver and may not add one, so the same rows live in one
//!   JSON file (`composer.json`) behind the same reader/writer contract; the
//!   format version clears the file exactly like `PRAGMA user_version` did.
//! - `ComposerStatusCache` comes from [`crate::prompt::composer`] and its
//!   `statusLine` payload is validated structurally (no status-line startup port).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::Value;

use crate::prompt::composer::{
	BorderColor, ComposerPreferences, ComposerStatusCache, ResizeScrollbackMode,
};
use crate::prompt::word_completion::is_word_completion_method;
use crate::theme::symbols::SymbolPreset;

/// Bump whenever any payload format changes; older stores are cleared on open.
const FORMAT_VERSION: i64 = 1;
/// Project key of rows that serve every project lacking its own.
const ANY_PROJECT: &str = "";

/// Theme inputs cached from the last resolved settings load for stable prepaint
/// colors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposerThemePreferences {
	pub symbol_preset: Option<SymbolPreset>,
	pub color_blind_mode: Option<bool>,
	pub dark_theme: Option<String>,
	pub light_theme: Option<String>,
}

/// Speculative composer state read before the settings/session graph is
/// available.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComposerStartupCache {
	pub preferences: Option<ComposerPreferences>,
	pub theme: Option<ComposerThemePreferences>,
	pub status: Option<ComposerStatusCache>,
}

/// Default store path (`~/.omp/agent/cache/composer.json`).
pub fn composer_cache_path() -> PathBuf {
	let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
	home.join(".omp").join("agent").join("cache").join("composer.json")
}

fn is_record(value: &Value) -> bool {
	value.is_object()
}

fn parse_json(value: Option<&String>) -> Option<Value> {
	let value = value?;
	serde_json::from_str(value).ok()
}

/// Validate a status payload: a status-line object, plus an optional
/// `{ prefix, suffix }` border color.
fn parse_status(value: &Value) -> Option<ComposerStatusCache> {
	if !is_record(value) {
		return None;
	}
	let status_line = value.get("statusLine")?;
	if !is_record(status_line) {
		return None;
	}
	let Some(raw_border) = value.get("borderColor") else {
		return Some(ComposerStatusCache {
			border_color: None,
			status_line: status_line.clone(),
		});
	};
	if !is_record(raw_border) {
		return None;
	}
	let prefix = raw_border.get("prefix")?.as_str()?.to_string();
	let suffix = raw_border.get("suffix")?.as_str()?.to_string();
	Some(ComposerStatusCache {
		border_color: Some(BorderColor { prefix, suffix }),
		status_line: status_line.clone(),
	})
}

fn parse_theme(value: &Value) -> Option<ComposerThemePreferences> {
	let symbol_preset = match value.get("symbolPreset") {
		None | Some(Value::Null) => None,
		Some(raw) => Some(SymbolPreset::from_str(raw.as_str()?)?),
	};
	let color_blind_mode = match value.get("colorBlindMode") {
		None | Some(Value::Null) => None,
		Some(raw) => Some(raw.as_bool()?),
	};
	let string_field = |key: &str| match value.get(key) {
		None | Some(Value::Null) => None,
		Some(raw) => Some(raw.as_str()?.to_string()),
	};
	Some(ComposerThemePreferences {
		symbol_preset,
		color_blind_mode,
		dark_theme: string_field("darkTheme"),
		light_theme: string_field("lightTheme"),
	})
}

fn parse_ui_state(value: &Value) -> Option<(ComposerPreferences, ComposerThemePreferences)> {
	if !is_record(value) {
		return None;
	}
	let preferences = value.get("preferences")?;
	let theme = value.get("theme")?;
	if !is_record(preferences) || !is_record(theme) {
		return None;
	}
	let quiet = preferences.get("quiet")?.as_bool()?;
	let composer_shape = preferences.get("composerShape")?.as_str()?.to_string();
	let show_hardware_cursor = preferences.get("showHardwareCursor")?.as_bool()?;
	let max_inline_images = preferences.get("maxInlineImages")?.as_u64()? as usize;
	let resize_scrollback = match preferences.get("resizeScrollback") {
		None | Some(Value::Null) => ResizeScrollbackMode::Rebuild,
		Some(raw) => ResizeScrollbackMode::from_str(raw.as_str()?)?,
	};
	let complete_scrollback = match preferences.get("completeScrollback") {
		None | Some(Value::Null) => false,
		Some(raw) => raw.as_bool()?,
	};
	let ime_safe_cursor = preferences.get("imeSafeCursor")?.as_bool()?;
	let autocomplete_max_visible = preferences.get("autocompleteMaxVisible")?.as_u64()? as usize;
	let spelling_typo_detection = preferences.get("spellingTypoDetection")?.as_bool()?;
	let spelling_autocomplete_value = preferences.get("spellingAutocomplete")?.as_str()?;
	if !is_word_completion_method(spelling_autocomplete_value) {
		return None;
	}
	let spelling_autocomplete =
		crate::prompt::word_completion::WordCompletionMethod::from_str(spelling_autocomplete_value)?;
	let spelling_autocorrect = preferences.get("spellingAutocorrect")?.as_bool()?;
	Some((
		ComposerPreferences {
			quiet,
			composer_shape,
			show_hardware_cursor,
			max_inline_images,
			resize_scrollback,
			complete_scrollback,
			ime_safe_cursor,
			autocomplete_max_visible,
			spelling_typo_detection,
			spelling_autocomplete,
			spelling_autocorrect,
		},
		parse_theme(theme)?,
	))
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
	serde_json::to_string(value).unwrap_or_default()
}

/// Row store backing [`ComposerCache`]: `project\0kind` → payload JSON.
type Rows = HashMap<String, String>;

fn row_key(project: &str, kind: &str) -> String {
	format!("{project}\u{0}{kind}")
}

static SHARED: LazyLock<Mutex<Option<Arc<Mutex<ComposerCache>>>>> =
	LazyLock::new(|| Mutex::new(None));

/// Process-wide store at [`composer_cache_path`], opened on first use. `None`
/// when it cannot be opened (startup then paints without speculation) and under
/// the test runner, so tests never read or clobber the user's cache; tests open
/// [`ComposerCache::open`] explicitly.
pub fn shared_composer_cache() -> Option<Arc<Mutex<ComposerCache>>> {
	if cfg!(test) {
		return None;
	}
	let mut guard = SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	if guard.is_none() {
		match ComposerCache::open(None) {
			Ok(cache) => *guard = Some(Arc::new(Mutex::new(cache))),
			Err(_) => return None,
		}
	}
	guard.clone()
}

/// Store of the composer state the next launch paints before its session
/// exists.
pub struct ComposerCache {
	path: PathBuf,
	rows: Rows,
	/// Payload this process last read or wrote per `project\0kind`; identical
	/// writes skip the file transaction entirely.
	known: HashMap<String, String>,
}

impl ComposerCache {
	/// Open (creating if needed) the store at `path`, defaulting to
	/// [`composer_cache_path`]. A version mismatch clears the store.
	pub fn open(path: Option<&Path>) -> Result<ComposerCache, String> {
		let path = path.map(Path::to_path_buf).unwrap_or_else(composer_cache_path);
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
		}
		let mut rows: Rows = HashMap::new();
		if let Ok(text) = std::fs::read_to_string(&path) {
			if let Ok(value) = serde_json::from_str::<Value>(&text) {
				// A parse failure is a corrupt store; the write below replaces it.
				if value.get("version").and_then(Value::as_i64) == Some(FORMAT_VERSION) {
					if let Some(entries) = value.get("entries").and_then(Value::as_object) {
						for (key, payload) in entries {
							if let Some(payload) = payload.as_str() {
								rows.insert(key.clone(), payload.to_string());
							}
						}
					}
				}
			}
		}
		Ok(ComposerCache {
			path,
			rows,
			known: HashMap::new(),
		})
	}

	/// Everything cached for `cwd`, with any-project rows as fallback for shared
	/// kinds. Never fails.
	pub fn read(&mut self, cwd: &str) -> ComposerStartupCache {
		let project = std::path::absolute(cwd)
			.map(|path| path.to_string_lossy().into_owned())
			.unwrap_or_else(|_| cwd.to_string());
		let mut own: HashMap<&str, &String> = HashMap::new();
		let mut any_project: HashMap<&str, &String> = HashMap::new();
		for kind in ["ui", "status"] {
			for current in [project.as_str(), ANY_PROJECT] {
				let key = row_key(current, kind);
				if let Some(value) = self.rows.get(&key) {
					self.known.insert(key, value.clone());
					if current == project {
						own.insert(kind, value);
					} else {
						any_project.insert(kind, value);
					}
				}
			}
		}
		let ui = parse_json(own.get("ui").copied())
			.as_ref()
			.and_then(parse_ui_state)
			.or_else(|| parse_json(any_project.get("ui").copied()).as_ref().and_then(parse_ui_state));
		let status = parse_json(own.get("status").copied())
			.and_then(|value| parse_status(&value))
			.or_else(|| parse_json(any_project.get("status").copied()).and_then(|value| parse_status(&value)));
		ComposerStartupCache {
			preferences: ui.as_ref().map(|(preferences, _)| preferences.clone()),
			theme: ui.map(|(_, theme)| theme),
			status,
		}
	}

	/// Resolved theme and composer settings for the next prepaint.
	pub fn write_ui(&mut self, cwd: &str, preferences: &ComposerPreferences, theme: &ComposerThemePreferences) {
		let project = resolved_project(cwd);
		let payload = serde_json::json!({ "preferences": preferences_json(preferences), "theme": theme_json(theme) });
		self.put(&[project, ANY_PROJECT.to_string()], "ui", &payload);
	}

	/// Status-bar inputs for the next prepaint's startup status line.
	/// `shared: false` keeps them out of the any-project fallback (a
	/// project-scoped default model).
	pub fn write_status(&mut self, cwd: &str, status: &ComposerStatusCache, shared: bool) {
		let project = resolved_project(cwd);
		let payload = serde_json::json!({
			"borderColor": status.border_color.as_ref().map(|border| serde_json::json!({
				"prefix": border.prefix,
				"suffix": border.suffix,
			})),
			"statusLine": status.status_line,
		});
		let projects = if shared {
			vec![project, ANY_PROJECT.to_string()]
		} else {
			vec![project]
		};
		self.put(&projects, "status", &payload);
	}

	/// Flush and drop the store.
	pub fn close(self) {
		// Rows are written eagerly; nothing to finalize.
	}

	/// Best-effort upsert of `kind` under every project key, atomically: a failed
	/// write only costs the next launch its speculation.
	fn put(&mut self, projects: &[String], kind: &str, value: &Value) {
		let json = to_json(value);
		let keys: Vec<String> = projects.iter().map(|project| row_key(project, kind)).collect();
		if keys.iter().all(|key| self.known.get(key) == Some(&json)) {
			return;
		}
		for key in &keys {
			self.rows.insert(key.clone(), json.clone());
		}
		let entries: serde_json::Map<String, Value> = self
			.rows
			.iter()
			.map(|(key, payload)| (key.clone(), Value::String(payload.clone())))
			.collect();
		let document = serde_json::json!({ "version": FORMAT_VERSION, "entries": entries });
		if std::fs::write(&self.path, document.to_string()).is_ok() {
			for key in keys {
				self.known.insert(key, json.clone());
			}
		}
	}
}

fn resolved_project(cwd: &str) -> String {
	std::path::absolute(cwd)
		.map(|path| path.to_string_lossy().into_owned())
		.unwrap_or_else(|_| cwd.to_string())
}

fn preferences_json(preferences: &ComposerPreferences) -> Value {
	serde_json::json!({
		"quiet": preferences.quiet,
		"composerShape": preferences.composer_shape,
		"showHardwareCursor": preferences.show_hardware_cursor,
		"maxInlineImages": preferences.max_inline_images,
		"resizeScrollback": preferences.resize_scrollback.as_str(),
		"completeScrollback": preferences.complete_scrollback,
		"imeSafeCursor": preferences.ime_safe_cursor,
		"autocompleteMaxVisible": preferences.autocomplete_max_visible,
		"spellingTypoDetection": preferences.spelling_typo_detection,
		"spellingAutocomplete": preferences.spelling_autocomplete.as_str(),
		"spellingAutocorrect": preferences.spelling_autocorrect,
	})
}

fn theme_json(theme: &ComposerThemePreferences) -> Value {
	let mut map = serde_json::Map::new();
	if let Some(preset) = theme.symbol_preset {
		map.insert("symbolPreset".to_string(), Value::String(preset.as_str().to_string()));
	}
	if let Some(color_blind_mode) = theme.color_blind_mode {
		map.insert("colorBlindMode".to_string(), Value::Bool(color_blind_mode));
	}
	if let Some(dark) = &theme.dark_theme {
		map.insert("darkTheme".to_string(), Value::String(dark.clone()));
	}
	if let Some(light) = &theme.light_theme {
		map.insert("lightTheme".to_string(), Value::String(light.clone()));
	}
	Value::Object(map)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::prompt::word_completion::WordCompletionMethod;

	fn temp_path(name: &str) -> PathBuf {
		let dir = std::env::temp_dir().join(format!("maple-composer-cache-{}", std::process::id()));
		std::fs::create_dir_all(&dir).expect("temp dir");
		dir.join(name)
	}

	fn status() -> ComposerStatusCache {
		ComposerStatusCache {
			border_color: Some(BorderColor {
				prefix: "\u{1b}[38;2;1;2;3m".to_string(),
				suffix: "\u{1b}[39m".to_string(),
			}),
			status_line: serde_json::json!({ "settings": {}, "gitEnabled": false }),
		}
	}

	#[test]
	fn ui_round_trips_and_falls_back_to_any_project() {
		let path = temp_path("ui.json");
		let mut preferences = ComposerPreferences::default();
		preferences.quiet = true;
		preferences.composer_shape = "box".to_string();
		preferences.spelling_autocomplete = WordCompletionMethod::Ngram;
		let theme = ComposerThemePreferences {
			symbol_preset: Some(SymbolPreset::Nerd),
			color_blind_mode: Some(true),
			dark_theme: Some("dark".to_string()),
			light_theme: None,
		};
		{
			let mut cache = ComposerCache::open(Some(&path)).expect("open");
			cache.write_ui("/tmp/project-a", &preferences, &theme);
		}
		let mut cache = ComposerCache::open(Some(&path)).expect("reopen");
		let own = cache.read("/tmp/project-a");
		assert_eq!(own.preferences, Some(preferences.clone()));
		assert_eq!(own.theme, Some(theme.clone()));
		// A project without its own row inherits the any-project fallback.
		let other = cache.read("/tmp/project-b");
		assert_eq!(other.preferences, Some(preferences));
		assert_eq!(other.theme, Some(theme));
		std::fs::remove_file(&path).ok();
	}

	#[test]
	fn status_round_trips_and_unshared_rows_stay_local() {
		let path = temp_path("status.json");
		{
			let mut cache = ComposerCache::open(Some(&path)).expect("open");
			cache.write_status("/tmp/project-a", &status(), true);
		}
		let mut cache = ComposerCache::open(Some(&path)).expect("reopen");
		assert!(cache.read("/tmp/project-a").status.is_some());
		assert!(cache.read("/tmp/project-b").status.is_some());

		let mut cache = ComposerCache::open(Some(&path)).expect("reopen");
		cache.write_status("/tmp/project-c", &status(), false);
		cache.read("/tmp/project-d");
		let entries = std::fs::read_to_string(&path).expect("store");
		let document: Value = serde_json::from_str(&entries).expect("json");
		let keys: Vec<&String> = document["entries"].as_object().expect("entries").keys().collect();
		assert!(keys.iter().any(|key| key.contains("project-c")));
		std::fs::remove_file(&path).ok();
	}

	#[test]
	fn version_mismatch_clears_the_store() {
		let path = temp_path("version.json");
		std::fs::write(
			&path,
			serde_json::json!({
				"version": FORMAT_VERSION + 1,
				"entries": { "p\u{0}ui": "{}" }
			})
			.to_string(),
		)
		.expect("write");
		let mut cache = ComposerCache::open(Some(&path)).expect("open");
		let read = cache.read("/tmp/p");
		assert!(read.preferences.is_none());
		std::fs::remove_file(&path).ok();
	}

	#[test]
	fn malformed_ui_state_is_ignored() {
		let path = temp_path("malformed.json");
		std::fs::write(
			&path,
			serde_json::json!({
				"version": FORMAT_VERSION,
				"entries": { format!("{}\u{0}ui", ANY_PROJECT): "{\"preferences\":{\"quiet\":\"yes\"},\"theme\":{}}" }
			})
			.to_string(),
		)
		.expect("write");
		let mut cache = ComposerCache::open(Some(&path)).expect("open");
		let read = cache.read("/tmp/project");
		assert!(read.preferences.is_none());
		assert!(read.theme.is_none());
		std::fs::remove_file(&path).ok();
	}

	#[test]
	fn identical_writes_skip_the_file() {
		let path = temp_path("skip.json");
		let mut cache = ComposerCache::open(Some(&path)).expect("open");
		cache.write_status("/tmp/project", &status(), true);
		let first = std::fs::metadata(&path).expect("written").modified().expect("mtime");
		std::thread::sleep(std::time::Duration::from_millis(10));
		cache.write_status("/tmp/project", &status(), true);
		let second = std::fs::metadata(&path).expect("written").modified().expect("mtime");
		assert_eq!(first, second);
		std::fs::remove_file(&path).ok();
	}
}
