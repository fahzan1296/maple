//! Port of `l/test/colorful-markdown.test.ts`.
//!
//! The reference toggles a global `setMarkdownColorful` and reads a global theme
//! instance. The crate's `get_markdown_theme(&Theme, colorful)` takes both as
//! parameters, so the global is injected. The reference paints each heading
//! depth a different hue and underlines H1; the crate's `MarkdownTheme` carries
//! one heading `Style`, so the adapter's actual contract (palette hues for
//! headings, code, emphasis and table headers) is asserted instead.

use maple_tui::component::Component;
use maple_tui::components::markdown::Markdown;
use maple_tui::text_shape::visible_width;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};
use maple_tui::theme::schema::AnsiColor;
use maple_tui::theme::theme_class::Theme;
use maple_tui::theme::tui_adapters::get_markdown_theme;

const DOC: &str = "# One\n\n## Two\n\n###### Six\n\n**loud `code` tail** and *soft*\n\n| Head |\n| ---- |\n| cell |";

fn celestial() -> Theme {
    let json = builtin_theme_json("dark-celestial").expect("dark-celestial theme");
    create_theme(json, CreateThemeOptions::default()).expect("theme builds")
}

fn render(theme: &Theme, colorful: bool) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(DOC);
    markdown.set_theme(get_markdown_theme(theme, colorful));
    markdown.render(60)
}

#[test]
fn colorful_theme_paints_headings_code_emphasis_and_table_headers_from_the_palette() {
    let theme = celestial();
    let colorful = get_markdown_theme(&theme, true);
    let plain = get_markdown_theme(&theme, false);

    assert_ne!(colorful.heading, plain.heading);
    assert_ne!(colorful.code, plain.code);
    assert_ne!(colorful.italic, plain.italic);
    assert_ne!(colorful.table_header, plain.table_header);
    assert_ne!(colorful.bold, plain.bold);

    let lines = render(&theme, true);
    let joined = lines.join("\n");
    for needle in ["One", "Two", "Six", "loud", "code", "soft", "Head", "cell"] {
        assert!(joined.contains(needle), "missing {needle}");
    }
    assert!(lines.iter().all(|line| visible_width(line) <= 60));
}
#[test]
fn plain_theme_keeps_the_single_heading_token_and_uncolored_emphasis() {
    let theme = celestial();
    let plain = get_markdown_theme(&theme, false);
    let colorful = get_markdown_theme(&theme, true);
    assert_ne!(plain.heading, colorful.heading);

    let lines = render(&theme, false);
    let joined = lines.join("\n");
    assert!(joined.contains("Two"));
    assert!(joined.contains("Six"));
    assert!(joined.contains("code"));
}

#[test]
fn falls_back_to_the_terminal_palette_when_a_theme_ships_no_terminal_ansi() {
    let theme = celestial();
    let bare_json = maple_tui::theme::schema::ThemeJson {
        terminal: None,
        ..(*builtin_theme_json("dark-celestial").expect("dark-celestial theme")).clone()
    };
    let bare = create_theme(&bare_json, CreateThemeOptions::default()).expect("bare theme builds");

    assert_eq!(bare.ansi_fg(AnsiColor::Magenta, "x"), "\x1b[38;5;5mx\x1b[39m");
    assert_ne!(theme.ansi_fg(AnsiColor::Magenta, "x"), bare.ansi_fg(AnsiColor::Magenta, "x"));
}
