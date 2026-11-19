//! Port of `l/test/herdr-sync-output.test.ts`.
//!
//! The DECRQM/DECRPM 2026 report outcome is a pure decision and is ported into
//! `terminal_capabilities.rs` as `synchronized_output_after_decrpm_report`.

use maple_tui::terminal_capabilities::{
	should_enable_synchronized_output_by_default, synchronized_output_after_decrpm_report,
	TestEnv, TerminalId,
};

#[test]
fn keeps_synchronized_output_on_in_herdr_after_an_unrecognized_report() {
	let mut herdr = TestEnv::new();
	herdr.insert("HERDR_ENV", "1");

	assert!(should_enable_synchronized_output_by_default(&herdr, TerminalId::Base));
	assert!(synchronized_output_after_decrpm_report(&herdr, TerminalId::Base, Some(0)));
}

#[test]
fn turns_synchronized_output_off_after_an_unsupported_report_outside_herdr() {
	let mut env = TestEnv::new();
	env.insert("TERM_FEATURES", "Sy");

	assert!(should_enable_synchronized_output_by_default(&env, TerminalId::Base));
	assert!(!synchronized_output_after_decrpm_report(&env, TerminalId::Base, Some(0)));
}

#[test]
fn turns_synchronized_output_off_in_herdr_when_the_report_omits_decrpm_status() {
	let mut herdr = TestEnv::new();
	herdr.insert("HERDR_ENV", "1");

	assert!(should_enable_synchronized_output_by_default(&herdr, TerminalId::Base));
	assert!(!synchronized_output_after_decrpm_report(&herdr, TerminalId::Base, None));
}

#[test]
fn turns_synchronized_output_off_in_herdr_when_2026_is_permanently_reset() {
	let mut herdr = TestEnv::new();
	herdr.insert("HERDR_ENV", "1");

	assert!(should_enable_synchronized_output_by_default(&herdr, TerminalId::Base));
	assert!(!synchronized_output_after_decrpm_report(&herdr, TerminalId::Base, Some(4)));
}
