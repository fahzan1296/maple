//! Port of `l/test/markdown-math.test.ts`.
//!
//! The crate's `Markdown` is a `Component` (`new`/`set_text`/`set_theme`/`render`)
//! where the reference constructs `new Markdown(md, 0, 0, theme)` and calls
//! `render(width)`; `renderInlineMarkdown` maps to `render_inline_markdown`.
//! The reference's `TERMINAL.trueColor` is a mutable global, while the crate's
//! `latex_to_unicode` always renders truecolor, so the truecolor assertions use
//! the crate's constant red.

use maple_tui::ansi_scan::strip_ansi;
use maple_tui::component::Component;
use maple_tui::components::markdown::{render_inline_markdown, Markdown, MarkdownTheme};
use maple_tui::terminal_capabilities::set_terminal_text_sizing;

const RED: &str = "\x1b[38;2;255;0;0m";

fn render_lines(md: &str, width: u16) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(md);
    markdown.set_theme(MarkdownTheme::default());
    markdown
        .render(width)
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn converts_inline_math_inside_prose() {
    let lines = render_lines("the area is $A = \\pi r^2$ exactly", 100);
    assert_eq!(lines, vec!["the area is A = π r² exactly"]);
}

#[test]
fn converts_subscripts_and_superscripts_without_markdown_mangling() {
    let lines = render_lines("energy $x_i^2 + y_j^2$ done", 100);
    assert_eq!(lines, vec!["energy xᵢ² + yⱼ² done"]);
}

#[test]
fn renders_an_own_line_matrix_block_as_a_bracketed_grid() {
    let lines = render_lines("$$\n\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}\n$$", 100);
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("⎡"));
    assert!(lines[lines.len() - 1].ends_with("⎦"));
    let joined: String = lines
        .join("")
        .chars()
        .filter(|c| !c.is_whitespace() && !"⎡⎤⎢⎥⎣⎦".contains(*c))
        .collect();
    assert_eq!(joined, "abcd");
}

#[test]
fn stacks_a_display_quadratic_formula_with_a_drawn_radical() {
    let lines = render_lines("\\[\nx = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}\n\\]", 100);
    let bar_row = lines.iter().position(|line| line.contains("x =")).expect("bar row");
    assert!(bar_row > 1);
    assert!(lines[bar_row].contains("───"));
    assert!(lines[bar_row - 1].contains("-b ± ╲│ b² - 4ac"));
    assert!(lines[bar_row - 2].contains("┌"));
    assert!(lines[bar_row + 1].contains("2a"));
}

#[test]
fn stacks_a_single_line_display_fraction_in_a_paragraph() {
    let lines = render_lines("Here:\n\n$$\\frac{a+b}{c}$$\n\nDone.", 100);
    let bar_row = lines.iter().position(|line| line.contains("─")).expect("bar row");
    assert!(bar_row > 0);
    assert!(lines[bar_row - 1].contains("a+b"));
    assert!(lines[bar_row + 1].contains("c"));
}

#[test]
fn keeps_display_math_inside_a_list_item_multi_line() {
    let lines = render_lines("- result:\n\n  $$\n  \\begin{bmatrix} a \\\\ b \\end{bmatrix}\n  $$", 100);
    let open_row = lines.iter().position(|line| line.contains("⎡ a"));
    let close_row = lines.iter().position(|line| line.contains("b ⎦"));
    assert!(open_row.is_some());
    assert!(close_row.is_some());
    assert!(close_row.unwrap() > open_row.unwrap());
}

#[test]
fn leaves_math_inside_an_inline_code_span_literal() {
    let lines = render_lines("use `$x^2$` literally and $y^2$ as math", 100);
    assert_eq!(lines, vec!["use $x^2$ literally and y² as math"]);
}

#[test]
fn leaves_math_inside_a_fenced_code_block_literal() {
    let lines = render_lines("```\n$a$ and $b$\n```", 100);
    assert!(lines.iter().any(|line| line.contains("$a$ and $b$")));
}

#[test]
fn does_not_convert_currency_style_dollars() {
    let lines = render_lines("it costs $5 and $10 total", 100);
    assert_eq!(lines, vec!["it costs $5 and $10 total"]);
}

#[test]
fn closes_a_span_at_the_first_unescaped_delimiter() {
    let paren = render_lines(r"prose \(x \\) y\) end", 100);
    assert_eq!(paren, vec!["prose x  ) y end"]);
    let dollar = render_lines(r"$$a \$$ b$$", 100);
    assert_eq!(dollar, vec!["a $ b"]);
}

#[test]
fn render_inline_markdown_converts_inline_math() {
    let out = strip_ansi(&render_inline_markdown(
        "energy $E=mc^2$ here",
        &MarkdownTheme::default(),
    ));
    assert_eq!(out, "energy E=mc² here");
}

#[test]
fn render_inline_markdown_handles_a_top_level_display_math_token() {
    let out = strip_ansi(&render_inline_markdown(
        "$$E = mc^2$$",
        &MarkdownTheme::default(),
    ));
    assert_eq!(out, "E = mc²");
}

#[test]
fn stacks_a_display_fraction_across_multiple_lines() {
    let lines = render_lines("$$\n\\frac{a+b}{c}\n$$", 100);
    let bar_row = lines.iter().position(|line| line.contains("─")).expect("bar row");
    assert!(bar_row > 0);
    assert!(lines[bar_row - 1].contains("a+b"));
    assert!(lines[bar_row + 1].contains("c"));
}

#[test]
fn stacks_fractions_inside_a_bare_equation_block() {
    let lines = render_lines("\\begin{equation}\nx = \\frac{a+b}{c}\n\\end{equation}", 100);
    let bar_row = lines.iter().position(|line| line.contains("─")).expect("bar row");
    assert!(bar_row >= 0);
    assert!(lines[bar_row].contains("x ="));
    assert!(lines[bar_row - 1].contains("a+b"));
    assert!(lines[bar_row + 1].contains("c"));
}

#[test]
fn preserves_ansi_styles_through_markdown_math_wrappers() {
    let markdown = "$\\boxed{\\textcolor{red}{x}}+y_{\\textit{word}}+\\textcolor{red}{\\sqrt[n]{x}}$\n\n$$\\textbf{\\frac{a}{b}}$$";
    let mut renderer = Markdown::new();
    renderer.set_text(markdown);
    renderer.set_theme(MarkdownTheme::default());
    let rendered = renderer.render(100).join("\n");

    assert!(rendered.contains(&format!("[{RED}x\x1b[39m]")), "boxed red: {rendered:?}");
    assert!(rendered.contains("y_(\x1b[3mword\x1b[23m)"), "italic subscript: {rendered:?}");
    assert!(rendered.contains(&format!("{RED}ⁿ√x\x1b[39m")), "colored radical: {rendered:?}");
    assert!(rendered.contains("\x1b[1ma\x1b[22m"), "bold numerator: {rendered:?}");
    assert!(rendered.contains("\x1b[1mb\x1b[22m"), "bold denominator: {rendered:?}");
}

#[test]
fn sized_headings_draw_the_same_bytes_for_styled_and_plain_inline_math() {
    set_terminal_text_sizing(true);
    let plain = render_lines("# x", 100).join("\n");
    let styled = render_lines(r"# $\textcolor{red}{x}$", 100).join("\n");
    assert_eq!(styled, plain);
}
