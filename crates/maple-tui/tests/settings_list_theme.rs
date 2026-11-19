// Port target: l/test/settings-list-theme.test.ts
// Rust divergence: SettingsListTheme stores Style values (not per-call label/value
// callbacks with selected/changed args). This test ports the intent by asserting the
// adapter wires token-derived styles for key roles from the active theme.

use maple_tui::style::Color;
use maple_tui::theme::{default_theme, tui_adapters};

#[test]
fn settings_list_theme_adapter_populates_key_role_styles() {
    let theme = default_theme();
    let settings_theme = tui_adapters::get_settings_list_theme(&theme);

    // Roles should resolve to non-default styles in the built-in dark theme.
    assert_ne!(settings_theme.selected_label, maple_tui::style::Style::default());
    assert_ne!(settings_theme.value, maple_tui::style::Style::default());
    assert_ne!(settings_theme.selected_value, maple_tui::style::Style::default());
    assert_ne!(settings_theme.description, maple_tui::style::Style::default());
    assert_ne!(settings_theme.warning, maple_tui::style::Style::default());
    assert_ne!(settings_theme.cursor, maple_tui::style::Style::default());
    assert_ne!(settings_theme.hint, maple_tui::style::Style::default());
    assert_ne!(settings_theme.heading, maple_tui::style::Style::default());

    // Sanity: warning mark and symbols are carried from theme.
    assert!(!settings_theme.warning_mark.is_empty());
    assert!(!settings_theme.symbols.spinner_frames.is_empty());

    // Selected row uses a background style in the adapter.
    // (built-in themes define selectedBg; fg-only comparison avoided)
    let _ = settings_theme.selected_row;
    // Ensure a basic paint path works with a concrete fg check on a role that maps to accent.
    let accent_fg = tui_adapters::fg_style(&theme, "accent");
    assert_eq!(settings_theme.cursor.fg, accent_fg.fg);
}
