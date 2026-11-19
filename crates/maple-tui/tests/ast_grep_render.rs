//! Port of `l/test/ast-grep-render.test.ts`.
//!
//! `render_result_collapsed` ports the reference's collapsed preview window:
//! head of the output, hidden-match count, per-file cut, and dropped capture
//! rows. Header subject/scope order, error rendering, and the zero-match
//! summary are asserted too.

use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};
use maple_tui::tools::ast_grep::{
    AstGrepRenderArgs, AstGrepToolDetails, ast_grep_call_meta, ast_grep_subject_plain, render_call,
    render_result, render_result_collapsed, render_result_themed,
};

fn theme() -> Theme {
    create_theme(&builtin_theme_json("dark").expect("dark theme json"), CreateThemeOptions::default())
        .expect("dark theme")
}

#[test]
fn header_names_scope_right_after_pattern_before_counts() {
    let args = AstGrepRenderArgs {
        pat: Some("f($A)".to_string()),
        path: vec!["src".to_string()],
        ..AstGrepRenderArgs::default()
    };
    assert_eq!(render_call(Some(&args)), "f($A) in src");
    assert_eq!(
        ast_grep_subject_plain(Some("f($A)"), Some(&args), None),
        Some("f($A) in src".to_string())
    );
    assert_eq!(ast_grep_call_meta(Some(&args)), vec!["in src".to_string()]);
    // Scope still leads when the pattern is absent.
    assert_eq!(
        ast_grep_subject_plain(None, Some(&args), None),
        Some("in src".to_string())
    );
}

#[test]
fn error_shows_title_and_message() {
    assert_eq!(
        render_result(None, "Pattern parse error", true),
        vec!["ast_grep failed: Pattern parse error".to_string()]
    );
    assert_eq!(render_result(None, "", true), vec!["ast_grep failed".to_string()]);
}

#[test]
fn zero_matches_renders_single_summary_line() {
    let details = AstGrepToolDetails {
        match_count: 0,
        file_count: 0,
        files_searched: 4,
        scope_path: Some("src".to_string()),
        ..AstGrepToolDetails::default()
    };
    assert_eq!(render_result(Some(&details), "", false), vec!["No matches".to_string()]);
    let themed = render_result_themed(Some(&details), "", false, &theme());
    assert_eq!(themed.len(), 1);
    assert!(themed[0].contains("No matches"));
}

#[test]
fn preview_shows_head_and_counts_hidden_matches() {
    let output = (0..8).map(|i| format!("*{}│f({})", i + 1, i)).collect::<Vec<_>>().join("\n");
    let details = AstGrepToolDetails {
        match_count: 8,
        file_count: 2,
        files_searched: 2,
        display_content: Some(output.clone()),
        ..AstGrepToolDetails::default()
    };
    let body = render_result_collapsed(Some(&details), &output, false);
    assert_eq!(body.iter().filter(|line| line.contains("│f(")).count(), 5);
    assert!(body.last().expect("summary").contains("… 3 more matches"));
}

#[test]
fn next_file_cut_counts_matches_not_files() {
    let output = [
        "# src/",
        "## a.ts",
        "*1│f(a)",
        "## b.ts",
        "*1│f(b)",
        "*2│f(c)",
        "*3│f(d)",
        "*4│f(e)",
        "## c.ts",
        "*1│f(f)",
        "*2│f(g)",
    ]
    .join("\n");
    let details = AstGrepToolDetails {
        match_count: 7,
        file_count: 2,
        files_searched: 2,
        display_content: Some(output.clone()),
        ..AstGrepToolDetails::default()
    };
    let body = render_result_collapsed(Some(&details), &output, false);
    assert_eq!(body.iter().filter(|line| line.contains("│f(")).count(), 3);
    assert!(!body.iter().any(|line| line.contains("src/c.ts")));
    assert!(body.last().expect("summary").contains("… 4 more matches"));
}

#[test]
fn capture_rows_are_dropped_but_match_rows_kept() {
    let output = [
        "## a.ts",
        "*1│f(",
        " 2│  a)",
        "  meta: $A=a",
        "## b.ts",
        "*3│f(b)",
        "  meta: $A=b",
    ]
    .join("\n");
    let details = AstGrepToolDetails {
        match_count: 2,
        file_count: 2,
        files_searched: 2,
        display_content: Some(output.clone()),
        ..AstGrepToolDetails::default()
    };
    let body = render_result_collapsed(Some(&details), &output, false);
    assert!(!body.iter().any(|line| line.contains("meta:")));
    assert!(body.iter().any(|line| line.contains("2│  a)")));
    assert!(body.iter().any(|line| line.contains("│f(b)")));
}
