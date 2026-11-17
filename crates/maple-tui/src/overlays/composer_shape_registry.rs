//! Composer shape registry and built-in definitions.
//!
//! Reference: `l/src/overlays/composer-shape-registry.ts`.
//!
//! Deviations:
//! - `registerComposerStyle` and the TUI composer style system have no direct
//!   crate equivalent in this module; `install_extension_composer_shape` tracks
//!   selector options only and returns a deregistration closure. Actual runtime
//!   style installation must be wired by the host/composer subsystem.
//! - Registry is process-local global state (`LazyLock<Mutex<BTreeMap<..>>>`)
//!   so selector options can be queried without a host object.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

/// Composer shape id; extensions may register additional values at runtime.
pub type ComposerShape = String;

/// Option shown in composer shape selectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmenuOption {
	pub value: String,
	pub label: String,
	pub description: Option<String>,
}

/// Built-in composer shape definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinComposerShape {
	pub value: &'static str,
	pub label: &'static str,
	pub description: &'static str,
}

/// Built-in composer choices and their shared settings/setup copy.
pub const BUILTIN_COMPOSER_SHAPES: &[BuiltinComposerShape] = &[
	BuiltinComposerShape {
		value: "band",
		label: "Status Band (Default)",
		description: "Flush soft-capped status band above a curved prompt, no frame",
	},
	BuiltinComposerShape {
		value: "box",
		label: "Rounded Box",
		description: "Status line embedded in top border, compact 2-line prompt",
	},
	BuiltinComposerShape {
		value: "claude",
		label: "Claude Code",
		description: "Full-width horizontal rules above and below, status line at bottom",
	},
	BuiltinComposerShape {
		value: "pi",
		label: "Pi",
		description: "Framed horizontal rules with status line at bottom",
	},
	BuiltinComposerShape {
		value: "borderless",
		label: "Borderless",
		description: "Clean prompt glyph with status line at bottom, no box borders",
	},
	BuiltinComposerShape {
		value: "rule",
		label: "Top Rule Dock",
		description: "Single top rule with status docked onto it and below",
	},
	BuiltinComposerShape {
		value: "field",
		label: "Compact Field",
		description: "Filled one-row field with accent end caps",
	},
	BuiltinComposerShape {
		value: "rail",
		label: "Accent Rail",
		description: "Filled one-row field anchored by a single accent rail",
	},
];

/// Visual composer style definition registered by an extension.
///
/// `style` is opaque JSON-like data in this port because the composer style
/// runtime is not vendored in `maple-tui`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerShapeDefinition {
	pub id: String,
	pub label: String,
	pub description: Option<String>,
	pub style: serde_json::Value,
}

static REGISTRY: LazyLock<Mutex<BTreeMap<String, SubmenuOption>>> =
	LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Built-in composer ids.
#[must_use]
pub fn composer_shape_values() -> Vec<&'static str> {
	BUILTIN_COMPOSER_SHAPES.iter().map(|s| s.value).collect()
}

/// Install one extension composer shape into selector registries.
///
/// Returns a closure that removes the option when called (style unregister is a
/// host responsibility; see module deviations).
pub fn install_extension_composer_shape(definition: ComposerShapeDefinition) -> impl FnOnce() {
	let id = definition.id.clone();
	let option = SubmenuOption {
		value: definition.id,
		label: definition.label,
		description: definition.description,
	};
	let mut guard = REGISTRY.lock().expect("composer shape registry poisoned");
	guard.insert(id.clone(), option);
	move || {
		let mut guard = REGISTRY.lock().expect("composer shape registry poisoned");
		guard.remove(&id);
	}
}

/// Available built-in and extension composer choices in selector order.
#[must_use]
pub fn get_composer_shape_options() -> Vec<SubmenuOption> {
	let mut options: Vec<SubmenuOption> = BUILTIN_COMPOSER_SHAPES
		.iter()
		.map(|shape| SubmenuOption {
			value: shape.value.to_string(),
			label: shape.label.to_string(),
			description: Some(shape.description.to_string()),
		})
		.collect();
	let guard = REGISTRY.lock().expect("composer shape registry poisoned");
	options.extend(guard.values().cloned());
	options
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn builtin_shapes_are_in_expected_order() {
		let values = composer_shape_values();
		assert_eq!(values.first().copied(), Some("band"));
		assert_eq!(values.last().copied(), Some("rail"));
		assert_eq!(values.len(), 8);
	}

	#[test]
	fn install_and_remove_extension_shape_updates_options() {
		let uninstall = install_extension_composer_shape(ComposerShapeDefinition {
			id: "ext-test-shape".into(),
			label: "Ext Test".into(),
			description: Some("desc".into()),
			style: json!({"id":"ext-test-shape"}),
		});
		let options = get_composer_shape_options();
		assert!(options.iter().any(|o| o.value == "ext-test-shape"));
		uninstall();
		let options = get_composer_shape_options();
		assert!(!options.iter().any(|o| o.value == "ext-test-shape"));
	}

	#[test]
	fn builtin_options_include_descriptions() {
		let options = get_composer_shape_options();
		assert!(options.iter().take(8).all(|o| o.description.is_some()));
	}
}
