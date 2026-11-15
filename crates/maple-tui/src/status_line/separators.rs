//! Separator glyph definitions per style.
//!
//! Ported from `l/src/status-line/separators.ts`. Glyphs come from the active
//! theme's `sep` group; widths are measured with [`visible_width`].

use crate::status_line::schema::SeparatorStyle;
use crate::status_line::StatusTheme;
use crate::text_shape::visible_width;

/// End caps drawn at the outer edges of a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndCaps {
    /// Cap for right segments (points left).
    pub left: String,
    /// Cap for left segments (points right).
    pub right: String,
    /// Paint the cap with the background color used as a foreground color.
    pub use_bg_as_fg: bool,
}

/// Separator glyphs and optional end caps for one group edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeparatorDef {
    /// Character for left-to-right segments.
    pub left: String,
    /// Character for right-to-left segments (reversed).
    pub right: String,
    /// Optional powerline caps at the outer edges.
    pub end_caps: Option<EndCaps>,
}

impl SeparatorDef {
    /// Visible width of the left-to-right separator.
    pub fn left_width(&self) -> usize {
        visible_width(&self.left)
    }

    /// Visible width of the right-to-left separator.
    pub fn right_width(&self) -> usize {
        visible_width(&self.right)
    }

    /// Width of the cap closing a left group.
    pub fn left_cap_width(&self) -> usize {
        self.end_caps
            .as_ref()
            .map(|caps| visible_width(&caps.right))
            .unwrap_or(0)
    }

    /// Width of the cap opening a right group.
    pub fn right_cap_width(&self) -> usize {
        self.end_caps
            .as_ref()
            .map(|caps| visible_width(&caps.left))
            .unwrap_or(0)
    }
}

/// Look up the separator definition for `style`.
///
/// `powerline-thin` is also the fallback for unknown styles.
pub fn get_separator(style: SeparatorStyle, theme: &dyn StatusTheme) -> SeparatorDef {
    match style {
        SeparatorStyle::Powerline => SeparatorDef {
            left: theme.sep("powerline-left"),
            right: theme.sep("powerline-right"),
            end_caps: Some(EndCaps {
                left: theme.sep("powerline-right"),
                right: theme.sep("powerline-left"),
                use_bg_as_fg: true,
            }),
        },
        SeparatorStyle::Slash => {
            let slash = theme.sep("slash").trim().to_string();
            SeparatorDef {
                left: slash.clone(),
                right: slash,
                end_caps: None,
            }
        }
        SeparatorStyle::Pipe => {
            let pipe = theme.sep("pipe").trim().to_string();
            SeparatorDef {
                left: pipe.clone(),
                right: pipe,
                end_caps: None,
            }
        }
        SeparatorStyle::Block => {
            let block = theme.sep("block");
            SeparatorDef {
                left: block.clone(),
                right: block,
                end_caps: None,
            }
        }
        SeparatorStyle::None => {
            let space = theme.sep("space");
            SeparatorDef {
                left: space.clone(),
                right: space,
                end_caps: None,
            }
        }
        SeparatorStyle::Ascii => SeparatorDef {
            left: theme.sep("ascii-left"),
            right: theme.sep("ascii-right"),
            end_caps: None,
        },
        SeparatorStyle::PowerlineThin => SeparatorDef {
            left: theme.sep("powerline-thin-left"),
            right: theme.sep("powerline-thin-right"),
            end_caps: Some(EndCaps {
                left: theme.sep("powerline-right"),
                right: theme.sep("powerline-left"),
                use_bg_as_fg: true,
            }),
        },
    }
}

/// The dot separator used by the plain (borderless) layouts.
pub fn plain_separator() -> SeparatorDef {
    SeparatorDef {
        left: "·".to_string(),
        right: "·".to_string(),
        end_caps: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_line::FallbackTheme;

    #[test]
    fn powerline_uses_arrows_and_contrasting_caps() {
        let theme = FallbackTheme::new();
        let def = get_separator(SeparatorStyle::Powerline, &theme);
        assert_eq!(def.left, "▶");
        assert_eq!(def.right, "◀");
        let caps = def.end_caps.as_ref().unwrap();
        assert_eq!(caps.left, "◀");
        assert_eq!(caps.right, "▶");
        assert!(caps.use_bg_as_fg);
        assert_eq!(def.left_cap_width(), 1);
        assert_eq!(def.right_cap_width(), 1);
    }

    #[test]
    fn powerline_thin_matches_typescript() {
        let theme = FallbackTheme::new();
        let def = get_separator(SeparatorStyle::PowerlineThin, &theme);
        assert_eq!(def.left, ">");
        assert_eq!(def.right, "<");
        let caps = def.end_caps.unwrap();
        assert_eq!(caps.left, "◀");
        assert_eq!(caps.right, "▶");
        assert!(caps.use_bg_as_fg);
    }

    #[test]
    fn text_separators_are_trimmed() {
        let theme = FallbackTheme::new();
        let slash = get_separator(SeparatorStyle::Slash, &theme);
        assert_eq!(slash.left, "/");
        assert_eq!(slash.right, "/");
        assert!(slash.end_caps.is_none());

        let pipe = get_separator(SeparatorStyle::Pipe, &theme);
        assert_eq!(pipe.left, "│");
        assert_eq!(pipe.right, "│");
        assert!(pipe.end_caps.is_none());
    }

    #[test]
    fn block_space_and_ascii() {
        let theme = FallbackTheme::new();
        assert_eq!(get_separator(SeparatorStyle::Block, &theme).left, "▌");
        assert_eq!(get_separator(SeparatorStyle::None, &theme).left, " ");
        let ascii = get_separator(SeparatorStyle::Ascii, &theme);
        assert_eq!(ascii.left, ">");
        assert_eq!(ascii.right, "<");
        assert!(ascii.end_caps.is_none());
    }

    #[test]
    fn plain_separator_is_a_dot_without_caps() {
        let def = plain_separator();
        assert_eq!(def.left, "·");
        assert_eq!(def.right, "·");
        assert!(def.end_caps.is_none());
    }
}
