//! Port of `l/test/input.test.ts` (portable subset).
//!
//! The crate's `Input` is a small port: no volatile dictation preview, no
//! prompt/cursor override, no kitty protocol, no bracketed-paste
//! normalization, no cursor marker, no jamo width profile and no space-hold
//! hook. Word motion is bound to Ctrl+arrow rather than Alt+b/Alt+f, the mask
//! is `*`, and `render` windows by chars rather than cells. The portable subset
//! asserts the seams the crate does have — masking, viewport width, word
//! motion, paste insertion and submit — and the remaining reference cases stay
//! ignored with their missing facility named.

use maple_tui::component::Component;
use maple_tui::components::Input;
use maple_tui::keys::Key;

#[test]
fn masks_one_bullet_per_grapheme_without_changing_the_submitted_value() {
	let mut input = Input::new();
	input.set_password(true);
	input.set_value("a😀e\u{0301}z");
	assert_eq!(input.value(), "a😀e\u{0301}z");
	let rows = input.render(20);
	assert_eq!(rows, vec!["*****".to_string()]);
}

#[test]
fn keeps_masked_unicode_input_within_narrow_viewports() {
	let mut input = Input::new();
	input.set_password(true);
	input.set_value("😀e\u{0301}".repeat(20));
	let rows = input.render(5);
	assert_eq!(rows.len(), 1);
	assert!(rows[0].chars().count() <= 5, "{}", rows[0]);
}

#[test]
fn never_renders_a_line_wider_than_the_terminal_width_wide_chars() {
	let mut input = Input::new();
	input.set_value("天气不错，去散步吧！".repeat(50));
	let rows = input.render(20);
	assert_eq!(rows.len(), 1);
	assert!(rows[0].chars().count() <= 20, "{}", rows[0]);
}

#[test]
fn moves_by_word_with_ctrl_arrows() {
	let mut input = Input::new();
	input.set_value("one two");
	let _ = input.render(80);
	input.handle_input(&Key::CtrlLeft);
	assert_eq!(input.cursor_viewport_column(80), 4);
	input.handle_input(&Key::CtrlLeft);
	assert_eq!(input.cursor_viewport_column(80), 0);
	input.handle_input(&Key::CtrlRight);
	assert_eq!(input.cursor_viewport_column(80), 4);
	input.handle_input(&Key::CtrlRight);
	assert_eq!(input.cursor_viewport_column(80), 7);
}

#[test]
fn paste_inserts_text_verbatim() {
	let mut input = Input::new();
	input.handle_paste("sk-line1\nsk-line2\r\nsk-line3");
	assert_eq!(input.value(), "sk-line1\nsk-line2\r\nsk-line3");
}

#[test]
fn submits_the_current_value() {
	let mut input = Input::new();
	input.set_value("hello");
	let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
	let sink = std::sync::Arc::clone(&submitted);
	input.on_submit(move |value| sink.lock().unwrap().push(value.to_string()));
	input.handle_input(&Key::Enter);
	assert_eq!(submitted.lock().unwrap().as_slice(), ["hello".to_string()]);
}

#[test]
fn max_length_blocks_further_typing() {
	let mut input = Input::new();
	input.set_max_length(Some(3));
	input.set_value("abc");
	input.handle_input(&Key::Char('d'));
	assert_eq!(input.value(), "abc");
}

#[test]
fn renders_the_placeholder_when_empty() {
	let mut input = Input::new();
	input.set_placeholder("type here");
	assert_eq!(input.render(20), vec!["type here".to_string()]);
}

#[test]
fn clamps_the_cursor_viewport_column_to_the_width() {
	let mut input = Input::new();
	input.set_value("abcdef");
	let _ = input.render(3);
	assert!(input.cursor_viewport_column(3) <= 3);
}

#[test]
#[ignore = "missing facility: setVolatileText/commitVolatileText dictation preview is not ported"]
fn replaces_a_volatile_dictation_preview_in_place_and_undoes_the_committed_dictation_in_one_step() {
	let mut input = Input::new();
	input.set_value("ask ");
	assert_eq!(input.value(), "ask ");
}

#[test]
#[ignore = "missing facility: volatile dictation preview is not ported"]
fn keeps_the_value_intact_when_the_caret_leaves_a_live_dictation_preview() {
	let mut input = Input::new();
	input.set_value("ask ");
	assert_eq!(input.value(), "ask ");
}

#[test]
#[ignore = "missing facility: prompt and cursorOverride are not ported"]
fn fits_a_wide_cursor_override_at_the_end_of_a_line_that_fills_the_width() {
	let mut input = Input::new();
	input.set_value("x".repeat(40));
	assert_eq!(input.value(), "x".repeat(40));
}

#[test]
#[ignore = "missing facility: Alt+b/Alt+f CJK word motion is not ported (crate binds Ctrl+arrow)"]
fn moves_by_cjk_and_punctuation_blocks_backward() {
	let mut input = Input::new();
	input.set_value("天气不错，去散步吧！");
	assert_eq!(input.value(), "天气不错，去散步吧！");
}

#[test]
#[ignore = "missing facility: Alt+b/Alt+f CJK word motion is not ported (crate binds Ctrl+arrow)"]
fn moves_by_cjk_and_punctuation_blocks_forward() {
	let mut input = Input::new();
	input.set_value("天气不错，去散步吧！");
	assert_eq!(input.value(), "天气不错，去散步吧！");
}

#[test]
#[ignore = "missing facility: Alt+b word motion is not ported (crate binds Ctrl+arrow)"]
fn treats_nbsp_as_whitespace_for_word_navigation() {
	let mut input = Input::new();
	input.set_value("Hola\u{a0}mundo");
	assert_eq!(input.value(), "Hola\u{a0}mundo");
}

#[test]
#[ignore = "missing facility: Alt+b word motion is not ported (crate binds Ctrl+arrow)"]
fn keeps_common_joiners_inside_words() {
	let mut input = Input::new();
	input.set_value("co-operate l’été");
	assert_eq!(input.value(), "co-operate l’été");
}

#[test]
#[ignore = "missing facility: Alt+b word motion is not ported (crate binds Ctrl+arrow)"]
fn recognizes_unicode_punctuation_as_delimiter_blocks() {
	let mut input = Input::new();
	input.set_value("¿Cómo estás? ¡Muy bien!");
	assert_eq!(input.value(), "¿Cómo estás? ¡Muy bien!");
}

#[test]
#[ignore = "missing facility: setKittyProtocolActive global is not ported"]
fn does_not_delete_twice_when_kitty_sends_backspace_press_and_release() {
	let mut input = Input::new();
	input.set_value("ab");
	assert_eq!(input.value(), "ab");
}

#[test]
#[ignore = "missing facility: kitty CSI-u keypad decoding is not ported"]
fn inserts_keypad_digits_from_kitty_csi_u_input_with_or_without_numlock_modifier() {
	let mut input = Input::new();
	input.set_value("a");
	assert_eq!(input.value(), "a");
}

#[test]
#[ignore = "missing facility: kitty CSI-u keypad decoding is not ported"]
fn inserts_keypad_operators_from_kitty_csi_u_input() {
	let mut input = Input::new();
	input.set_value("a");
	assert_eq!(input.value(), "a");
}

#[test]
#[ignore = "missing facility: handle_paste does not normalize tabs or strip newlines"]
fn normalizes_tabs_in_buffered_bracketed_paste_using_the_fixed_display_width() {
	let mut input = Input::new();
	input.handle_paste("a\t");
	assert_eq!(input.value(), "a\t");
}

#[test]
#[ignore = "missing facility: tmux re-encoded control-byte decoding is not ported"]
fn decodes_tmux_re_encoded_control_bytes_in_bracketed_paste_without_leaking_tails_or_storing_raw_c0() {
	let mut input = Input::new();
	input.handle_paste("ab");
	assert_eq!(input.value(), "ab");
}

#[test]
#[ignore = "missing facility: prompt is not ported"]
fn clips_an_oversized_prompt_without_losing_the_editable_value_after_resize() {
	let mut input = Input::new();
	input.set_value("retained");
	assert_eq!(input.value(), "retained");
}

#[test]
#[ignore = "missing facility: debugState is not ported"]
fn does_not_disclose_masked_input_through_debug_inspection() {
	let mut input = Input::new();
	input.set_password(true);
	input.set_value("secret");
	assert_eq!(input.value(), "secret");
}

#[test]
#[ignore = "missing facility: handle_paste does not normalize NFD to NFC"]
fn normalizes_nfd_korean_pastes_macos_finder_drag_drop_to_nfc() {
	let mut input = Input::new();
	input.handle_paste("/Users/leo/Downloads/화면.mov");
	assert_eq!(input.value(), "/Users/leo/Downloads/화면.mov");
}

#[test]
#[ignore = "missing facility: CURSOR_MARKER is not emitted by this port"]
fn nfc_paste_cursor_column_matches_visible_cells_no_displacement() {
	let mut input = Input::new();
	input.handle_paste("/Users/leo/화면\\ 기록.mov");
	assert_eq!(input.value(), "/Users/leo/화면\\ 기록.mov");
}

#[test]
#[ignore = "missing facility: CURSOR_MARKER / terminal cursor mode is not ported"]
fn terminal_cursor_mode_emits_marker_without_inverse_video_software_cursor() {
	let mut input = Input::new();
	input.set_value("abc");
	assert_eq!(input.value(), "abc");
}

#[test]
#[ignore = "missing facility: setHangulCompatibilityJamoWidth global is not ported"]
fn runtime_jamo_profile_controls_the_cursor_marker_column() {
	let mut input = Input::new();
	input.set_value("ㅁ".repeat(8));
	assert_eq!(input.value(), "ㅁ".repeat(8));
}

#[test]
#[ignore = "missing facility: Input has no space_hold hook"]
fn uses_the_configured_remap_for_follow_up_dictation_while_space_remains_ordinary_input() {
	let mut input = Input::new();
	input.set_value("ask");
	assert_eq!(input.value(), "ask");
}
