//! Key-hint formatting for every key shown to the user.
//!
//! Reference: `l/src/key-hint-format.ts`.
//!
//! Deviations forced by Rust:
//!
//! - The reference reads a module-level `keyHintPlatformOverride` and the
//!   process platform. Rust has no module-level mutable singleton, so
//!   [`KeyHintFormatter`] carries the platform; [`KeyHintFormatter::set_platform`]
//!   with `None` resolves to the host ([`Platform::host`]), matching
//!   `setKeyHintPlatform(undefined)`.
//! - The reference resolves glyphs through `activeThemeSymbol` (the active
//!   theme, or the ascii preset before one loads). The formatter takes an
//!   explicit `&Theme`; [`crate::theme::theme_class::Theme::symbol`] already
//!   falls back to the preset, so a constructed theme covers both cases.

use crate::theme::symbols::{preset_symbols, SymbolPreset};
use crate::theme::theme_class::Theme;

/// Platform whose modifier keycaps differ (`darwin` uses ⌃⌥⇧⌘).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Platform {
    Darwin,
    Other,
}

impl Default for Platform {
    fn default() -> Self {
        Self::host()
    }
}

impl Platform {
    /// Resolve a `NodeJS.Platform`-style name; only `darwin` changes rendering.
    pub fn from_name(name: &str) -> Self {
        if name == "darwin" {
            Self::Darwin
        } else {
            Self::Other
        }
    }

    /// The host platform.
    pub fn host() -> Self {
        if cfg!(target_os = "macos") {
            Self::Darwin
        } else {
            Self::Other
        }
    }

    /// True on macOS, whose keycaps are labelled with ⌃⌥⇧⌘.
    pub fn is_darwin(&self) -> bool {
        matches!(self, Self::Darwin)
    }
}

/// Modifier → `[generic symbol, macOS symbol]`, in display order (⌃⌥⇧⌘).
const MODIFIER_SYMBOLS: &[(&str, &str, &str)] = &[
    ("ctrl", "key.ctrl", "key.ctrlMac"),
    ("alt", "key.alt", "key.altMac"),
    ("shift", "key.shift", "key.shift"),
    ("super", "key.super", "key.superMac"),
];

const KEY_SYMBOLS: &[(&str, &str)] = &[
    ("esc", "key.esc"),
    ("escape", "key.esc"),
    ("enter", "key.enter"),
    ("return", "key.enter"),
    ("space", "key.space"),
    ("tab", "key.tab"),
    ("backspace", "key.backspace"),
    ("delete", "key.delete"),
    ("up", "key.up"),
    ("down", "key.down"),
    ("left", "key.left"),
    ("right", "key.right"),
];

/// Keys without a keycap glyph; spelled out in every preset.
const KEY_WORDS: &[(&str, &str)] = &[
    ("home", "Home"),
    ("end", "End"),
    ("pageup", "PgUp"),
    ("pagedown", "PgDn"),
    ("insert", "Ins"),
];

/// `/[A-Za-z0-9]/`: a label containing a letter renders with a `+` separator.
fn has_letter(label: &str) -> bool {
    label.chars().any(|c| c.is_ascii_alphanumeric())
}

/// Format one key with glyphs from `symbol`.
fn format_key(key: &str, mac: bool, symbol: &dyn Fn(&str) -> String) -> String {
    // A trailing `+` is the plus key itself (`+`, `ctrl++`), not a separator.
    let mut parts: Vec<&str> = match key.strip_suffix('+') {
        Some(stripped) => {
            let mut parts: Vec<&str> = stripped.split('+').collect();
            parts.pop();
            parts.push("+");
            parts
        }
        None => key.split('+').collect(),
    };
    let base = parts.pop().unwrap_or("");

    let mut out = String::new();
    for (modifier, generic, mac_glyph) in MODIFIER_SYMBOLS {
        if !parts.iter().any(|part| *part == *modifier) {
            continue;
        }
        let label = symbol(if mac { mac_glyph } else { generic });
        out.push_str(&label);
        if has_letter(&label) {
            out.push('+');
        } else {
            out.push_str(&symbol("key.joiner"));
        }
    }

    let lower = base.to_ascii_lowercase();
    if let Some((_, generic, mac_glyph)) = MODIFIER_SYMBOLS.iter().find(|(m, _, _)| *m == lower) {
        out.push_str(&symbol(if mac { mac_glyph } else { generic }));
    } else if let Some((_, glyph)) = KEY_SYMBOLS.iter().find(|(k, _)| *k == lower) {
        out.push_str(&symbol(glyph));
    } else if let Some((_, word)) = KEY_WORDS.iter().find(|(k, _)| *k == lower) {
        out.push_str(word);
    } else if base.chars().count() == 1 {
        if parts.is_empty() {
            out.push_str(base);
        } else {
            out.push_str(&base.to_ascii_uppercase());
        }
    } else {
        if let Some(first) = base.chars().next() {
            out.extend(first.to_uppercase());
            out.push_str(&base[first.len_utf8()..]);
        }
    }
    out
}

/// Formats key hints against one theme and platform.
pub struct KeyHintFormatter<'a> {
    theme: &'a Theme,
    platform: Option<Platform>,
}

impl<'a> KeyHintFormatter<'a> {
    /// Format with the host platform.
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme, platform: None }
    }

    /// Format with a pinned platform (the `setKeyHintPlatform` seam).
    pub fn with_platform(theme: &'a Theme, platform: Platform) -> Self {
        Self {
            theme,
            platform: Some(platform),
        }
    }

    /// Platform currently used for key-hint rendering.
    pub fn platform(&self) -> Platform {
        self.platform.clone().unwrap_or_else(Platform::host)
    }

    /// Pin the platform; `None` resolves to the host.
    pub fn set_platform(&mut self, platform: Option<Platform>) {
        self.platform = platform;
    }

    /// Format one key for display: `"ctrl+shift+c"` → `⌃⇧C` (macOS) /
    /// `Ctrl+⇧C`, `"escape"` → `⎋`, `"alt+up"` → `⌥↑` / `Alt+↑`,
    /// `"shift"` → `⇧`.
    pub fn format_key_hint(&self, key: &str) -> String {
        let mac = self.platform().is_darwin();
        let theme = self.theme;
        format_key(key, mac, &|symbol| theme.symbol(symbol).to_string())
    }

    /// Format one key for a native tooltip (a TSP `title`): unicode keycap
    /// glyphs whatever the theme's symbol preset, and `esc` as Tern reads it.
    pub fn format_tooltip_key(&self, key: &str) -> String {
        if key == "escape" {
            return "esc".to_string();
        }
        let mac = self.platform().is_darwin();
        let unicode = preset_symbols(SymbolPreset::Unicode);
        format_key(key, mac, &|symbol| unicode.get(symbol).unwrap_or("").to_string())
    }

    /// Format alternative keys as a slash-separated hint
    /// (`["f5", "alt+r"]` → `F5/⌥R`).
    pub fn format_key_hints(&self, keys: &[&str]) -> String {
        keys.iter()
            .map(|key| self.format_key_hint(key))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Format a double-tap gesture: `←←` with glyphs, `Left Left` with words.
    pub fn format_double_tap(&self, key: &str) -> String {
        let label = self.format_key_hint(key);
        let separator = if has_letter(&label) {
            " ".to_string()
        } else {
            self.theme.symbol("key.joiner").to_string()
        };
        format!("{label}{separator}{label}")
    }
}

/// Build a theme for tests and callers that only format hints.
#[cfg(test)]
pub(crate) fn test_theme(preset: SymbolPreset) -> Theme {
    use crate::theme::loader::CreateThemeOptions;

    crate::theme::builtin::builtin_theme_json("dark")
        .and_then(|json| {
            crate::theme::loader::create_theme(
                json,
                CreateThemeOptions {
                    symbol_preset_override: Some(preset),
                    ..CreateThemeOptions::default()
                },
            )
            .ok()
        })
        .expect("dark theme builds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_chords_words_and_bare_keys() {
        let theme = test_theme(SymbolPreset::Unicode);
        let linux = KeyHintFormatter::with_platform(&theme, Platform::Other);
        let cases = [
            ("ctrl+shift+c", "Ctrl+⇧C"),
            ("escape", "⎋"),
            ("alt+up", "Alt+↑"),
            ("shift", "⇧"),
            ("q", "q"),
            ("f5", "F5"),
            ("+", "+"),
            ("ctrl++", "Ctrl++"),
            ("home", "Home"),
            ("end", "End"),
            ("pageup", "PgUp"),
            ("pagedown", "PgDn"),
            ("insert", "Ins"),
            ("return", "⏎"),
            ("backspace", "⌫"),
            ("space", "␣"),
        ];
        for (key, expected) in cases {
            assert_eq!(linux.format_key_hint(key), expected, "{key}");
        }
    }

    #[test]
    fn formats_macos_keycaps() {
        let theme = test_theme(SymbolPreset::Unicode);
        let mac = KeyHintFormatter::with_platform(&theme, Platform::Darwin);
        let cases = [
            ("alt+up", "⌥↑"),
            ("super+x", "⌘X"),
            ("ctrl+alt+shift+super+x", "⌃⌥⇧⌘X"),
        ];
        for (key, expected) in cases {
            assert_eq!(mac.format_key_hint(key), expected, "{key}");
        }
    }

    #[test]
    fn modifiers_render_in_display_order() {
        let theme = test_theme(SymbolPreset::Unicode);
        let linux = KeyHintFormatter::with_platform(&theme, Platform::Other);
        assert_eq!(linux.format_key_hint("shift+ctrl+p"), "Ctrl+⇧P");
        assert_eq!(linux.format_key_hint("alt+ctrl+p"), "Ctrl+Alt+P");
    }

    #[test]
    fn bare_letter_lowercase_in_chord_capitalized() {
        let theme = test_theme(SymbolPreset::Unicode);
        let formatter = KeyHintFormatter::with_platform(&theme, Platform::Other);
        assert_eq!(formatter.format_key_hint("a"), "a");
        assert_eq!(formatter.format_key_hint("ctrl+a"), "Ctrl+A");
    }

    #[test]
    fn tooltip_uses_unicode_preset_and_esc_alias() {
        let ascii = test_theme(SymbolPreset::Ascii);
        let formatter = KeyHintFormatter::with_platform(&ascii, Platform::Darwin);
        assert_eq!(formatter.format_tooltip_key("escape"), "esc");
        assert_eq!(formatter.format_tooltip_key("shift+tab"), "⇧⇥");
        assert_eq!(formatter.format_tooltip_key("alt+r"), "⌥R");
    }

    #[test]
    fn alternative_keys_join_with_slash() {
        let theme = test_theme(SymbolPreset::Unicode);
        let mac = KeyHintFormatter::with_platform(&theme, Platform::Darwin);
        assert_eq!(mac.format_key_hints(&["f5", "alt+r"]), "F5/⌥R");
    }

    #[test]
    fn double_tap_uses_joiner_or_space() {
        let unicode = test_theme(SymbolPreset::Unicode);
        let formatter = KeyHintFormatter::with_platform(&unicode, Platform::Other);
        assert_eq!(formatter.format_double_tap("left"), "←←");

        let ascii = test_theme(SymbolPreset::Ascii);
        let formatter = KeyHintFormatter::with_platform(&ascii, Platform::Other);
        assert_eq!(formatter.format_double_tap("left"), "Left Left");
    }

    #[test]
    fn platform_defaults_to_host_and_overrides() {
        let theme = test_theme(SymbolPreset::Unicode);
        let mut formatter = KeyHintFormatter::new(&theme);
        assert_eq!(formatter.platform(), Platform::host());
        formatter.set_platform(Some(Platform::Darwin));
        assert_eq!(formatter.platform(), Platform::Darwin);
        formatter.set_platform(None);
        assert_eq!(formatter.platform(), Platform::host());
    }

    #[test]
    fn ascii_words_preserve_reference_spelling() {
        let ascii = test_theme(SymbolPreset::Ascii);
        let formatter = KeyHintFormatter::with_platform(&ascii, Platform::Other);
        assert_eq!(formatter.format_key_hint("escape"), "Esc");
        assert_eq!(formatter.format_key_hint("space"), "Space");
        assert_eq!(formatter.format_key_hint("ctrl+o"), "Ctrl+O");
    }
}
