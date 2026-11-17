//! Port of `l/test/latex-to-unicode.test.ts`.
//!
//! The crate exposes the same pure converter plus an explicit color-depth
//! variant; the reference toggles a global `TERMINAL.trueColor` flag, so the
//! depth is passed per call here.

use maple_tui::latex_to_unicode::{
	is_bare_math_environment, latex_to_unicode, latex_to_unicode_with, render_math_in_text,
};
use maple_tui::strip_ansi;

const FG_RESET: &str = "\x1b[39m";
const BG_RESET: &str = "\x1b[49m";

/// Truecolor foreground SGR for a `#rrggbb` color, matching the crate's emitter.
fn fg(hex: &str) -> String {
	let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).expect("hex color");
	let (r, g, b) = ((value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff);
	format!("\x1b[38;2;{r};{g};{b}m")
}

#[test]
fn renders_textcolor_with_css_xcolor_models_and_mixes() {
	assert_eq!(
		latex_to_unicode("\\textcolor{red}{x}"),
		format!("{}x{fg_reset}", fg("#ff0000"), fg_reset = FG_RESET)
	);
	assert_eq!(
		latex_to_unicode("\\textcolor{red}{x}"),
		format!("{}x{}", fg("#ff0000"), FG_RESET)
	);
	assert_eq!(
		latex_to_unicode("\\textcolor[RGB]{128,64,32}{x}"),
		format!("{}x{}", fg("#804020"), FG_RESET)
	);
	assert_eq!(
		latex_to_unicode("\\textcolor{red!50!blue}{x}"),
		format!("{}x{}", fg("#800080"), FG_RESET)
	);
}

#[test]
fn keeps_color_declarations_scoped_to_their_brace_group() {
	assert_eq!(
		latex_to_unicode("{\\color{red}x}y"),
		format!("{}x{}y", fg("#ff0000"), FG_RESET)
	);
	assert_eq!(
		latex_to_unicode("{\\color{red}a {\\color{blue}b} c} d"),
		format!("{}a {}b{} c{} d", fg("#ff0000"), fg("#0000ff"), fg("#ff0000"), FG_RESET)
	);
}

#[test]
fn renders_colorbox_and_fcolorbox_with_resets() {
	let yellow_background = fg("#ffff00").replacen("\x1b[38;", "\x1b[48;", 1);
	assert_eq!(
		latex_to_unicode("\\colorbox{yellow}{x}"),
		format!("{}x{}", yellow_background, BG_RESET)
	);
	assert_eq!(
		latex_to_unicode("\\fcolorbox{red}{yellow}{x}"),
		format!(
			"{}[{}{}x{}{}]{}",
			fg("#ff0000"),
			FG_RESET,
			yellow_background,
			BG_RESET,
			fg("#ff0000"),
			FG_RESET
		)
	);
}

#[test]
fn uses_256_color_ansi_when_truecolor_is_unavailable() {
	let rendered = latex_to_unicode_with("\\textcolor{#C5FFD6}{x}", false);
	assert!(rendered.starts_with("\x1b[38;5;"), "{rendered:?}");
	assert!(rendered.ends_with(FG_RESET), "{rendered:?}");
	assert_ne!(rendered, latex_to_unicode("\\textcolor{#C5FFD6}{x}"));
}

#[test]
fn converts_bare_color_command_lines_in_prose() {
	let rendered = render_math_in_text("\\textcolor{red}{alert}");
	assert_eq!(strip_ansi(&rendered), "alert");
	assert!(rendered.contains(&fg("#ff0000")), "{rendered:?}");
}

#[test]
fn keeps_synthetic_wrappers_in_the_caller_style_across_nested_styles() {
	assert_eq!(
		latex_to_unicode("\\textbf{\\frac{\\textit{ab}}{c}}"),
		"\x1b[1m(\x1b[3mab\x1b[23m)/c\x1b[22m"
	);
}

#[test]
fn keeps_fallback_script_markers_in_the_caller_style_across_nested_colors() {
	assert_eq!(
		latex_to_unicode("\\textit{x^{\\textcolor{red}{q}}}"),
		format!("\x1b[3mx^({}q{})\x1b[23m", fg("#ff0000"), FG_RESET)
	);
}

#[test]
fn preserves_the_active_color_while_parsing_optional_arguments() {
	assert_eq!(
		latex_to_unicode("\\color{red}\\xrightarrow[n]{x}"),
		format!("{}→ˣₙ{}", fg("#ff0000"), FG_RESET)
	);
}

#[test]
fn restores_the_active_color_after_an_environment_body() {
	assert_eq!(
		latex_to_unicode("\\begin{pmatrix}\\color{red}a\\end{pmatrix}b"),
		format!("({}a{})b", fg("#ff0000"), FG_RESET)
	);
}

#[test]
fn resets_bold_and_italic_inside_textnormal() {
	assert_eq!(
		latex_to_unicode("\\textbf{\\textit{A\\textnormal{B}C}}"),
		"\x1b[1m\x1b[3mA\x1b[22m\x1b[23mB\x1b[1m\x1b[3mC\x1b[22m\x1b[23m"
	);
}

#[test]
fn renders_text_mode_styles_with_terminal_attributes() {
	let rendered = latex_to_unicode("\\textcolor{red}{\\textbf{strongest}}");
	assert_eq!(strip_ansi(&rendered), "strongest");
	assert!(rendered.contains(&fg("#ff0000")));
	assert!(rendered.contains("\x1b[1m"));
	assert!(!rendered.chars().any(|ch| ('\u{1d400}'..='\u{1d7ff}').contains(&ch)));
}

#[test]
fn maps_all_supported_text_styles_to_terminal_attributes() {
	for (command, on, off) in [
		("textbf", "\x1b[1m", "\x1b[22m"),
		("textit", "\x1b[3m", "\x1b[23m"),
		("textsl", "\x1b[3m", "\x1b[23m"),
		("emph", "\x1b[3m", "\x1b[23m"),
	] {
		assert_eq!(latex_to_unicode(&format!("\\{command}{{x}}")), format!("{on}x{off}"));
	}
}

#[test]
fn reopens_nested_terminal_styles_after_nested_resets() {
	let rendered = latex_to_unicode("\\textbf{outer \\textbf{inner} outer}");
	assert_eq!(strip_ansi(&rendered), "outer inner outer");
	assert!(rendered.contains("\x1b[22m\x1b[1m outer"), "{rendered:?}");
}

#[test]
fn keeps_terminal_attributes_out_of_glyph_transforms() {
	assert_eq!(latex_to_unicode("\\hat{\\textbf{x}}"), "\x1b[1mx̂\x1b[22m");
	assert_eq!(latex_to_unicode("\\frac{\\textbf{a}}{b}"), "\x1b[1ma\x1b[22m/b");
	assert_eq!(latex_to_unicode("\\sqrt{\\textbf{x}}"), "√\x1b[1mx\x1b[22m");
}

#[test]
fn keeps_synthetic_wrapper_glyphs_at_the_caller_style() {
	assert_eq!(
		latex_to_unicode("\\boxed{\\textcolor{red}{x}}"),
		format!("[{}x{}]", fg("#ff0000"), FG_RESET)
	);
	assert_eq!(latex_to_unicode("\\pmod{\\textbf{x}}"), "(mod \x1b[1mx\x1b[22m)");
}

#[test]
fn keeps_fallback_script_glyphs_at_the_script_site_style() {
	assert_eq!(latex_to_unicode("x_{\\textit{word}}"), "x_(\x1b[3mword\x1b[23m)");
	assert_eq!(latex_to_unicode("x^{\\textbf{foo!}}"), "x^(\x1b[1mfoo!\x1b[22m)");
}

#[test]
fn cancels_an_enclosing_terminal_style_for_nested_text_overrides() {
	let bold = latex_to_unicode("\\textbf{A\\textmd{B}C}");
	let italic = latex_to_unicode("\\textit{A\\textup{B}C}");
	assert_eq!(strip_ansi(&bold), "ABC");
	assert_eq!(strip_ansi(&italic), "ABC");
	assert_eq!(bold, "\x1b[1mA\x1b[22mB\x1b[1mC\x1b[22m");
	assert_eq!(italic, "\x1b[3mA\x1b[23mB\x1b[3mC\x1b[23m");
}

#[test]
fn leaves_terminal_fallbacks_as_plain_text() {
	assert_eq!(latex_to_unicode("\\texttt{x}\\textsf{y}"), "xy");
}

#[test]
fn accepts_display_math_environments_including_starred_variants() {
	for env in ["cases", "align", "align*", "pmatrix", "array", "gather", "equation*", "aligned"] {
		assert!(is_bare_math_environment(env), "{env}");
	}
}

#[test]
fn rejects_text_mode_table_list_and_float_environments() {
	for env in ["tabular", "tabular*", "itemize", "enumerate", "verbatim", "document", "figure"] {
		assert!(!is_bare_math_environment(env), "{env}");
	}
}

#[test]
fn maps_ams_glyphs_and_collapses_vulgar_fractions() {
	assert_eq!(latex_to_unicode("\\subseteqq"), "⫅");
	assert_eq!(latex_to_unicode("\\supseteqq"), "⫆");
	assert_eq!(latex_to_unicode("\\frac{1}{2}"), "½");
}

#[test]
fn converts_a_bare_math_environment_without_delimiters() {
	let out = render_math_in_text("\\begin{align}\na &= b + c \\\\\nx &= y\n\\end{align}");
	assert!(!out.contains("\\begin{align}"), "{out:?}");
	assert!(out.contains("="));
}

#[test]
fn pulls_a_preceding_plain_lhs_line_into_the_converted_block() {
	let out = render_math_in_text("f(x) =\n\\begin{cases}\n1 & x > 0 \\\\\n0 & x \\le 0\n\\end{cases}");
	assert!(!out.contains("\\begin{cases}"), "{out:?}");
	assert!(out.contains("f(x) ="));
	assert!(!out.contains("f(x) =\n"), "{out:?}");
}

#[test]
fn leaves_non_math_environments_verbatim_shielding_their_body() {
	let verbatim = "\\begin{verbatim}\n\\frac{a}{b}\n\\end{verbatim}";
	assert_eq!(render_math_in_text(verbatim), verbatim);
}

#[test]
fn leaves_a_bare_itemize_list_verbatim() {
	let list = "Here:\n\\begin{itemize}\n\\item first\n\\item second\n\\end{itemize}\nDone.";
	assert_eq!(render_math_in_text(list), list);
}

#[test]
fn does_not_treat_a_prose_dollar_amount_as_math() {
	assert_eq!(render_math_in_text("I paid \\$5 for it."), "I paid $5 for it.");
}
