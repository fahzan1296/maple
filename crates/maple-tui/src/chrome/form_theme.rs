//! Shared accent form-field theme used by overlay dialogs.
//!
//! Reference: `l/src/chrome/form-theme.ts`.
//!
//! Deviations: the reference exports a `FormFieldTheme` object whose four fields
//! are `(text: string) => string` closures over the process-global `theme`;
//! maple-tui has no global theme and [`crate::style::Style`] carries resolved
//! SGR values rather than tokens. [`form_theme`] therefore takes an explicit
//! `&Theme` and returns the crate's [`FormTheme`], overriding `label`,
//! `description`, `error` and `hint` with the `accent`, `muted`, `error` and
//! `dim` tokens; the remaining fields keep [`FormTheme::default`]. A token that
//! does not resolve to a colour leaves its field unstyled.

use crate::components::form::FormTheme;
use crate::style::{Color, Style};
use crate::theme::color::parse_hex;
use crate::theme::Theme;

/// The shared accent form-field theme: bold accent labels, muted descriptions,
/// error-coloured validation text and dim hints.
pub fn form_theme(theme: &Theme) -> FormTheme {
    let mut base = FormTheme::default();
    base.label = token_style(theme, "accent", false).bold();
    base.description = token_style(theme, "muted", false);
    base.error = token_style(theme, "error", false);
    base.hint = token_style(theme, "dim", false);
    base
}

/// Resolve one theme token to a foreground/background [`Style`].
fn token_style(theme: &Theme, token: &str, background: bool) -> Style {
    let resolved = if background {
        theme.get_bg_hex(token)
    } else {
        theme.get_color_hex(token)
    };
    match resolved.ok().and_then(|hex| parse_hex(&hex)) {
        Some((red, green, blue)) => {
            if background {
                Style::bg(Color::Rgb(red, green, blue))
            } else {
                Style::fg(Color::Rgb(red, green, blue))
            }
        }
        None => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin;
    use crate::theme::{create_theme, CreateThemeOptions};
    use crate::theme::color::ColorMode;

    fn theme() -> Theme {
        create_theme(
            builtin::builtin_theme_json("dark").expect("builtin theme"),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .expect("theme resolves")
    }

    #[test]
    fn label_is_bold_accent_and_hint_is_dim() {
        let dark = theme();
        let form = form_theme(&dark);
        assert!(form.label.bold);
        assert_eq!(
            form.label.fg,
            parse_hex(&dark.get_color_hex("accent").expect("accent")).map(|(r, g, b)| Color::Rgb(r, g, b))
        );
        assert_eq!(
            form.hint.fg,
            parse_hex(&dark.get_color_hex("dim").expect("dim")).map(|(r, g, b)| Color::Rgb(r, g, b))
        );
    }

    #[test]
    fn description_and_error_use_their_tokens() {
        let dark = theme();
        let form = form_theme(&dark);
        assert_eq!(
            form.description.fg,
            parse_hex(&dark.get_color_hex("muted").expect("muted")).map(|(r, g, b)| Color::Rgb(r, g, b))
        );
        assert_eq!(
            form.error.fg,
            parse_hex(&dark.get_color_hex("error").expect("error")).map(|(r, g, b)| Color::Rgb(r, g, b))
        );
    }

    #[test]
    fn other_form_fields_keep_the_default_theme() {
        let dark = theme();
        let form = form_theme(&dark);
        let default = FormTheme::default();
        assert_eq!(form.cursor, default.cursor);
        assert_eq!(form.selected, default.selected);
        assert_eq!(form.symbols.len(), default.symbols.len());
    }
}
