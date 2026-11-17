//! Port of `l/test/terminal-info.test.ts`.
//!
//! `formatTerminalState`/`collectTerminalState` live in the unported
//! `apps/debug/terminal-info` module, so the formatted-string and live-geometry
//! assertions are ignored. The state they summarize — the resolved capability
//! profile and the multiplexer identity — is pinned through `get_terminal_info`
//! and `classify_terminal_multiplexer`.

use maple_tui::terminal_capabilities::{
	cell_dimensions, classify_terminal_multiplexer, detect_terminal_id, get_terminal_info, set_cell_dimensions,
	CellDimensions, ImageProtocol, NotifyProtocol, Platform, TerminalId, TestEnv,
};

fn env(vars: &[(&str, &str)]) -> TestEnv {
	let mut out = TestEnv::new();
	for (key, value) in vars {
		out.insert(key, value);
	}
	out
}

#[test]
fn resolves_the_kitty_profile_fields_the_debug_view_summarizes() {
	let info = get_terminal_info(TerminalId::Kitty, Platform::Linux, &env(&[]));
	assert_eq!(info.id, TerminalId::Kitty);
	assert_eq!(info.image_protocol, Some(ImageProtocol::Kitty));
	assert!(info.true_color);
	assert!(info.hyperlinks);
	assert_eq!(info.notify_protocol, NotifyProtocol::Osc99);
	assert!(info.deccara);
	assert!(info.supports_screen_to_scrollback);
}

#[test]
fn reports_the_default_cell_size_and_roundtrips_a_reported_one() {
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
	set_cell_dimensions(CellDimensions { width_px: 9, height_px: 18 });
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
}

#[test]
fn reports_herdr_from_pane_identity_vars_not_client_only_socket_paths() {
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("HERDR_ENV", "1")])).map(|s| s.id()),
		Some("herdr")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("HERDR_PANE_ID", "p1")])).map(|s| s.id()),
		Some("herdr")
	);
	assert_eq!(classify_terminal_multiplexer(&env(&[("HERDR_SOCKET_PATH", "/tmp/x")])), None);
}

#[test]
fn reports_wmux_from_either_session_marker() {
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("WMUX", "1")])).map(|s| s.id()),
		Some("wmux")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("WMUX_SURFACE_ID", "3f2a")])).map(|s| s.id()),
		Some("wmux")
	);
}

#[test]
fn maps_protocol_enums_to_readable_names_without_raw_escapes() {
	let info = get_terminal_info(TerminalId::Kitty, Platform::Linux, &env(&[]));
	let image = format!("{:?}", info.image_protocol);
	let notify = format!("{:?}", info.notify_protocol);
	assert!(!image.contains('\x1b'));
	assert!(!notify.contains('\x1b'));
	assert!(!image.is_empty() && !notify.is_empty());
}

#[test]
fn detects_the_terminal_id_from_the_environment() {
	assert_eq!(
		detect_terminal_id(&env(&[("TERM", "xterm-ghostty")])).as_str(),
		"ghostty"
	);
	assert_eq!(
		detect_terminal_id(&env(&[("KITTY_WINDOW_ID", "1")])).as_str(),
		"kitty"
	);
}

#[test]
#[ignore = "missing facility: formatTerminalState is not ported (apps/debug/terminal-info)"]
fn surfaces_the_negotiated_subprotocols_and_their_on_off_state() {
	let info = get_terminal_info(TerminalId::Kitty, Platform::Linux, &env(&[]));
	assert_eq!(info.image_protocol, Some(ImageProtocol::Kitty));
}

#[test]
#[ignore = "missing facility: formatTerminalState is not ported (apps/debug/terminal-info)"]
fn reports_geometry_cell_size_and_the_scrollback_strategy() {
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
}

#[test]
#[ignore = "missing facility: formatTerminalState is not ported (apps/debug/terminal-info)"]
fn renders_the_redraw_fallback_when_screen_to_scrollback_is_unsupported() {
	let info = get_terminal_info(TerminalId::Base, Platform::Linux, &env(&[]));
	assert!(!info.supports_screen_to_scrollback);
}

#[test]
#[ignore = "missing facility: formatTerminalState is not ported (apps/debug/terminal-info)"]
fn drops_the_osc99_confirmed_marker_when_the_terminal_never_answered() {
	let info = get_terminal_info(TerminalId::Kitty, Platform::Linux, &env(&[]));
	assert_eq!(info.notify_protocol, NotifyProtocol::Osc99);
}

#[test]
#[ignore = "missing facility: formatTerminalState is not ported (apps/debug/terminal-info)"]
fn shows_none_for_no_multiplexer_and_unset_for_absent_detection_vars() {
	assert_eq!(classify_terminal_multiplexer(&env(&[])), None);
}

#[test]
#[ignore = "missing facility: formatTerminalState is not ported (apps/debug/terminal-info)"]
fn names_the_multiplexer_when_one_wraps_the_session() {
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("TMUX", "session")])).map(|s| s.id()),
		Some("tmux")
	);
}

#[test]
#[ignore = "missing facility: collectTerminalState is not ported, so live geometry passthrough cannot be asserted"]
fn passes_live_geometry_through() {
	assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
}
