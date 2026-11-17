//! Port of `l/test/markdown-table-align.test.ts`.
//!
//! The reference theme draws ASCII `|`/`-`/`+` table glyphs; the crate's default
//! `MarkdownTheme` uses unicode box drawing, so the theme is built with
//! `SymbolTheme::ascii()` to keep the cell geometry identical.

use maple_tui::component::Component;
use maple_tui::components::markdown::{Markdown, MarkdownTheme};
use maple_tui::strip_ansi;
use maple_tui::symbols::SymbolTheme;

fn theme() -> MarkdownTheme {
    MarkdownTheme {
        symbols: SymbolTheme::ascii(),
        ..MarkdownTheme::default()
    }
}

fn render(text: &str, width: u16) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(text);
    markdown.set_theme(theme());
    markdown.render(width)
}

fn table_cells(text: &str, width: u16) -> Vec<Vec<String>> {
    render(text, width)
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .filter(|line| line.starts_with('|'))
        .map(|row| {
            row.split('|')
                .skip(1)
                .take(row.matches('|').count().saturating_sub(1))
                .map(|field| field[1..field.len() - 1].to_string())
                .collect()
        })
        .collect()
}

#[test]
fn places_header_and_body_cells_according_to_the_column_alignment_marker() {
    let source = "| A | B | C |\n| :--- | ---: | :---: |\n| Alpha | 1200 | ripe |\n| a | 1 | ok |";
    let rows = table_cells(source, 80);
    assert_eq!(rows.len(), 3);

    let header = &rows[0];
    let first_row = &rows[1];
    let second_row = &rows[2];

    assert_eq!(header[0], "A    ");
    assert_eq!(second_row[0], "a    ");
    assert_eq!(header[1], "   B");
    assert_eq!(second_row[1], "   1");
    assert_eq!(header[2], " C  ");
    assert_eq!(second_row[2], " ok ");
    assert_eq!(first_row, &["Alpha".to_string(), "1200".to_string(), "ripe".to_string()]);
}

#[test]
fn aligns_every_wrapped_line_of_a_cell_to_its_column() {
    let rows = table_cells("| L | R |\n| :--- | ---: |\n| abc | alpha beta |", 19);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], ["L  ".to_string(), "        R".to_string()]);
    assert_eq!(rows[1], ["abc".to_string(), "    alpha".to_string()]);
    assert_eq!(rows[2], ["   ".to_string(), "     beta".to_string()]);

    for row in &rows {
        let right = &row[1];
        assert_eq!(right, right.trim_end());
        assert!(!right.trim().is_empty());
    }
}
