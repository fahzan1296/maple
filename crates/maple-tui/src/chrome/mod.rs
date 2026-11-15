//! Terminal chrome: message frames, compact diffs, context indicators, and
//! overlay boxes.
//!
//! Ported from `l/src/chrome/**` (message-frame, diff, context-usage,
//! overlay-box) and `l/src/overlays/overlay-box.ts`.

pub mod context;
pub mod diff;
pub mod message;
pub mod overlay_box;

pub use context::{
    context_meter_thresholds, context_usage_level, format_context_usage, plan_context_map,
    ContextGauge, ContextGaugeStyles, ContextLineMode, ContextMap, ContextMapCell,
    ContextMeterThresholds, ContextSegmentId, ContextSymbols, ContextUsageLevel,
    CONTEXT_BUFFER_GLYPH, CONTEXT_COMPACTION_GLYPH, CONTEXT_CONTENT_GLYPH, CONTEXT_FREE_GLYPH,
    CONTEXT_MESSAGES_GLYPH, CONTEXT_SPECULATION_GLYPH,
};
pub use diff::{
    diff_stats, parse_diff_line, parse_hunks, render_diff, DiffHunk, DiffLine, DiffPrefix, DiffStats,
    DiffTheme, DiffView,
};
pub use message::{Message, MessageDivider, MessageNotice, MessageRole, MessageTheme, NoticeTheme};
pub use overlay_box::{
    bottom_border, divider, divider_split, split_body_width, split_divider_col, split_row,
    top_border, top_border_split, OverlayPanel, OverlayTheme, PanelDivider, PanelRows,
};
