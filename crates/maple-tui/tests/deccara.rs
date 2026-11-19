//! Port of `l/test/deccara.test.ts` (kill-switch env renamed to `MAPLE_NO_DECCARA`).

use maple_tui::deccara::{
	analyze_bg_fill_line, encode_deccara, plan_deccara_fills, DECSACE_DEFAULT, DECSACE_RECT,
};
use maple_tui::terminal_capabilities::{detect_rectangular_sgr_support, TerminalId, TestEnv};

const BG_OPEN: &str = "\x1b[48;2;10;20;30m";
const BG_SGR: &str = "48;2;10;20;30";

fn count_occurrences(haystack: &str, needle: &str) -> usize {
	haystack.matches(needle).count()
}

#[test]
fn rectangular_sgr_is_kitty_only() {
	let env = TestEnv::new();
	assert!(detect_rectangular_sgr_support(TerminalId::Kitty, &env));
	// Ghostty leaves CSI $r unimplemented (ghostty-org/ghostty#632) — excluded.
	assert!(!detect_rectangular_sgr_support(TerminalId::Ghostty, &env));
	assert!(!detect_rectangular_sgr_support(TerminalId::Wezterm, &env));
	assert!(!detect_rectangular_sgr_support(TerminalId::ITerm2, &env));
	assert!(!detect_rectangular_sgr_support(TerminalId::Alacritty, &env));
	assert!(!detect_rectangular_sgr_support(TerminalId::Base, &env));
	assert!(!detect_rectangular_sgr_support(TerminalId::TrueColor, &env));
}

#[test]
fn honors_the_no_deccara_kill_switch_truthy_values_only() {
	let mut env = TestEnv::new();
	env.insert("MAPLE_NO_DECCARA", "1");
	assert!(!detect_rectangular_sgr_support(TerminalId::Kitty, &env));
	let mut env = TestEnv::new();
	env.insert("MAPLE_NO_DECCARA", "true");
	assert!(!detect_rectangular_sgr_support(TerminalId::Kitty, &env));
	// A falsey assignment is not a kill: support stays on.
	let mut env = TestEnv::new();
	env.insert("MAPLE_NO_DECCARA", "0");
	assert!(detect_rectangular_sgr_support(TerminalId::Kitty, &env));
	let mut env = TestEnv::new();
	env.insert("MAPLE_NO_DECCARA", "false");
	assert!(detect_rectangular_sgr_support(TerminalId::Kitty, &env));
}

#[test]
fn disabled_under_tmux_screen_zellij_cmux_multiplexers() {
	for key in ["TMUX", "STY", "ZELLIJ", "CMUX_SURFACE_ID"] {
		let mut env = TestEnv::new();
		env.insert(key, "surface");
		assert!(
			!detect_rectangular_sgr_support(TerminalId::Kitty, &env),
			"{key} must disable DECCARA"
		);
	}
	let mut env = TestEnv::new();
	env.insert("TERM", "tmux-256color");
	assert!(!detect_rectangular_sgr_support(TerminalId::Kitty, &env));
	let mut env = TestEnv::new();
	env.insert("TERM", "screen.xterm");
	assert!(!detect_rectangular_sgr_support(TerminalId::Kitty, &env));
}

#[test]
fn encode_matches_kittys_documented_background_fill_example() {
	assert_eq!(
		format!("{DECSACE_RECT}{}", encode_deccara(4, 3, 11, 10, "44")),
		"\x1b[2*x\x1b[4;3;11;10;44$r"
	);
	assert_eq!(
		format!(
			"{DECSACE_RECT}{}{DECSACE_DEFAULT}",
			encode_deccara(4, 3, 11, 10, "44")
		),
		"\x1b[2*x\x1b[4;3;11;10;44$r\x1b[*x"
	);
}

#[test]
fn treats_an_all_space_background_row_as_a_whole_row_fill() {
	let close = "\x1b[49m\x1b[0m";
	let line = format!("{BG_OPEN}{}{}close", " ".repeat(10));
	assert_eq!(
		analyze_bg_fill_line(&line, 10),
		Some(maple_tui::deccara::BgFillAnalysis {
			cut: 0,
			left_col: 0,
			bg: Some(BG_SGR.to_string()),
		})
	);
}

#[test]
fn locates_the_trailing_pad_after_content_under_a_single_background() {
	let close = "\x1b[49m\x1b[0m";
	let line = format!("\x1b[48;5;4mHi{}{close}", " ".repeat(8));
	let result = analyze_bg_fill_line(&line, 10).expect("trailing pad is a fill");
	assert_eq!(result.left_col, 2);
	assert_eq!(result.bg.as_deref(), Some("48;5;4"));
	assert_eq!(&line[..result.cut], "\x1b[48;5;4mHi");
}

#[test]
fn recognizes_16_color_and_bright_background_params() {
	let close = "\x1b[49m\x1b[0m";
	assert_eq!(
		analyze_bg_fill_line(&format!("\x1b[41m{}{close}", " ".repeat(6)), 6)
			.and_then(|a| a.bg),
		Some("41".to_string())
	);
	assert_eq!(
		analyze_bg_fill_line(&format!("\x1b[101m{}{close}", " ".repeat(6)), 6)
			.and_then(|a| a.bg),
		Some("101".to_string())
	);
}

#[test]
fn rejects_rows_with_no_trailing_padding() {
	let close = "\x1b[49m\x1b[0m";
	assert_eq!(
		analyze_bg_fill_line(&format!("{BG_OPEN}{}{close}", "x".repeat(10)), 10),
		None
	);
}

#[test]
fn rejects_default_background_trailing_spaces() {
	assert_eq!(analyze_bg_fill_line(&format!("hello{}", " ".repeat(5)), 10), None);
}

#[test]
fn rejects_colored_trailing_fills_after_default_background_gap_cells() {
	let close = "\x1b[49m\x1b[0m";
	assert_eq!(
		analyze_bg_fill_line(&format!("{}{BG_OPEN}{}{close}", " ".repeat(2), " ".repeat(8)), 10),
		None
	);
	assert_eq!(
		analyze_bg_fill_line(&format!("X {BG_OPEN}{}{close}", " ".repeat(8)), 10),
		None
	);
}

#[test]
fn allows_a_colored_trailing_fill_immediately_after_default_content() {
	let close = "\x1b[49m\x1b[0m";
	assert_eq!(
		analyze_bg_fill_line(&format!("X{BG_OPEN}{}{close}", " ".repeat(9)), 10),
		Some(maple_tui::deccara::BgFillAnalysis {
			cut: 1,
			left_col: 1,
			bg: Some(BG_SGR.to_string()),
		})
	);
}

#[test]
fn rejects_rows_narrower_than_the_full_width() {
	let close = "\x1b[49m\x1b[0m";
	assert_eq!(
		analyze_bg_fill_line(&format!("\x1b[41mab{}{close}", " ".repeat(3)), 10),
		None
	);
}

#[test]
fn rejects_colon_form_extended_background() {
	let close = "\x1b[49m\x1b[0m";
	assert_eq!(
		analyze_bg_fill_line(&format!("\x1b[48:2:1:2:3m{}{close}", " ".repeat(5)), 5),
		None
	);
}

#[test]
fn rejects_lines_carrying_osc_sequences() {
	let close = "\x1b[49m\x1b[0m";
	let line = format!(
		"\x1b[41m\x1b]8;;https://x\x07L\x1b]8;;\x07{}{close}",
		" ".repeat(8)
	);
	assert_eq!(analyze_bg_fill_line(&line, 10), None);
}

#[test]
fn rejects_a_background_change_inside_the_trailing_region() {
	let line = "\x1b[41mab\x1b[49m   \x1b[42m   \x1b[49m\x1b[0m";
	assert_eq!(analyze_bg_fill_line(line, 8), None);
}

fn blank(width: usize, bg_open: &str) -> String {
	format!("{bg_open}{}\x1b[49m\x1b[0m", " ".repeat(width))
}

#[test]
fn coalesces_adjacent_identical_fills_into_one_rectangle() {
	let lines: Vec<String> = (0..3).map(|_| blank(10, BG_OPEN)).collect();
	let plan = plan_deccara_fills(&lines, 10, 1);
	assert_eq!(plan.texts, ["", "", ""]);
	assert_eq!(
		plan.sequence,
		format!(
			"{DECSACE_RECT}{}{DECSACE_DEFAULT}",
			encode_deccara(1, 1, 3, 10, BG_SGR)
		)
	);
}

#[test]
fn respects_the_screen_row_offset() {
	let lines: Vec<String> = (0..2).map(|_| blank(10, BG_OPEN)).collect();
	let plan = plan_deccara_fills(&lines, 10, 3);
	assert_eq!(
		plan.sequence,
		format!(
			"{DECSACE_RECT}{}{DECSACE_DEFAULT}",
			encode_deccara(4, 1, 5, 10, BG_SGR)
		)
	);
}

fn splits_coalescing_when_the_background_differs() {
	let other = "\x1b[42m";
	let lines = vec![blank(8, BG_OPEN), blank(8, BG_OPEN), blank(8, other)];
	let plan = plan_deccara_fills(&lines, 8, 1);
	assert_eq!(
		plan.sequence,
		format!(
			"{DECSACE_RECT}{}{}{DECSACE_DEFAULT}",
			encode_deccara(1, 1, 2, 8, BG_SGR),
			encode_deccara(3, 1, 3, 8, "42")
		)
	);
}

#[test]
fn does_not_coalesce_non_adjacent_fills() {
	let lines = vec![
		blank(12, BG_OPEN),
		"plain text row padded out here".to_string(),
		blank(12, BG_OPEN),
	];
	let plan = plan_deccara_fills(&lines, 12, 1);
	assert_eq!(count_occurrences(&plan.sequence, "$r"), 2);
	assert_eq!(plan.texts[1], lines[1]);
}

#[test]
fn keeps_the_original_line_when_the_rectangle_would_not_save_bytes() {
	let line = format!("{BG_OPEN}{}  \x1b[49m\x1b[0m", "x".repeat(18));
	let plan = plan_deccara_fills(&[line.clone()], 20, 1);
	assert_eq!(plan.texts, [line]);
	assert_eq!(plan.sequence, "");
}

#[test]
fn optimizes_a_content_row_with_substantial_trailing_padding() {
	let line = format!("{BG_OPEN}Hi{}\x1b[49m\x1b[0m", " ".repeat(38));
	let plan = plan_deccara_fills(&[line], 40, 1);
	assert_eq!(plan.texts[0], format!("{BG_OPEN}Hi\x1b[0m"));
	assert_eq!(
		plan.sequence,
		format!(
			"{DECSACE_RECT}{}{DECSACE_DEFAULT}",
			encode_deccara(1, 3, 1, 40, BG_SGR)
		)
	);
}