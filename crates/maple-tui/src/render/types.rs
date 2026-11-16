//! Shared types for TUI rendering components.
//!
//! Reference: `l/src/render/types.ts`. `State` is not ported here; it already
//! lives as [`crate::render::render_utils::State`].

use crate::theme::Theme;

/// Position and theme context passed to tree item renderers.
#[derive(Clone)]
pub struct TreeContext {
	/// Flattened index of the item among its siblings.
	pub index: usize,
	/// Whether this item is the last sibling.
	pub is_last: bool,
	/// Depth from the root (roots are zero).
	pub depth: usize,
	/// Active resolved theme.
	pub theme: Theme,
	/// Branch prefix drawn before the item's own content.
	pub prefix: String,
	/// Prefix drawn before the item's continuation lines.
	pub continue_prefix: String,
	/// Maximum rendered branch/continuation prefix width reserved during
	/// pre-rendering.
	pub prefix_width: Option<usize>,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("embedded dark theme exists"),
			CreateThemeOptions::default(),
		)
		.expect("embedded dark theme resolves")
	}

	#[test]
	fn tree_context_round_trips_its_fields() {
		let theme = test_theme();
		let context = TreeContext {
			index: 3,
			is_last: true,
			depth: 2,
			theme: theme.clone(),
			prefix: "├─ ".to_string(),
			continue_prefix: "│  ".to_string(),
			prefix_width: Some(4),
		};
		assert_eq!(context.index, 3);
		assert!(context.is_last);
		assert_eq!(context.depth, 2);
		assert_eq!(context.prefix, "├─ ");
		assert_eq!(context.continue_prefix, "│  ");
		assert_eq!(context.prefix_width, Some(4));
		assert_eq!(
			context.theme.get_color_hex("accent").expect("accent token exists"),
			"#febc38"
		);
		assert_eq!(context.theme.get_color_hex("accent"), theme.get_color_hex("accent"));

		let absent = TreeContext {
			index: 0,
			is_last: false,
			depth: 0,
			theme,
			prefix: String::new(),
			continue_prefix: String::new(),
			prefix_width: None,
		};
		assert_eq!(absent.prefix_width, None);
		assert_eq!(absent.prefix, "");
	}
}
