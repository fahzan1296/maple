//! Port of `l/test/windows-input-mode.test.ts` (decoder subset).
//!
//! The reference drives the decoder through a Windows console PTY harness for
//! the mode-enable and relay-timing cases; the crate exposes the pure
//! [`Win32InputModeDecoder`] and [`Win32PasteMarkerNormalizer`] state machines,
//! so those are asserted here. Deviations: decoded keys are the legacy/kitty
//! byte strings the crate emits (the reference matches them against a key
//! matcher), and the paste normalizer returns text instead of pushing it to a
//! callback.

use maple_tui::windows_input::{
	parse_record, Win32InputModeDecoder, Win32PasteMarkerNormalizer,
};

const SHIFT_ENTER: &str = "\x1b[13;28;13;1;16;1_";
const CTRL_ENTER: &str = "\x1b[13;28;10;1;8;1_";
const CTRL_J: &str = "\x1b[74;36;10;1;8;1_";
const ENTER: &str = "\x1b[13;28;13;1;0;1_";
const ENTER_UP: &str = "\x1b[13;28;13;0;0;1_";

fn decode_one(data: &str) -> String {
	let mut decoder = Win32InputModeDecoder::new();
	let keys = decoder
		.decode(data)
		.unwrap_or_else(|| panic!("not a record: {data:?}"));
	assert_eq!(keys.len(), 1, "{data:?} decoded {keys:?}");
	keys[0].clone()
}

fn decode_with(data: &str) -> Option<Vec<String>> {
	Win32InputModeDecoder::new().decode(data)
}

/// One relay record per byte, the way a VT host forwards text.
fn relay_records(text: &str) -> Vec<String> {
	text.chars()
		.map(|ch| format!("\x1b[0;0;{};1;0;1_", ch as u32))
		.collect()
}

#[test]
fn keeps_shift_enter_and_ctrl_enter_distinct_from_enter() {
	assert_eq!(decode_one(ENTER), "\r");
	assert_eq!(decode_one(SHIFT_ENTER), "\x1b[13;2u");
	assert_eq!(decode_one(CTRL_ENTER), "\x1b[13;5u");
}

#[test]
fn keeps_ctrl_j_with_lf_as_ctrl_j() {
	assert_eq!(decode_one(CTRL_J), "\x1b[106;5u");
}

#[test]
fn drops_key_releases_and_bare_modifier_presses() {
	let mut decoder = Win32InputModeDecoder::new();
	assert_eq!(decoder.decode(ENTER_UP), Some(Vec::new()));
	assert_eq!(decoder.decode("\x1b[16;42;0;1;16;1_"), Some(Vec::new()));
	assert_eq!(decoder.decode("\x1b[17;29;0;1;8;1_"), Some(Vec::new()));
}

#[test]
fn passes_non_record_sequences_through_untouched() {
	let mut decoder = Win32InputModeDecoder::new();
	assert_eq!(decoder.decode("\x1b[A"), None);
	assert_eq!(decoder.decode("\x1b[200~"), None);
	assert_eq!(decoder.decode("a"), None);
}

#[test]
fn decodes_text_control_and_alt_chords_to_legacy_bytes() {
	assert_eq!(decode_one("\x1b[65;30;97;1;0;1_"), "a");
	assert_eq!(decode_one("\x1b[65;30;65;1;16;1_"), "A");
	assert_eq!(decode_one("\x1b[67;46;3;1;8;1_"), "\x03");
	assert_eq!(decode_one("\x1b[66;48;98;1;2;1_"), "\x1bb");
	assert_eq!(decode_one("\x1b[9;15;9;1;16;1_"), "\x1b[Z");
	assert_eq!(decode_one("\x1b[8;14;8;1;2;1_"), "\x1b\x7f");
	assert_eq!(decode_one("\x1b[27;1;27;1;0;1_"), "\x1b");
}

#[test]
fn keeps_ctrl_shift_letter_distinct_from_ctrl_letter() {
	assert_eq!(decode_one("\x1b[80;25;16;1;24;1_"), "\x1b[112;6u");
}

#[test]
fn encodes_navigation_keys_with_their_modifiers() {
	assert_eq!(decode_one("\x1b[38;72;0;1;256;1_"), "\x1b[A");
	assert_eq!(decode_one("\x1b[38;72;0;1;264;1_"), "\x1b[1;5A");
	assert_eq!(decode_one("\x1b[46;83;0;1;256;1_"), "\x1b[3~");
	assert_eq!(decode_one("\x1b[116;63;0;1;0;1_"), "\x1b[15~");
}

#[test]
fn emits_altgr_composed_text_instead_of_a_ctrl_alt_chord() {
	// German layout: AltGr+Q → "@" reported as Right Alt + Left Ctrl.
	assert_eq!(decode_one("\x1b[81;16;64;1;9;1_"), "@");
}

#[test]
fn emits_text_a_dead_key_composed_onto_space_enter_and_tab() {
	assert_eq!(decode_one("\x1b[32;57;39;1;0;1_"), "'");
	assert_eq!(decode_one("\x1b[32;57;34;1;0;1_"), "\"");
	let mut decoder = Win32InputModeDecoder::new();
	assert_eq!(decoder.decode("\x1b[13;28;39;1;0;1_"), Some(vec!["'".to_string()]));
	assert_eq!(decoder.decode(ENTER), Some(vec!["\r".to_string()]));
	assert_eq!(decode_one("\x1b[9;15;39;1;0;1_"), "'");
	assert_eq!(decode_one("\x1b[32;57;32;1;0;1_"), " ");
	assert_eq!(decode_one("\x1b[32;57;32;1;16;1_"), " ");
}

#[test]
fn joins_surrogate_halves_delivered_as_separate_records() {
	let mut decoder = Win32InputModeDecoder::new();
	assert_eq!(decoder.decode("\x1b[231;0;55357;1;0;1_"), Some(Vec::new()));
	assert_eq!(
		decoder.decode("\x1b[231;0;56832;1;0;1_"),
		Some(vec!["😀".to_string()])
	);
}

#[test]
fn delivers_alt_numpad_composition_on_the_alt_release() {
	assert_eq!(
		decode_with("\x1b[18;56;233;0;32;1_"),
		Some(vec!["é".to_string()])
	);
}

#[test]
fn repeats_auto_repeated_keys() {
	assert_eq!(
		decode_with("\x1b[65;30;97;1;0;3_"),
		Some(vec!["a".to_string(), "a".to_string(), "a".to_string()])
	);
}

#[test]
fn decodes_key_records_inside_a_paste_as_typed_text() {
	let mut decoder = Win32InputModeDecoder::new();
	let pasted = format!(
		"a{ENTER}{ENTER_UP}{ENTER}{ENTER_UP}b\x1b[231;0;55357;1;0;1_\x1b[231;0;56832;1;0;1_"
	);
	assert_eq!(decoder.decode_paste(&pasted), "a\r\rb😀");
	assert_eq!(
		decoder.decode_paste("see [13;28;13;1;0;1_ here"),
		"see [13;28;13;1;0;1_ here"
	);
}

#[test]
fn reassembles_escape_sequences_a_vt_host_relays_byte_by_byte() {
	let mut decoder = Win32InputModeDecoder::new();
	let mut keys = Vec::new();
	for record in relay_records("\x1b[1;3A") {
		keys.extend(decoder.decode(&record).unwrap_or_default());
	}
	assert_eq!(keys, vec!["\x1b[1;3A".to_string()]);
	assert!(!decoder.has_pending_sequence());

	let mut keys = Vec::new();
	for record in relay_records("\x1b\x1b[A") {
		keys.extend(decoder.decode(&record).unwrap_or_default());
	}
	assert_eq!(keys, vec!["\x1b\x1b[A".to_string()]);
	assert!(!decoder.has_pending_sequence());

	// A lone relayed ESC is held, then released as Escape; plain text is not held.
	let mut decoder = Win32InputModeDecoder::new();
	let esc = relay_records("\x1b");
	assert_eq!(decoder.decode(&esc[0]), Some(Vec::new()));
	assert_eq!(decoder.flush_pending_sequence(), vec!["\x1b".to_string()]);
	let x = relay_records("x");
	assert_eq!(decoder.decode(&x[0]), Some(vec!["x".to_string()]));

	// A real key record ends a held ESC instead of being swallowed into it.
	let mut decoder = Win32InputModeDecoder::new();
	let esc = relay_records("\x1b");
	decoder.decode(&esc[0]);
	assert_eq!(
		decoder.decode(ENTER),
		Some(vec!["\x1b".to_string(), "\r".to_string()])
	);

	// Escape then Alt+d stays two keys; only `ESC ESC [` / `ESC ESC O` group.
	let mut decoder = Win32InputModeDecoder::new();
	let mut split = Vec::new();
	for record in relay_records("\x1b\x1bd") {
		split.extend(decoder.decode(&record).unwrap_or_default());
	}
	assert_eq!(split, vec!["\x1b".to_string(), "\x1bd".to_string()]);
	assert!(!decoder.has_pending_sequence());
}

#[test]
fn reports_a_held_osc_dcs_or_apc_string_for_its_terminator() {
	let mut decoder = Win32InputModeDecoder::new();
	for record in relay_records("\x1b]877;tsp;e;") {
		assert_eq!(decoder.decode(&record), Some(Vec::new()));
	}
	assert!(decoder.has_pending_sequence());
	assert!(decoder.has_pending_string());
	let mut keys = Vec::new();
	for record in relay_records("{\"a\":1}\x07") {
		keys.extend(decoder.decode(&record).unwrap_or_default());
	}
	assert_eq!(keys, vec!["\x1b]877;tsp;e;{\"a\":1}\x07".to_string()]);
	assert!(!decoder.has_pending_string());

	// A bare introducer may still be an Alt chord (Alt+], Alt+P, Alt+_).
	for record in relay_records("\x1b]") {
		decoder.decode(&record);
	}
	assert!(decoder.has_pending_sequence());
	assert!(!decoder.has_pending_string());
	decoder.flush_pending_sequence();

	// A lone ESC or a CSI prefix is not a string: it keeps the short Escape timeout.
	let mut decoder = Win32InputModeDecoder::new();
	let esc = relay_records("\x1b");
	decoder.decode(&esc[0]);
	assert!(!decoder.has_pending_string());
	decoder.flush_pending_sequence();
	for record in relay_records("\x1b[1;") {
		decoder.decode(&record);
	}
	assert!(!decoder.has_pending_string());
}

#[test]
fn parses_records_into_their_fields() {
	let record = parse_record("\x1b[13;28;13;1;16;1_").expect("record");
	assert_eq!(record.vk, 13);
	assert_eq!(record.uc, 13);
	assert!(record.down);
	assert_eq!(record.state, 16);
	assert_eq!(record.repeat, 1);
}

#[test]
fn leaves_non_marker_key_records_unchanged_when_a_prefix_is_interrupted() {
	fn paste_records(text: &str) -> String {
		text.chars()
			.map(|ch| format!("\x1b[231;0;{};1;0;1_", ch as u32))
			.collect()
	}
	let prefix = paste_records("\x1b[20");
	let mut normalizer = Win32PasteMarkerNormalizer::new();
	let mut out = normalizer.process(&prefix[..8]);
	out.push_str(&normalizer.process(&format!("{}{SHIFT_ENTER}", &prefix[8..])));
	out.push_str(&normalizer.flush().unwrap_or_default());
	assert_eq!(out, format!("{prefix}{SHIFT_ENTER}"));
}
