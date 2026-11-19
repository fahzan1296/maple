//! Port of the uncovered parts of `l/test/markdown.test.ts`.
//!
//! Already covered elsewhere and deliberately not duplicated:
//! `markdown-math` (Math rendering, math start hint), `markdown-tree-wrap`
//! (wrapping), `markdown-incremental-lex`, `markdown-bplus-fast-path`
//! (transient render cache / fast path), `assistant-message-prose-color`.
//!
//! Not ported (named gaps): HTML tags (`<code>`, `<hr>`, `<blockquote>`) and
//! entity decoding, GFM's exact `~~` disambiguation, hard line breaks, and the
//! module-level LRU/streaming cache — the crate has no HTML block layout, no
//! hard-break rule, and its transient cache replaces the reference LRU.

use maple_tui::component::Component;
use maple_tui::components::markdown::{Markdown, MarkdownTheme, render_inline_markdown};
use maple_tui::strip_ansi;

fn theme() -> MarkdownTheme {
    MarkdownTheme::default()
}

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

#[test]
fn ordered_list_items_survive_inline_rendering() {
    let src = "1. Review against a base branch (PR Style)";
    let rendered = render_inline_markdown(src, &theme());
    assert_eq!(strip_ansi(&rendered), src);
}

#[test]
fn osc8_hyperlinks_survive_inline_rendering() {
    let st = "\x1b\\";
    let input = format!("\x1b]8;;file:///tmp/example.ts{st}`example.ts`\x1b]8;;{st}");
    let rendered = render_inline_markdown(&input, &theme());

    assert!(rendered.contains("\x1b]8;;file:///tmp/example.ts\x1b\\"));
    assert_eq!(strip_ansi(&rendered), "example.ts");
}

#[test]
fn strikethrough_span_is_painted() {
    let rendered = render_inline_markdown("a ~~gone~~ b", &theme());
    assert!(rendered.contains("\x1b[9mgone\x1b[0m"), "{rendered:?}");
    assert_eq!(strip_ansi(&rendered), "a gone b");
}

#[test]
fn codespan_delimiters_never_leak_into_rendered_text() {
    let rendered = render_inline_markdown("call `install()` now", &theme());
    assert!(rendered.contains("install()"));
    assert_eq!(strip_ansi(&rendered), "call install() now");
}

#[test]
fn links_are_collected_in_reading_order() {
    let mut markdown = Markdown::new();
    markdown.set_text(
        "[**bold** and _em_](https://example.com) then [two](https://example.org)",
    );
    assert_eq!(
        markdown.links(),
        vec![
            "https://example.com".to_string(),
            "https://example.org".to_string(),
        ]
    );

    let mut multiline = Markdown::new();
    multiline.set_text("[line one\nline two  \nline three](https://example.com)");
    assert_eq!(multiline.links(), vec!["https://example.com".to_string()]);
}

#[test]
fn repeated_renders_are_byte_identical() {
    let src = "# Title\n\nSome **bold** and *italic* and ~~gone~~ and `code`.\n\n- one\n- two";
    let first = render(src, 80);
    let second = render(src, 80);
    assert_eq!(first, second);
    assert!(plain(src, 80).iter().any(|line| line.contains("Title")));
    assert!(plain(src, 80).iter().any(|line| line.contains("one")));
}

#[test]
fn inline_html_code_matches_a_codespan() {
    let theme = theme();
    let html = render_inline_markdown("call <code>install()</code> now", &theme);
    let span = render_inline_markdown("call `install()` now", &theme);
    assert_eq!(strip_ansi(&html), strip_ansi(&span));
    assert!(!strip_ansi(&html).contains("<code>"));
    assert!(html.contains(&theme.code.paint("install()")));
}

#[test]
fn inline_html_bold_and_italic_paint_their_spans() {
    let theme = theme();
    let rendered = render_inline_markdown("a <b>bold</b> and <i>it</i>", &theme);
    assert!(rendered.contains(&theme.bold.paint("bold")), "{rendered:?}");
    assert!(rendered.contains(&theme.italic.paint("it")), "{rendered:?}");
    assert_eq!(strip_ansi(&rendered), "a bold and it");
}

#[test]
fn html_entities_decode_in_plain_text() {
    let rendered = render_inline_markdown(
        "Can Tap::read(&amp;self) be ok? &lt;tag&gt; &#38; &#x26;",
        &theme(),
    );
    let plain = strip_ansi(&rendered);
    assert!(plain.contains("Tap::read(&self)"), "{plain:?}");
    assert!(plain.contains("<tag>"), "{plain:?}");
    assert!(plain.contains("& &"), "{plain:?}");
    assert!(!plain.contains("&amp;") && !plain.contains("&#"), "{plain:?}");
}

#[test]
fn html_hr_renders_a_full_rule_line() {
    let lines = plain("before\n\n<hr>\n\nafter", 40);
    let border = theme().symbols.hr_char;
    assert!(
        lines
            .iter()
            .any(|line| line.len() >= 10 && line.chars().all(|ch| ch == border)),
        "{lines:?}"
    );
    assert!(lines.iter().any(|line| line.contains("before")));
    assert!(lines.iter().any(|line| line.contains("after")));
    assert!(!lines.join("\n").contains("<hr>"));
}

#[test]
fn html_blockquote_gets_the_quote_border() {
    let lines = plain("<blockquote>heads up, this is a warning</blockquote>", 60);
    let border = strip_ansi(&theme().quote_border.paint(&theme().symbols.quote_border));
    let quote = lines
        .iter()
        .find(|line| line.contains("heads up"))
        .expect("quote row");
    assert!(quote.starts_with(border.trim_end()), "{quote:?}");
    assert!(!lines.join("\n").contains("<blockquote>"));
}

#[test]
fn strikethrough_follows_the_gfm_delimiter_rules() {
    let strike = |src: &str| strip_ansi(&render_inline_markdown(src, &theme()));
    assert_eq!(strike("~~a~~ ~~a ~~b~~ c"), "a a ~~b c");
    assert_eq!(strike("~~a~~~ b~~ ~~ c~~ d~~"), "a~~~ b ~~ c~~ d~~");
    assert_eq!(strike(r"x~~y~~z ~~~~ ~~a\"), r"xy z ~~~~ ~~a\");
    assert_eq!(strike(r"~~a\~~ b~~ ~~a \\~~"), r"a~~ b a \");
    assert_eq!(strike("*a ~~b~~* [c ~~d~~](u)"), "a b c d (u)");
}

#[test]
fn hard_line_breaks_split_rows_and_keep_the_following_space() {
    let rows = plain("p  \n`c` b", 60);
    assert_eq!(rows[1], "c b");
    let backslash = plain("p\\\n*e* b", 60);
    assert_eq!(backslash[1], "e b");
    let indented = plain("p  \n   b", 60);
    assert_eq!(indented[1], "b");
}
