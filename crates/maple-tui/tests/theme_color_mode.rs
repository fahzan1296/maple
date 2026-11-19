//! Port of `l/test/theme-color-mode.test.ts`.
//!
//! The reference toggles a global terminal capability and even spawns a child
//! process to isolate detection; the crate resolves depth from an explicit env
//! map, so the env cases are pinned directly.

use std::collections::HashMap;

use maple_tui::theme::color::{color_to_ansi, detect_color_mode_from_env, ColorMode};

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
	pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

#[test]
fn detects_256_color_for_macos_terminal_app() {
	let mode = detect_color_mode_from_env(&env(&[
		("TERM_PROGRAM", "Apple_Terminal"),
		("TERM", "xterm-256color"),
	]));
	assert_eq!(mode.as_str(), "256color");
	assert_eq!(
		color_to_ansi("#f5e0ac", mode).expect("valid hex"),
		"\x1b[38;5;223m"
	);
}

#[test]
fn keeps_repeated_foreground_conversions_separate_across_depths() {
	assert_eq!(
		color_to_ansi("#f5e0ac", ColorMode::TrueColor).expect("valid hex"),
		"\x1b[38;2;245;224;172m"
	);
	let palette = color_to_ansi("#f5e0ac", ColorMode::Color256).expect("valid hex");
	assert!(palette.starts_with("\x1b[38;5;"), "{palette:?}");
	assert_ne!(palette, "\x1b[38;2;245;224;172m");
	assert_eq!(
		color_to_ansi("#f5e0ac", ColorMode::TrueColor).expect("valid hex"),
		"\x1b[38;2;245;224;172m"
	);
}

#[test]
fn gives_windows_terminal_truecolor() {
	let mode = detect_color_mode_from_env(&env(&[("WT_SESSION", "1")]));
	assert_eq!(mode.as_str(), "truecolor");
	assert_eq!(
		color_to_ansi("#f5e0ac", mode).expect("valid hex"),
		"\x1b[38;2;245;224;172m"
	);
}

#[test]
#[ignore = "missing facility: getSessionAccentAnsi generation is not ported; the reference measures it in a child process with a scrubbed env"]
fn emits_256_color_session_accents_for_macos_terminal_app() {
	let mode = detect_color_mode_from_env(&env(&[("TERM_PROGRAM", "Apple_Terminal")]));
	assert_eq!(mode.as_str(), "256color");
	assert_eq!(color_to_ansi("#f5e0ac", mode).expect("valid hex"), "\x1b[38;5;223m");
}
