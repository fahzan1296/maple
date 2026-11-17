//! Port of `l/test/windows-altgr.test.ts` (pure/portable subset).
//!
//! Divergence: Win32 `readAltGrLayer`/`LoadKeyboardLayoutW` coverage is omitted
//! (Windows-only FFI). Ports translation, latch, and sequence-interpretation
//! behavior via exported pure APIs and an injected `AltGrHost`.

use std::cell::Cell;

use maple_tui::windows_altgr::{
	create_right_alt_latch, table_key, translate_windows_altgr_sequence, AltGrHost, AltGrTable,
	RIGHT_ALT_LATCH_MS,
};

fn hungarian_layer() -> AltGrTable {
	let mut layer = AltGrTable::new();
	layer.insert(table_key('f' as u32, false), "[".to_string());
	layer.insert(table_key('g' as u32, false), "]".to_string());
	layer.insert(table_key('b' as u32, false), "{".to_string());
	layer.insert(table_key('n' as u32, false), "}".to_string());
	layer.insert(table_key('q' as u32, false), "\\".to_string());
	layer
}

struct Host {
	layer: AltGrTable,
	down: bool,
}

impl AltGrHost for Host {
	fn active_layer(&self) -> AltGrTable {
		self.layer.clone()
	}
	fn is_right_alt_down(&self) -> bool {
		self.down
	}
}

#[test]
fn recovers_altgr_text_reported_as_alt_or_ctrl_alt_chord() {
	let host = Host { layer: hungarian_layer(), down: true };
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;3u", &host).as_deref(), Some("["));
	assert_eq!(translate_windows_altgr_sequence("\x1b[103;7u", &host).as_deref(), Some("]"));
	assert_eq!(translate_windows_altgr_sequence("\x1b[98;3u", &host).as_deref(), Some("{"));
	assert_eq!(translate_windows_altgr_sequence("\x1b[110;3u", &host).as_deref(), Some("}"));
	// Caps/Num Lock bits do not hide the chord.
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;131u", &host).as_deref(), Some("["));
	// Uppercase base key (Caps Lock) still hits the lowercased table.
	assert_eq!(translate_windows_altgr_sequence("\x1b[70;3u", &host).as_deref(), Some("["));
}

#[test]
fn keeps_left_alt_chords_as_shortcuts_when_right_alt_not_held() {
	let host = Host { layer: hungarian_layer(), down: false };
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;3u", &host), None);
}

#[test]
fn keeps_alt_chords_on_layouts_without_altgr_layer() {
	let host = Host { layer: AltGrTable::new(), down: true };
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;3u", &host), None);
}

#[test]
fn ignores_non_altgr_keys_modifiers_and_events() {
	let host = Host { layer: hungarian_layer(), down: true };
	assert_eq!(translate_windows_altgr_sequence("\x1b[104;3u", &host), None); // h unmapped
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;5u", &host), None); // Ctrl only
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;11u", &host), None); // Super+Alt
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;3:3u", &host), None); // release
	assert_eq!(translate_windows_altgr_sequence("\x1b[102;4u", &host), None); // shift layer unmapped
	assert_eq!(translate_windows_altgr_sequence("\x1b[1;3A", &host), None); // Alt+Up (not CSI-u)
}

#[test]
fn right_alt_latch_observes_release_through_synchronous_batch() {
	let down = Cell::new(true);
	let now = Cell::new(1000u64);
	let latched = create_right_alt_latch(move || down.get(), move || now.get());
	assert!(latched());
	down.set(false);
	now.set(1001);
	assert!(latched());
	now.set(1000 + RIGHT_ALT_LATCH_MS);
	assert!(!latched());
}

#[test]
fn right_alt_latch_never_reports_unobserved_press() {
	let latched = create_right_alt_latch(|| false, || 0);
	assert!(!latched());
}
