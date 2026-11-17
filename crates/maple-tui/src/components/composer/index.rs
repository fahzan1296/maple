//! Composer chrome styles, contexts, and the style registry.
//!
//! Reference: `l/src/components/composer/index.ts`.
//!
//! The reference re-exports every composer submodule; this port does the same.
//! Submodules are declared with explicit `#[path]` attributes so this file can
//! be wired either as the `composer` module root or as `composer::index`.

#[path = "band.rs"]
pub mod band;
#[path = "borderless.rs"]
pub mod borderless;
#[path = "box.rs"]
pub mod r#box;
#[path = "claude.rs"]
pub mod claude;
#[path = "field.rs"]
pub mod field;
#[path = "pi.rs"]
pub mod pi;
#[path = "rail.rs"]
pub mod rail;
#[path = "registry.rs"]
pub mod registry;
#[path = "rule.rs"]
pub mod rule;
#[path = "types.rs"]
pub mod types;

pub use band::BAND_COMPOSER_STYLE;
pub use borderless::BORDERLESS_COMPOSER_STYLE;
pub use r#box::BOX_COMPOSER_STYLE;
pub use claude::CLAUDE_COMPOSER_STYLE;
pub use field::FIELD_COMPOSER_STYLE;
pub use pi::PI_COMPOSER_STYLE;
pub use rail::RAIL_COMPOSER_STYLE;
pub use registry::{
	ComposerStyleRegistration, RegisterComposerStyleError, get_composer_style,
	is_builtin_composer_style, is_filled_composer_style, register_composer_style,
};
pub use rule::{render_top_rule, RULE_COMPOSER_STYLE};
pub use types::{
	BUILTIN_EDITOR_BORDER_STYLES, BottomBar, BuiltinEditorBorderStyle, ComposerBox,
	ComposerChromeContext, ComposerPaint, ComposerRowContext, ComposerStyle, DefaultPaddingXFn,
	EditorBorderStyle, EditorTopBorder, RenderBottomFn, RenderRowFn, RenderTopFn,
	SideChromeWidthFn, StatusAttachment, VerticalChrome,
};

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reexports_expose_every_builtin_style() {
		assert_eq!(BOX_COMPOSER_STYLE.id, "box");
		assert_eq!(BAND_COMPOSER_STYLE.id, "band");
		assert_eq!(CLAUDE_COMPOSER_STYLE.id, "claude");
		assert_eq!(PI_COMPOSER_STYLE.id, "pi");
		assert_eq!(BORDERLESS_COMPOSER_STYLE.id, "borderless");
		assert_eq!(RULE_COMPOSER_STYLE.id, "rule");
		assert_eq!(FIELD_COMPOSER_STYLE.id, "field");
		assert_eq!(RAIL_COMPOSER_STYLE.id, "rail");
	}

	#[test]
	fn reexported_registry_helpers_are_callable() {
		assert!(is_builtin_composer_style("box"));
		assert!(!is_builtin_composer_style("nope"));
		assert_eq!(get_composer_style("nope").id, "box");
	}
}
