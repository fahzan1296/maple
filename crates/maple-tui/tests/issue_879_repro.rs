//! Port of `l/test/issue-879-repro.test.ts`.
//!
//! Deviations:
//! - The reference's `parseKey` decodes raw stdin bytes under a host key
//!   parser; the crate decodes keys upstream and exposes only the canonical
//!   id, so `ESC + 'P'` is represented as `Key::Alt('P')` directly.
//! - Kitty-protocol state (`setKittyProtocolActive`) has no crate equivalent;
//!   legacy `Alt+letter` decoding is unconditional here.

use maple_tui::keybindings::{canonical_key_id, key_id};
use maple_tui::keys::Key;

#[test]
fn uppercase_alt_pair_decodes_as_alt_shift_letter() {
	assert_eq!(key_id(&Key::Alt('P')).as_deref(), Some("alt+shift+p"));
	assert_eq!(key_id(&Key::Alt('A')).as_deref(), Some("alt+shift+a"));
	assert_eq!(key_id(&Key::Alt('Z')).as_deref(), Some("alt+shift+z"));
}

#[test]
fn lowercase_alt_pair_stays_alt_without_shift() {
	assert_eq!(key_id(&Key::Alt('p')).as_deref(), Some("alt+p"));
	assert_ne!(key_id(&Key::Alt('p')).as_deref(), Some("alt+shift+p"));
	assert_ne!(key_id(&Key::Alt('Q')).as_deref(), Some("alt+shift+p"));
	assert_eq!(key_id(&Key::Alt('Q')).as_deref(), Some("alt+shift+q"));
}

#[test]
fn canonical_ids_normalize_modifier_order_and_uppercase_shift() {
	assert_eq!(canonical_key_id(&["shift", "alt"], "p"), "alt+shift+p");
	assert_eq!(canonical_key_id(&["alt", "shift"], "p"), "alt+shift+p");
	assert_eq!(canonical_key_id(&["alt"], "P"), "alt+shift+p");
	assert_eq!(canonical_key_id(&[], "A"), "shift+a");
	assert_eq!(canonical_key_id(&["ctrl", "alt"], "p"), "ctrl+alt+p");
}