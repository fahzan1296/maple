//! Port of `l/test/theme-epoch-fallback.test.ts`.
//!
//! Contract: every change to the active theme bumps the theme epoch — including
//! `set_theme()`'s fallback path. Memoized renderers key on epoch, so a failed
//! theme load that swapped to the dark fallback without bumping the epoch would
//! leave stale colors until unrelated state changed.

use maple_tui::theme::{default_theme, get_theme_epoch, set_theme, set_theme_instance};

#[test]
fn bumps_epoch_when_invalid_theme_name_falls_back_to_dark() {
	// Leave a deterministic active theme.
	set_theme_instance(default_theme());
	let before = get_theme_epoch();

	let result = set_theme("__definitely_not_a_real_theme__");

	// Invalid theme -> load throws -> dark fallback applied.
	assert!(result.is_err(), "expected invalid theme to fail");
	// The active theme changed, so the epoch must advance for memoized renderers.
	assert!(get_theme_epoch() > before, "epoch must advance on fallback");
}
