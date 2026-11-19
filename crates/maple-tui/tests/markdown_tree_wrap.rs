//! Port of `l/test/markdown-tree-wrap.test.ts`.
//!
//! Deviations:
//! - The reference's caller-supplied `defaultTextStyle` has no crate API; the
//!   carry cases use raw ANSI in the source (or `set_text_transform`) instead,
//!   and `Style::paint` closes with `\x1b[0m` rather than per-attribute codes.
//! - `Markdown::render` pads rows to the viewport width, so assertions compare
//!   `trim_end()` rows like the reference's `stripVTControlCharacters(...).trimEnd()`.

use maple_tui::components::markdown::{Markdown, MarkdownTheme};
use maple_tui::strip_ansi;
use maple_tui::style::{Color, Style};
use maple_tui::component::Component;
use maple_tui::text_shape::{visible_width, wrap_text_with_ansi};

const WIDTH: u16 = 40;

fn render(text: &str, width: u16) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(text);
    markdown.render(width)
}

fn plain(text: &str, width: u16) -> Vec<String> {
    render(text, width)
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .collect()
}

fn plain_rows(raw: &[String]) -> Vec<String> {
    raw.iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .collect()
}

#[test]
fn hangs_wide_cjk_nodes_under_the_guide_rail() {
    const NODE: &str = "가나다라 마바사아 자차카타 파하가나 다라마바 사자차카";
    let raw = render(&format!("├── {NODE}"), WIDTH);

    for line in &raw {
        assert!(visible_width(line) <= WIDTH as usize, "row overflows: {line:?}");
    }
    // Concatenating the post-prefix cells reproduces the wrapped node, which
    // proves the wrapper consumed the break spaces instead of duplicating them.
    let rejoined: String = raw
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .map(|line| line.chars().skip(4).collect::<String>())
        .collect::<Vec<_>>()
        .join("")
        .replace(' ', "");
    assert_eq!(rejoined, NODE.replace(' ', ""));

    let rows = plain_rows(&raw);
    assert!(rows[0].starts_with("├── 가나다라"), "{rows:?}");
    assert_eq!(rows[0], "├── 가나다라 마바사아 자차카타 파하가나");
    assert!(rows[1].starts_with("│   "), "{rows:?}");
    assert!(rows[1].chars().nth(4) != Some(' '), "{rows:?}");
    assert_eq!(rows[1], "│   다라마바 사자차카");
}

#[test]
fn preserves_nested_prefixes_and_flushes_terminal_branches() {
    let doc = "│   └── delta echo foxtrot golf hotel india juliet\n\
               └── alpha bravo charlie delta echo foxtrot golf\n\
               ╰── alpha bravo charlie delta echo foxtrot golf";
    let raw = render(doc, WIDTH);
    let rows = plain_rows(&raw);
    assert_eq!(rows[0], "│   └── delta echo foxtrot golf hotel");
    assert_eq!(rows[1], "│       india juliet");
    // Terminal-branch prefixes hang on blank rails, never on `│`.
    assert_eq!(rows[2], "└── alpha bravo charlie delta echo");
    assert_eq!(rows[3], "    foxtrot golf");
    assert_eq!(rows[4], "╰── alpha bravo charlie delta echo");
    assert_eq!(rows[5], "    foxtrot golf");
    for line in raw.iter().skip(2) {
        assert!(!strip_ansi(line).contains('│'), "leaked rail: {line:?}");
    }
}

#[test]
fn leaves_non_tree_prose_to_the_plain_wrapper() {
    let text = "This is an ordinary paragraph with no tree glyphs but enough words to overflow the narrow column";
    let expected: Vec<String> = wrap_text_with_ansi(text, WIDTH as usize)
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    assert_eq!(plain(text, WIDTH), expected);
}

#[test]
fn does_not_treat_a_dash_only_start_as_a_tree() {
    let text = "── alpha bravo charlie delta echo foxtrot golf hotel india";
    let expected: Vec<String> = wrap_text_with_ansi(text, WIDTH as usize)
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    let rows = plain(text, WIDTH);
    assert_eq!(rows, expected);
    assert!(rows.len() >= 2);
    for line in &rows[1..] {
        let first = line.chars().next().unwrap();
        assert!(first != ' ' && first != '│', "unexpected hang: {line:?}");
    }
}

#[test]
fn renders_fitting_tree_lines_line_for_line() {
    let rows = plain("├── alpha\n│   └── beta\n└── gamma", WIDTH);
    assert_eq!(rows, vec!["├── alpha", "│   └── beta", "└── gamma"]);
}

#[test]
fn keeps_column_zero_wrap_inside_fenced_code() {
    let code_line = "├── alpha bravo charlie delta echo foxtrot golf hotel india";
    let raw = render(&format!("```\n{code_line}\n```"), WIDTH);
    let rows = plain_rows(&raw);
    assert_eq!(rows[0], "```");
    assert_eq!(rows[rows.len() - 1], "```");

    let tree_row = rows.iter().position(|line| line.contains("├──")).expect("tree row");
    assert!(tree_row > 0);
    let continuation = &rows[tree_row + 1];
    assert!(tree_row + 1 < rows.len() - 1, "no continuation before fence");
    assert!(!continuation.is_empty());
    let first = continuation.chars().next().unwrap();
    assert!(first != ' ' && first != '│', "code wrap hung: {continuation:?}");

    for line in &raw {
        assert!(visible_width(line) <= WIDTH as usize, "row overflows: {line:?}");
    }
}

#[test]
fn carries_an_open_bold_span_onto_the_continuation_row() {
    let raw = render("├── aaaa bbbb **cccc dddd eeee ffff gggg hhhh**", WIDTH);
    let rows = plain_rows(&raw);
    assert!(rows.len() >= 2);
    assert!(rows[1].starts_with("│   hhhh"), "{rows:?}");

    let continuation = &raw[1];
    let bold_open = continuation.find("\x1b[1m").expect("bold open on continuation");
    assert!(bold_open < continuation.find("hhhh").unwrap(), "{continuation:?}");
}

#[test]
fn ends_guide_styling_before_continuation_text() {
    let raw = render(
        "\x1b[2m├── \x1b[22malpha bravo charlie delta echo foxtrot golf hotel india",
        WIDTH,
    );
    let rows = plain_rows(&raw);
    assert_eq!(rows[1], "│   foxtrot golf hotel india");

    let continuation = &raw[1];
    assert!(continuation.starts_with("\x1b[2m│   "), "{continuation:?}");
    let text_at = continuation.find("foxtrot").unwrap();
    assert!(continuation[..text_at].contains("\x1b[22m"), "{continuation:?}");
    assert!(!continuation[text_at..].contains("\x1b[2m"), "{continuation:?}");
}

#[test]
fn hangs_inside_a_blockquote_after_the_border() {
    let rows = plain("> ├── alpha bravo charlie delta echo foxtrot golf hotel india", WIDTH);
    assert!(rows.len() >= 2);
    assert!(rows[0].starts_with("│ ├── alpha"), "{rows:?}");
    let continuations: Vec<&String> = rows[1..].iter().filter(|line| !line.is_empty()).collect();
    assert!(!continuations.is_empty());
    for line in continuations {
        assert!(line.starts_with("│ │   "), "{line:?}");
        assert!(line.chars().nth(6) != Some(' '), "{line:?}");
    }
}

#[test]
fn keeps_plain_wrap_for_rail_glyphs_without_a_branch_connector() {
    for text in [
        "│ is the Unicode vertical box drawing glyph used for rails in terminal trees",
        "├ marks a branch point in a tree diagram and has no horizontal connector here",
    ] {
        let expected: Vec<String> = wrap_text_with_ansi(text, WIDTH as usize)
            .iter()
            .map(|line| line.trim_end().to_string())
            .collect();
        let rows = plain(text, WIDTH);
        assert_eq!(rows, expected, "{text:?}");
        assert!(rows.len() >= 2, "{text:?}");
        for line in &rows[1..] {
            let first = line.chars().next().unwrap();
            assert!(first != ' ' && first != '│', "unexpected hang: {line:?}");
        }
    }
}

#[test]
fn falls_back_when_fewer_than_eight_content_cells_remain() {
    let text = "├── alpha bravo charlie delta echo foxtrot golf hotel india";

    // Width 10 leaves 6 content cells: below the minimum, plain wrap wins.
    let expected: Vec<String> = wrap_text_with_ansi(text, 10)
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    let narrow = plain(text, 10);
    assert_eq!(narrow, expected);
    assert!(narrow.len() >= 2);
    for line in &narrow[1..] {
        let first = line.chars().next().unwrap();
        assert!(first != ' ' && first != '│', "unexpected hang: {line:?}");
    }

    // Width 12 leaves exactly 8 content cells: the hanging wrap applies.
    let hung = plain(text, 12);
    assert_eq!(hung[0], "├── alpha");
    assert!(hung.len() >= 2);
    for line in &hung[1..] {
        assert!(line.starts_with("│   "), "{line:?}");
        assert!(line.chars().nth(4) != Some(' '), "{line:?}");
    }
}

#[test]
fn re_renders_byte_identically_after_a_width_round_trip() {
    let doc = "├── alpha bravo charlie delta echo foxtrot golf\n\
               └── hotel india juliet kilo lima mike november";
    let mut markdown = Markdown::new();
    markdown.set_text(doc);

    let first = markdown.render(44);
    assert_eq!(
        plain_rows(&first),
        vec![
            "├── alpha bravo charlie delta echo foxtrot",
            "│   golf",
            "└── hotel india juliet kilo lima mike",
            "    november",
        ]
    );

    let wide = markdown.render(80);
    assert_eq!(
        plain_rows(&wide),
        vec![
            "├── alpha bravo charlie delta echo foxtrot golf",
            "└── hotel india juliet kilo lima mike november",
        ]
    );

    assert_eq!(markdown.render(44), first);
    let mut fresh = Markdown::new();
    fresh.set_text(doc);
    assert_eq!(fresh.render(44), first);
}

#[test]
fn replays_an_unclosed_style_from_an_earlier_line() {
    let raw = render(
        "aaa \x1b[3mitalic tail\n├── alpha bravo charlie delta echo foxtrot golf hotel india",
        WIDTH,
    );
    let rows = plain_rows(&raw);
    assert_eq!(rows.len(), 3);
    assert!(rows[1].starts_with("├── alpha"), "{rows:?}");
    assert!(rows[2].starts_with("│   foxtrot"), "{rows:?}");
    assert!(raw[1].starts_with("\x1b[3m├── "), "{:?}", raw[1]);

    let continuation = &raw[2];
    let hang_at = continuation.find("│   ").expect("hang");
    assert!(hang_at > 0);
    assert_eq!(&continuation[..hang_at], "\x1b[3m");
}

#[test]
fn does_not_leak_styles_across_a_full_sgr_reset() {
    let raw = render(
        "aaa \x1b[1mbold\x1b[0m\x1b[3mrest and filler\n\
         ├── alpha bravo charlie delta echo foxtrot golf hotel india",
        WIDTH,
    );
    let rows = plain_rows(&raw);
    assert_eq!(rows.len(), 3);
    assert!(rows[1].starts_with("├── alpha"), "{rows:?}");
    assert!(rows[2].starts_with("│   foxtrot"), "{rows:?}");

    for row in &raw[1..] {
        assert!(!row.contains("\x1b[1m"), "dead pre-reset style re-played: {row:?}");
        assert!(!row.contains("\x1b[0m"), "reset leaked: {row:?}");
    }
    assert!(raw[1].starts_with("\x1b[3m├── "), "{:?}", raw[1]);
    let continuation = &raw[2];
    let hang_at = continuation.find("│   ").expect("hang");
    assert!(hang_at > 0);
    assert_eq!(&continuation[..hang_at], "\x1b[3m");
}

#[test]
fn keeps_a_leading_styled_span_off_the_continuation_guides() {
    let mut theme = MarkdownTheme::default();
    theme.bold = Style::fg(Color::Magenta).bold();
    let doc = "├── **alpha bravo**: charlie delta echo foxtrot golf hotel india";

    let mut markdown = Markdown::new();
    markdown.set_text(doc);
    markdown.set_theme(theme);
    let raw = markdown.render(WIDTH);
    let rows = plain_rows(&raw);
    assert_eq!(rows[0], "├── alpha bravo: charlie delta echo");
    assert_eq!(rows[1], "│   foxtrot golf hotel india");
    // Rust paints the span as one combined SGR and closes with a full reset;
    // the wrapper re-emits the row's live style, so the open appears twice.
    assert!(raw[0].starts_with("├── \x1b[35;1m"), "{:?}", raw[0]);
    assert!(raw[0].contains("\x1b[35;1malpha bravo"), "{:?}", raw[0]);
    assert!(raw[1].starts_with("│   "), "{:?}", raw[1]);
    assert!(!raw[1].contains("\x1b["), "span styling reached the rail: {:?}", raw[1]);

    // With a prose color the rails wear it, as on the first row, and only it.
    let mut colored = Markdown::new();
    colored.set_text(doc);
    colored.set_theme(MarkdownTheme {
        bold: Style::fg(Color::Magenta).bold(),
        ..MarkdownTheme::default()
    });
    colored.set_text_transform(Some(Box::new(|text: &str| {
        format!("\x1b[31m{text}\x1b[39m")
    })));
    let colored_rows = colored.render(WIDTH);
    let continuation = &colored_rows[1];
    let hang_at = continuation.find("│   ").expect("hang");
    assert_eq!(&continuation[..hang_at], "\x1b[31m", "{continuation:?}");
    assert!(!continuation.contains("\x1b[1m"), "{continuation:?}");
    assert_eq!(
        strip_ansi(continuation).trim_end(),
        "│   foxtrot golf hotel india"
    );
}
