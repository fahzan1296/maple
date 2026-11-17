//! Port of `l/test/image-render.test.ts`.
//!
//! The pure pieces are ported: the cell-box fit (`calculate_image_fit`), the
//! in-band transmit sequence (`encode_kitty_transmit`), image-line detection,
//! and Windows Terminal Preview SIXEL detection. The cached `Image` component,
//! SIXEL encoder injection and the live renderer stay ignored.

use maple_tui::components::image::{calculate_image_fit, encode_kitty_transmit, ImageDimensions};
use maple_tui::terminal_capabilities::{
	is_windows_terminal_preview_sixel_supported, CellDimensions, ImageProtocol, Platform,
	RuntimeTerminal, TerminalId, TestEnv,
};

const BASE64_ONE_PIXEL_PNG: &str =
	"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGNgAAAAAgABSK+kcQAAAABJRU5ErkJggg==";

fn square_info() -> RuntimeTerminal {
	RuntimeTerminal::new(TerminalId::Base, Some(ImageProtocol::Kitty), true, true)
}

#[test]
fn fits_kitty_images_within_max_width_and_height_preserving_aspect_ratio() {
	let fit = calculate_image_fit(
		ImageDimensions::new(100, 100),
		Some(10),
		Some(2),
		CellDimensions { width_px: 10, height_px: 10 },
	);
	assert_eq!((fit.columns, fit.rows), (2, 2));
}

#[test]
fn uses_intrinsic_image_size_when_no_bounds_are_provided() {
	let fit = calculate_image_fit(
		ImageDimensions::new(100, 100),
		None,
		None,
		CellDimensions { width_px: 10, height_px: 10 },
	);
	assert_eq!((fit.columns, fit.rows), (10, 10));
}

#[test]
fn reduces_width_when_max_height_is_the_limiting_bound() {
	let fit = calculate_image_fit(
		ImageDimensions::new(100, 100),
		Some(10),
		Some(2),
		CellDimensions { width_px: 10, height_px: 10 },
	);
	assert_eq!(fit.rows, 2);
	assert_eq!(fit.columns, 2);
}

#[test]
fn transmits_stable_kitty_images_in_band_before_placement() {
	let env = TestEnv::new();
	let _ = env;
	let transmit = encode_kitty_transmit(BASE64_ONE_PIXEL_PNG, 42);
	assert_eq!(transmit, format!("\x1b_Ga=t,f=100,q=2,i=42;{BASE64_ONE_PIXEL_PNG}\x1b\\"));
	assert!(!transmit.contains("t=t"));
}

#[test]
fn requires_windows_wt_session_and_version_122_for_preview_sixel() {
	let supported = TestEnv::new();
	let mut supported = supported;
	supported
		.insert("WT_SESSION", "1")
		.insert("TERM_PROGRAM", "Windows_Terminal")
		.insert("TERM_PROGRAM_VERSION", "1.22.2362.0");
	assert!(is_windows_terminal_preview_sixel_supported(&supported, Platform::Windows));

	let mut old = TestEnv::new();
	old.insert("WT_SESSION", "1")
		.insert("TERM_PROGRAM", "Windows_Terminal")
		.insert("TERM_PROGRAM_VERSION", "1.21.0.0");
	assert!(!is_windows_terminal_preview_sixel_supported(&old, Platform::Windows));

	let mut no_version = TestEnv::new();
	no_version.insert("WT_SESSION", "1").insert("TERM_PROGRAM", "Windows_Terminal");
	assert!(!is_windows_terminal_preview_sixel_supported(&no_version, Platform::Windows));

	let mut supported = TestEnv::new();
	supported
		.insert("WT_SESSION", "1")
		.insert("TERM_PROGRAM", "Windows_Terminal")
		.insert("TERM_PROGRAM_VERSION", "1.22.2362.0");
	assert!(!is_windows_terminal_preview_sixel_supported(&supported, Platform::Linux));
}

#[test]
fn keeps_deeply_prefixed_kitty_placeholder_rows_on_the_verbatim_image_line_path() {
	let info = square_info();
	let cells = "\u{10eeee}\u{030d}\u{0305}".repeat(9);
	let chip_row = format!(
		"\x1b[38;2;255;179;102m\u{2502}\x1b[39m \x1b[38;2;122;231;55m\x1b[58:2::122:231:55m{cells}\x1b[39;59m  \x1b[38;2;255;179;102m\u{2502}\x1b[39m"
	);
	assert!(info.is_image_line(&chip_row));
	let second_card = format!("{chip_row}  {chip_row}");
	assert!(info.is_image_line(&second_card[chip_row.len() + 2..]));
	assert!(info.is_image_line(&second_card));
}

#[test]
fn still_rejects_plain_styled_text_rows() {
	let info = square_info();
	assert!(!info.is_image_line("\x1b[38;2;255;179;102m\u{2502}\x1b[39m plain text row"));
}

#[test]
#[ignore = "missing harness: Image component render cache keyed on protocol availability"]
fn re_renders_a_cached_fallback_once_a_protocol_becomes_available() {
}

#[test]
#[ignore = "missing harness: Image component render cache keyed on cell dimensions"]
fn re_renders_a_cached_image_when_cell_dimensions_change() {
}

#[test]
#[ignore = "missing harness: Image component render cache keyed on Unicode placeholder support"]
fn re_renders_a_cached_kitty_image_when_placeholder_support_changes() {
}

#[test]
#[ignore = "missing harness: ImageBudget release/replay bookkeeping"]
fn replays_released_image_rows_without_retransmitting() {
}

#[test]
#[ignore = "missing harness: ImageBudget pass/demotion and row ceilings"]
fn keeps_a_demoted_images_drawn_height_within_a_lowered_row_ceiling() {
}

#[test]
#[ignore = "missing harness: SIXEL encoder injection and pending-row reservation"]
fn encodes_sixel_at_the_cell_box_size_and_reserves_rows() {
}

#[test]
#[ignore = "missing harness: SIXEL encoder registration"]
fn encodes_sixel_synchronously_without_a_provider() {
}

#[test]
#[ignore = "missing harness: async SIXEL recomposition with terminal background"]
fn recomposites_a_cached_sixel_with_the_latest_background() {
}

#[test]
#[ignore = "missing harness: SIXEL scrollback commit against a new background"]
fn commits_sixel_scrollback_pixels_against_the_new_background() {
}

#[test]
#[ignore = "missing harness: Image component direct-placement cursor movement"]
fn moves_back_up_before_multi_row_direct_kitty_output() {
}

#[test]
#[ignore = "missing harness: Image component direct-placement cursor movement"]
fn does_not_emit_cursor_movement_around_single_row_direct_kitty_output() {
}
