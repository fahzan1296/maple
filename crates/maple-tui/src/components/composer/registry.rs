//! Composer style registry: built-in lookup plus extension registration.
//!
//! Reference: `l/src/components/composer/registry.ts`.
//!
//! Deviation: the reference keeps `extensionComposerStyles` in a module-level
//! `Map`. Rust has no module-level mutable singleton, so the map lives behind
//! a process-global `Mutex`. Registration is process-wide, and the disposer
//! compares the registered `Arc` by identity exactly like the reference's
//! `extensionComposerStyles.get(id) === style` check.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use super::band::BAND_COMPOSER_STYLE;
use super::borderless::BORDERLESS_COMPOSER_STYLE;
use super::r#box::BOX_COMPOSER_STYLE;
use super::claude::CLAUDE_COMPOSER_STYLE;
use super::field::FIELD_COMPOSER_STYLE;
use super::pi::PI_COMPOSER_STYLE;
use super::rail::RAIL_COMPOSER_STYLE;
use super::rule::RULE_COMPOSER_STYLE;
use super::types::{BUILTIN_EDITOR_BORDER_STYLES, ComposerStyle};

static EXTENSION_COMPOSER_STYLES: LazyLock<Mutex<HashMap<String, Arc<ComposerStyle>>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

/// Why a composer style registration was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterComposerStyleError {
	/// The id is empty or has surrounding whitespace.
	InvalidId,
	/// The id names a built-in composer style.
	Builtin,
	/// An extension style with this id is already registered.
	Duplicate,
}

impl std::fmt::Display for RegisterComposerStyleError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RegisterComposerStyleError::InvalidId => {
				f.write_str("Composer style id must be a non-empty trimmed string")
			}
			RegisterComposerStyleError::Builtin => {
				f.write_str("Cannot replace built-in composer style")
			}
			RegisterComposerStyleError::Duplicate => {
				f.write_str("Composer style is already registered")
			}
		}
	}
}

impl std::error::Error for RegisterComposerStyleError {}

/// Removes one [`register_composer_style`] registration when disposed.
pub struct ComposerStyleRegistration {
	id: String,
	style: Arc<ComposerStyle>,
}

impl ComposerStyleRegistration {
	/// Remove this registration, but only while it is still the registered one.
	pub fn dispose(&self) {
		let mut styles = EXTENSION_COMPOSER_STYLES.lock().unwrap();
		let remove = styles
			.get(&self.id)
			.map(|current| Arc::ptr_eq(current, &self.style))
			.unwrap_or(false);
		if remove {
			styles.remove(&self.id);
		}
	}
}

fn builtin_composer_style(id: &str) -> Option<&'static ComposerStyle> {
	Some(match id {
		"box" => &*BOX_COMPOSER_STYLE,
		"band" => &*BAND_COMPOSER_STYLE,
		"claude" => &*CLAUDE_COMPOSER_STYLE,
		"pi" => &*PI_COMPOSER_STYLE,
		"borderless" => &*BORDERLESS_COMPOSER_STYLE,
		"rule" => &*RULE_COMPOSER_STYLE,
		"field" => &*FIELD_COMPOSER_STYLE,
		"rail" => &*RAIL_COMPOSER_STYLE,
		_ => return None,
	})
}

/// Whether an id names a composer style shipped by maple-tui.
pub fn is_builtin_composer_style(id: &str) -> bool {
	BUILTIN_EDITOR_BORDER_STYLES.contains(&id)
}

/// Whether a style paints its own row foreground.
///
/// Extensions registered before `filledSurface` existed received undecorated
/// row text, so an omitted flag remains filled for extension-owned ids.
pub fn is_filled_composer_style(style: &ComposerStyle) -> bool {
	style
		.filled_surface
		.unwrap_or_else(|| !is_builtin_composer_style(&style.id))
}

/// Register one extension-owned composer style for this process.
///
/// Built-in ids and duplicate extension ids are rejected. The returned
/// registration removes only this registration.
pub fn register_composer_style(
	style: ComposerStyle,
) -> Result<ComposerStyleRegistration, RegisterComposerStyleError> {
	let id = style.id.clone();
	let trimmed = id.trim();
	if trimmed.is_empty() || trimmed != id.as_str() {
		return Err(RegisterComposerStyleError::InvalidId);
	}
	if is_builtin_composer_style(trimmed) {
		return Err(RegisterComposerStyleError::Builtin);
	}
	let mut styles = EXTENSION_COMPOSER_STYLES.lock().unwrap();
	if styles.contains_key(trimmed) {
		return Err(RegisterComposerStyleError::Duplicate);
	}
	let style = Arc::new(style);
	styles.insert(trimmed.to_string(), Arc::clone(&style));
	Ok(ComposerStyleRegistration {
		id: trimmed.to_string(),
		style,
	})
}

/// Style object for a composer shape; unknown ids fall back to `box`.
pub fn get_composer_style(id: &str) -> ComposerStyle {
	let styles = EXTENSION_COMPOSER_STYLES.lock().unwrap();
	if let Some(style) = styles.get(id) {
		return (**style).clone();
	}
	drop(styles);
	if let Some(style) = builtin_composer_style(id) {
		return style.clone();
	}
	(*BOX_COMPOSER_STYLE).clone()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::composer::types::{BottomBar, StatusAttachment, VerticalChrome};

	fn custom_style(id: &str) -> ComposerStyle {
		let mut style = (*BORDERLESS_COMPOSER_STYLE).clone();
		style.id = id.to_string();
		style.filled_surface = None;
		style
	}

	#[test]
	fn builtin_ids_are_recognized() {
		for id in BUILTIN_EDITOR_BORDER_STYLES {
			assert!(is_builtin_composer_style(id), "{id}");
		}
		for id in ["", "nope", "Box", "box "] {
			assert!(!is_builtin_composer_style(id), "{id}");
		}
	}

	#[test]
	fn filled_defaults_to_extension_ownership() {
		let cases = [
			(&*BOX_COMPOSER_STYLE, false),
			(&*BAND_COMPOSER_STYLE, false),
			(&*CLAUDE_COMPOSER_STYLE, false),
			(&*PI_COMPOSER_STYLE, false),
			(&*BORDERLESS_COMPOSER_STYLE, false),
			(&*RULE_COMPOSER_STYLE, false),
			(&*FIELD_COMPOSER_STYLE, true),
			(&*RAIL_COMPOSER_STYLE, true),
		];
		for (style, expected) in cases {
			assert_eq!(is_filled_composer_style(style), expected, "{}", style.id);
		}
		assert!(is_filled_composer_style(&custom_style("ext-unfilled")));
		let mut filled = custom_style("ext-filled");
		filled.filled_surface = Some(true);
		assert!(is_filled_composer_style(&filled));
		let mut transparent = custom_style("ext-transparent");
		transparent.filled_surface = Some(false);
		assert!(!is_filled_composer_style(&transparent));
	}

	#[test]
	fn unknown_ids_fall_back_to_box() {
		assert_eq!(get_composer_style("nope").id, "box");
		assert_eq!(get_composer_style("").id, "box");
		assert_eq!(get_composer_style("field").id, "field");
	}

	#[test]
	fn extension_registration_round_trips_and_disposes() {
		let id = "test-ext-round-trip";
		let registration = register_composer_style(custom_style(id)).expect("register");
		assert_eq!(get_composer_style(id).id, id);
		registration.dispose();
		assert_eq!(get_composer_style(id).id, "box");
	}

	#[test]
	fn registration_rejects_invalid_builtin_and_duplicate_ids() {
		for id in ["", "   ", " box", "box "] {
			assert!(
				matches!(
					register_composer_style(custom_style(id)),
					Err(RegisterComposerStyleError::InvalidId)
				),
				"{id}"
			);
		}
		assert!(matches!(
			register_composer_style(custom_style("box")),
			Err(RegisterComposerStyleError::Builtin)
		));

		let id = "test-ext-duplicate";
		let first = register_composer_style(custom_style(id)).expect("first");
		assert!(matches!(
			register_composer_style(custom_style(id)),
			Err(RegisterComposerStyleError::Duplicate)
		));
		first.dispose();
		assert!(register_composer_style(custom_style(id)).is_ok());
	}

	#[test]
	fn disposing_one_registration_keeps_the_others() {
		let keep = "test-ext-keep";
		let drop = "test-ext-drop";
		let keep_registration = register_composer_style(custom_style(keep)).expect("keep");
		let drop_registration = register_composer_style(custom_style(drop)).expect("drop");
		drop_registration.dispose();
		assert_eq!(get_composer_style(drop).id, "box");
		assert_eq!(get_composer_style(keep).id, keep);
		keep_registration.dispose();
	}

	#[test]
	fn disposer_is_idempotent() {
		let id = "test-ext-idempotent";
		let registration = register_composer_style(custom_style(id)).expect("register");
		registration.dispose();
		registration.dispose();
		assert_eq!(get_composer_style(id).id, "box");
	}

	#[test]
	fn extension_metadata_is_preserved() {
		let id = "test-ext-metadata";
		let mut style = custom_style(id);
		style.side_borders = true;
		style.vertical_chrome = VerticalChrome::Two;
		style.status_attachment = StatusAttachment::TopBand;
		style.bottom_bar = BottomBar::Left;
		let registration = register_composer_style(style).expect("register");
		let resolved = get_composer_style(id);
		assert!(resolved.side_borders);
		assert_eq!(resolved.vertical_chrome, VerticalChrome::Two);
		assert_eq!(resolved.status_attachment, StatusAttachment::TopBand);
		assert_eq!(resolved.bottom_bar, BottomBar::Left);
		registration.dispose();
	}
}
