// Port target: l/test/render-utils.test.ts (portable subset)
// Many TS helpers (embedded home path shortening, feed model badges, sanitize display
// warnings/controls variants, parse/diagnostic formatters, sixel masks, etc.) have no
// direct 1:1 Rust export in maple_tui::render::render_utils. This file ports cases for
// APIs that exist with matching intent.

use maple_tui::render::render_utils::{
    expand_key_hint, format_code_frame_line, format_diff_stats, format_expand_hint,
    format_styled_path, get_diff_stats, pad_to_width, path_name_start, replace_tabs,
    resolve_image_options, sanitize_carriage_returns, set_expand_key_hint,
    set_inline_image_max_columns, set_inline_image_max_rows, split_terminal_output_lines,
    DEFAULT_TERMINAL_PREVIEW_LINES, PREVIEW_LIMITS,
};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::default_theme;

#[test]
fn replace_tabs_expands_to_tab_width_spaces() {
    assert_eq!(replace_tabs("a\tb"), "a   b");
    assert_eq!(replace_tabs("plain"), "plain");
}

#[test]
fn split_terminal_output_lines_handles_cr_and_newlines() {
    assert_eq!(
        split_terminal_output_lines("progress 1\rprogress 2\ndone"),
        vec!["progress 2".to_string(), "done".to_string()]
    );
    assert_eq!(split_terminal_output_lines(""), vec!["".to_string()]);
}

#[test]
fn sanitize_carriage_returns_normalizes_crlf_and_cr() {
    assert_eq!(sanitize_carriage_returns("a\r\nb\rc"), "a\nb c");
    assert_eq!(sanitize_carriage_returns("plain"), "plain");
}

#[test]
fn path_name_start_finds_final_segment_after_separators() {
    assert_eq!(path_name_start("src/tools/grep.ts"), "src/tools/".len());
    assert_eq!(path_name_start("C:\\repo\\src\\grep.ts"), "C:\\repo\\src\\".len());
    assert_eq!(path_name_start("README.md"), 0);
}

#[test]
fn format_styled_path_preserves_full_text_and_accents_name() {
    let theme = default_theme();
    let styled = format_styled_path("src/tools/grep.ts", &theme);
    assert!(styled.contains("src/tools/"));
    assert!(styled.contains("grep.ts"));
    assert_eq!(visible_width(&styled), visible_width("src/tools/grep.ts"));

    let leaf = format_styled_path("README.md", &theme);
    assert!(leaf.contains("README.md"));
}

#[test]
fn diff_stats_count_hunks_and_added_removed_lines() {
    let diff = "+a\n-b\n context\n+c\n-d";
    let stats = get_diff_stats(diff);
    assert_eq!(stats.added, 2);
    assert_eq!(stats.removed, 2);
    assert_eq!(stats.hunks, 2);

    let theme = default_theme();
    let summary = format_diff_stats(stats.added, stats.removed, stats.hunks, &theme);
    assert!(summary.contains("+2"));
    assert!(summary.contains("-2"));
    assert!(summary.contains("2 hunks"));
}

#[test]
fn format_code_frame_line_pads_gutter_and_separates_content() {
    let line = format_code_frame_line("", "42", "let x = 1;", 4);
    assert_eq!(line, "   42│let x = 1;");
    let marked = format_code_frame_line(">", "7", "x", 3);
    assert!(marked.contains('│'));
    assert!(marked.ends_with("x"));
}

#[test]
fn expand_key_hint_defaults_and_can_be_overridden() {
    let theme = default_theme();
    set_expand_key_hint("");
    assert_eq!(expand_key_hint(), "ctrl+o");
    assert!(format_expand_hint(&theme, Some(true), Some(true)).is_empty());
    assert!(format_expand_hint(&theme, Some(false), Some(false)).is_empty());

    set_expand_key_hint("Ctrl+E");
    assert_eq!(expand_key_hint(), "Ctrl+E");
    let hint = format_expand_hint(&theme, Some(false), Some(true));
    assert!(hint.contains("Ctrl+E"));
    assert!(hint.contains("Expand"));
    // restore default-ish for other tests in process
    set_expand_key_hint("ctrl+o");
}

#[test]
fn preview_limits_and_defaults_are_stable() {
    assert_eq!(PREVIEW_LIMITS.COLLAPSED_LINES, 3);
    assert_eq!(PREVIEW_LIMITS.EXPANDED_LINES, 12);
    assert_eq!(PREVIEW_LIMITS.COLLAPSED_ITEMS, 8);
    assert_eq!(PREVIEW_LIMITS.LOCATIONS_COLLAPSED, 2);
    assert_eq!(PREVIEW_LIMITS.OUTPUT_COLLAPSED, 3);
    assert_eq!(PREVIEW_LIMITS.OUTPUT_EXPANDED, 10);
    assert_eq!(DEFAULT_TERMINAL_PREVIEW_LINES, 10);
}

#[test]
fn resolve_image_options_reflects_inline_caps() {
    // resolve_image_options reads terminal viewport; in non-tty/tests viewport may be unknown.
    // We still assert configured width/row settings are applied coherently.
    set_inline_image_max_columns(72);
    set_inline_image_max_rows(10);
    let opts = resolve_image_options();
    assert_eq!(opts.max_width_cells, 72);
    assert_eq!(opts.max_height_cells, Some(10));

    set_inline_image_max_columns(100);
    set_inline_image_max_rows(20);
}

#[test]
fn pad_to_width_is_pad_only_for_wide_input() {
    // render_utils::pad_to_width pads short lines and leaves wide input untouched.
    assert_eq!(visible_width(&pad_to_width("ab", 5, None)), 5);
    let wide = "abcdef";
    assert_eq!(pad_to_width(wide, 3, None), wide);
}
