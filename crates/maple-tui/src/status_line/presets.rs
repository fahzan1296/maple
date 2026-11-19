//! Status-line preset table.
//!
//! Reference: `l/src/status-line/presets.ts`.
//!
//! Deviation: the reference exports `STATUS_LINE_PRESETS`, a
//! `Record<StatusLinePreset, PresetDef>` indexed by preset name, plus
//! `getPreset(name)` with a `?? default` fallback. Rust has no map literal
//! keyed by an enum and no unknown key to fall back from, so the table itself
//! lives in [`schema::preset`] (which ports it) and this module exposes the
//! same lookup as [`STATUS_LINE_PRESETS`] and [`get_preset`].

use crate::status_line::schema::{self, PresetDef, StatusLinePreset};

/// Handle to the preset table; `STATUS_LINE_PRESETS` in the reference.
pub struct StatusLinePresets;

/// The preset table (`STATUS_LINE_PRESETS` in the reference).
pub const STATUS_LINE_PRESETS: StatusLinePresets = StatusLinePresets;

impl StatusLinePresets {
    /// `STATUS_LINE_PRESETS[name]`.
    pub fn get(self, name: StatusLinePreset) -> PresetDef {
        schema::preset(name)
    }
}

/// Look up a preset by name; the `default` preset is the baseline.
pub fn get_preset(name: StatusLinePreset) -> PresetDef {
    schema::preset(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_line::schema::{SegmentId, SeparatorStyle};

    #[test]
    fn preset_table_matches_reference() {
        let cases: &[(StatusLinePreset, &[SegmentId], &[SegmentId], SeparatorStyle)] = &[
            (
                StatusLinePreset::Default,
                &[
                    SegmentId::Brand,
                    SegmentId::Vim,
                    SegmentId::Model,
                    SegmentId::Mode,
                    SegmentId::Collab,
                    SegmentId::Stream,
                    SegmentId::Path,
                    SegmentId::Git,
                    SegmentId::Pr,
                    SegmentId::ContextPct,
                    SegmentId::Cost,
                ],
                &[SegmentId::SessionName],
                SeparatorStyle::PowerlineThin,
            ),
            (
                StatusLinePreset::Minimal,
                &[SegmentId::Vim, SegmentId::Path, SegmentId::Git],
                &[
                    SegmentId::SessionName,
                    SegmentId::Mode,
                    SegmentId::ContextPct,
                ],
                SeparatorStyle::Slash,
            ),
            (
                StatusLinePreset::Compact,
                &[
                    SegmentId::Vim,
                    SegmentId::Model,
                    SegmentId::Mode,
                    SegmentId::Git,
                    SegmentId::Pr,
                ],
                &[
                    SegmentId::SessionName,
                    SegmentId::Cost,
                    SegmentId::ContextPct,
                ],
                SeparatorStyle::PowerlineThin,
            ),
            (
                StatusLinePreset::Full,
                &[
                    SegmentId::Brand,
                    SegmentId::Vim,
                    SegmentId::Hostname,
                    SegmentId::Model,
                    SegmentId::Mode,
                    SegmentId::Path,
                    SegmentId::Git,
                    SegmentId::Pr,
                    SegmentId::Subagents,
                ],
                &[
                    SegmentId::SessionName,
                    SegmentId::CacheHit,
                    SegmentId::TokenIn,
                    SegmentId::TokenOut,
                    SegmentId::TokenRate,
                    SegmentId::CacheRead,
                    SegmentId::Cache,
                    SegmentId::Cost,
                    SegmentId::ContextPct,
                    SegmentId::TimeSpent,
                    SegmentId::Time,
                ],
                SeparatorStyle::Powerline,
            ),
            (
                StatusLinePreset::Nerd,
                &[
                    SegmentId::Brand,
                    SegmentId::Vim,
                    SegmentId::Hostname,
                    SegmentId::Model,
                    SegmentId::Mode,
                    SegmentId::Path,
                    SegmentId::Git,
                    SegmentId::Pr,
                    SegmentId::Session,
                    SegmentId::Subagents,
                ],
                &[
                    SegmentId::SessionName,
                    SegmentId::TokenIn,
                    SegmentId::TokenOut,
                    SegmentId::CacheRead,
                    SegmentId::CacheWrite,
                    SegmentId::TokenRate,
                    SegmentId::Cache,
                    SegmentId::Cost,
                    SegmentId::ContextPct,
                    SegmentId::ContextTotal,
                    SegmentId::TimeSpent,
                    SegmentId::Time,
                ],
                SeparatorStyle::Powerline,
            ),
            (
                StatusLinePreset::Ascii,
                &[
                    SegmentId::Vim,
                    SegmentId::Model,
                    SegmentId::Mode,
                    SegmentId::Path,
                    SegmentId::Git,
                    SegmentId::Pr,
                ],
                &[
                    SegmentId::SessionName,
                    SegmentId::TokenTotal,
                    SegmentId::Cost,
                    SegmentId::ContextPct,
                ],
                SeparatorStyle::Ascii,
            ),
            (
                StatusLinePreset::Custom,
                &[
                    SegmentId::Vim,
                    SegmentId::Model,
                    SegmentId::Mode,
                    SegmentId::Path,
                    SegmentId::Git,
                    SegmentId::Pr,
                ],
                &[
                    SegmentId::SessionName,
                    SegmentId::TokenTotal,
                    SegmentId::Cost,
                    SegmentId::ContextPct,
                ],
                SeparatorStyle::PowerlineThin,
            ),
        ];
        for (name, left, right, separator) in cases {
            let def = get_preset(*name);
            assert_eq!(&def.left_segments, left, "left group for {name}");
            assert_eq!(&def.right_segments, right, "right group for {name}");
            assert_eq!(def.separator, *separator, "separator for {name}");
            assert_eq!(STATUS_LINE_PRESETS.get(*name), def, "table for {name}");
        }
    }

    #[test]
    fn preset_segment_options_match_reference() {
        let compact = get_preset(StatusLinePreset::Compact);
        assert_eq!(
            compact
                .segment_options
                .model
                .as_ref()
                .and_then(|options| options.show_thinking_level),
            Some(false)
        );
        let full = get_preset(StatusLinePreset::Full);
        assert_eq!(
            full.segment_options
                .path
                .as_ref()
                .and_then(|options| options.max_length),
            Some(50)
        );
        assert_eq!(
            full.segment_options
                .time
                .as_ref()
                .and_then(|options| options.show_seconds),
            Some(false)
        );
        let custom = get_preset(StatusLinePreset::Custom);
        assert_eq!(custom.segment_options, Default::default());
    }
}
