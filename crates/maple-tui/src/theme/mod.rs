//! Theme system: JSON schema, color math, symbol presets, built-in themes and
//! the resolved [`Theme`] used by every renderer.
//!
//! Ported from the reference `l/src/theme/` directory. The module is split the
//! same way:
//!
//! - [`schema`] — color token key sets and the serde `ThemeJson` shape.
//! - [`color`] — color values, ANSI conversion, var resolution and color math.
//! - [`symbols`] — the unicode/nerd/ascii symbol vocabulary and spinner frames.
//! - [`builtin`] — the embedded `dark` and `light` themes.
//! - [`loader`] — parsing, validation, discovery and [`loader::create_theme`].
//! - [`theme_class`] — the resolved [`Theme`] with per-token ANSI/hex caches.
//!
//! # Key representation
//!
//! Theme colors are addressed by their JSON token string (`"accent"`,
//! `"mdHeading`, …) rather than by a generated enum. The TS API is
//! `fg(color: ThemeColor, …)` where `ThemeColor` is a string union, and the
//! theme JSON itself contains at least one key (`link` in `dark.json`) that is
//! *not* a `ThemeColor`; string keys keep that behavior exact and let callers
//! pass [`schema::ThemeColor::Accent`] (or any `&str`) interchangeably.

pub mod builtin;
pub mod color;
pub mod loader;
pub mod schema;
pub mod symbols;
pub mod theme_class;

pub use color::{ColorMode, ColorValue};
pub use loader::{create_theme, load_theme_json, load_theme_json_sync, CreateThemeOptions};
pub use schema::{ThemeBg, ThemeColor, ThemeJson};
pub use symbols::{SymbolPreset, SymbolMap};
pub use theme_class::{SessionAccentTheme, Theme};
