//! Keybinding hint formatting utilities.
//!
//! Reference: `l/src/chrome/keybinding-hints.ts`.
//!
//! Deviations:
//! - The reference reads editor/app keybindings from global managers and
//!   formats via `app-keybindings` helpers. maple-tui uses [`KeybindingsManager`]
//!   with string action ids and [`KeyHintFormatter`] that formats keys against a
//!   [`Theme`], so hint functions take `(bindings, theme, action/keys)`
//!   explicitly.
//! - `AppKeybinding`/`Keybinding`/`KeyName` map to `&str` identifiers here.
//! - Styling applies `theme.fg("dim"/"muted", ...)` to match intent.

use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::theme::Theme;

/// Primary (first) key bound to an action, formatted for hints; empty when unbound.
pub fn editor_key(bindings: &KeybindingsManager, theme: &Theme, action: &str) -> String {
	let formatter = KeyHintFormatter::new(theme);
	bindings
		.get_keys(action)
		.first()
		.map(|key| formatter.format_key_hint(key))
		.unwrap_or_default()
}

/// Primary keys of several actions, slash-joined.
pub fn editor_keys(bindings: &KeybindingsManager, theme: &Theme, actions: &[&str]) -> String {
	actions
		.iter()
		.map(|action| editor_key(bindings, theme, action))
		.collect::<Vec<_>>()
		.join("/")
}

/// Keys bound to `action`, or `fallback` when none.
pub fn bound_keys(
	bindings: &KeybindingsManager,
	action: &str,
	fallback: &[String],
) -> Vec<String> {
	let keys = bindings.get_keys(action);
	if keys.is_empty() {
		fallback.to_vec()
	} else {
		keys
	}
}

/// Primary interrupt key (`app.interrupt`, raw Escape when unbound).
pub fn interrupt_key(bindings: &KeybindingsManager, theme: &Theme) -> String {
	let fallback = vec!["escape".to_string()];
	let formatter = KeyHintFormatter::new(theme);
	let keys = bound_keys(bindings, "app.interrupt", &fallback);
	keys.first()
		.map(|k| formatter.format_key_hint(k))
		.unwrap_or_else(|| formatter.format_key_hint("escape"))
}

/// Primary key bound to an app action.
pub fn app_key(bindings: &KeybindingsManager, theme: &Theme, action: &str) -> String {
	editor_key(bindings, theme, action)
}

fn dim_muted(theme: &Theme, key: &str, description: &str) -> String {
	let dim = theme.fg("dim", key).unwrap_or_else(|_| key.to_string());
	let muted = theme
		.fg("muted", &format!(" {description}"))
		.unwrap_or_else(|_| format!(" {description}"));
	format!("{dim}{muted}")
}

/// Format a keybinding hint with consistent styling: dim key, muted description.
pub fn key_hint(
	bindings: &KeybindingsManager,
	theme: &Theme,
	action: &str,
	description: &str,
) -> String {
	dim_muted(theme, &editor_key(bindings, theme, action), description)
}

/// Format a keybinding hint for app-level actions.
pub fn app_key_hint(
	bindings: &KeybindingsManager,
	theme: &Theme,
	action: &str,
	description: &str,
) -> String {
	dim_muted(theme, &app_key(bindings, theme, action), description)
}

/// Format a hint for fixed (non-configurable) keys.
pub fn raw_key_hint(theme: &Theme, keys: &[&str], description: &str) -> String {
	let formatter = KeyHintFormatter::new(theme);
	dim_muted(theme, &formatter.format_key_hints(keys), description)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::keybindings::KeybindingsManager;
	use crate::theme::loader::{create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		let json = crate::theme::builtin::builtin_theme_json("dark").expect("dark");
		create_theme(json, CreateThemeOptions::default()).expect("theme")
	}

	#[test]
	fn editor_and_interrupt_keys_resolve_defaults() {
		let bindings = KeybindingsManager::new();
		let theme = test_theme();
		assert!(!editor_key(&bindings, &theme, "tui.select.up").is_empty());
		assert!(!interrupt_key(&bindings, &theme).is_empty());
	}

	#[test]
	fn editor_keys_join_multiple_actions() {
		let bindings = KeybindingsManager::new();
		let theme = test_theme();
		let joined = editor_keys(&bindings, &theme, &["tui.select.up", "tui.select.down"]);
		assert!(joined.contains('/'));
	}

	#[test]
	fn bound_keys_falls_back_when_unbound() {
		let mut bindings = KeybindingsManager::new();
		bindings.set_user_binding("tui.select.up", Some(Vec::new()));
		let fallback = vec!["up".to_string()];
		assert_eq!(bound_keys(&bindings, "tui.select.up", &fallback), fallback);
	}

	#[test]
	fn raw_key_hint_formats_fixed_keys() {
		let theme = test_theme();
		let hint = raw_key_hint(&theme, &["up", "down"], "navigate");
		assert!(hint.contains("navigate"));
		assert!(hint.contains('/'));
	}
}
