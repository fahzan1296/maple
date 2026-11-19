//! Port of `l/test/terminal-capabilities.test.ts`.
//!
//! The crate renamed the `PI_*` overrides to `MAPLE_*` and replaced the
//! subprocess/fake-tmux probes with pure env seams
//! (`MAPLE_TMUX_CLIENT_TERMINAL_NAMES`, `MAPLE_TMUX_ALLOW_PASSTHROUGH`), so the
//! child-process and tmux-query cases are ignored while every pure decision is
//! pinned.

use maple_tui::terminal_capabilities::{
	classify_terminal_multiplexer, detect_styled_underline_support, detect_terminal_id,
	has_terminal_multiplexer_session, hyperlinks_user_override, is_inside_terminal_multiplexer,
	is_paseo_embedder, resolve_image_protocol, resolve_warp_image_protocol,
	should_enable_hyperlinks_by_default, should_enable_synchronized_output_by_default,
	synchronized_output_user_override, ImageProtocol, MultiplexerSession, NotifyProtocol, Platform,
	TerminalId, TestEnv,
};

fn env(vars: &[(&str, &str)]) -> TestEnv {
	let mut out = TestEnv::new();
	for (key, value) in vars {
		out.insert(key, value);
	}
	out
}

#[test]
fn recognizes_herdr_session_identity_while_ignoring_client_only_settings() {
	let session = MultiplexerSession::Herdr;
	assert!(has_terminal_multiplexer_session(&env(&[("HERDR_ENV", "1")]), session));
	for key in ["HERDR_PANE_ID", "HERDR_TAB_ID", "HERDR_WORKSPACE_ID"] {
		assert!(has_terminal_multiplexer_session(&env(&[(key, "abc")]), session));
	}
	for vars in [
		[("HERDR_ENV", "0")],
		[("HERDR_PANE_ID", "")],
		[("HERDR_TAB_ID", "")],
		[("HERDR_WORKSPACE_ID", "")],
		[("HERDR_SOCKET_PATH", "/tmp/x")],
		[("HERDR_BIN_PATH", "/usr/bin/herdr")],
		[("HERDR_SESSION", "s1")],
		[("HERDR_CONFIG_PATH", "/tmp/herdr.toml")],
		[("HERDR_CLIENT_SOCKET_PATH", "/tmp/client.sock")],
	] {
		assert!(!has_terminal_multiplexer_session(&env(&vars), session), "{vars:?}");
	}
}

#[test]
fn matches_explicit_provider_markers_and_excludes_term_fallbacks() {
	assert!(has_terminal_multiplexer_session(&env(&[("TMUX", "/tmp/tmux,1,0")]), MultiplexerSession::Tmux));
	assert!(has_terminal_multiplexer_session(&env(&[("STY", "1234.pts-0.host")]), MultiplexerSession::Screen));
	assert!(has_terminal_multiplexer_session(&env(&[("ZELLIJ", "0")]), MultiplexerSession::Zellij));
	assert!(has_terminal_multiplexer_session(
		&env(&[("CMUX_WORKSPACE_ID", "workspace")]),
		MultiplexerSession::Cmux
	));
	assert!(has_terminal_multiplexer_session(&env(&[("CMUX_SURFACE_ID", "surface")]), MultiplexerSession::Cmux));
	assert!(has_terminal_multiplexer_session(
		&env(&[("CMUX_REMOTE_TRANSPORT", "ssh")]),
		MultiplexerSession::Cmux
	));
	assert!(!has_terminal_multiplexer_session(
		&env(&[("CMUX_SOCKET_PATH", "/tmp/cmux.sock")]),
		MultiplexerSession::Cmux
	));
	assert!(!has_terminal_multiplexer_session(&env(&[("TERM", "tmux-256color")]), MultiplexerSession::Tmux));
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("TERM", "tmux-256color")])).map(|s| s.id()),
		Some("tmux")
	);
}

#[test]
fn recognizes_wmux_session_markers_but_not_cli_overrides() {
	let session = MultiplexerSession::Wmux;
	assert!(has_terminal_multiplexer_session(&env(&[("WMUX", "1")]), session));
	assert!(has_terminal_multiplexer_session(&env(&[("WMUX_SURFACE_ID", "3f2a")]), session));
	assert!(!has_terminal_multiplexer_session(&env(&[("WMUX", "0")]), session));
	assert!(!has_terminal_multiplexer_session(&env(&[("WMUX_SURFACE_ID", "")]), session));
	assert!(!has_terminal_multiplexer_session(&env(&[("WMUX_CLI", "C:/wmux/wmux.exe")]), session));
	assert!(!has_terminal_multiplexer_session(&env(&[("WMUX_PIPE", "\\\\.\\pipe\\wmux")]), session));
}

#[test]
fn requires_both_nonempty_orca_worktree_identity_markers() {
	let session = MultiplexerSession::Orca;
	let both = env(&[
		("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"),
		("ORCA_WORKTREE_ID", "worktree-1"),
	]);
	assert!(has_terminal_multiplexer_session(&both, session));
	for vars in [
		[("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc")],
		[("ORCA_WORKTREE_ID", "worktree-1")],
		[("ORCA_PANE_KEY", ""), ("ORCA_WORKTREE_ID", "worktree-1")],
		[("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"), ("ORCA_WORKTREE_ID", "")],
		[("ORCA_PANE_KEY", " "), ("ORCA_WORKTREE_ID", "worktree-1")],
		[("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"), ("ORCA_WORKTREE_ID", " ")],
		[("TERM_PROGRAM", "orca")],
	] {
		assert!(!has_terminal_multiplexer_session(&env(&vars), session), "{vars:?}");
	}
}

#[test]
fn preserves_nested_provider_precedence_and_term_only_classification() {
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("HERDR_PANE_ID", "p1"), ("TMUX", "session")])).map(|s| s.id()),
		Some("herdr")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("TMUX", "session"), ("STY", "screen")])).map(|s| s.id()),
		Some("tmux")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("STY", "screen"), ("ZELLIJ", "session")])).map(|s| s.id()),
		Some("screen")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[("TERM", "screen-256color")])).map(|s| s.id()),
		Some("screen")
	);
}

#[test]
fn classifies_orca_after_nested_multiplexers_without_moving_it_off_the_direct_render_path() {
	let orca = env(&[
		("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"),
		("ORCA_WORKTREE_ID", "worktree-1"),
	]);
	assert_eq!(classify_terminal_multiplexer(&orca).map(|s| s.id()), Some("orca"));
	assert!(!is_inside_terminal_multiplexer(&orca));

	let mut with_tmux = env(&[("TMUX", "session")]);
	for (key, value) in orca_vars() {
		with_tmux.insert(key, value);
	}
	assert_eq!(classify_terminal_multiplexer(&with_tmux).map(|s| s.id()), Some("tmux"));
	assert!(is_inside_terminal_multiplexer(&with_tmux));
	assert_eq!(
		classify_terminal_multiplexer(&env(&[
			("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"),
			("ORCA_WORKTREE_ID", "worktree-1"),
			("ZELLIJ", "session"),
		]))
		.map(|s| s.id()),
		Some("zellij")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[
			("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"),
			("ORCA_WORKTREE_ID", "worktree-1"),
			("TERM", "tmux-256color"),
		]))
		.map(|s| s.id()),
		Some("tmux")
	);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[
			("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"),
			("ORCA_WORKTREE_ID", "worktree-1"),
			("TERM", "screen-256color"),
		]))
		.map(|s| s.id()),
		Some("screen")
	);
	assert_eq!(classify_terminal_multiplexer(&env(&[("TERM_PROGRAM", "orca")])), None);
	assert_eq!(
		classify_terminal_multiplexer(&env(&[(
			"ORCA_PANE_KEY",
			"tab-1:12345678-1234-1234-1234-123456789abc"
		)])),
		None
	);
	assert_eq!(classify_terminal_multiplexer(&env(&[("ORCA_WORKTREE_ID", "worktree-1")])), None);
}

fn orca_vars() -> [(&'static str, &'static str); 2] {
	[
		("ORCA_PANE_KEY", "tab-1:12345678-1234-1234-1234-123456789abc"),
		("ORCA_WORKTREE_ID", "worktree-1"),
	]
}

#[test]
fn recognizes_session_markers_through_the_generic_render_path_gate() {
	assert!(is_inside_terminal_multiplexer(&env(&[("HERDR_PANE_ID", "p1")])));
	assert!(is_inside_terminal_multiplexer(&env(&[("WMUX", "1")])));
	assert!(is_inside_terminal_multiplexer(&env(&[("WMUX_SURFACE_ID", "3f2a")])));
	assert!(!is_inside_terminal_multiplexer(&env(&[("WMUX", "0")])));
	assert!(!is_inside_terminal_multiplexer(&env(&[("WMUX_SURFACE_ID", "")])));
	assert!(!is_inside_terminal_multiplexer(&env(&[("WMUX_CLI", "C:/wmux/wmux.exe")])));
	assert!(!is_inside_terminal_multiplexer(&env(&[("WMUX_PIPE", "\\\\.\\pipe\\wmux")])));
}

#[test]
fn recognizes_rio_and_tern_via_term_program() {
	assert_eq!(
		detect_terminal_id(&env(&[("TERM_PROGRAM", "rio"), ("COLORTERM", "truecolor")])).as_str(),
		"rio"
	);
	assert_eq!(
		detect_terminal_id(&env(&[("TERM_PROGRAM", "tern"), ("TERM", "xterm-256color"), ("COLORTERM", "truecolor")]))
			.as_str(),
		"tern"
	);
}

#[test]
fn maps_rio_to_kitty_graphics_with_verified_hyperlink_capability() {
	let info = get_terminal_info(TerminalId::Rio, Platform::Linux, &env(&[]));
	assert_eq!(info.id, TerminalId::Rio);
	assert_eq!(info.image_protocol, Some(ImageProtocol::Kitty));
	assert!(info.true_color);
	assert!(info.hyperlinks);
	assert_eq!(info.notify_protocol, NotifyProtocol::Bell);
}

#[test]
fn recognizes_warp_before_the_true_color_fallback() {
	assert_eq!(
		detect_terminal_id(&env(&[("TERM_PROGRAM", "WarpTerminal"), ("COLORTERM", "truecolor")])).as_str(),
		"warp"
	);
}

#[test]
fn recognizes_monstar_by_term_and_routes_notifications_through_osc_9() {
	let env = env(&[("TERM", "monstar"), ("COLORTERM", "truecolor")]);
	let id = detect_terminal_id(&env);
	assert_eq!(id.as_str(), "monstar");
	assert_eq!(get_terminal_info(id, Platform::Linux, &env).notify_protocol, NotifyProtocol::Osc9);
}

#[test]
fn does_not_treat_an_inherited_rex_session_as_rex() {
	let env = env(&[
		("REX_SESSION", "s1"),
		("TERM", "xterm-256color"),
		("COLORTERM", "truecolor"),
		("VTE_VERSION", "8400"),
	]);
	assert_eq!(detect_terminal_id(&env).as_str(), "trueColor");
}

#[test]
fn falls_back_to_true_color_on_vte_environments() {
	let env = env(&[
		("TERM", "xterm-256color"),
		("TERM_PROGRAM", ""),
		("COLORTERM", "truecolor"),
		("VTE_VERSION", "8400"),
	]);
	assert_eq!(detect_terminal_id(&env).as_str(), "trueColor");
}

#[test]
fn selects_kitty_graphics_for_rex_via_term_program_or_term() {
	for vars in [
		[("TERM_PROGRAM", "rex"), ("TERM", "xterm-256color"), ("COLORTERM", "truecolor")],
		[("TERM", "xterm-rex"), ("COLORTERM", "truecolor"), ("SSH_CONNECTION", "client 1 server 22")],
	] {
		let env = env(&vars);
		let id = detect_terminal_id(&env);
		assert_eq!(id.as_str(), "rex");
		assert_eq!(resolve_image_protocol(id, &env, true), Some(ImageProtocol::Kitty));
		assert!(should_enable_hyperlinks_by_default(&env, id));
	}
}

#[test]
fn detects_iterm2_from_forwarded_lc_terminal_and_keeps_inline_images() {
	let env = env(&[
		("TERM", "xterm-256color"),
		("COLORTERM", "truecolor"),
		("LC_TERMINAL", "iTerm2"),
		("LC_TERMINAL_VERSION", "3.6.11"),
		("SSH_CONNECTION", "client 1 server 22"),
	]);
	let id = detect_terminal_id(&env);
	assert_eq!(id.as_str(), "iterm2");
	assert_eq!(resolve_image_protocol(id, &env, true), Some(ImageProtocol::Iterm2));
	assert!(should_enable_hyperlinks_by_default(&env, id));
	assert_eq!(get_terminal_info(id, Platform::Linux, &env).notify_protocol, NotifyProtocol::Osc9);
}

#[test]
fn keeps_the_flat_underline_until_lc_terminal_version_proves_iterm2_3_5_or_newer() {
	assert!(detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("LC_TERMINAL_VERSION", "3.6.11")])
	));
	assert!(!detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("LC_TERMINAL_VERSION", "3.4.0")])
	));
	assert!(!detect_styled_underline_support(TerminalId::ITerm2, &env(&[])));
}

#[test]
fn does_not_claim_iterm2_inside_a_multiplexer() {
	let env = env(&[
		("TERM", "xterm-256color"),
		("COLORTERM", "truecolor"),
		("LC_TERMINAL", "iTerm2"),
		("LC_TERMINAL_VERSION", "3.6.11"),
		("SSH_CONNECTION", "client 1 server 22"),
		("TMUX", "/tmp/tmux-1000/default,4242,0"),
	]);
	let id = detect_terminal_id(&env);
	assert_ne!(id, TerminalId::ITerm2);
	assert_ne!(resolve_image_protocol(id, &env, true), Some(ImageProtocol::Iterm2));
}

#[test]
fn resolves_the_attached_tmux_clients_terminal_profile() {
	let env = env(&[
		("TERM", "tmux-256color"),
		("TERM_PROGRAM", "tmux"),
		("TERM_PROGRAM_VERSION", "3.6b"),
		("COLORTERM", "truecolor"),
		("TMUX", "/tmp/tmux-1000/default,4242,0"),
		("MAPLE_TMUX_CLIENT_TERMINAL_NAMES", "wezterm"),
	]);
	assert_eq!(detect_terminal_id(&env).as_str(), "wezterm");
	assert_eq!(get_terminal_info(detect_terminal_id(&env), Platform::Linux, &env).notify_protocol, NotifyProtocol::Osc9);
}

#[test]
fn recognizes_a_rex_client_from_the_tmux_client_termname() {
	let env = env(&[
		("TERM", "tmux-256color"),
		("TERM_PROGRAM", "tmux"),
		("TMUX", "/tmp/tmux-1000/default,4242,0"),
		("MAPLE_TMUX_CLIENT_TERMINAL_NAMES", "rex"),
	]);
	assert_eq!(detect_terminal_id(&env).as_str(), "rex");
	assert_eq!(resolve_image_protocol(TerminalId::Rex, &env, true), Some(ImageProtocol::Kitty));
}

#[test]
#[ignore = "missing facility: TerminalId::from_program maps TERM_PROGRAM values only, not the TERM-style client_termname xterm-rex"]
fn recognizes_a_rex_client_from_a_term_style_client_termname() {
	let env = env(&[("MAPLE_TMUX_CLIENT_TERMINAL_NAMES", "xterm-rex")]);
	assert_eq!(detect_terminal_id(&env).as_str(), "base");
}

#[test]
fn synchronized_output_override_prefers_opt_out_over_force_on() {
	assert_eq!(synchronized_output_user_override(&env(&[])), None);
	assert_eq!(synchronized_output_user_override(&env(&[("TERM", "xterm-256color")])), None);
	assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_NO_SYNC_OUTPUT", "1")])), Some(false));
	assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_TUI_SYNC_OUTPUT", "0")])), Some(false));
	assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_FORCE_SYNC_OUTPUT", "1")])), Some(true));
	assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_TUI_SYNC_OUTPUT", "1")])), Some(true));
	assert_eq!(
		synchronized_output_user_override(&env(&[("MAPLE_NO_SYNC_OUTPUT", "1"), ("MAPLE_FORCE_SYNC_OUTPUT", "1")])),
		Some(false)
	);
	assert_eq!(
		synchronized_output_user_override(&env(&[("MAPLE_TUI_SYNC_OUTPUT", "0"), ("MAPLE_FORCE_SYNC_OUTPUT", "1")])),
		Some(false)
	);
}

#[test]
fn enables_synchronized_output_for_every_known_direct_terminal() {
	for id in [
		TerminalId::Kitty,
		TerminalId::Ghostty,
		TerminalId::Monstar,
		TerminalId::Wezterm,
		TerminalId::ITerm2,
		TerminalId::Alacritty,
		TerminalId::VSCode,
	] {
		assert!(should_enable_synchronized_output_by_default(&env(&[]), id), "{}", id.as_str());
	}
}

#[test]
fn enables_sync_only_when_wt_session_does_not_contradict_the_terminal_identity() {
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("WT_SESSION", "abc")]),
		TerminalId::TrueColor
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("WT_SESSION", "abc"), ("TERM_PROGRAM", "Windows_Terminal"), ("COLORTERM", "truecolor")]),
		TerminalId::TrueColor
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("WT_SESSION", "0"), ("TERM_PROGRAM", "Tabby"), ("COLORTERM", "truecolor")]),
		TerminalId::TrueColor
	));
}

#[test]
fn enables_sync_when_term_features_advertises_the_sy_capability() {
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("TERM_FEATURES", "ClSyTc")]),
		TerminalId::Base
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("TERM_FEATURES", "ClSyTc"), ("SSH_CONNECTION", "1 2 3 4")]),
		TerminalId::Base
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("TERM_FEATURES", "ClSyTc"), ("TMUX", "1")]),
		TerminalId::Base
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("TERM_FEATURES", "ClTc")]),
		TerminalId::Base
	));
}

#[test]
fn no_longer_blanket_disables_ssh_for_recognized_terminals() {
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("SSH_CONNECTION", "1 2 3 4")]),
		TerminalId::ITerm2
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("SSH_TTY", "/dev/pts/3")]),
		TerminalId::Kitty
	));
}

#[test]
fn keeps_risky_multiplexers_off_by_default_even_when_an_inner_terminal_id_leaks() {
	assert!(!should_enable_synchronized_output_by_default(&env(&[("TMUX", "1")]), TerminalId::Kitty));
	assert!(!should_enable_synchronized_output_by_default(&env(&[("ZELLIJ", "0")]), TerminalId::Ghostty));
	assert!(!should_enable_synchronized_output_by_default(&env(&[("STY", "x")]), TerminalId::Wezterm));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("CMUX_WORKSPACE_ID", "workspace")]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("TERM", "tmux-256color")]),
		TerminalId::ITerm2
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("TERM", "screen-256color")]),
		TerminalId::Kitty
	));
}

#[test]
fn enables_sync_inside_herdr_even_when_a_kitty_or_ghostty_identity_leaks() {
	for vars in [
		[("HERDR_ENV", "1")],
		[("HERDR_PANE_ID", "p1")],
		[("HERDR_PANE_ID", "p1"), ("TMUX", "1")],
		[("HERDR_PANE_ID", "p1"), ("STY", "x")],
		[("HERDR_PANE_ID", "p1"), ("ZELLIJ", "0")],
	] {
		for id in [TerminalId::Kitty, TerminalId::Ghostty, TerminalId::TrueColor, TerminalId::Base] {
			assert!(
				should_enable_synchronized_output_by_default(&env(&vars), id),
				"{vars:?} {}",
				id.as_str()
			);
		}
	}
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("HERDR_SOCKET_PATH", "/tmp/x"), ("TMUX", "1")]),
		TerminalId::Kitty
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("HERDR_ENV", "1"), ("MAPLE_NO_SYNC_OUTPUT", "1")]),
		TerminalId::Ghostty
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("HERDR_ENV", "1"), ("MAPLE_TUI_SYNC_OUTPUT", "0")]),
		TerminalId::Kitty
	));
}

#[test]
fn keeps_known_unsupported_and_unknown_profiles_off() {
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("VTE_VERSION", "6800")]),
		TerminalId::Base
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("TERM", "xterm-256color")]),
		TerminalId::Base
	));
	assert!(!should_enable_synchronized_output_by_default(&env(&[]), TerminalId::Base));
	assert!(!should_enable_synchronized_output_by_default(&env(&[]), TerminalId::TrueColor));
}

#[test]
fn lets_a_user_opt_out_or_force_on_beat_every_heuristic() {
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[("MAPLE_NO_SYNC_OUTPUT", "1")]),
		TerminalId::Kitty
	));
	assert!(!should_enable_synchronized_output_by_default(
		&env(&[
			("MAPLE_NO_SYNC_OUTPUT", "1"),
			("WT_SESSION", "abc"),
			("TERM_FEATURES", "Sy")
		]),
		TerminalId::Kitty
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("MAPLE_FORCE_SYNC_OUTPUT", "1")]),
		TerminalId::Base
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("MAPLE_TUI_SYNC_OUTPUT", "1"), ("TMUX", "1")]),
		TerminalId::Base
	));
	assert!(should_enable_synchronized_output_by_default(
		&env(&[("MAPLE_FORCE_SYNC_OUTPUT", "1"), ("SSH_CONNECTION", "1 2 3 4")]),
		TerminalId::Base
	));
}

#[test]
fn warp_is_kitty_capable_with_true_color_but_no_osc_8_hyperlinks() {
	let warp = get_terminal_info(TerminalId::Warp, Platform::Linux, &env(&[]));
	assert_eq!(warp.image_protocol, Some(ImageProtocol::Kitty));
	assert!(warp.true_color);
	assert!(!warp.hyperlinks);
	assert_eq!(warp.notify_protocol, NotifyProtocol::Osc9);
	assert!(!warp.supports_text_sizing);
}

#[test]
fn warp_uses_kitty_images_on_macos_linux_and_disables_them_on_windows() {
	assert_eq!(resolve_warp_image_protocol(Platform::Macos, &env(&[])), Some(ImageProtocol::Kitty));
	assert_eq!(resolve_warp_image_protocol(Platform::Linux, &env(&[])), Some(ImageProtocol::Kitty));
	assert_eq!(resolve_warp_image_protocol(Platform::Windows, &env(&[])), None);
	assert_eq!(
		get_terminal_info(TerminalId::Warp, Platform::Macos, &env(&[])).image_protocol,
		Some(ImageProtocol::Kitty)
	);
	assert_eq!(
		get_terminal_info(TerminalId::Warp, Platform::Linux, &env(&[])).image_protocol,
		Some(ImageProtocol::Kitty)
	);
	assert_eq!(get_terminal_info(TerminalId::Warp, Platform::Windows, &env(&[])).image_protocol, None);
	let linux = get_terminal_info(TerminalId::Warp, Platform::Linux, &env(&[]));
	assert!(linux.true_color);
	assert!(!linux.hyperlinks);
	assert!(!linux.deccara);
	assert!(!linux.supports_text_sizing);
}

#[test]
fn treats_wsl_as_the_windows_host_so_kitty_apc_garbage_never_reaches_warp() {
	let distro = get_terminal_info(TerminalId::Warp, Platform::Linux, &env(&[("WSL_DISTRO_NAME", "Ubuntu")]));
	let interop = get_terminal_info(
		TerminalId::Warp,
		Platform::Linux,
		&env(&[("WSL_INTEROP", "/run/WSL/1_interop")]),
	);
	assert_eq!(resolve_warp_image_protocol(Platform::Linux, &env(&[("WSL_DISTRO_NAME", "Ubuntu")])), None);
	assert_eq!(
		resolve_warp_image_protocol(Platform::Linux, &env(&[("WSL_INTEROP", "/run/WSL/1_interop")])),
		None
	);
	assert_eq!(distro.image_protocol, None);
	assert_eq!(interop.image_protocol, None);
}

#[test]
fn warp_leaves_sync_off_by_default_and_honors_hyperlink_force_on() {
	assert!(!should_enable_synchronized_output_by_default(&env(&[]), TerminalId::Warp));
	assert!(!should_enable_hyperlinks_by_default(&env(&[]), TerminalId::Warp));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("MAPLE_FORCE_HYPERLINKS", "1")]),
		TerminalId::Warp
	));
}

#[test]
fn recognizes_otty_before_the_true_color_fallback_and_keeps_kitty_capabilities() {
	let env = env(&[
		("TERM_PROGRAM", "otty"),
		("TERM_PROGRAM_VERSION", "1.5.1"),
		("TERM", "xterm-256color"),
		("COLORTERM", "truecolor"),
	]);
	assert_eq!(detect_terminal_id(&env).as_str(), "otty");
	let otty = get_terminal_info(TerminalId::Otty, Platform::Linux, &env(&[]));
	assert_eq!(otty.image_protocol, Some(ImageProtocol::Kitty));
	assert!(otty.true_color);
	assert!(otty.hyperlinks);
	assert_eq!(otty.notify_protocol, NotifyProtocol::Osc99);
	assert!(!otty.deccara);
	assert!(!otty.supports_text_sizing);
	assert_eq!(resolve_image_protocol(TerminalId::Otty, &env, true), Some(ImageProtocol::Kitty));
}

#[test]
fn tmux_probes_unknown_clients_instead_of_sending_kitty_graphics() {
	let env = env(&[
		("TMUX", "/tmp/tmux/default,1,0"),
		("TERM_PROGRAM", "tmux"),
		("TERM", "tmux-256color"),
		("COLORTERM", "truecolor"),
	]);
	let id = detect_terminal_id(&env);
	assert_eq!(resolve_image_protocol(id, &env, true), None);
	assert_eq!(
		resolve_image_protocol(
			TerminalId::Base,
			&env(&[("TMUX", "/tmp/tmux/default,1,0"), ("TERM", "screen-256color")]),
			true
		),
		None
	);
	assert_eq!(
		resolve_image_protocol(
			TerminalId::Base,
			&env(&[("TMUX", "/tmp/tmux/default,1,0"), ("TERM", "tmux-256color")]),
			true
		),
		None
	);
}

#[test]
fn keeps_a_known_kitty_client_only_when_tmux_forwards_passthrough() {
	let on = env(&[
		("TMUX", "/tmp/tmux-1000/default,5101,0"),
		("TERM", "tmux-256color"),
		("MAPLE_TMUX_ALLOW_PASSTHROUGH", "on"),
	]);
	assert_eq!(resolve_image_protocol(TerminalId::Ghostty, &on, true), Some(ImageProtocol::Kitty));
	let off = env(&[
		("TMUX", "/tmp/tmux-1000/default,5102,0"),
		("TERM", "tmux-256color"),
		("MAPLE_TMUX_ALLOW_PASSTHROUGH", "off"),
	]);
	assert_eq!(resolve_image_protocol(TerminalId::Ghostty, &off, true), None);
}

#[test]
fn keeps_the_gnu_screen_kitty_fallback() {
	assert_eq!(
		resolve_image_protocol(TerminalId::Base, &env(&[("TERM", "screen-256color")]), true),
		Some(ImageProtocol::Kitty)
	);
}

#[test]
fn rejects_a_leaked_ghostty_protocol_when_herdr_owns_the_pane_grid() {
	for vars in [
		[
			("COLORTERM", "truecolor"),
			("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty"),
			("HERDR_ENV", "1"),
			("TERM", "xterm-256color"),
			("TERM_PROGRAM", "ghostty"),
		],
		[
			("COLORTERM", "truecolor"),
			("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty"),
			("HERDR_PANE_ID", "p1"),
			("TERM", "xterm-256color"),
			("TERM_PROGRAM", "ghostty"),
		],
	] {
		let env = env(&vars);
		assert_eq!(detect_terminal_id(&env), TerminalId::Ghostty);
		assert_eq!(resolve_image_protocol(TerminalId::Ghostty, &env, true), None);
	}
}

#[test]
fn detects_paseo_and_drops_the_gnu_screen_kitty_fallback_inside_it() {
	assert!(!is_paseo_embedder(&env(&[])));
	assert!(is_paseo_embedder(&env(&[("PASEO_TERMINAL_ID", "term-1")])));
	assert_eq!(
		resolve_image_protocol(
			TerminalId::Base,
			&env(&[("TERM", "screen-256color"), ("PASEO_TERMINAL_ID", "term-1")]),
			true
		),
		None
	);
	assert_eq!(
		resolve_image_protocol(TerminalId::Base, &env(&[("TERM", "screen-256color")]), true),
		Some(ImageProtocol::Kitty)
	);
	assert_eq!(
		resolve_image_protocol(
			TerminalId::Kitty,
			&env(&[("TERM_PROGRAM", "kitty"), ("PASEO_TERMINAL_ID", "term-1")]),
			true
		),
		None
	);
	assert_eq!(
		resolve_image_protocol(TerminalId::Kitty, &env(&[("TERM_PROGRAM", "kitty")]), true),
		Some(ImageProtocol::Kitty)
	);
}

#[test]
fn hyperlinks_override_ignores_values_other_than_the_literal_one() {
	assert_eq!(hyperlinks_user_override(&env(&[])), None);
	assert_eq!(hyperlinks_user_override(&env(&[("TERM", "xterm-256color")])), None);
	assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_FORCE_HYPERLINKS", "1")])), Some(true));
	assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_NO_HYPERLINKS", "1")])), Some(false));
	assert_eq!(
		hyperlinks_user_override(&env(&[("MAPLE_NO_HYPERLINKS", "1"), ("MAPLE_FORCE_HYPERLINKS", "1")])),
		Some(false)
	);
	assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_FORCE_HYPERLINKS", "true")])), None);
	assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_FORCE_HYPERLINKS", "0")])), None);
	assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_NO_HYPERLINKS", "0")])), None);
}

#[test]
fn enables_hyperlinks_on_every_known_direct_terminal() {
	for id in [
		TerminalId::Kitty,
		TerminalId::Ghostty,
		TerminalId::Wezterm,
		TerminalId::ITerm2,
		TerminalId::Alacritty,
		TerminalId::VSCode,
	] {
		assert!(should_enable_hyperlinks_by_default(&env(&[]), id), "{}", id.as_str());
	}
	assert!(!should_enable_hyperlinks_by_default(&env(&[]), TerminalId::Base));
	assert!(!should_enable_hyperlinks_by_default(&env(&[]), TerminalId::TrueColor));
}

#[test]
fn keeps_gnu_screen_always_off_even_when_the_inner_terminal_supports_osc_8() {
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("STY", "1234.pts-0.host")]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("TERM", "screen-256color")]),
		TerminalId::Kitty
	));
}

#[test]
fn treats_tmux_as_authoritative_even_when_term_is_screen_family() {
	assert!(should_enable_hyperlinks_by_default(
		&env(&[
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM", "screen-256color"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.4"),
		]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM", "screen"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.3a"),
		]),
		TerminalId::Wezterm
	));
}

#[test]
fn lets_gnu_screen_sty_veto_tmux_enabling_in_nested_sessions() {
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("STY", "1234.pts-0.host"),
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.4"),
		]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("STY", "1234.pts-0.host"),
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM", "screen-256color"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.5a"),
		]),
		TerminalId::Kitty
	));
}

#[test]
fn keeps_tmux_off_without_a_version_or_below_3_4() {
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("TMUX", "/tmp/tmux-1000/default,1,0")]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("TERM", "tmux-256color")]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.3a"),
		]),
		TerminalId::Wezterm
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "2.9"),
		]),
		TerminalId::Kitty
	));
}

#[test]
fn enables_tmux_at_3_4_and_newer() {
	for version in ["3.4", "3.5a", "4.0"] {
		assert!(
			should_enable_hyperlinks_by_default(
				&env(&[
					("TMUX", "/tmp/tmux-1000/default,1,0"),
					("TERM_PROGRAM", "tmux"),
					("TERM_PROGRAM_VERSION", version),
				]),
				TerminalId::Wezterm
			),
			"{version}"
		);
	}
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("TMUX", "/tmp/tmux-1000/default,1,0"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.5a"),
		]),
		TerminalId::Base
	));
}

#[test]
fn enables_herdr_panes_because_herdr_renders_osc_8_itself() {
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("HERDR_ENV", "1"), ("TERM", "xterm-256color")]),
		TerminalId::Base
	));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("HERDR_PANE_ID", "w1:p1"), ("TERM", "xterm-256color")]),
		TerminalId::Base
	));
}

#[test]
fn keeps_screen_and_tmux_nested_in_a_herdr_pane_on_their_own_rules() {
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("HERDR_ENV", "1"), ("STY", "1234.pts-0.host")]),
		TerminalId::Base
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("HERDR_ENV", "1"), ("TMUX", "/tmp/tmux-1000/default,1,0")]),
		TerminalId::Base
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("HERDR_ENV", "1"), ("MAPLE_NO_HYPERLINKS", "1")]),
		TerminalId::Base
	));
}

#[test]
fn keeps_herdr_rendering_when_other_nested_layers_have_no_osc_8_policy() {
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("HERDR_ENV", "1"), ("ZELLIJ", "0")]),
		TerminalId::Base
	));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("HERDR_ENV", "1"), ("TERM", "screen-256color")]),
		TerminalId::Base
	));
}

#[test]
fn gates_a_herdr_pane_nested_with_tmux_on_tmux_version_and_screen_vetoes_it() {
	let tmux34 = [
		("TMUX", "/tmp/tmux-1000/default,1,0"),
		("TERM_PROGRAM", "tmux"),
		("TERM_PROGRAM_VERSION", "3.4"),
	];
	let mut env = env(&[("HERDR_ENV", "1")]);
	for (key, value) in tmux34 {
		env.insert(key, value);
	}
	assert!(should_enable_hyperlinks_by_default(&env, TerminalId::Kitty));

	let mut old = env.clone();
	old.insert("TERM_PROGRAM_VERSION", "3.3a");
	assert!(!should_enable_hyperlinks_by_default(&old, TerminalId::Kitty));

	let mut screened = env.clone();
	screened.insert("STY", "1234.pts-0.host");
	assert!(!should_enable_hyperlinks_by_default(&screened, TerminalId::Kitty));
}

#[test]
fn treats_a_multiplexer_term_as_off_under_a_session_without_an_osc_8_policy() {
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("ZELLIJ", "0"), ("TERM", "screen-256color")]),
		TerminalId::Kitty
	));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("ZELLIJ", "0"), ("TERM", "xterm-256color")]),
		TerminalId::Kitty
	));
}

#[test]
fn lets_hyperlink_overrides_beat_every_heuristic() {
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[("MAPLE_NO_HYPERLINKS", "1")]),
		TerminalId::Kitty
	));
	assert!(!should_enable_hyperlinks_by_default(
		&env(&[
			("MAPLE_NO_HYPERLINKS", "1"),
			("TERM_PROGRAM", "tmux"),
			("TERM_PROGRAM_VERSION", "3.5a"),
			("TMUX", "1"),
		]),
		TerminalId::Wezterm
	));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("MAPLE_FORCE_HYPERLINKS", "1")]),
		TerminalId::Base
	));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("MAPLE_FORCE_HYPERLINKS", "1"), ("TMUX", "1")]),
		TerminalId::Wezterm
	));
	assert!(should_enable_hyperlinks_by_default(
		&env(&[("MAPLE_FORCE_HYPERLINKS", "1"), ("STY", "1.pts-0")]),
		TerminalId::Kitty
	));
}

#[test]
fn enables_the_colon_underline_only_on_terminals_that_implement_styled_underlines() {
	for id in [TerminalId::Kitty, TerminalId::Ghostty, TerminalId::Monstar, TerminalId::Wezterm] {
		assert!(detect_styled_underline_support(id, &env(&[])), "{}", id.as_str());
	}
	assert!(detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("TERM_PROGRAM_VERSION", "3.5.0")])
	));
}

#[test]
fn keeps_unproven_hosts_on_the_flat_underline() {
	assert_eq!(detect_terminal_id(&env(&[("TERM_PROGRAM", "Apple_Terminal")])), TerminalId::Base);
	for id in [
		TerminalId::Base,
		TerminalId::TrueColor,
		TerminalId::VSCode,
		TerminalId::Alacritty,
		TerminalId::Warp,
		TerminalId::Orca,
	] {
		assert!(!detect_styled_underline_support(id, &env(&[])), "{}", id.as_str());
	}
}

#[test]
fn enables_iterm2_colon_underline_only_on_a_confirmed_version_at_least_3_5() {
	assert!(!detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("TERM_PROGRAM_VERSION", "2.1.4")])
	));
	assert!(!detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("TERM_PROGRAM_VERSION", "3.4.0")])
	));
	assert!(detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("TERM_PROGRAM_VERSION", "3.5.0")])
	));
	assert!(detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("TERM_PROGRAM_VERSION", "4.0.0")])
	));
	assert!(!detect_styled_underline_support(TerminalId::ITerm2, &env(&[])));
}

#[test]
fn disables_the_colon_underline_under_a_multiplexer_even_when_a_proven_id_leaks() {
	assert!(!detect_styled_underline_support(
		TerminalId::Kitty,
		&env(&[("TMUX", "/tmp/tmux-1000/default,1,0")])
	));
	assert!(!detect_styled_underline_support(
		TerminalId::Ghostty,
		&env(&[("STY", "1234.pts-0.host")])
	));
	assert!(!detect_styled_underline_support(
		TerminalId::Wezterm,
		&env(&[("TERM", "screen-256color")])
	));
	assert!(!detect_styled_underline_support(
		TerminalId::ITerm2,
		&env(&[("TERM_PROGRAM_VERSION", "3.5.0"), ("ZELLIJ", "0")])
	));
}

#[test]
#[ignore = "missing facility: process-wide TERMINAL/TERMINAL_ID resolution needs a subprocess; the crate resolves per-call from TestEnv"]
fn resolves_process_wide_terminal_id_and_image_protocol() {
	let env = env(&[("TERM_PROGRAM", "otty"), ("TERM", "xterm-256color"), ("COLORTERM", "truecolor")]);
	assert_eq!(detect_terminal_id(&env).as_str(), "otty");
}

#[test]
#[ignore = "missing facility: fake-tmux subprocess harness is not ported; MAPLE_TMUX_CLIENT_TERMINAL_NAMES covers the pure seam"]
fn resolves_tmux_client_terminal_type_and_termname() {
	let env = env(&[("MAPLE_TMUX_CLIENT_TERMINAL_NAMES", "wezterm")]);
	assert_eq!(detect_terminal_id(&env).as_str(), "wezterm");
}

#[test]
#[ignore = "missing facility: the crate's TerminalId::from_program maps TERM_PROGRAM values only, not TERM-style client_termname values like xterm-kitty"]
fn falls_back_to_the_client_term_when_the_terminal_type_is_unknown() {
	let env = env(&[("MAPLE_TMUX_CLIENT_TERMINAL_NAMES", "xterm-kitty")]);
	assert_eq!(detect_terminal_id(&env).as_str(), "base");
}

#[test]
#[ignore = "missing facility: crate tmux passthrough defaults to off (safe) rather than tmux's absent-option allow"]
fn keeps_a_known_kitty_client_on_tmux_without_the_passthrough_option() {
	let env = env(&[("TMUX", "/tmp/tmux-1000/default,5103,0"), ("TERM", "tmux-256color")]);
	assert_eq!(resolve_image_protocol(TerminalId::Ghostty, &env, true), None);
}
