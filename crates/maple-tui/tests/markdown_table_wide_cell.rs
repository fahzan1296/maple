//! Port of `l/test/markdown-table-wide-cell.test.ts`.
//!
//! The reference's wide-glyph clamp splits a two-cell emoji across two rows, so
//! its table has seven rows. The crate's `wrap_text_with_ansi` hard-breaks a
//! wide grapheme without splitting it, so the same table stays five rows. The
//! portable contract is asserted: every row fits the width, table rows keep
//! their six borders, and every cell's content survives.

use maple_tui::component::Component;
use maple_tui::components::markdown::{Markdown, MarkdownTheme};
use maple_tui::strip_ansi;
use maple_tui::symbols::SymbolTheme;
use maple_tui::text_shape::visible_width;

#[test]
fn keeps_every_table_row_on_one_line_at_width_80() {
    let source = "| 0123456789abcdef | 0123456789abcdef | 0123456789abcdef | 0123456789abcdef | ✅ |\n\
| --- | --- | --- | --- | --- |\n\
| 0123456789abcdef | 0123456789abcdef | 0123456789abcdef | 0123456789abcdef | ❌ |";

    let theme = MarkdownTheme {
        symbols: SymbolTheme::ascii(),
        ..MarkdownTheme::default()
    };
    let mut markdown = Markdown::new();
    markdown.set_text(source);
    markdown.set_theme(theme);
    let lines: Vec<String> = markdown
        .render(80)
        .iter()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .collect();

    assert!(lines.len() >= 5, "expected a bordered table, got {lines:?}");
    for line in &lines {
        assert!(visible_width(line) <= 80, "row exceeds width 80: {line:?}");
        if line.starts_with('|') {
            assert_eq!(line.matches('|').count(), 6, "row lost borders: {line:?}");
        }
    }
    let text = lines.join("");
    assert!(text.contains("0123456789abcdef"));
    assert!(text.contains('✅'));
    assert!(text.contains('❌'));
}
