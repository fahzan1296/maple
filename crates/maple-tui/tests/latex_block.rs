//! Port of `l/test/latex-block.test.ts`.

use maple_tui::latex_block::latex_to_block;
use maple_tui::strip_ansi;

fn plain(lines: &[String]) -> Vec<String> {
	lines.iter().map(|line| strip_ansi(line)).collect()
}

#[test]
fn stacks_a_simple_fraction_with_a_centered_bar() {
	assert_eq!(latex_to_block("\\frac{1}{2}"), vec![" 1 ", "───", " 2 "]);
}

#[test]
fn sizes_the_bar_to_the_wider_of_numerator_and_denominator() {
	assert_eq!(latex_to_block("\\frac{a+b}{c}"), vec![" a+b ", "─────", "  c  "]);
}

#[test]
fn aligns_surrounding_text_to_the_fraction_bar() {
	assert_eq!(latex_to_block("x = \\frac{a+b}{c}"), vec!["     a+b ", "x = ─────", "      c  "]);
}

#[test]
fn nests_fractions() {
	assert_eq!(
		latex_to_block("\\frac{\\frac{a}{b}}{c}"),
		vec!["  a  ", " ─── ", "  b  ", "─────", "  c  "]
	);
}

#[test]
fn keeps_a_fully_convertible_expression_on_one_line() {
	assert_eq!(latex_to_block("x^2 + y_1 = 0"), vec!["x² + y₁ = 0"]);
}

#[test]
fn raises_a_non_convertible_exponent_as_a_block() {
	assert_eq!(
		latex_to_block("e^{i\\pi} + 1 = 0")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec![" iπ", "e   + 1 = 0"]
	);
}

#[test]
fn stacks_fractions_inside_wrapper_environments() {
	assert_eq!(
		latex_to_block("\\begin{equation} x = \\frac{a+b}{c} \\end{equation}"),
		vec!["     a+b ", "x = ─────", "      c  "]
	);
}

#[test]
fn skips_the_column_count_preamble_of_alignat() {
	let lines = latex_to_block("\\begin{alignat}{2} a &= \\frac{1}{2} \\end{alignat}");
	assert!(!lines.join("\n").contains("{2}"), "{lines:?}");
	assert!(lines.iter().any(|line| line.contains("───")), "{lines:?}");
	assert_eq!(lines[0].trim(), "1");
}

#[test]
fn stacks_each_row_of_an_aligned_environment() {
	let lines = latex_to_block("\\begin{aligned} y &= \\frac{1}{2} \\\\ z &= \\frac{3}{4} \\end{aligned}");
	assert_eq!(lines.len(), 6, "{lines:?}");
	assert!(lines[1].contains("───"));
	assert!(lines[4].contains("───"));
	assert!(plain(&lines)[0].contains('1'));
	assert!(plain(&lines)[5].contains('4'));
}

#[test]
fn renders_matrix_environments_as_center_baselined_grids() {
	assert_eq!(
		latex_to_block("\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}"),
		vec!["⎡ a  b ⎤", "⎢      ⎥", "⎣ c  d ⎦"]
	);
	assert_eq!(
		latex_to_block("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}"),
		vec!["⎛ a  b ⎞", "⎜      ⎟", "⎝ c  d ⎠"]
	);
	assert_eq!(
		latex_to_block("\\begin{pmatrix} a & b & c \\end{pmatrix}"),
		vec!["(a  b  c)"]
	);
}

#[test]
fn centers_using_visible_width_ignoring_ansi_color_codes() {
	let lines = latex_to_block("\\frac{\\textcolor{red}{a}}{b}");
	assert_eq!(plain(&lines)[1], "───");
	assert_eq!(plain(&lines), vec![" a ", "───", " b "]);
	assert!(lines[0].contains('\x1b'), "{lines:?}");
}

#[test]
fn returns_no_lines_for_empty_input() {
	assert!(latex_to_block("").is_empty());
	assert!(latex_to_block("   ").is_empty());
}

#[test]
fn baseline_aligns_matrix_cells_containing_fractions() {
	assert_eq!(
		latex_to_block("\\begin{bmatrix} \\frac{1}{2} & x \\\\ y & z \\end{bmatrix}"),
		vec!["⎡  1     ⎤", "⎢ ───  x ⎥", "⎢  2     ⎥", "⎢        ⎥", "⎣  y   z ⎦"]
	);
}

#[test]
fn centers_surrounding_text_on_the_matrix_middle() {
	assert_eq!(
		latex_to_block("A = \\begin{bmatrix} a \\\\ b \\end{bmatrix}"),
		vec!["    ⎡ a ⎤", "A = ⎢   ⎥", "    ⎣ b ⎦"]
	);
}

#[test]
fn renders_vmatrix_with_full_height_bars() {
	assert_eq!(
		latex_to_block("\\begin{vmatrix} a & b \\\\ c & d \\end{vmatrix}"),
		vec!["│ a  b │", "│      │", "│ c  d │"]
	);
}

#[test]
fn honors_the_array_column_specification() {
	assert_eq!(
		latex_to_block("\\begin{array}{lcr} 1 & 22 & 333 \\\\ aaa & b & c \\end{array}")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["1    22  333", "", "aaa  b     c"]
	);
}

#[test]
fn renders_cases_with_a_stretched_left_brace() {
	let lines = latex_to_block("f(x) = \\begin{cases} x & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}");
	assert_eq!(
		lines.iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["       ⎧ x  x > 0", "f(x) = ⎨", "       ⎩ 0  otherwise"]
	);
}

#[test]
fn stacks_big_operator_limits_above_and_below_the_symbol() {
	assert_eq!(latex_to_block("\\sum_{i=0}^{n} i^2"), vec![" n    ", " ∑  i²", "i=0   "]);
}

#[test]
fn places_lim_scripts_underneath() {
	let lines = latex_to_block("\\lim_{x \\to 0} \\frac{\\sin x}{x}");
	assert!(lines[1].contains("lim"), "{lines:?}");
	assert!(lines[2].contains("x → 0"), "{lines:?}");
	assert!(lines[1].contains("───"), "{lines:?}");
}

#[test]
fn keeps_integral_bounds_beside_the_symbol_unless_limits_is_given() {
	assert_eq!(latex_to_block("\\int_a^b f(x) dx"), vec!["∫ₐᵇ f(x) dx"]);
	assert_eq!(
		latex_to_block("\\int\\limits_a^b f(x) dx")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["b", "∫ f(x) dx", "a"]
	);
}

#[test]
fn stretches_left_right_delimiters_and_pins_corner_scripts() {
	assert_eq!(
		latex_to_block("\\left( \\frac{a+b}{c} \\right)^2")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["⎛  a+b  ⎞²", "⎜ ───── ⎟", "⎝   c   ⎠"]
	);
}

#[test]
fn stretches_bare_parentheses_around_a_fraction() {
	assert_eq!(
		latex_to_block("( \\frac{a}{b} )"),
		vec!["⎛   a   ⎞", "⎜  ───  ⎟", "⎝   b   ⎠"]
	);
}

#[test]
fn leaves_unbalanced_interval_brackets_on_the_baseline() {
	assert_eq!(latex_to_block("[0, 1)"), vec!["[0, 1)"]);
}

#[test]
fn renders_middle_delimiters_at_full_height_inside_left_right() {
	let lines = latex_to_block("\\left\\{ x \\middle| \\frac{x}{2} \\in \\mathbb{Z} \\right\\}");
	assert_eq!(lines.len(), 3, "{lines:?}");
	assert!(lines[1].starts_with("⎨"), "{:?}", lines[1]);
	assert!(lines[1].contains('│'));
	assert!(lines[1].ends_with("⎬"), "{:?}", lines[1]);
}

#[test]
fn always_draws_the_radical_roof_in_display_math() {
	assert_eq!(
		latex_to_block("\\sqrt{\\frac{a+1}{b}}")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec![" ┌──────", " │  a+1", " │ ─────", "╲│   b"]
	);
	assert_eq!(
		latex_to_block("\\sqrt{x}").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec![" ┌──", "╲│ x"]
	);
}

#[test]
fn keeps_a_display_radical_degree_inside_its_enclosing_style_wrapper() {
	let lines = latex_to_block("\\textsl{\\sqrt[n]{x}}");
	assert_eq!(
		plain(&lines).iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["ⁿ ┌──", " ╲│ x"]
	);
	assert!(lines[0].contains("\x1b[3mⁿ\x1b[23m"), "{:?}", lines[0]);
}

#[test]
fn stacks_binom_inside_stretched_parentheses() {
	assert_eq!(latex_to_block("\\binom{n}{k}"), vec!["⎛ n ⎞", "⎜   ⎟", "⎝ k ⎠"]);
}

#[test]
fn raises_a_block_superscript_containing_a_fraction() {
	assert_eq!(
		latex_to_block("e^{\\frac{x}{2}}").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["  x", " ───", "  2", "e"]
	);
}

#[test]
fn raises_or_lowers_one_line_scripts_without_unicode_form() {
	assert_eq!(latex_to_block("x^q").iter().map(|line| line.trim_end()).collect::<Vec<_>>(), vec![" q", "x"]);
	assert_eq!(latex_to_block("x_q").iter().map(|line| line.trim_end()).collect::<Vec<_>>(), vec!["x", " q"]);
	assert_eq!(
		latex_to_block("x_q^q").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec![" q", "x", " q"]
	);
}

#[test]
fn boxes_multi_letter_script_words() {
	assert_eq!(
		latex_to_block("N_{turns} + 1").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["N      + 1", " turns"]
	);
	assert_eq!(latex_to_block("x^{ab}").iter().map(|line| line.trim_end()).collect::<Vec<_>>(), vec![" ab", "x"]);
	assert_eq!(latex_to_block("x_i + y_1"), vec!["xᵢ + y₁"]);
}

#[test]
fn pins_both_scripts_of_a_tall_base_in_one_shared_column() {
	assert_eq!(
		latex_to_block("\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}_0^T")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["⎡ a  b ⎤ᵀ", "⎢      ⎥", "⎣ c  d ⎦₀"]
	);
}

#[test]
fn draws_a_labeled_underbrace_with_the_baseline_on_the_content() {
	assert_eq!(
		latex_to_block("x + \\underbrace{a+b}_{\\text{sum}}")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["x + a+b", "    ╰┬╯", "    sum"]
	);
}

#[test]
fn draws_a_labeled_overbrace_above_the_content() {
	assert_eq!(
		latex_to_block("\\overbrace{a+b}^{\\text{sum}} + x")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["sum", "╭┴╮", "a+b + x"]
	);
}

#[test]
fn centers_content_and_label_on_the_wider_of_the_two() {
	assert_eq!(
		latex_to_block("\\underbrace{ab}_{\\text{longer label}}")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["     ab", "    ╰┬╯", "longer label"]
	);
}

#[test]
fn draws_an_unlabeled_underbrace() {
	assert_eq!(
		latex_to_block("\\underbrace{a+b+c}").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["a+b+c", "╰─┬─╯"]
	);
}

#[test]
fn stacks_overset_above_and_underset_below_the_base() {
	assert_eq!(
		latex_to_block("A \\overset{!}{=} B").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["  !", "A = B"]
	);
	assert_eq!(
		latex_to_block("A \\underset{0}{=} B").iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["A = B", "  0"]
	);
}

#[test]
fn aligns_align_environment_rows_on_the_ampersand_column() {
	assert_eq!(
		latex_to_block("\\begin{align} f(x) &= x^2 + 1 \\\\ g(x) &= \\frac{x}{2} \\end{align}")
			.iter()
			.map(|line| line.trim_end())
			.collect::<Vec<_>>(),
		vec!["f(x) = x² + 1", "        x", "g(x) = ───", "        2"]
	);
}

#[test]
fn centers_gather_environment_rows() {
	assert_eq!(
		latex_to_block("\\begin{gather} a = b \\\\ longer = expression \\end{gather}"),
		vec!["       a = b       ", "longer = expression"]
	);
}

#[test]
fn splits_top_level_double_backslash_into_vertical_rows() {
	assert_eq!(latex_to_block("a \\\\ b"), vec!["a", "b"]);
}

#[test]
fn keeps_a_fraction_intact_across_argument_continuation_newlines() {
	assert_eq!(latex_to_block("\\frac{a}\n{b}"), vec![" a ", "───", " b "]);
	assert_eq!(latex_to_block("\\frac\n{a}{b}"), vec![" a ", "───", " b "]);
	assert_eq!(latex_to_block("\\frac{a}\n\\sqrt{b}"), latex_to_block("\\frac{a}\\sqrt{b}"));
	assert_eq!(latex_to_block("\\frac\na\nb"), vec![" a ", "───", " b "]);
	assert_eq!(latex_to_block("x^\n\\alpha"), vec!["xᵅ"]);
}

#[test]
fn preserves_outer_arity_when_an_unbraced_command_is_itself_an_argument() {
	let sqrt_fraction = vec!["  ┌── ", " ╲│ a ", "──────", "  b   "];
	assert_eq!(latex_to_block("\\frac\\sqrt{a}{b}"), sqrt_fraction);
	assert_eq!(latex_to_block("\\frac\\sqrt {a} {b}"), sqrt_fraction);
	assert_eq!(latex_to_block("\\frac\\sqrt\n{a}\n{b}"), sqrt_fraction);
	assert_eq!(
		latex_to_block("\\frac\\frac{a}{b}\n{c}"),
		vec!["  a  ", " ─── ", "  b  ", "─────", "  c  "]
	);
	assert_eq!(
		latex_to_block("\\frac\\frac\n{a}\n{b}\n{c}"),
		vec!["  a  ", " ─── ", "  b  ", "─────", "  c  "]
	);
	assert_eq!(latex_to_block("\\frac\\hat{a}\n{b}"), vec![" â ", "───", " b "]);
}

#[test]
fn still_treats_a_top_level_newline_as_a_row_break_outside_arguments() {
	assert_eq!(latex_to_block("y =\n\\frac{1}{2}"), vec!["y =", " 1 ", "───", " 2 "]);
	assert_eq!(
		latex_to_block("\\frac{a}{b}\n\\frac{c}{d}"),
		vec![" a ", "───", " b ", " c ", "───", " d "]
	);
	assert_eq!(latex_to_block("a\n{b+c}"), vec!["a  ", "b+c"]);
	assert_eq!(latex_to_block("x+1\n{y+2}"), vec!["x+1", "y+2"]);
}

#[test]
fn keeps_color_scope_across_a_stacked_fraction_painting_the_bar() {
	let lines = latex_to_block("\\color{red} x + \\frac{a}{b}");
	assert_eq!(
		plain(&lines).iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec!["      a", " x + ───", "      b"]
	);
	assert!(lines[0].contains("\x1b[38;"), "{lines:?}");
	assert!(lines[1].contains("\x1b[38;"), "{lines:?}");
}

#[test]
fn paints_textcolor_scoped_structural_glyphs() {
	let lines = latex_to_block("\\textcolor{red}{\\frac{a}{b}}");
	assert_eq!(plain(&lines), vec![" a ", "───", " b "]);
	assert!(lines[1].contains("\x1b[38;"), "{lines:?}");
}

#[test]
fn styles_fonts_across_a_stacked_fraction() {
	assert_eq!(latex_to_block("\\mathbf{\\frac{a}{b}}"), vec![" 𝐚 ", "───", " 𝐛 "]);
}

#[test]
fn styles_text_mode_fonts_across_a_stacked_fraction() {
	let lines = latex_to_block("\\textbf{\\frac{a}{b}}");
	assert_eq!(plain(&lines), vec![" a ", "───", " b "]);
	assert!(lines[0].contains("\x1b[1m"), "{lines:?}");
	assert!(lines[2].contains("\x1b[1m"), "{lines:?}");
}

#[test]
fn stacks_limit_operators_inside_styling_wrappers() {
	let lines = latex_to_block("\\textcolor{red}{\\sum_{i=1}^n}");
	assert_eq!(
		plain(&lines).iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
		vec![" n", " ∑", "i=1"]
	);
}
