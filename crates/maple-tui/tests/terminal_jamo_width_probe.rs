//! Port of `l/test/terminal-jamo-width-probe.test.ts`.
//!
//! `getTerminalInfo(detectTerminalId(env)).hangulJamoWidth` becomes
//! `get_terminal_info(detect_terminal_id(&env), Platform::current(), &env)`.

use maple_tui::terminal_capabilities::{
	detect_terminal_id, get_terminal_info, HangulCompatibilityJamoWidth, Platform, TestEnv,
};

fn jamo_width_for(env: &TestEnv) -> HangulCompatibilityJamoWidth {
	let id = detect_terminal_id(env);
	get_terminal_info(id, Platform::current(), env).hangul_jamo_width
}

#[test]
fn matches_terminal_specific_hangul_compatibility_jamo_widths() {
	let mut env = TestEnv::new();
	env.insert("GHOSTTY_RESOURCES_DIR", "/Applications/Ghostty.app");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::Two);

	let mut env = TestEnv::new();
	env.insert("TERM_PROGRAM", "ghostty");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::Two);

	// Ghostty identified only via TERM (env-filtered shells that drop
	// GHOSTTY_RESOURCES_DIR / TERM_PROGRAM) must still resolve wide.
	let mut env = TestEnv::new();
	env.insert("TERM", "xterm-ghostty");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::Two);

	let mut env = TestEnv::new();
	env.insert("TERM_PROGRAM", "WarpTerminal");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::One);

	let mut env = TestEnv::new();
	env.insert("TERM_PROGRAM", "Orca");
	env.insert("TERM", "xterm-256color");
	env.insert("COLORTERM", "truecolor");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::Two);

	let mut env = TestEnv::new();
	env.insert("TERM_PROGRAM", "iTerm.app");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::Platform);

	let mut env = TestEnv::new();
	env.insert("TERM_PROGRAM", "Apple_Terminal");
	assert_eq!(jamo_width_for(env), HangulCompatibilityJamoWidth::Platform);

	assert_eq!(jamo_width_for(TestEnv::new()), HangulCompatibilityJamoWidth::Platform);
}
