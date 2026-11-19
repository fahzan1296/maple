//! Symbol lookup against the active theme, falling back to the ascii preset.
//!
//! Reference: `l/src/theme/active-symbols.ts`.
//!
//! # Divergences
//!
//! - The reference keeps the active theme in a module-level `let` binding
//!   reassigned by `setActiveSymbolTheme`. Rust has no such mutable module
//!   singleton (a global would need a lock), so the state travels as
//!   [`ActiveSymbols`] and callers pass it along.
//! - `SYMBOL_PRESETS.ascii[key]` does not exist in this crate; the equivalent
//!   static table is `preset_symbols(SymbolPreset::Ascii)`. An unknown key
//!   yields `""` rather than `undefined`.

use std::rc::Rc;

use crate::theme::symbols::{preset_symbols, SymbolPreset};
use crate::theme::Theme;

/// Active-theme symbol state, mirroring the reference's module binding.
#[derive(Clone, Default)]
pub struct ActiveSymbols {
	theme: Option<Rc<Theme>>,
}

impl ActiveSymbols {
	/// No active theme; every lookup falls back to the ascii preset.
	pub fn new() -> Self {
		Self { theme: None }
	}

	/// Replace the active theme; `None` clears it, matching
	/// `setActiveSymbolTheme(undefined)`.
	pub fn set_active_symbol_theme(&mut self, value: Option<Rc<Theme>>) {
		self.theme = value;
	}

	/// The active theme's glyph for `key`, or the ascii preset's when no theme
	/// is active. Unknown keys yield `""`.
	pub fn active_theme_symbol(&self, key: &str) -> &str {
		match &self.theme {
			Some(theme) => theme.symbol(key),
			None => ascii_symbol(key),
		}
	}
}

/// The ascii preset glyph for `key`; empty when the key is unknown.
pub fn ascii_symbol(key: &str) -> &'static str {
	preset_symbols(SymbolPreset::Ascii)
		.get(key)
		.unwrap_or("")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::create_theme;

	#[test]
	fn falls_back_to_ascii_before_a_theme_loads() {
		let symbols = ActiveSymbols::new();
		assert_eq!(symbols.active_theme_symbol("sep.dot"), ascii_symbol("sep.dot"));
		assert_eq!(symbols.active_theme_symbol("status.success"), ascii_symbol("status.success"));
		assert_eq!(symbols.active_theme_symbol("missing.key"), "");
	}

	#[test]
	fn active_theme_overrides_ascii_and_can_be_cleared() {
		let theme = create_theme("dark", None).expect("dark theme");
		let mut symbols = ActiveSymbols::new();
		symbols.set_active_symbol_theme(Some(Rc::new(theme.clone())));
		assert_eq!(
			symbols.active_theme_symbol("sep.dot"),
			theme.symbol("sep.dot")
		);

		symbols.set_active_symbol_theme(None);
		assert_eq!(symbols.active_theme_symbol("sep.dot"), ascii_symbol("sep.dot"));
	}

	#[test]
	fn ascii_table_matches_the_reference_preset() {
		assert_eq!(ascii_symbol("sep.dot"), " - ");
		assert_eq!(ascii_symbol("status.success"), "[+]");
	}
}
