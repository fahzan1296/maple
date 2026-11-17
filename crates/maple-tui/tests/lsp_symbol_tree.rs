//! Port of `l/test/lsp-symbol-tree.test.ts`.
//!
//! Deviations:
//! - The crate renders plain rows (no ANSI card header), so the header is
//!   asserted by its meta suffix rather than by a stripped status line.
//! - Collapsed-symbol capping is exercised separately from the expanded tree.

use maple_tui::tools::lsp::{parse_symbols, render_result_expanded, render_symbols};
use maple_tui::theme::default_theme;

fn transcript() -> String {
    [
        "Symbols in src/a.ts:",
        "class Alpha @ line 1",
        "  method one @ line 2",
        "    var inner @ line 3",
        "  method two @ line 5",
        "class Beta @ line 10",
        "  method three @ line 11",
        "function gamma @ line 20",
    ]
    .join("\n")
}

#[test]
fn draws_branches_and_rails_for_nested_siblings_including_a_sibling_after_a_deeper_child() {
    let rows = render_result_expanded(None, &transcript(), false, true);
    assert!(rows[0].ends_with("7 symbols · in src/a.ts"), "{}", rows[0]);
    let body: Vec<String> = rows[1..].iter().map(|row| row.trim_end().to_string()).collect();
    assert_eq!(
        body,
        [
            "├─ class Alpha",
            "│  line 1",
            "│  ├─ method one",
            "│  │  line 2",
            "│  │  └─ var inner",
            "│  │     line 3",
            "│  └─ method two",
            "│     line 5",
            "├─ class Beta",
            "│  line 10",
            "│  └─ method three",
            "│     line 11",
            "└─ function gamma",
            "   line 20",
        ]
    );
}

#[test]
fn parses_the_header_file_and_symbol_fields() {
    let parsed = parse_symbols(&transcript()).expect("symbols header");
    assert_eq!(parsed.file, "src/a.ts");
    assert_eq!(parsed.symbols.len(), 7);
    assert_eq!(parsed.symbols[0].icon, "class");
    assert_eq!(parsed.symbols[0].name, "Alpha");
    assert_eq!(parsed.symbols[0].line, 1);
    assert_eq!(parsed.symbols[0].indent, 0);
    assert_eq!(parsed.symbols[2].name, "var inner");
    assert_eq!(parsed.symbols[2].indent, 4);
    assert_eq!(parse_symbols("no header here\nclass A @ line 1"), None);
}

#[test]
fn collapsed_view_caps_top_level_symbols_and_counts_the_hidden_nested_ones() {
    let parsed = parse_symbols(&transcript()).unwrap();
    let rows = render_symbols(&parsed, &default_theme(), false);
    assert_eq!(
        rows,
        [
            "class Alpha line 1",
            "class Beta line 10",
            "function gamma line 20",
            "… 4 more symbols",
        ]
    );
}

#[test]
fn a_known_request_file_drops_the_in_file_meta() {
    use maple_tui::tools::lsp::{LspAction, LspParams};
    let args = LspParams {
        action: Some(LspAction::Symbols),
        file: Some("src/a.ts".into()),
        ..Default::default()
    };
    let rows = render_result_expanded(Some(&args), &transcript(), false, true);
    assert!(rows[0].ends_with("7 symbols"), "{}", rows[0]);
    assert!(!rows[0].contains("in src/a.ts"), "{}", rows[0]);
}
