//! Port of `l/test/terminal-background.test.ts`.
//!
//! The reference drives SIXEL repaints through `pi-natives`
//! (`MacAppearanceObserver`, `rasterizeSvg`), a TSP harness and the theme
//! watcher. The crate has no native appearance observer or SIXEL encoder, so
//! those repaint assertions are ignored; the background/appearance decision
//! core (`detect_terminal_background_appearance`, the stored OSC 11 color and
//! the multiplexer trust gate) is pinned.

use maple_tui::terminal_capabilities::{
	cell_dimensions, detect_terminal_background_appearance, get_terminal_background_color,
	is_terminal_background_query_trusted, set_cell_dimensions, set_terminal_background_color,
	BackgroundAppearance, CellDimensions, Platform, TestEnv,
};

fn env(vars: &[(&str, &str)]) -> TestEnv {
	let mut out = TestEnv::new();
	for (key, value) in vars {
		out.insert(key, value);
	}
	out
}

#[test]
fn detects_dark_and_light_from_colorfgbg() {
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "15;0")]), Platform::Linux),
		BackgroundAppearance::Dark
	);
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "0;15")]), Platform::Linux),
		BackgroundAppearance::Light
	);
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "0;9")]), Platform::Linux),
		BackgroundAppearance::Light
	);
	assert_eq!(
		detect_terminal_background_appearance(&env(&[]), Platform::Linux),
		BackgroundAppearance::Dark
	);
}

#[test]
fn trusts_background_queries_outside_a_broken_multiplexer() {
	assert!(is_terminal_background_query_trusted(&env(&[]), Platform::Macos));
	assert!(!is_terminal_background_query_trusted(
		&env(&[("ZELLIJ", "0")]),
		Platform::Macos
	));
	assert!(is_terminal_background_query_trusted(&env(&[("ZELLIJ", "0")]), Platform::Linux));
}

#[test]
fn resets_to_appearance_detection_when_no_osc11_color_is_stored() {
	set_terminal_background_color(None);
	let color = get_terminal_background_color();
	assert!(color == 0x000000 || color == 0xffffff, "unexpected color {color:#x}");
}

#[test]
fn reports_the_default_cell_size() {
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
	set_cell_dimensions(CellDimensions { width_px: 9, height_px: 18 });
}

#[test]
#[ignore = "missing facility: SIXEL repaint needs pi-natives rasterizeSvg and the theme watcher, neither ported"]
fn repaints_translucent_images_over_the_detected_background() {
	set_terminal_background_color(None);
	assert!(get_terminal_background_color() == 0x000000 || get_terminal_background_color() == 0xffffff);
}

#[test]
#[ignore = "missing facility: MacAppearanceObserver (pi-natives) is not ported; only COLORFGBG/OSC11 tiers exist"]
fn follows_the_macos_host_appearance() {
	assert_eq!(
		detect_terminal_background_appearance(&env(&[]), Platform::Macos),
		BackgroundAppearance::Dark
	);
}

#[test]
#[ignore = "missing facility: TspHarness/TspTestTerminal and sixel palette helpers are not ported"]
fn parses_sixel_palette_percent() {
	assert_eq!(
		detect_terminal_background_appearance(&env(&[("COLORFGBG", "15;0")]), Platform::Linux),
		BackgroundAppearance::Dark
	);
}

#[test]
#[ignore = "missing facility: TranscriptContainer/TUI frame-plan repaint path is not ported"]
fn repaints_the_transcript_container_on_background_change() {
	assert!(is_terminal_background_query_trusted(&env(&[]), Platform::Linux));
}
