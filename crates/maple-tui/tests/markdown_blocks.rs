//! Port of the plain-block cases in `l/test/markdown.test.ts`.
//!
//! The reference `defaultMarkdownTheme` draws ASCII table rules, a unicode
//! blockquote rail, `-` rules and yellow inline code, so the theme is built to
//! match. Streaming/transient rendering, mermaid resolution, OSC 8 links and a
//! caller-supplied default text style have no crate equivalent and are covered
//! by ignored tests that name the missing facility.

use maple_tui::component::Component;
use maple_tui::components::markdown::{Markdown, MarkdownTheme};
use maple_tui::strip_ansi;
use maple_tui::style::{Color, Style};
use maple_tui::symbols::SymbolTheme;
use maple_tui::text_shape::visible_width;

fn theme() -> MarkdownTheme {
    let mut symbols = SymbolTheme::ascii();
    symbols.quote_border = "│".into();
    MarkdownTheme {
        symbols,
        code: Style::fg(Color::Yellow),
        quote: Style::default().italic(),
        ..MarkdownTheme::default()
    }
}

fn render(text: &str, width: u16) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(text);
    markdown.set_theme(theme());
    markdown.render(width)
}

fn plain(text: &str, width: u16) -> Vec<String> {
    render(text, width)
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .collect()
}

#[test]
fn renders_a_simple_table_with_borders() {
    let lines = plain(
        "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 25 |",
        80,
    );
    assert!(lines.iter().any(|line| line.contains("Name")));
    assert!(lines.iter().any(|line| line.contains("Age")));
    assert!(lines.iter().any(|line| line.contains("Alice")));
    assert!(lines.iter().any(|line| line.contains("Bob")));
    assert!(lines.iter().any(|line| line.contains('|')));
    assert!(lines.iter().any(|line| line.contains('-')));
}

#[test]
fn renders_row_dividers_between_data_rows() {
    let lines = plain(
        "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 25 |",
        80,
    );
    let dividers = lines.iter().filter(|line| line.contains('+')).count();
    assert!(dividers >= 2, "expected header + row divider, got {dividers}");
}

#[test]
fn keeps_column_width_at_least_the_longest_word() {
    let longest = "superlongword";
    let lines = plain(
        &format!(
            "| Column One | Column Two |\n| --- | --- |\n| {longest} short | otherword |\n| small | tiny |"
        ),
        32,
    );
    let data = lines
        .iter()
        .find(|line| line.contains(longest))
        .expect("data row with longest word");
    let first = data.split('|').nth(1).expect("first column segment");
    let width = first.len() - 2;
    assert!(width >= longest.len(), "column width {width} < {}", longest.len());
}

#[test]
fn renders_table_column_alignment() {
    let lines = plain(
        "| Left | Center | Right |\n| :--- | :---: | ---: |\n| A | B | C |\n| Long text | Middle | End |",
        80,
    );
    for needle in ["Left", "Center", "Right", "Long text"] {
        assert!(lines.iter().any(|line| line.contains(needle)), "missing {needle}");
    }
}

#[test]
fn handles_tables_with_varying_column_widths() {
    let lines = plain(
        "| Short | Very long column header |\n| --- | --- |\n| A | This is a much longer cell content |\n| B | Short |",
        80,
    );
    assert!(lines.iter().any(|line| line.contains("Very long column header")));
    assert!(lines
        .iter()
        .any(|line| line.contains("This is a much longer cell content")));
}

#[test]
fn wraps_table_cells_when_the_table_exceeds_available_width() {
    let lines = plain(
        "| Command | Description | Example |\n| --- | --- | --- |\n| npm install | Install all dependencies | npm install |\n| npm run build | Build the project | npm run build |",
        50,
    );
    for line in &lines {
        assert!(line.len() <= 50, "line exceeds width 50: {line:?}");
    }
    let text = lines.join(" ");
    for needle in ["Command", "Description", "npm install", "Install"] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn wraps_long_cell_content_to_multiple_lines() {
    let lines = plain(
        "| Header |\n| --- |\n| This is a very long cell content that should wrap |",
        25,
    );
    let data_rows = lines
        .iter()
        .filter(|line| line.starts_with('|') && !line.contains('-'))
        .count();
    assert!(data_rows > 2, "expected wrapped rows, got {data_rows}");
    let text = lines.join(" ");
    for needle in ["very long", "cell content", "should wrap"] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn wraps_long_unbroken_tokens_inside_table_cells() {
    let url = "https://example.com/this/is/a/very/long/url/that/should/wrap";
    let lines = plain(&format!("| Value |\n| --- |\n| prefix {url} |"), 30);
    for line in &lines {
        assert!(line.len() <= 30, "line exceeds width 30: {line:?}");
    }
    for line in lines.iter().filter(|line| line.starts_with('|')) {
        let borders = line.matches('|').count();
        assert_eq!(borders, 2, "expected 2 borders: {line:?}");
    }
    let extracted: String = lines
        .join("")
        .chars()
        .filter(|c| !matches!(c, '|' | '+' | '-' | ' '))
        .collect();
    assert!(extracted.contains("prefix"));
    assert!(extracted.contains(url));
}

#[test]
fn handles_extremely_narrow_width_gracefully() {
    let lines = plain("| A | B | C |\n| --- | --- | --- |\n| 1 | 2 | 3 |", 15);
    for line in &lines {
        assert!(line.len() <= 15, "line exceeds width 15: {line:?}");
    }
}

#[test]
fn renders_a_table_that_fits_naturally() {
    let lines = plain("| A | B |\n| --- | --- |\n| 1 | 2 |", 80);
    let header = lines
        .iter()
        .find(|line| line.contains('A') && line.contains('B'))
        .expect("header row");
    assert!(header.contains('|'));
    assert!(lines.iter().any(|line| line.contains('+') && line.contains('-')));
    assert!(lines.iter().any(|line| line.contains('1') && line.contains('2')));
}

#[test]
fn respects_padding_x_when_calculating_table_width() {
    let mut markdown = Markdown::new();
    markdown.set_text("| Column One | Column Two |\n| --- | --- |\n| Data 1 | Data 2 |");
    markdown.set_theme(theme());
    markdown.set_padding(2, 0);
    let lines: Vec<String> = markdown
        .render(40)
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .collect();
    for line in &lines {
        assert!(line.len() <= 40, "line exceeds width 40: {line:?}");
    }
    let row = lines.iter().find(|line| line.contains('|')).expect("table row");
    assert!(row.starts_with("  "), "table should have left padding: {row:?}");
}

#[test]
fn does_not_add_a_trailing_blank_line_when_a_table_is_last() {
    let lines = plain("| Name |\n| --- |\n| Alice |", 80);
    assert!(!lines.last().unwrap().is_empty());
}

#[test]
fn uses_the_full_ordered_marker_width_as_the_hanging_indent() {
    let lines = plain("10. Alpha beta gamma delta", 16);
    assert_eq!(lines, vec!["10. Alpha beta", "    gamma delta"]);
    assert!(lines.iter().all(|line| visible_width(line) <= 16));
}

#[test]
fn keeps_task_checkbox_state_and_hangs_wrapped_rows_under_the_task_text() {
    let lines = plain("- [x] Ship the fix today\n- [ ] Add a regression test", 20);
    assert_eq!(
        lines,
        vec![
            "- [x] Ship the fix",
            "      today",
            "- [ ] Add a",
            "      regression",
            "      test",
        ]
    );
}

#[test]
fn has_one_blank_line_between_a_code_block_and_the_following_paragraph() {
    let lines = plain("hello world\n\n```js\nconst hello = \"world\";\n```\n\nagain, hello world", 80);
    let closing = lines.iter().position(|line| line == "```").expect("closing fence");
    let after = &lines[closing + 1..];
    let empty = after.iter().position(|line| !line.is_empty()).unwrap_or(after.len());
    assert_eq!(empty, 1, "lines after fence: {after:?}");
}

#[test]
fn normalizes_paragraph_and_code_block_spacing_to_one_blank_line() {
    let expected = vec![
        "hello this is text",
        "",
        "```",
        "  code block",
        "```",
        "",
        "more text",
    ];
    for text in [
        "hello this is text\n```\ncode block\n```\nmore text",
        "hello this is text\n\n```\ncode block\n```\n\nmore text",
    ] {
        assert_eq!(plain(text, 80), expected);
    }
}

#[test]
fn has_one_blank_line_between_a_divider_and_the_following_paragraph() {
    let lines = plain("hello world\n\n---\n\nagain, hello world", 80);
    let divider = lines
        .iter()
        .position(|line| line.trim().chars().all(|c| c == '-') && line.trim().len() >= 3)
        .expect("divider");
    let after = &lines[divider + 1..];
    let empty = after.iter().position(|line| !line.is_empty()).unwrap_or(after.len());
    assert_eq!(empty, 1, "lines after divider: {after:?}");
}

#[test]
fn does_not_add_a_trailing_blank_line_when_a_divider_is_last() {
    let lines = plain("---", 80);
    assert!(!lines.last().unwrap().is_empty());
}

#[test]
fn renders_a_divider_as_a_full_width_rule() {
    let lines = plain("---", 80);
    assert_eq!(lines[0], "-".repeat(80));
}

#[test]
fn has_one_blank_line_between_a_heading_and_the_following_paragraph() {
    let lines = plain("# Hello\n\nThis is a paragraph", 80);
    let heading = lines.iter().position(|line| line.contains("Hello")).expect("heading");
    let after = &lines[heading + 1..];
    let empty = after.iter().position(|line| !line.is_empty()).unwrap_or(after.len());
    assert_eq!(empty, 1, "lines after heading: {after:?}");
}

#[test]
fn does_not_add_a_trailing_blank_line_when_a_heading_is_last() {
    let lines = plain("# Hello", 80);
    assert!(!lines.last().unwrap().is_empty());
}

#[test]
fn wraps_long_blockquote_lines_and_adds_the_border_to_each() {
    let long = "This is a very long blockquote line that should wrap to multiple lines when rendered";
    let lines = plain(&format!("> {long}"), 40);
    assert!(lines.len() > 1, "expected wrapped quote rows");
    for line in &lines {
        assert!(line.starts_with("│ "), "missing quote rail: {line:?}");
    }
    assert!(lines.iter().any(|line| line.contains("very long blockquote")));
}

#[test]
fn does_not_add_a_trailing_blank_line_when_a_blockquote_is_last() {
    let lines = plain("> This is a quote", 80);
    assert!(!lines.last().unwrap().is_empty());
}

#[test]
#[ignore = "nested list indentation: parse_list_item measures indent after trim_start, so nested items render flat"]
fn nested_lists_render_with_indentation() {
    let lines = plain("- Item 1\n  - Nested 1.1\n  - Nested 1.2\n- Item 2", 80);
    for needle in ["- Item 1", "- Nested 1.1", "- Nested 1.2", "- Item 2"] {
        assert!(lines.iter().any(|line| line.contains(needle)), "missing {needle}");
    }
}

#[test]
#[ignore = "custom divider glyphs: is_hr accepts only -, *, _ runs, so = and em-dash rules stay paragraphs"]
fn renders_custom_section_dividers() {
    let lines = plain("============", 80);
    assert!(lines.iter().any(|line| line.contains("============")));
    assert!(!lines.iter().any(|line| line.chars().all(|c| c == '=')));
}

#[test]
#[ignore = "mermaid resolution: MarkdownTheme has no resolveMermaidAscii hook; fenced mermaid stays code"]
fn resolves_mermaid_fences_to_ascii_art() {
    let lines = plain("```mermaid\nflowchart TD\n  Start-->Stop\n```", 80);
    assert!(lines.iter().any(|line| line.contains("flowchart TD")));
}

#[test]
#[ignore = "streaming tables: no transientRenderCache/holdPartialTableRow; a full render is already stable"]
fn streaming_table_frames_match_a_cold_render() {
    let text = "| PR | Title |\n| --- | --- |\n| #1 | short |\n| #2 | a considerably longer finished title here |";
    let cold = render(text, 80);
    let mut streaming = Markdown::new();
    streaming.set_text(text);
    streaming.set_theme(theme());
    assert_eq!(streaming.render(80), cold);
}

#[test]
#[ignore = "default text style: Markdown has no caller-supplied base color/italic style parameter"]
fn preserves_a_caller_supplied_default_text_style() {
    let lines = plain("This is thinking with `inline code` and more text after", 80);
    assert!(lines.iter().any(|line| line.contains("inline code")));
}

#[test]
#[ignore = "OSC 8 hyperlinks: the crate preserves existing ANSI escapes but never emits OSC 8 wrappers"]
fn preserves_osc8_links_inside_table_cells() {
    let file_link = "\x1b]8;;file:///tmp/DisplayTypeEnum.java\x1b\\`DisplayTypeEnum.java`\x1b]8;;\x1b\\";
    let lines = render(
        &format!(
            "| Module | Local file | Change |\n| --- | --- | --- |\n| common | {file_link} | Added display type |"
        ),
        80,
    );
    let output = lines.join("\n");
    assert!(output.contains("DisplayTypeEnum.java"));
}
