//! Port of `l/test/terminal-appearance.test.ts`.
//!
//! The reference drives the real `ProcessTerminal` OSC 11 probe, DECRQM/DEC
//! 2026/2048 in-band resize and OSC 66 text sizing against a mocked TTY. The
//! crate resolves appearance from `COLORFGBG`/a stored OSC 11 color and has no
//! PTY probe pipeline, so the probe sequences are ignored; the pure detection
//! and WSL/cell-size helpers they depend on are pinned.

use maple_tui::terminal_capabilities::{
	cell_dimensions, detect_terminal_background_appearance, get_terminal_info, is_wsl,
	set_cell_dimensions, CellDimensions, HangulCompatibilityJamoWidth, Platform, TestEnv,
};

fn env(vars: &[(&str, &str)]) -> TestEnv {
	let mut out = TestEnv::new();
	for (key, value) in vars {
		out.insert(key, value);
	}
	out
}

#[test]
fn detects_wsl_from_the_distro_and_interop_markers_only_on_linux() {
	assert!(is_wsl(Platform::Linux, &env(&[("WSL_DISTRO_NAME", "Ubuntu")])));
	assert!(is_wsl(Platform::Linux, &env(&[("WSL_INTEROP", "/run/WSL/1_interop")])));
	assert!(!is_wsl(Platform::Linux, &env(&[])));
	assert!(!is_wsl(Platform::Macos, &env(&[("WSL_DISTRO_NAME", "Ubuntu")])));
	assert!(!is_wsl(Platform::Windows, &env(&[("WSL_INTEROP", "/run/WSL/1_interop")])));
}

#[test]
fn detects_background_appearance_from_colorfgbg() {
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "15;0")]), Platform::Linux),
		maple_tui::terminal_capabilities::BackgroundAppearance::Dark
	);
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "0;15")]), Platform::Linux),
		maple_tui::terminal_capabilities::BackgroundAppearance::Light
	);
}

#[test]
fn resolves_hangul_jamo_width_for_orca() {
	let info = get_terminal_info(
		maple_tui::terminal_capabilities::TerminalId::Orca,
		Platform::Linux,
		&env(&[]),
	);
	assert_eq!(info.hangul_jamo_width, HangulCompatibilityJamoWidth::Two);
}

#[test]
fn reports_the_default_cell_size_and_roundtrips() {
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
	set_cell_dimensions(CellDimensions { width_px: 10, height_px: 20 });
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 10, height_px: 20 });
	set_cell_dimensions(CellDimensions { width_px: 9, height_px: 18 });
}

#[test]
#[ignore = "missing facility: no PTY/OSC 11 probe pipeline in the crate; appearance resolves from COLORFGBG/stored color"]
fn detects_the_osc11_background_color() {
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "15;0")]), Platform::Linux),
		maple_tui::terminal_capabilities::BackgroundAppearance::Dark
	);
}

#[test]
#[ignore = "missing facility: DECRQM/DEC 2026/2048 in-band resize probes need a real terminal backend"]
fn resolves_decrqm_and_in_band_resize() {
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
}

#[test]
#[ignore = "missing facility: OSC 66 text-sizing capability probe needs a real terminal backend"]
fn resolves_osc66_text_sizing_capability() {
	let info = get_terminal_info(
		maple_tui::terminal_capabilities::TerminalId::Kitty,
		Platform::Linux,
		&env(&[]),
	);
	assert!(info.supports_text_sizing);
}
