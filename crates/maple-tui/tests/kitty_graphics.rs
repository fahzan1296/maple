//! Port of `l/test/kitty-graphics.test.ts`.
//!
//! The reference reads `PI_*` env overrides; the crate renamed them to
//! `MAPLE_*`, so the detection tests substitute those keys. Terminal identity
//! strings map onto `TerminalId`.

use maple_tui::kitty_graphics::{
	KITTY_PLACEHOLDER, KITTY_PLACEHOLDER_MAX_CELLS, detect_kitty_unicode_placeholders_support,
	encode_kitty_placeholder_grid, encode_kitty_virtual_placement, kitty_placeholders_fit,
	render_kitty_placeholder_lines,
};
use maple_tui::terminal_capabilities::{TerminalId, TestEnv};
use maple_tui::text_shape::visible_width;

fn env(vars: &[(&str, &str)]) -> TestEnv {
	let mut env = TestEnv::new();
	for (key, value) in vars {
		env.insert(key, value);
	}
	env
}

fn id(name: &str) -> TerminalId {
	match name {
		"kitty" => TerminalId::Kitty,
		"ghostty" => TerminalId::Ghostty,
		"wezterm" => TerminalId::Wezterm,
		"iterm2" => TerminalId::ITerm2,
		"alacritty" => TerminalId::Alacritty,
		"warp" => TerminalId::Warp,
		"otty" => TerminalId::Otty,
		"rio" => TerminalId::Rio,
		"tern" => TerminalId::Tern,
		"monstar" => TerminalId::Monstar,
		_ => TerminalId::Base,
	}
}

fn detect(name: &str, env: &TestEnv) -> bool {
	detect_kitty_unicode_placeholders_support(id(name), env)
}

#[test]
fn encode_kitty_virtual_placement_emits_ap_with_u1_and_the_geometry() {
	let env = TestEnv::new();
	assert_eq!(
		encode_kitty_virtual_placement(7, Some(7), 4, 2, &env),
		"\x1b_Ga=p,U=1,q=2,i=7,p=7,c=4,r=2\x1b\\"
	);
	assert_eq!(
		encode_kitty_virtual_placement(7, None, 4, 2, &env),
		"\x1b_Ga=p,U=1,q=2,i=7,c=4,r=2\x1b\\"
	);
}

#[test]
fn encode_kitty_placeholder_grid_returns_one_row_per_line_with_explicit_cells() {
	let grid = encode_kitty_placeholder_grid(1, Some(1), 3, 2);
	assert_eq!(grid.len(), 2);
	for row in &grid {
		assert!(row.contains("\x1b[38;2;0;0;1m"));
		assert!(row.contains("\x1b[58:2::0:0:1m"));
		assert!(row.ends_with("\x1b[39;59m"));
		assert_eq!(row.matches(KITTY_PLACEHOLDER).count(), 3);
	}
	assert_ne!(grid[0], grid[1]);
}

#[test]
fn a_placeholder_row_measures_exactly_columns_cells_wide() {
	let grid = encode_kitty_placeholder_grid(5, Some(5), 6, 1);
	assert_eq!(visible_width(&grid[0]), 6);
}

#[test]
fn render_kitty_placeholder_lines_prefixes_line_zero_with_the_placement_apc() {
	let env = TestEnv::new();
	let placement = encode_kitty_virtual_placement(2, Some(2), 2, 3, &env);
	let grid = encode_kitty_placeholder_grid(2, Some(2), 2, 3);
	let lines = render_kitty_placeholder_lines(2, Some(2), 2, 3, &env);
	assert_eq!(lines.len(), 3);
	assert_eq!(lines[0], format!("{placement}{}", grid[0]));
	assert_eq!(&lines[1..], &grid[1..]);
}

#[test]
fn kitty_placeholders_fit_guards_the_diacritic_table_capacity() {
	assert!(kitty_placeholders_fit(1, 1));
	assert!(kitty_placeholders_fit(KITTY_PLACEHOLDER_MAX_CELLS, KITTY_PLACEHOLDER_MAX_CELLS));
	assert!(!kitty_placeholders_fit(0, 5));
	assert!(!kitty_placeholders_fit(5, 0));
	assert!(!kitty_placeholders_fit(KITTY_PLACEHOLDER_MAX_CELLS + 1, 1));
	assert!(!kitty_placeholders_fit(1, KITTY_PLACEHOLDER_MAX_CELLS + 1));
}

#[test]
fn enables_for_kitty_and_ghostty_by_default() {
	let env = TestEnv::new();
	assert!(detect("kitty", &env));
	assert!(detect("ghostty", &env));
}

#[test]
fn enables_the_scroll_aware_placeholder_path_for_rio() {
	assert!(detect("rio", &TestEnv::new()));
	let off = env(&[("MAPLE_NO_KITTY_PLACEHOLDERS", "1")]);
	assert!(!detect("rio", &off));
	let herdr = env(&[("HERDR_ENV", "1")]);
	assert!(!detect("rio", &herdr));
}

#[test]
fn disables_for_wezterm_and_other_kitty_protocol_paths_that_render_pua_glyphs() {
	let env = TestEnv::new();
	assert!(!detect("wezterm", &env));
	assert!(!detect("warp", &env));
	assert!(!detect("base", &env));
	assert!(!detect("iterm2", &env));
	assert!(!detect("alacritty", &env));
}

#[test]
fn enables_for_otty() {
	assert!(detect("otty", &TestEnv::new()));
	let off = env(&[("MAPLE_NO_KITTY_PLACEHOLDERS", "1")]);
	assert!(!detect("otty", &off));
	let herdr = env(&[("HERDR_ENV", "1")]);
	assert!(!detect("otty", &herdr));
}

#[test]
fn enables_for_tern() {
	assert!(detect("tern", &TestEnv::new()));
}

#[test]
fn uses_scroll_aware_placeholders_when_kitty_is_forced_through_a_multiplexer() {
	let forced_tmux = env(&[
		("TMUX", "/tmp/tmux-1000/default,1,0"),
		("MAPLE_FORCE_IMAGE_PROTOCOL", "kitty"),
	]);
	assert!(detect("base", &forced_tmux));
	assert!(detect("wezterm", &forced_tmux));
	let forced_herdr = env(&[
		("HERDR_ENV", "1"),
		("MAPLE_FORCE_IMAGE_PROTOCOL", "kitty"),
	]);
	assert!(detect("ghostty", &forced_herdr));
	let tmux_only = env(&[("TMUX", "/tmp/tmux-1000/default,1,0")]);
	assert!(!detect("base", &tmux_only));
	assert!(detect("ghostty", &tmux_only));
	assert!(detect("monstar", &tmux_only));
}

#[test]
fn ignores_leaked_kitty_capable_identities_inside_herdr_unless_forced() {
	let leaked = env(&[
		("HERDR_ENV", "1"),
		("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty"),
	]);
	assert!(!detect("ghostty", &leaked));
	let forced = env(&[
		("HERDR_ENV", "1"),
		("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty"),
		("MAPLE_KITTY_PLACEHOLDERS", "1"),
	]);
	assert!(detect("ghostty", &forced));
	let pane_only = env(&[
		("HERDR_PANE_ID", "p1"),
		("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty"),
	]);
	assert!(!detect("ghostty", &pane_only));
	let pane_forced = env(&[
		("HERDR_PANE_ID", "p1"),
		("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty"),
		("MAPLE_KITTY_PLACEHOLDERS", "1"),
	]);
	assert!(detect("ghostty", &pane_forced));
	let nested = env(&[
		("HERDR_PANE_ID", "p1"),
		("TMUX", "/tmp/tmux-1000/default,1,0"),
	]);
	assert!(!detect("ghostty", &nested));
}

#[test]
fn honors_the_hard_off_override_on_supporting_terminals() {
	let kitty_off = env(&[("MAPLE_NO_KITTY_PLACEHOLDERS", "1")]);
	assert!(!detect("kitty", &kitty_off));
	let ghostty_off = env(&[("MAPLE_NO_KITTY_PLACEHOLDERS", "true")]);
	assert!(!detect("ghostty", &ghostty_off));
}

#[test]
fn honors_the_opt_in_on_otherwise_unsupported_terminals() {
	let wezterm_on = env(&[("MAPLE_KITTY_PLACEHOLDERS", "1")]);
	assert!(detect("wezterm", &wezterm_on));
}

#[test]
fn the_hard_off_beats_the_opt_in_when_both_are_set() {
	let both = env(&[
		("MAPLE_NO_KITTY_PLACEHOLDERS", "1"),
		("MAPLE_KITTY_PLACEHOLDERS", "1"),
	]);
	assert!(!detect("kitty", &both));
}

#[test]
fn the_opt_in_zero_forces_off_on_a_default_on_terminal() {
	let kitty_off = env(&[("MAPLE_KITTY_PLACEHOLDERS", "0")]);
	assert!(!detect("kitty", &kitty_off));
	let ghostty_off = env(&[("MAPLE_KITTY_PLACEHOLDERS", "off")]);
	assert!(!detect("ghostty", &ghostty_off));
}

#[test]
fn the_placeholder_opt_out_beats_forced_kitty_under_tmux() {
	let forced_off = env(&[
		("TMUX", "/tmp/tmux-1000/default,1,0"),
		("MAPLE_FORCE_IMAGE_PROTOCOL", "kitty"),
		("MAPLE_KITTY_PLACEHOLDERS", "0"),
	]);
	assert!(!detect("base", &forced_off));
}
