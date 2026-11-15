//! Status-line schema: segment catalog, presets, settings, and separator styles.
//!
//! Ported from `l/src/status-line/schema.ts`, `types.ts`, and `presets.ts`.

use std::fmt;

/// Segment identifiers accepted by custom status-line settings, in catalog order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentId {
    /// Brand mark, working spinner, and turn timer.
    Pi,
    /// Extension/hook status text.
    Status,
    /// Model name plus thinking-level tail.
    Model,
    /// Plan / prewalk / goal / vibe / loop mode.
    Mode,
    /// Working directory.
    Path,
    /// Git branch and dirty counts.
    Git,
    /// Pull-request link.
    Pr,
    /// Subagent count.
    Subagents,
    /// Prompt input tokens.
    TokenIn,
    /// Output tokens.
    TokenOut,
    /// Total tokens, cache read excluded.
    TokenTotal,
    /// Tokens per second.
    TokenRate,
    /// Session cost.
    Cost,
    /// Context-window occupancy percentage.
    ContextPct,
    /// Context-window size.
    ContextTotal,
    /// Active processing time.
    TimeSpent,
    /// Wall clock.
    Time,
    /// Session id prefix.
    Session,
    /// Machine hostname.
    Hostname,
    /// Cache-read tokens.
    CacheRead,
    /// Cache-write tokens.
    CacheWrite,
    /// Cache hit rate.
    CacheHit,
    /// Prompt-cache state glyph.
    Cache,
    /// Session title.
    SessionName,
    /// Quota windows.
    Usage,
    /// Collaboration indicator.
    Collab,
    /// Live-stream / recording badges.
    Stream,
    /// Vim modal state.
    Vim,
}

impl SegmentId {
    /// Every catalog identifier, in declaration order.
    pub const ALL: &'static [SegmentId] = &[
        SegmentId::Pi,
        SegmentId::Status,
        SegmentId::Model,
        SegmentId::Mode,
        SegmentId::Path,
        SegmentId::Git,
        SegmentId::Pr,
        SegmentId::Subagents,
        SegmentId::TokenIn,
        SegmentId::TokenOut,
        SegmentId::TokenTotal,
        SegmentId::TokenRate,
        SegmentId::Cost,
        SegmentId::ContextPct,
        SegmentId::ContextTotal,
        SegmentId::TimeSpent,
        SegmentId::Time,
        SegmentId::Session,
        SegmentId::Hostname,
        SegmentId::CacheRead,
        SegmentId::CacheWrite,
        SegmentId::CacheHit,
        SegmentId::Cache,
        SegmentId::SessionName,
        SegmentId::Usage,
        SegmentId::Collab,
        SegmentId::Stream,
        SegmentId::Vim,
    ];

    /// The stable identifier string used by settings and the TypeScript source.
    pub fn as_str(self) -> &'static str {
        match self {
            SegmentId::Pi => "pi",
            SegmentId::Status => "status",
            SegmentId::Model => "model",
            SegmentId::Mode => "mode",
            SegmentId::Path => "path",
            SegmentId::Git => "git",
            SegmentId::Pr => "pr",
            SegmentId::Subagents => "subagents",
            SegmentId::TokenIn => "token_in",
            SegmentId::TokenOut => "token_out",
            SegmentId::TokenTotal => "token_total",
            SegmentId::TokenRate => "token_rate",
            SegmentId::Cost => "cost",
            SegmentId::ContextPct => "context_pct",
            SegmentId::ContextTotal => "context_total",
            SegmentId::TimeSpent => "time_spent",
            SegmentId::Time => "time",
            SegmentId::Session => "session",
            SegmentId::Hostname => "hostname",
            SegmentId::CacheRead => "cache_read",
            SegmentId::CacheWrite => "cache_write",
            SegmentId::CacheHit => "cache_hit",
            SegmentId::Cache => "cache",
            SegmentId::SessionName => "session_name",
            SegmentId::Usage => "usage",
            SegmentId::Collab => "collab",
            SegmentId::Stream => "stream",
            SegmentId::Vim => "vim",
        }
    }

    /// Parse a configured identifier; `None` when it is not in the catalog.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| id.as_str() == value)
    }

    /// True for the segments the context gauge absorbs in `embedded` mode.
    pub fn is_context(self) -> bool {
        matches!(self, SegmentId::ContextPct | SegmentId::ContextTotal)
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which half of the bar a configured segment belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentSide {
    /// Left group, reading outward from the left edge.
    Left,
    /// Right group, reading outward from the right edge.
    Right,
}

impl SegmentSide {
    /// The side's string label (`left` / `right`).
    pub fn as_str(self) -> &'static str {
        match self {
            SegmentSide::Left => "left",
            SegmentSide::Right => "right",
        }
    }
}

/// One configured status-line item: a catalog segment on a side.
///
/// The TypeScript source keeps the two groups as separate arrays; this pair is
/// the port-level handle used by `set_segments`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StatusLineItem {
    /// The segment to render.
    pub id: SegmentId,
    /// The group the segment belongs to.
    pub side: SegmentSide,
}

impl StatusLineItem {
    /// Create an item from a segment id and side.
    pub const fn new(id: SegmentId, side: SegmentSide) -> Self {
        StatusLineItem { id, side }
    }

    /// A left-group item.
    pub const fn left(id: SegmentId) -> Self {
        StatusLineItem { id, side: SegmentSide::Left }
    }

    /// A right-group item.
    pub const fn right(id: SegmentId) -> Self {
        StatusLineItem { id, side: SegmentSide::Right }
    }
}

/// Status-line preset names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StatusLinePreset {
    /// Balanced default bar.
    #[default]
    Default,
    /// Three left segments, compact right group.
    Minimal,
    /// Dense bar without the brand segment.
    Compact,
    /// Every monitoring segment, powerline separators.
    Full,
    /// Full bar tuned for Nerd Font icons.
    Nerd,
    /// ASCII-only glyphs.
    Ascii,
    /// User-defined baseline.
    Custom,
}

impl StatusLinePreset {
    /// Every preset name, in declaration order.
    pub const ALL: &'static [StatusLinePreset] = &[
        StatusLinePreset::Default,
        StatusLinePreset::Minimal,
        StatusLinePreset::Compact,
        StatusLinePreset::Full,
        StatusLinePreset::Nerd,
        StatusLinePreset::Ascii,
        StatusLinePreset::Custom,
    ];

    /// The stable preset name used by settings.
    pub fn as_str(self) -> &'static str {
        match self {
            StatusLinePreset::Default => "default",
            StatusLinePreset::Minimal => "minimal",
            StatusLinePreset::Compact => "compact",
            StatusLinePreset::Full => "full",
            StatusLinePreset::Nerd => "nerd",
            StatusLinePreset::Ascii => "ascii",
            StatusLinePreset::Custom => "custom",
        }
    }

    /// Parse a preset name; `None` when unknown.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.as_str() == value)
    }
}

impl fmt::Display for StatusLinePreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the gap line between the two groups reacts to context usage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContextLineMode {
    /// Solid accent line, no context feedback.
    Off,
    /// Used share in accent, remainder in the border color.
    Percentage,
    /// Percentage plus compaction boundary markers.
    Annotated,
    /// Markers plus context labels absorbed from configured segments.
    #[default]
    Embedded,
}

impl ContextLineMode {
    /// Every mode, in declaration order.
    pub const ALL: &'static [ContextLineMode] = &[
        ContextLineMode::Off,
        ContextLineMode::Percentage,
        ContextLineMode::Annotated,
        ContextLineMode::Embedded,
    ];

    /// The stable mode name used by settings.
    pub fn as_str(self) -> &'static str {
        match self {
            ContextLineMode::Off => "off",
            ContextLineMode::Percentage => "percentage",
            ContextLineMode::Annotated => "annotated",
            ContextLineMode::Embedded => "embedded",
        }
    }

    /// Parse a mode name; `None` when unknown.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|m| m.as_str() == value)
    }
}

/// Separator glyph styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SeparatorStyle {
    /// Thick powerline arrows with contrasting caps.
    Powerline,
    /// Thin powerline arrows with contrasting caps; the fallback style.
    #[default]
    PowerlineThin,
    /// ` / ` text separator.
    Slash,
    /// ` │ ` text separator.
    Pipe,
    /// Solid block glyph.
    Block,
    /// Single space.
    None,
    /// ASCII `>` / `<` separators.
    Ascii,
}

impl SeparatorStyle {
    /// Every style, in declaration order.
    pub const ALL: &'static [SeparatorStyle] = &[
        SeparatorStyle::Powerline,
        SeparatorStyle::PowerlineThin,
        SeparatorStyle::Slash,
        SeparatorStyle::Pipe,
        SeparatorStyle::Block,
        SeparatorStyle::None,
        SeparatorStyle::Ascii,
    ];

    /// The stable style name used by settings.
    pub fn as_str(self) -> &'static str {
        match self {
            SeparatorStyle::Powerline => "powerline",
            SeparatorStyle::PowerlineThin => "powerline-thin",
            SeparatorStyle::Slash => "slash",
            SeparatorStyle::Pipe => "pipe",
            SeparatorStyle::Block => "block",
            SeparatorStyle::None => "none",
            SeparatorStyle::Ascii => "ascii",
        }
    }

    /// Parse a style name; `None` when unknown.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.as_str() == value)
    }
}

/// `pi` segment options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PiOptions {
    /// Append the active profile tag to the brand glyph.
    pub show_profile: Option<bool>,
}

impl PiOptions {
    /// Effective `show_profile`; defaults to `true`.
    pub fn show_profile(&self) -> bool {
        self.show_profile.unwrap_or(true)
    }
}

/// `model` segment options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelOptions {
    /// Append the thinking-level tail.
    pub show_thinking_level: Option<bool>,
}

impl ModelOptions {
    /// Effective `show_thinking_level`; defaults to `true`.
    pub fn show_thinking_level(&self) -> bool {
        self.show_thinking_level.unwrap_or(true)
    }
}

/// `path` segment options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathOptions {
    /// Replace a leading home directory with `~`.
    pub abbreviate: Option<bool>,
    /// Maximum displayed path length.
    pub max_length: Option<usize>,
    /// Strip the work-prefix display roots.
    pub strip_work_prefix: Option<bool>,
}

impl PathOptions {
    /// Effective `abbreviate`; defaults to `true`.
    pub fn abbreviate(&self) -> bool {
        self.abbreviate.unwrap_or(true)
    }

    /// Effective `max_length`; defaults to 40.
    pub fn max_length(&self) -> usize {
        self.max_length.unwrap_or(40)
    }

    /// Effective `strip_work_prefix`; defaults to `true`.
    pub fn strip_work_prefix(&self) -> bool {
        self.strip_work_prefix.unwrap_or(true)
    }
}

/// `git` segment options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitOptions {
    /// Show the branch name.
    pub show_branch: Option<bool>,
    /// Show the staged count.
    pub show_staged: Option<bool>,
    /// Show the unstaged count.
    pub show_unstaged: Option<bool>,
    /// Show the untracked count.
    pub show_untracked: Option<bool>,
}

impl GitOptions {
    /// Effective `show_branch`; defaults to `true`.
    pub fn show_branch(&self) -> bool {
        self.show_branch.unwrap_or(true)
    }

    /// Effective `show_staged`; defaults to `true`.
    pub fn show_staged(&self) -> bool {
        self.show_staged.unwrap_or(true)
    }

    /// Effective `show_unstaged`; defaults to `true`.
    pub fn show_unstaged(&self) -> bool {
        self.show_unstaged.unwrap_or(true)
    }

    /// Effective `show_untracked`; defaults to `true`.
    pub fn show_untracked(&self) -> bool {
        self.show_untracked.unwrap_or(true)
    }
}

/// Clock format for the `time` segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeFormat {
    /// 12-hour clock with an `am`/`pm` suffix.
    H12,
    /// 24-hour clock.
    H24,
}

/// `time` segment options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeOptions {
    /// Clock format.
    pub format: Option<TimeFormat>,
    /// Append seconds.
    pub show_seconds: Option<bool>,
}

impl TimeOptions {
    /// Effective `format`; defaults to 24-hour.
    pub fn format(&self) -> TimeFormat {
        self.format.unwrap_or(TimeFormat::H24)
    }

    /// Effective `show_seconds`; defaults to `false`.
    pub fn show_seconds(&self) -> bool {
        self.show_seconds.unwrap_or(false)
    }
}

/// Per-segment display options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SegmentOptions {
    /// `pi` options.
    pub pi: Option<PiOptions>,
    /// `model` options.
    pub model: Option<ModelOptions>,
    /// `path` options.
    pub path: Option<PathOptions>,
    /// `git` options.
    pub git: Option<GitOptions>,
    /// `time` options.
    pub time: Option<TimeOptions>,
}

impl SegmentOptions {
    /// Merge `over` on top of `base`; set fields win, unset fields fall through.
    pub fn merge(base: &SegmentOptions, over: &SegmentOptions) -> SegmentOptions {
        SegmentOptions {
            pi: merge_group(base.pi.as_ref(), over.pi.as_ref(), |b, o| PiOptions {
                show_profile: o.show_profile.or(b.show_profile),
            }),
            model: merge_group(base.model.as_ref(), over.model.as_ref(), |b, o| {
                ModelOptions {
                    show_thinking_level: o.show_thinking_level.or(b.show_thinking_level),
                }
            }),
            path: merge_group(base.path.as_ref(), over.path.as_ref(), |b, o| {
                PathOptions {
                    abbreviate: o.abbreviate.or(b.abbreviate),
                    max_length: o.max_length.or(b.max_length),
                    strip_work_prefix: o.strip_work_prefix.or(b.strip_work_prefix),
                }
            }),
            git: merge_group(base.git.as_ref(), over.git.as_ref(), |b, o| GitOptions {
                show_branch: o.show_branch.or(b.show_branch),
                show_staged: o.show_staged.or(b.show_staged),
                show_unstaged: o.show_unstaged.or(b.show_unstaged),
                show_untracked: o.show_untracked.or(b.show_untracked),
            }),
            time: merge_group(base.time.as_ref(), over.time.as_ref(), |b, o| {
                TimeOptions {
                    format: o.format.or(b.format),
                    show_seconds: o.show_seconds.or(b.show_seconds),
                }
            }),
        }
    }
}

fn merge_group<T: Clone, F: FnOnce(&T, &T) -> T>(
    base: Option<&T>,
    over: Option<&T>,
    merge: F,
) -> Option<T> {
    match (base, over) {
        (None, None) => None,
        (Some(_), None) => base.cloned(),
        (None, Some(over)) => Some(over.clone()),
        (Some(base), Some(over)) => Some(merge(base, over)),
    }
}

/// User-facing status-line settings; every field is optional and merges over
/// the selected preset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusLineSettings {
    /// Preset supplying the baseline groups, separator, and options.
    pub preset: Option<StatusLinePreset>,
    /// Left-group override.
    pub left_segments: Option<Vec<SegmentId>>,
    /// Right-group override.
    pub right_segments: Option<Vec<SegmentId>>,
    /// Separator style override.
    pub separator: Option<SeparatorStyle>,
    /// Per-segment option overrides.
    pub segment_options: Option<SegmentOptions>,
    /// Render the hook-status rows under the bar.
    pub show_hook_status: Option<bool>,
    /// Enable hash-derived session accent colors.
    pub session_accent: Option<bool>,
    /// Drop the status-line background fill and powerline caps.
    pub transparent: Option<bool>,
    /// Render the thinking level as a compact leading glyph.
    pub compact_thinking_level: Option<bool>,
    /// Context gauge mode.
    pub context_line: Option<ContextLineMode>,
}

/// A preset's concrete segment groups, separator, and options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetDef {
    /// Left-group segments in display order.
    pub left_segments: Vec<SegmentId>,
    /// Right-group segments in display order.
    pub right_segments: Vec<SegmentId>,
    /// Separator style.
    pub separator: SeparatorStyle,
    /// Per-segment options.
    pub segment_options: SegmentOptions,
}

/// Baseline segments used when the `custom` preset has no overrides.
pub const CUSTOM_STATUS_LINE_DEFAULTS_LEFT: &[SegmentId] =
    &[SegmentId::Vim, SegmentId::Model, SegmentId::Mode, SegmentId::Path, SegmentId::Git, SegmentId::Pr];

/// Baseline right segments for the `custom` preset.
pub const CUSTOM_STATUS_LINE_DEFAULTS_RIGHT: &[SegmentId] = &[
    SegmentId::SessionName,
    SegmentId::TokenTotal,
    SegmentId::Cost,
    SegmentId::ContextPct,
];

/// Look up a preset by name; unknown names fall back to `default`.
pub fn preset(name: StatusLinePreset) -> PresetDef {
    match name {
        StatusLinePreset::Default => PresetDef {
            left_segments: vec![
                SegmentId::Pi,
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
            right_segments: vec![SegmentId::SessionName],
            separator: SeparatorStyle::PowerlineThin,
            segment_options: SegmentOptions {
                model: Some(ModelOptions {
                    show_thinking_level: Some(true),
                }),
                path: Some(PathOptions {
                    abbreviate: Some(true),
                    max_length: Some(40),
                    strip_work_prefix: Some(true),
                }),
                git: Some(GitOptions {
                    show_branch: Some(true),
                    show_staged: Some(true),
                    show_unstaged: Some(true),
                    show_untracked: Some(true),
                }),
                ..SegmentOptions::default()
            },
        },
        StatusLinePreset::Minimal => PresetDef {
            left_segments: vec![SegmentId::Vim, SegmentId::Path, SegmentId::Git],
            right_segments: vec![
                SegmentId::SessionName,
                SegmentId::Mode,
                SegmentId::ContextPct,
            ],
            separator: SeparatorStyle::Slash,
            segment_options: SegmentOptions {
                path: Some(PathOptions {
                    abbreviate: Some(true),
                    max_length: Some(30),
                    strip_work_prefix: None,
                }),
                git: Some(GitOptions {
                    show_branch: Some(true),
                    show_staged: Some(false),
                    show_unstaged: Some(false),
                    show_untracked: Some(false),
                }),
                ..SegmentOptions::default()
            },
        },
        StatusLinePreset::Compact => PresetDef {
            left_segments: vec![
                SegmentId::Vim,
                SegmentId::Model,
                SegmentId::Mode,
                SegmentId::Git,
                SegmentId::Pr,
            ],
            right_segments: vec![
                SegmentId::SessionName,
                SegmentId::Cost,
                SegmentId::ContextPct,
            ],
            separator: SeparatorStyle::PowerlineThin,
            segment_options: SegmentOptions {
                model: Some(ModelOptions {
                    show_thinking_level: Some(false),
                }),
                git: Some(GitOptions {
                    show_branch: Some(true),
                    show_staged: Some(true),
                    show_unstaged: Some(true),
                    show_untracked: Some(false),
                }),
                ..SegmentOptions::default()
            },
        },
        StatusLinePreset::Full => PresetDef {
            left_segments: vec![
                SegmentId::Pi,
                SegmentId::Vim,
                SegmentId::Hostname,
                SegmentId::Model,
                SegmentId::Mode,
                SegmentId::Path,
                SegmentId::Git,
                SegmentId::Pr,
                SegmentId::Subagents,
            ],
            right_segments: vec![
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
            separator: SeparatorStyle::Powerline,
            segment_options: SegmentOptions {
                model: Some(ModelOptions {
                    show_thinking_level: Some(true),
                }),
                path: Some(PathOptions {
                    abbreviate: Some(true),
                    max_length: Some(50),
                    strip_work_prefix: None,
                }),
                git: Some(GitOptions {
                    show_branch: Some(true),
                    show_staged: Some(true),
                    show_unstaged: Some(true),
                    show_untracked: Some(true),
                }),
                time: Some(TimeOptions {
                    format: Some(TimeFormat::H24),
                    show_seconds: Some(false),
                }),
                ..SegmentOptions::default()
            },
        },
        StatusLinePreset::Nerd => PresetDef {
            left_segments: vec![
                SegmentId::Pi,
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
            right_segments: vec![
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
            separator: SeparatorStyle::Powerline,
            segment_options: SegmentOptions {
                model: Some(ModelOptions {
                    show_thinking_level: Some(true),
                }),
                path: Some(PathOptions {
                    abbreviate: Some(true),
                    max_length: Some(60),
                    strip_work_prefix: None,
                }),
                git: Some(GitOptions {
                    show_branch: Some(true),
                    show_staged: Some(true),
                    show_unstaged: Some(true),
                    show_untracked: Some(true),
                }),
                time: Some(TimeOptions {
                    format: Some(TimeFormat::H24),
                    show_seconds: Some(true),
                }),
                ..SegmentOptions::default()
            },
        },
        StatusLinePreset::Ascii => PresetDef {
            left_segments: vec![
                SegmentId::Vim,
                SegmentId::Model,
                SegmentId::Mode,
                SegmentId::Path,
                SegmentId::Git,
                SegmentId::Pr,
            ],
            right_segments: vec![
                SegmentId::SessionName,
                SegmentId::TokenTotal,
                SegmentId::Cost,
                SegmentId::ContextPct,
            ],
            separator: SeparatorStyle::Ascii,
            segment_options: SegmentOptions {
                model: Some(ModelOptions {
                    show_thinking_level: Some(true),
                }),
                path: Some(PathOptions {
                    abbreviate: Some(true),
                    max_length: Some(40),
                    strip_work_prefix: None,
                }),
                git: Some(GitOptions {
                    show_branch: Some(true),
                    show_staged: Some(true),
                    show_unstaged: Some(true),
                    show_untracked: Some(true),
                }),
                ..SegmentOptions::default()
            },
        },
        StatusLinePreset::Custom => PresetDef {
            left_segments: CUSTOM_STATUS_LINE_DEFAULTS_LEFT.to_vec(),
            right_segments: CUSTOM_STATUS_LINE_DEFAULTS_RIGHT.to_vec(),
            separator: SeparatorStyle::PowerlineThin,
            segment_options: SegmentOptions::default(),
        },
    }
}

/// Settings with every field resolved from a preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveSettings {
    /// Resolved left group.
    pub left_segments: Vec<SegmentId>,
    /// Resolved right group.
    pub right_segments: Vec<SegmentId>,
    /// Resolved separator style.
    pub separator: SeparatorStyle,
    /// Resolved per-segment options.
    pub segment_options: SegmentOptions,
    /// Resolved preset name.
    pub preset: StatusLinePreset,
    /// Render hook-status rows.
    pub show_hook_status: bool,
    /// Enable session accent colors.
    pub session_accent: bool,
    /// Drop the background fill and caps.
    pub transparent: bool,
    /// Compact thinking-level glyph.
    pub compact_thinking_level: bool,
    /// Context gauge mode.
    pub context_line: ContextLineMode,
}

impl EffectiveSettings {
    /// Resolve `settings` over its preset, with optional group overrides.
    pub fn resolve(settings: &StatusLineSettings) -> EffectiveSettings {
        let preset = settings.preset.unwrap_or_default();
        let def = self::preset(preset);
        let mut options = def.segment_options;
        if let Some(over) = &settings.segment_options {
            options = SegmentOptions::merge(&options, over);
        }
        EffectiveSettings {

            left_segments: if preset == StatusLinePreset::Custom {
                settings
                    .left_segments
                    .clone()
                    .unwrap_or_else(|| def.left_segments.clone())
            } else {
                def.left_segments.clone()
            },
            right_segments: if preset == StatusLinePreset::Custom {
                settings
                    .right_segments
                    .clone()
                    .unwrap_or_else(|| def.right_segments.clone())
            } else {
                def.right_segments.clone()
            },
            separator: settings.separator.unwrap_or(def.separator),
            segment_options: options,
            preset,
            show_hook_status: settings.show_hook_status.unwrap_or(true),
            session_accent: session_accent_or_default(settings.session_accent),
            transparent: settings.transparent.unwrap_or(false),
            compact_thinking_level: settings.compact_thinking_level.unwrap_or(false),
            context_line: settings.context_line.unwrap_or_default(),
        }
    }
}

fn session_accent_or_default(value: Option<bool>) -> bool {
    // `false` disables hash-derived accents; `true` or omission enables them.
    value.unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_catalog_matches_typescript() {
        let ids: Vec<&str> = SegmentId::ALL.iter().map(|id| id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "pi", "status", "model", "mode", "path", "git", "pr", "subagents",
                "token_in", "token_out", "token_total", "token_rate", "cost",
                "context_pct", "context_total", "time_spent", "time", "session",
                "hostname", "cache_read", "cache_write", "cache_hit", "cache",
                "session_name", "usage", "collab", "stream", "vim",
            ]
        );
        assert_eq!(SegmentId::parse("context_pct"), Some(SegmentId::ContextPct));
        assert_eq!(SegmentId::parse("nope"), None);
        assert!(SegmentId::ContextPct.is_context());
        assert!(!SegmentId::Model.is_context());
    }

    #[test]
    fn presets_match_typescript() {
        let default = preset(StatusLinePreset::Default);
        let left: Vec<&str> = default.left_segments.iter().map(|id| id.as_str()).collect();
        assert_eq!(
            left,
            vec![
                "pi", "vim", "model", "mode", "collab", "stream", "path", "git",
                "pr", "context_pct", "cost",
            ]
        );
        assert_eq!(
            default.right_segments.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            vec!["session_name"]
        );
        assert_eq!(default.separator, SeparatorStyle::PowerlineThin);
        assert_eq!(
            default.segment_options.path.as_ref().unwrap().max_length(),
            40
        );

        let minimal = preset(StatusLinePreset::Minimal);
        assert_eq!(minimal.separator, SeparatorStyle::Slash);
        assert_eq!(
            minimal.segment_options.git.as_ref().unwrap().show_staged(),
            false
        );

        let full = preset(StatusLinePreset::Full);
        assert_eq!(full.separator, SeparatorStyle::Powerline);
        assert_eq!(
            full.right_segments.len(),
            11,
            "full preset right group has eleven segments"
        );

        let nerd = preset(StatusLinePreset::Nerd);
        assert_eq!(nerd.separator, SeparatorStyle::Powerline);
        assert_eq!(nerd.right_segments.len(), 12);

        let ascii = preset(StatusLinePreset::Ascii);
        assert_eq!(ascii.separator, SeparatorStyle::Ascii);

        let custom = preset(StatusLinePreset::Custom);
        assert_eq!(
            custom.left_segments.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            vec!["vim", "model", "mode", "path", "git", "pr"]
        );
        assert_eq!(custom.separator, SeparatorStyle::PowerlineThin);
    }

    #[test]
    fn unknown_preset_name_falls_back_to_default() {
        assert_eq!(
            StatusLinePreset::parse("nope").unwrap_or_default(),
            StatusLinePreset::Default
        );
        assert_eq!(
            preset(StatusLinePreset::parse("nope").unwrap_or_default()).left_segments,
            preset(StatusLinePreset::Default).left_segments
        );
    }

    #[test]
    fn option_merge_prefers_overrides() {
        let base = SegmentOptions {
            path: Some(PathOptions {
                abbreviate: Some(true),
                max_length: Some(40),
                strip_work_prefix: Some(true),
            }),
            ..SegmentOptions::default()
        };
        let over = SegmentOptions {
            path: Some(PathOptions {
                max_length: Some(12),
                ..PathOptions::default()
            }),
            ..SegmentOptions::default()
        };
        let merged = SegmentOptions::merge(&base, &over);
        let path = merged.path.unwrap();
        assert_eq!(path.max_length(), 12);
        assert_eq!(path.abbreviate(), true);
        assert_eq!(path.strip_work_prefix(), true);
    }

    #[test]
    fn settings_resolve_over_preset() {
        // Only the `custom` preset honors group overrides; other presets keep
        // their catalog groups and merge option overrides field-by-field.
        let settings = StatusLineSettings {
            preset: Some(StatusLinePreset::Custom),
            left_segments: Some(vec![SegmentId::Model]),
            segment_options: Some(SegmentOptions {
                model: Some(ModelOptions {
                    show_thinking_level: Some(false),
                }),
                ..SegmentOptions::default()
            }),
            ..StatusLineSettings::default()
        };
        let eff = EffectiveSettings::resolve(&settings);
        assert_eq!(eff.left_segments, vec![SegmentId::Model]);
        assert_eq!(eff.separator, SeparatorStyle::PowerlineThin);
        assert!(!eff.segment_options.model.unwrap().show_thinking_level());
        assert!(eff.show_hook_status);
        assert!(eff.session_accent);
        assert!(!eff.transparent);
        assert_eq!(eff.context_line, ContextLineMode::Embedded);
    }

    #[test]
    fn non_custom_preset_ignores_group_overrides() {
        let settings = StatusLineSettings {
            preset: Some(StatusLinePreset::Minimal),
            left_segments: Some(vec![SegmentId::Model]),
            ..StatusLineSettings::default()
        };
        let eff = EffectiveSettings::resolve(&settings);
        assert_eq!(
            eff.left_segments,
            preset(StatusLinePreset::Minimal).left_segments
        );
    }
}
