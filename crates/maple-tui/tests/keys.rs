//! Port of `l/test/keys.test.ts`.
//!
//! The reference `matchesKey`/`parseKey` shell out to `@oh-my-pi/pi-natives`
//! (a Bun native parser). The crate has no raw-input key parser, so the tests
//! that depend on native normalization are `#[ignore]`d with that named missing
//! API. The pure wrapper behaviour — Kitty CSI-u text decoding, keypad text,
//! `modifyOtherKeys` text, and the raw-Backspace heuristic — is ported for real.

use maple_tui::keys::{
	decode_kitty_keypad_text, decode_modify_other_keys_printable, extract_printable_text,
	is_windows_terminal_session, matches_raw_backspace,
};
use maple_tui::terminal_capabilities::TestEnv;

fn env(vars: &[(&str, &str)]) -> TestEnv {
	let mut env = TestEnv::new();
	for (key, value) in vars {
		env.insert(key, value);
	}
	env
}

#[test]
fn extracts_keypad_digits_from_kitty_csi_u_sequences() {
	assert_eq!(extract_printable_text("\x1b[57407u").as_deref(), Some("8"));
	assert_eq!(extract_printable_text("\x1b[57407;129u").as_deref(), Some("8"));
	assert_eq!(extract_printable_text("\x1b[57404u").as_deref(), Some("5"));
}

#[test]
fn extracts_keypad_operators_from_kitty_csi_u_sequences() {
	assert_eq!(extract_printable_text("\x1b[57410u").as_deref(), Some("/"));
	assert_eq!(extract_printable_text("\x1b[57413u").as_deref(), Some("+"));
}

#[test]
fn does_not_treat_modified_numlock_keypad_navigation_keys_as_text() {
	assert_eq!(extract_printable_text("\x1b[57400;133u"), None);
}

#[test]
fn does_not_extract_del_from_modify_other_keys_sequences_as_text() {
	assert_eq!(extract_printable_text("\x1b[27;1;127~"), None);
}

#[test]
fn ignores_unsupported_modifiers_on_kitty_csi_u_text() {
	assert_eq!(extract_printable_text("\x1b[99;9u"), None);
	assert_eq!(extract_printable_text("\x1b[97;9;229u"), None);
}

#[test]
fn preserves_kitty_csi_u_text_field_decoding_for_supported_modifiers() {
	assert_eq!(extract_printable_text("\x1b[97;1;229u").as_deref(), Some("å"));
}

#[test]
fn decodes_modify_other_keys_text_only_for_plain_or_shift() {
	assert_eq!(
		decode_modify_other_keys_printable("\x1b[27;1;32~").as_deref(),
		Some(" ")
	);
	assert_eq!(decode_modify_other_keys_printable("\x1b[27;5;97~"), None);
}

#[test]
fn keypad_fast_path_matches_digits_and_rejects_named_keys() {
	assert_eq!(decode_kitty_keypad_text("\x1b[57400u").as_deref(), Some("1"));
	assert_eq!(decode_kitty_keypad_text("\x1b[57400;129u").as_deref(), Some("1"));
	assert_eq!(decode_kitty_keypad_text("\x1b[57404u").as_deref(), Some("5"));
	assert_ne!(decode_kitty_keypad_text("\x1b[57400u").as_deref(), Some("end"));
	assert_ne!(decode_kitty_keypad_text("\x1b[57404u").as_deref(), Some("clear"));
}

#[test]
fn maps_raw_0x08_to_ctrl_backspace_only_in_a_genuine_windows_terminal_session() {
	let env = env(&[("WT_SESSION", "1")]);
	assert!(is_windows_terminal_session(&env));
	assert!(matches_raw_backspace("\x08", 4, &env));
	assert!(!matches_raw_backspace("\x08", 0, &env));
	assert!(matches_raw_backspace("\x7f", 0, &env));
	assert!(!matches_raw_backspace("\x7f", 4, &env));

	let env = self::env(&[]);
	assert!(!is_windows_terminal_session(&env));
	assert!(matches_raw_backspace("\x08", 0, &env));
	assert!(!matches_raw_backspace("\x08", 4, &env));
}

#[test]
fn does_not_apply_the_heuristic_when_wt_session_is_forwarded_over_ssh() {
	let env = env(&[
		("WT_SESSION", "1"),
		("SSH_CONNECTION", "1.2.3.4 5 6.7.8.9 22"),
	]);
	assert!(!is_windows_terminal_session(&env));
	assert!(matches_raw_backspace("\x08", 0, &env));
}

#[test]
fn does_not_apply_the_heuristic_inside_a_multiplexer_that_inherited_wt_session() {
	let multiplexers: [&[(&str, &str)]; 5] = [
		&[("WT_SESSION", "1"), ("TMUX", "/tmp/tmux-1000/default,1,0")],
		&[("WT_SESSION", "1"), ("STY", "1234.pts-0")],
		&[("WT_SESSION", "1"), ("ZELLIJ", "0")],
		&[("WT_SESSION", "1"), ("TERM", "tmux-256color")],
		&[("WT_SESSION", "1"), ("TERM", "screen-256color")],
	];
	for vars in multiplexers {
		let env = env(vars);
		assert!(matches_raw_backspace("\x08", 0, &env), "{vars:?}");
		assert!(!matches_raw_backspace("\x08", 4, &env), "{vars:?}");
	}
	let env = env(&[
		("WT_SESSION", "1"),
		("TMUX", "/tmp/tmux-1000/default,1,0"),
		("PI_TUI_RAW_BACKSPACE_IS_CTRL", "1"),
	]);
	assert!(matches_raw_backspace("\x08", 4, &env));
	assert!(!matches_raw_backspace("\x08", 0, &env));
}

#[test]
fn supports_an_explicit_opt_in_when_remote_sessions_lose_terminal_identity() {
	let env = env(&[
		("PI_TUI_RAW_BACKSPACE_IS_CTRL", "1"),
		("SSH_CONNECTION", "1.2.3.4 5 6.7.8.9 22"),
	]);
	assert!(!is_windows_terminal_session(&env));
	assert!(matches_raw_backspace("\x08", 4, &env));
	assert!(!matches_raw_backspace("\x08", 0, &env));
}

#[test]
fn leaves_0x7f_as_plain_backspace_regardless_of_windows_terminal_or_opt_in() {
	let env = env(&[("WT_SESSION", "1"), ("PI_TUI_RAW_BACKSPACE_IS_CTRL", "1")]);
	assert!(matches_raw_backspace("\x7f", 0, &env));
	assert!(!matches_raw_backspace("\x7f", 4, &env));
}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn matches_ctrl_letter_sequences() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn matches_shifted_tab() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn matches_page_up_legacy_sequence_with_mixed_case_key_id() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn matches_legacy_alt_letter_pairs_in_enhanced_keyboard_mixed_mode() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn prefers_codepoint_for_latin_letters_even_when_base_layout_differs() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn prefers_codepoint_for_symbol_keys_even_when_base_layout_differs() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn matches_non_latin_shortcuts_by_their_base_layout_key() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn ignores_kitty_release_events_while_still_matching_repeats() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn defers_csi_u_modify_other_keys_named_keys_to_native_normalization() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn matches_keypad_operators_as_their_printable_symbols() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn preserves_keypad_navigation_matches_when_numlock_is_on_but_modifiers_are_held() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives matchesKey (Bun native parser)"]
fn recognises_super_alt_backspace_from_ghosttys_default_option_backspace_wire() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parses_legacy_alt_letter_pairs_in_enhanced_keyboard_mixed_mode() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parse_prefers_codepoint_for_latin_letters_when_base_layout_differs() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parse_ignores_kitty_release_events_while_still_parsing_repeats() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parse_prefers_codepoint_for_symbol_keys_when_base_layout_differs() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parses_keypad_digits_as_digits_with_or_without_numlock_modifier() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn normalizes_csi_u_modify_other_keys_named_keys_instead_of_returning_raw_printables() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parses_keypad_operators_as_printable_keys() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn parses_modified_numlock_keypad_navigation_keys_consistently() {}

#[test]
#[ignore = "missing API: @oh-my-pi/pi-natives parseKey (Bun native parser)"]
fn ignores_kitty_sequences_with_unsupported_modifiers() {}
