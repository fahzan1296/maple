//! Shared formatting, truncation, and styling helpers for the render
//! primitives.
//!
//! Every helper here is deterministic: they shape strings for display and never
//! touch I/O, except [`resolve_image_options`], which reads the terminal size
//! (the reference reads `process.stdout.rows`). The few process-wide knobs
//! (tight-layout padding, the expand key hint, sixel passthrough, inline image
//! caps) are set once at startup by the embedder; `render` only reads them.
//!
//! Reference: `l/src/render/utils.ts` contributes the tree-prefix guides, the
//! background-aware width fitter, the incremental cache-key hasher, and the
//! bounded render cache; `l/src/render/render-utils.ts` contributes the other
//! primitives in this file.
//!
//! Deviations:
//! - `pad_to_width` truncates with the Unicode ellipsis and returns its input
//!   unchanged at width zero, matching `padToWidth`. Padding is delegated to
//!   the pad-only [`crate::text_shape::pad_to_width`], which leaves wide input
//!   untouched; the two are complementary entry points, not wrappers.
//! - `Hasher` uses FNV-1a 64 rather than `Bun.hash.xxHash64`: the crate has no
//!   hashing dependency and keys need determinism, not collision resistance.
//!   `update` is the reference's `str` feed; `update_flag`, `update_u32`,
//!   `update_u64`, and `update_optional` keep its byte encodings; `digest_u32`
//!   exposes the low 32 bits.
//! - `RenderCache` implements the LRU in-crate (the reference's
//!   `@oh-my-pi/pi-utils/lru` is not vendored): least-recently-used evicts
//!   first, re-setting a key refreshes it, and misses allocate nothing.
//! - `get_tree_branch` and `get_tree_continue_prefix` take `(is_last, theme)`,
//!   exactly as every reference call site does; the reference has no
//!   depth/`TreeContext` overload to port.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::LazyLock;
use regex::Regex;

use crate::ansi_scan::{scan, Token};
use crate::component::Component;
use crate::style::{Color, Style};
use crate::text_shape::visible_width;
use crate::theme::{Theme, ThemeBg, ThemeColor};
use unicode_width::UnicodeWidthStr;
use unicode_segmentation::UnicodeSegmentation;

pub use crate::overlays::jobs_panel::format_age;
pub use crate::status_line::segments::{format_duration, format_number};
pub use crate::text_shape::{truncate_to_width, wrap_text_with_ansi, Ellipsis};
pub use crate::thinking::ConfiguredThinkingLevel;
pub use crate::tools::ast_grep::format_count;
pub use crate::tools::native_view::{parse_diagnostic_message, DiagnosticSeverity, ParsedDiagnostic};
pub use crate::tools::output_meta::format_bytes;

/// Display columns one tab expands to.
pub const TAB_WIDTH: usize = 3;

/// Marker drawn in a code-frame gutter.
pub type CodeFrameMarker = str;

/// Lifecycle state shared by output frames and tool cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Pending,
    Running,
    Success,
    Error,
    Warning,
}

impl State {
    /// The background token this state tints its frame with.
    pub fn bg_color(self) -> ThemeBg {
        match self {
            State::Success => ThemeBg::ToolSuccessBg,
            State::Error => ThemeBg::ToolErrorBg,
            _ => ThemeBg::ToolPendingBg,
        }
    }

    /// Whether the state is a finished (`success`/`error`) one.
    pub fn is_finished(self) -> bool {
        matches!(self, State::Success | State::Error)
    }
}

/// Status glyphs supported by the shared tool header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolUiStatus {
    Success,
    Done,
    Error,
    Warning,
    Info,
    Pending,
    Running,
    Aborted,
}

/// Semantic foreground colors used by tool labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolUIColor {
    Success,
    Error,
    Warning,
    Accent,
    Muted,
}

impl ToolUIColor {
    fn token(self) -> ThemeColor {
        match self {
            ToolUIColor::Success => ThemeColor::Success,
            ToolUIColor::Error => ThemeColor::Error,
            ToolUIColor::Warning => ThemeColor::Warning,
            ToolUIColor::Accent => ThemeColor::Accent,
            ToolUIColor::Muted => ThemeColor::Muted,
        }
    }
}

/// Added/removed line counts and hunk metadata for a unified diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
    pub hunks: usize,
    pub lines: usize,
}

// ── Padding and tabs ────────────────────────────────────────────────────────

/// Expand every tab to [`TAB_WIDTH`] spaces so tab stops cannot desync the
/// frame layout.
pub fn replace_tabs(text: &str) -> String {
    if !text.contains('\t') {
        return text.to_string();
    }
    text.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// A string of `n` spaces.
pub fn padding(n: usize) -> String {
    " ".repeat(n)
}

static TIGHT_LAYOUT: AtomicBool = AtomicBool::new(false);

/// Set the process-wide tight-layout flag. Tight mode drops one column of
/// configured horizontal padding so dense views fit more content.
pub fn set_tight_layout(tight: bool) {
    TIGHT_LAYOUT.store(tight, Ordering::Relaxed);
}

/// Whether tight layout is active.
pub fn is_tight_layout() -> bool {
    TIGHT_LAYOUT.load(Ordering::Relaxed)
}

/// Resolve configured horizontal padding under the current tight-layout flag.
pub fn get_padding_x(base: usize) -> usize {
    if is_tight_layout() {
        base.saturating_sub(1)
    } else {
        base
    }
}

// ── Line shaping ────────────────────────────────────────────────────────────

/// Collapse a possibly multi-line string into one line; whitespace runs
/// (tabs included) become a single space.
pub fn flatten_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse to one line, then cap it at `max_width` display cells.
pub fn preview_line(text: &str, max_width: usize, ellipsis: Ellipsis) -> String {
    truncate_to_width(&flatten_line(text), max_width, ellipsis)
}

/// Normalize stray carriage returns in model-authored display text: CRLF
/// becomes LF and lone CR runs collapse to a single space.
pub fn sanitize_carriage_returns(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Sanitize raw ask option labels into unique, action-safe display copies.
///
/// Reference: `l/src/render/render-utils.ts::disambiguateDisplayLabels`.
/// Degenerate input can sanitize alike (`Retry\rnow`/`Retry now`) or match a
/// runtime action row (`Other (type your own)`); both would answer the wrong
/// row, so colliding entries take a numeric suffix. Order and length are
/// preserved, so indices still align with the original labels for mapping
/// answers and dialog state back.
pub fn disambiguate_display_labels(raw_labels: &[String], reserved_labels: &[String]) -> Vec<String> {
    let mut taken: HashSet<String> = reserved_labels.iter().cloned().collect();
    raw_labels
        .iter()
        .map(|raw| {
            let base = sanitize_carriage_returns(raw);
            let mut candidate = base.clone();
            let mut suffix = 2;
            while taken.contains(&candidate) {
                candidate = format!("{base} ({suffix})");
                suffix += 1;
            }
            taken.insert(candidate.clone());
            candidate
        })
        .collect()
}

/// Split terminal output without letting carriage-return progress updates
/// corrupt following rows: the final segment after a bare CR wins.
pub fn split_terminal_output_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| match line.rfind('\r') {
            Some(index) => line[index + 1..].to_string(),
            None => line.to_string(),
        })
        .collect()
}

/// Split tool text into display rows: CRLF/LF split, CR progress overwrites
/// collapse to the final segment, tabs expand.
pub fn sanitize_display_lines(text: &str) -> Vec<String> {
    split_terminal_output_lines(text)
        .into_iter()
        .map(|line| replace_tabs(&line))
        .collect()
}

/// Select a leading line window and report how many rows were hidden.
pub fn capped_head_lines(lines: &[String], max: usize) -> (Vec<String>, usize) {
    let count = lines.len().min(max);
    (lines[..count].to_vec(), lines.len() - count)
}

/// A slice of a rendered row: the kept text and its display width.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SliceResult {
    pub text: String,
    pub width: usize,
}

/// Slice `line` by display column, ANSI-aware. `strict` drops a wide grapheme
/// that would cross the end boundary instead of overflowing it.
pub fn slice_by_column(line: &str, start: usize, length: usize, strict: bool) -> SliceResult {
    let end = start + length;
    let tokens = scan(line);
    let mut text = String::new();
    let mut column = 0usize;
    let mut index = 0usize;
    while index < tokens.len() {
        match tokens[index] {
            Token::Escape(seq) => {
                text.push_str(seq);
                index += 1;
            }
            Token::Text(chunk) => {
                let mut done = false;
                for grapheme in chunk.graphemes(true) {
                    let width = UnicodeWidthStr::width(grapheme);
                    let grapheme_end = column + width;
                    if grapheme_end <= start {
                        column = grapheme_end;
                        continue;
                    }
                    if column >= end || (strict && grapheme_end > end) {
                        done = true;
                        break;
                    }
                    text.push_str(grapheme);
                    column = grapheme_end;
                }
                index += 1;
                if done {
                    break;
                }
            }
        }
    }
    // Trailing escape sequences (a reset, say) ride along with the slice.
    while let Some(Token::Escape(seq)) = tokens.get(index) {
        text.push_str(seq);
        index += 1;
    }
    SliceResult {
        width: column.saturating_sub(start.min(column)),
        text,
    }
}

/// Keep both ends of a single-line label without splitting wide characters.
pub fn truncate_middle_to_width(text: &str, max_width: usize) -> String {
    let width = visible_width(text);
    if width <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return if max_width == 1 { "…".to_string() } else { String::new() };
    }
    let tail_width = (max_width - 1).div_ceil(2);
    let head_width = max_width - 1 - tail_width;
    let head = slice_by_column(text, 0, head_width, true);
    let tail = slice_by_column(text, width - tail_width, tail_width, true);
    format!("{}…{}", head.text, tail.text)
}

/// Pad `text` with spaces to exactly `width` columns, or truncate it when it
/// is wider. `bg` optionally tints the whole field.
pub fn pad_or_truncate(text: &str, width: usize, bg: Option<Color>) -> String {
    if width == 0 {
        return String::new();
    }
    let current = visible_width(text);
    let fitted = if current < width {
        let mut out = String::with_capacity(text.len() + width - current);

        out.push_str(text);
        out.extend(std::iter::repeat_n(' ', width - current));
        out
    } else if current > width {
        truncate_to_width(text, width, Ellipsis::Omit)
    } else {
        text.to_string()
    };
    match bg {
        Some(color) => apply_background(&fitted, color),
        None => fitted,
    }
}

/// Pad `text` to `width` columns, or truncate it, optionally tinting the field
/// with a raw SGR background sequence.
pub fn pad_or_truncate_ansi(text: &str, width: usize, bg_ansi: Option<&str>) -> String {
    let fitted = pad_or_truncate(text, width, None);
    match bg_ansi {
        Some(ansi) => apply_background_ansi(&fitted, ansi),
        None => fitted,
    }
}

/// Fit `text` to exactly `width` display columns, then optionally tint the
/// whole field with a background styler.
///
/// Reference: `l/src/render/utils.ts::padToWidth`. Short text is padded, wide
/// text is cut with the Unicode ellipsis, and a zero width returns the input
/// unchanged (still tinted when `bg` is given). Padding itself is delegated
/// to [`crate::text_shape::pad_to_width`]; that helper is the pad-only entry
/// point and deliberately leaves wide input untouched.
pub fn pad_to_width(text: &str, width: usize, bg: Option<&dyn Fn(&str) -> String>) -> String {
    let fitted = if width == 0 || visible_width(text) == width {
        text.to_string()
    } else if visible_width(text) < width {
        crate::text_shape::pad_to_width(text, width)
    } else {
        truncate_to_width(text, width, Ellipsis::Unicode)
    };
    let drift = width.saturating_sub(visible_width(&fitted));
    let mut padded = fitted;
    if drift > 0 {
        padded.extend(std::iter::repeat_n(' ', drift));
    }
    match bg {
        Some(styler) => styler(&padded),
        None => padded,
    }
}

/// Wrap `text` in a raw SGR background, closing with the background reset.
pub fn apply_background_ansi(text: &str, bg_ansi: &str) -> String {
    format!("{bg_ansi}{text}\x1b[49m")
}

/// Re-apply a raw SGR background after every inner reset.
pub fn stabilize_background_ansi(text: &str, bg_ansi: &str) -> String {
    let mut out = String::with_capacity(text.len() + bg_ansi.len() * 4);
    let mut rest = text;
    while let Some(index) = rest.find('\x1b') {
        let (head, tail) = rest.split_at(index);
        out.push_str(head);
        if let Some(sequence) = escape_sequence(tail) {
            out.push_str(sequence);
            if is_full_reset(sequence) || sequence == "\x1b[49m" {
                out.push_str(bg_ansi);
            }
            rest = &tail[sequence.len()..];
        } else {
            out.push_str(tail);
            rest = "";
        }
    }
    out.push_str(rest);
    format!("{bg_ansi}{out}\x1b[49m")
}
/// Wrap `text` in an SGR background, closing with the background reset.
pub fn apply_background(text: &str, color: Color) -> String {
    format!("\x1b[{}m{}\x1b[49m", color.bg_sequence(), text)
}

/// Re-apply `bg` after every inner SGR reset so styled content cannot clear
/// the frame background mid-row.
pub fn stabilize_background(text: &str, bg: Color) -> String {
    let ansi = format!("\x1b[{}m", bg.bg_sequence());
    stabilize_background_ansi(text, &ansi)
}

fn escape_sequence(input: &str) -> Option<&str> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&0x1b) {
        return None;
    }
    if bytes.get(1) == Some(&b'[') {
        for (offset, byte) in bytes.iter().enumerate().skip(2) {
            if (0x40..=0x7e).contains(byte) {
                return Some(&input[..offset + 1]);
            }
        }
        return Some(input);
    }
    if bytes.get(1) == Some(&b']') {
        let mut index = 2;
        while index < bytes.len() {
            if bytes[index] == 0x07 {
                return Some(&input[..index + 1]);
            }
            if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                return Some(&input[..index + 2]);
            }
            index += 1;
        }
        return Some(input);
    }
    Some(input)
}

fn is_full_reset(sequence: &str) -> bool {
    let Some(params) = sequence
        .strip_prefix("\x1b[")
        .and_then(|rest| rest.strip_suffix('m'))
    else {
        return false;
    };
    params.is_empty() || params.split(';').all(|part| part.is_empty() || part == "0")
}

/// Render rows for a fixed viewport width: every row is truncated to `width`
/// so a frame can never overflow the terminal.
pub fn render_lines(lines: &[String], width: usize) -> Vec<String> {
    lines
        .iter()
        .map(|line| truncate_to_width(line, width, Ellipsis::Omit))
        .collect()
}

// ── Styling coalescing ──────────────────────────────────────────────────────

/// One styled run of text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledRun {
    pub style: Style,
    pub text: String,
}

/// Merge adjacent runs carrying the same style, preserving order.
pub fn coalesce_styles(runs: &[StyledRun]) -> Vec<StyledRun> {
    let mut out: Vec<StyledRun> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if last.style == run.style {
                last.text.push_str(&run.text);
                continue;
            }
        }
        out.push(run.clone());
    }
    out
}

/// Paint coalesced runs into one string.
pub fn paint_runs(runs: &[StyledRun]) -> String {
    let mut out = String::new();
    for run in coalesce_styles(runs) {
        out.push_str(&run.style.paint(&run.text));
    }
    out
}

/// Bold `text` with the additive SGR bold pair (`1`/`22`), matching the
/// theme's bold wrapper rather than a full reset.
pub fn bold_text(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

/// Paint `text` with a foreground token. Unknown tokens fall back to plain
/// text so a partially configured theme cannot panic a renderer.
pub fn paint<C: AsRef<str>>(theme: &Theme, color: C, text: &str) -> String {
    theme.fg(color, text).unwrap_or_else(|_| text.to_string())
}

/// The background token that tints an output frame for `state`.
pub fn state_bg_color(state: State) -> ThemeBg {
    state.bg_color()
}

// ── Theme label helpers ─────────────────────────────────────────────────────

/// Wrap `text` in the theme's badge brackets.
pub fn wrap_brackets(text: &str, theme: &Theme) -> String {
    format!(
        "{}{text}{}",
        theme.symbol("format.bracketLeft"),
        theme.symbol("format.bracketRight")
    )
}

/// The expand hint suffix, e.g. `ctrl+o: Expand` in brackets. Empty when the
/// view is already expanded or nothing more can be shown.
pub fn format_expand_hint(theme: &Theme, expanded: Option<bool>, has_more: Option<bool>) -> String {
    if expanded == Some(true) || has_more == Some(false) {
        return String::new();
    }
    let label = format!("{}: Expand", expand_key_hint());
    paint(theme, ThemeColor::Dim, &wrap_brackets(&label, theme))
}

static EXPAND_KEY_HINT: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Set the key hint shown in expand affordances (defaults to `ctrl+o`).
pub fn set_expand_key_hint(hint: impl Into<String>) {
    if let Ok(mut guard) = EXPAND_KEY_HINT.write() {
        *guard = hint.into();
    }
}

/// The key hint shown in expand affordances.
pub fn expand_key_hint() -> String {
    EXPAND_KEY_HINT
        .read()
        .map(|guard| {
            if guard.is_empty() {
                "ctrl+o".to_string()
            } else {
                guard.clone()
            }
        })
        .unwrap_or_else(|_| "ctrl+o".to_string())
}

/// Format a bracketed, colored badge such as `[done]`.
pub fn format_badge(label: &str, color: ToolUIColor, theme: &Theme) -> String {
    paint(theme, color.token(), &wrap_brackets(label, theme))
}

/// A "more items" suffix for a truncated list.
pub fn format_more_items(remaining: usize, item_type: &str) -> String {
    format!("… {remaining} more {}", pluralize(item_type, remaining))
}

/// English pluralization for the fixed labels the renderers use.
pub fn pluralize(word: &str, count: usize) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Resolve the status glyph for a tool state, coloring it with its token.
pub fn format_status_icon(
    status: ToolUiStatus,
    theme: &Theme,
    spinner_frame: Option<usize>,
) -> String {
    let (key, color) = match status {
        ToolUiStatus::Success | ToolUiStatus::Done => ("status.success", ThemeColor::Success),
        ToolUiStatus::Error => ("status.error", ThemeColor::Error),
        ToolUiStatus::Warning => ("status.warning", ThemeColor::Warning),
        ToolUiStatus::Info => ("status.info", ThemeColor::Accent),
        ToolUiStatus::Pending => ("status.pending", ThemeColor::Muted),
        ToolUiStatus::Aborted => ("status.aborted", ThemeColor::Error),
        ToolUiStatus::Running => {
            if let Some(frame) = spinner_frame {
                let frames = theme.spinner_frames();
                if !frames.is_empty() {
                    return frames[frame % frames.len()].clone();
                }
            }
            ("status.running", ThemeColor::Accent)
        }
    };
    paint(theme, color, &theme.symbol(key))
}

/// A code-frame row with a padded line-number gutter and a `│` separator.
pub fn format_code_frame_line(
    marker: &CodeFrameMarker,
    line_number: &str,
    content: &str,
    line_number_width: usize,
) -> String {
    let marker_text = marker.trim();
    let number_text = line_number.trim();
    let gutter = match (marker_text.is_empty(), number_text.is_empty()) {
        (false, false) => format!("{marker_text}{number_text}"),
        (true, false) => number_text.to_string(),
        _ => marker_text.to_string(),
    };
    format!("{gutter:>width$}│{content}", width = line_number_width + 1)
}

/// Count added lines, removed lines, and hunks in a unified diff.
pub fn get_diff_stats(diff_text: &str) -> DiffStats {
    let lines: Vec<&str> = if diff_text.is_empty() {
        Vec::new()
    } else {
        diff_text.split('\n').collect()
    };
    let mut stats = DiffStats {
        lines: lines.len(),
        ..DiffStats::default()
    };
    let mut in_hunk = false;
    for line in lines {
        let added = line.starts_with('+');
        let removed = line.starts_with('-');
        let change = added || removed;
        if added {
            stats.added += 1;
        }
        if removed {
            stats.removed += 1;
        }
        if change && !in_hunk {
            stats.hunks += 1;
            in_hunk = true;
        } else if !change {
            in_hunk = false;
        }
    }
    stats
}

/// Render a compact themed summary of diff counts.
pub fn format_diff_stats(added: usize, removed: usize, hunks: usize, theme: &Theme) -> String {
    let mut parts: Vec<String> = Vec::new();
    if added > 0 {
        parts.push(paint(theme, ThemeColor::ToolDiffAdded, &format!("+{added}")));
    }
    if removed > 0 {
        parts.push(paint(theme, ThemeColor::ToolDiffRemoved, &format!("-{removed}")));
    }
    if hunks > 0 {
        parts.push(paint(theme, ThemeColor::Dim, &format!("{hunks} {}", pluralize("hunk", hunks))));
    }
    parts.join(&paint(theme, ThemeColor::Dim, " / "))
}

/// Where a display path's final segment starts, past its last `/` or `\`.
pub fn path_name_start(path: &str) -> usize {
    let bytes = path.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'/' || bytes[end - 1] == b'\\') {
        end -= 1;
    }
    if end == 0 {
        return 0;
    }
    let slice = &path[..end];
    let slash = slice.rfind('/').map(|i| i + 1).unwrap_or(0);
    let backslash = slice.rfind('\\').map(|i| i + 1).unwrap_or(0);
    slash.max(backslash)
}

/// A file path in a tool body: the directory dim, the final segment accented.
pub fn format_styled_path(path: &str, theme: &Theme) -> String {
    let cut = path_name_start(path);
    let name = paint(theme, ThemeColor::CustomMessageLabel, &path[cut..]);
    if cut > 0 {
        format!("{}{name}", paint(theme, ThemeColor::Dim, &path[..cut]))
    } else {
        name
    }
}

/// Style a tool output row, preserving ANSI already present in the text.
pub fn style_tool_output_line(line: &str, theme: &Theme) -> String {
    let normalized = replace_tabs(line);
    if normalized.contains("\x1b[") {
        normalized
    } else {
        paint(theme, ThemeColor::ToolOutput, &normalized)
    }
}

/// Render a sanitized error message with its status icon.
pub fn format_error_message(message: Option<&str>, theme: &Theme) -> String {
    let icon = format_status_icon(ToolUiStatus::Error, theme, None);
    format!(
        "{icon} {}",
        paint(theme, ThemeColor::Error, &format!("Error: {}", sanitize_error_text(message)))
    )
}

/// Render an error as a subordinate detail line beneath a status header.
pub fn format_error_detail(message: Option<&str>, theme: &Theme) -> String {
    format!(
        "  {}",
        paint(theme, ThemeColor::Error, &sanitize_error_text(message))
    )
}

/// Render a muted empty-state message with a warning icon.
pub fn format_empty_message(message: &str, theme: &Theme) -> String {
    let icon = format_status_icon(ToolUiStatus::Warning, theme, None);
    format!("{icon} {}", paint(theme, ThemeColor::Muted, message))
}

fn sanitize_error_text(message: Option<&str>) -> String {
    let raw = message.unwrap_or("");
    let clean = raw
        .strip_prefix("Error:")
        .map(str::trim_start)
        .unwrap_or(raw)
        .trim();
    if clean.is_empty() {
        "Unknown error".to_string()
    } else {
        truncate_to_width(&replace_tabs(clean), 110, Ellipsis::Unicode)
    }
}

// ── Output pane labels ──────────────────────────────────────────────────────

/// Which side of a capped output stream stays visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneEdge {
    #[default]
    Head,
    Tail,
}

/// Default hidden-line marker for a capped pane.
pub fn default_hidden_label(hidden: usize, shown: usize, total: usize, edge: PaneEdge) -> String {
    match edge {
        PaneEdge::Tail => format!("… ({hidden} earlier lines, showing {shown} of {total})"),
        PaneEdge::Head => format!("… {hidden} more {}", pluralize("line", hidden)),
    }
}

// ── Sixel passthrough ───────────────────────────────────────────────────────

static SIXEL_PASSTHROUGH: AtomicBool = AtomicBool::new(false);
static IMAGE_PROTOCOL: AtomicU8 = AtomicU8::new(0);

/// Record the terminal's active image protocol (`0` = none, `1` = sixel).
pub fn set_image_protocol(protocol: u8) {
    IMAGE_PROTOCOL.store(protocol, Ordering::Relaxed);
}

/// Whether the terminal renders sixel images inline.
pub fn is_sixel_protocol() -> bool {
    IMAGE_PROTOCOL.load(Ordering::Relaxed) == 1
}

// ── Inline image caps ───────────────────────────────────────────────────────

static INLINE_IMAGE_MAX_COLUMNS: AtomicUsize = AtomicUsize::new(100);
static INLINE_IMAGE_MAX_ROWS: AtomicUsize = AtomicUsize::new(20);

/// Set the maximum inline image width in terminal columns; zero leaves it
/// uncapped.
pub fn set_inline_image_max_columns(max_columns: usize) {
    INLINE_IMAGE_MAX_COLUMNS.store(max_columns, Ordering::Relaxed);
}

/// Set the maximum inline image height in terminal rows; zero uses the viewport
/// limit.
pub fn set_inline_image_max_rows(max_rows: usize) {
    INLINE_IMAGE_MAX_ROWS.store(max_rows, Ordering::Relaxed);
}

/// Inline image dimension caps resolved from the settings and viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageOptions {
    /// Maximum image width in terminal columns.
    pub max_width_cells: usize,
    /// Maximum image height in terminal rows; `None` leaves it uncapped.
    pub max_height_cells: Option<usize>,
}

/// Resolve the caps against an explicit viewport row count.
///
/// `viewport_rows == None` is the reference's `process.stdout.rows` being
/// undefined (non-TTY, tests): the explicit row setting is honored unchanged.
fn resolve_image_options_with_rows(viewport_rows: Option<usize>) -> ImageOptions {
    let max_width_cells = INLINE_IMAGE_MAX_COLUMNS.load(Ordering::Relaxed);
    let row_setting = INLINE_IMAGE_MAX_ROWS.load(Ordering::Relaxed);
    let viewport_fraction = viewport_rows
        .map(|rows| (rows as f64 * 0.6).floor() as usize)
        .unwrap_or(0);
    let max_height_cells = if row_setting == 0 {
        if viewport_fraction == 0 {
            None
        } else {
            Some(viewport_fraction)
        }
    } else if viewport_fraction > 0 {
        Some(row_setting.min(viewport_fraction))
    } else {
        Some(row_setting)
    };
    ImageOptions {
        max_width_cells,
        max_height_cells,
    }
}

/// Resolve inline image dimension caps from the configured limits and viewport.
///
/// Reference: `l/src/render/render-utils.ts::resolveImageOptions`; the viewport
/// is `crossterm::terminal::size()`, the Rust counterpart of
/// `process.stdout.rows`.
pub fn resolve_image_options() -> ImageOptions {
    let viewport_rows = crossterm::terminal::size().ok().map(|(_, rows)| rows as usize);
    resolve_image_options_with_rows(viewport_rows)
}

/// Allow sixel payload rows to pass through unsplit and unstyled.
pub fn set_sixel_passthrough(enabled: bool) {
    SIXEL_PASSTHROUGH.store(enabled, Ordering::Relaxed);
}

/// Whether sixel passthrough is enabled.
pub fn is_sixel_passthrough_enabled() -> bool {
    SIXEL_PASSTHROUGH.load(Ordering::Relaxed)
}

/// Whether the text contains a sixel start sequence.
pub fn contains_sixel_sequence(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(offset) = text[index..].find("\x1bP") else {
            break;
        };
        let start = index + offset;
        let mut saw_q = false;
        for ch in text[start + 2..].chars() {
            if ch == 'q' {
                saw_q = true;
                break;
            }
            if ch.is_ascii_digit() || ch == ';' {
                continue;
            }
            break;
        }
        if saw_q {
            return true;
        }
        index = start + 2;
    }
    false
}

/// Mark which rows belong to a sixel sequence block, so they are never styled
/// or split.
pub fn get_sixel_line_mask(lines: &[String]) -> Vec<bool> {
    let mut in_sequence = false;
    lines
        .iter()
        .map(|line| {
            if contains_sixel_sequence(line) {
                in_sequence = true;
            }
            let masked = in_sequence;
            if in_sequence && (line.contains("\x1b\\") || line.contains('\x07')) {
                in_sequence = false;
            }
            masked
        })
        .collect()
}

// ── Output pane formatting ─────────────────────────────────────────────────

/// Inputs for [`format_output_pane_lines`].
pub struct OutputPaneFormat<'a> {
    /// Retained rows; a caller styling them passes `style_line`.
    pub lines: &'a [String],
    pub expanded: bool,
    pub collapsed_max_lines: usize,
    pub expanded_max_lines: Option<usize>,
    /// Which side of an overflowing stream stays visible.
    pub edge: PaneEdge,
    /// Apply the cap after terminal-width wrapping instead of to logical rows.
    pub visual: bool,
    /// Content width used by visual capping.
    pub width: usize,
    /// Keep sixel payload rows byte-for-byte and show the complete payload.
    pub uncap_sixel: bool,
    pub show_hidden_marker: bool,
    pub show_expand_hint: bool,
    pub show_expand_hint_when_uncapped: bool,
    /// Override the hidden-line label.
    pub format_hidden: Option<fn(usize, usize, usize, PaneEdge) -> String>,
    /// Style one row before capping; masked sixel rows are skipped.
    pub style_line: Option<&'a dyn Fn(&str, usize) -> String>,
}

/// Result of [`format_output_pane_lines`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPaneFormatResult {
    pub lines: Vec<String>,
    pub hidden_count: usize,
    pub has_sixel: bool,
}

/// Style and cap output rows for code cells, tool cards, and live panes.
pub fn format_output_pane_lines(options: &OutputPaneFormat, theme: &Theme) -> OutputPaneFormatResult {
    let edge = options.edge;
    let mask = if is_sixel_protocol() && !options.lines.is_empty() {
        get_sixel_line_mask(options.lines)
    } else {
        Vec::new()
    };
    let has_sixel = mask.iter().any(|masked| *masked);
    let styled: Vec<String> = options
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let masked = mask.get(index).copied().unwrap_or(false);
            if masked {
                line.clone()
            } else if let Some(style) = options.style_line {
                style(line, index)
            } else {
                line.clone()
            }
        })
        .collect();

    let configured_limit = if options.expanded {
        options.expanded_max_lines
    } else {
        Some(options.collapsed_max_lines)
    };
    let limit = if has_sixel && options.uncap_sixel {
        None
    } else {
        configured_limit
    };

    let mut visible: Vec<String> = styled.clone();
    let mut hidden_count = 0usize;
    if let Some(limit) = limit {
        if limit == 0 {
            visible = Vec::new();
            hidden_count = styled.len();
        } else if options.visual && edge == PaneEdge::Tail {
            let wrapped = wrap_text_with_ansi(&styled.join("\n"), options.width.max(1));
            hidden_count = wrapped.len().saturating_sub(limit);
            visible = wrapped[hidden_count..].to_vec();
        } else if options.visual {
            let wrapped = wrap_text_with_ansi(&styled.join("\n"), options.width.max(1));
            hidden_count = wrapped.len().saturating_sub(limit);
            visible = wrapped.into_iter().take(limit).collect();
        } else if styled.len() > limit {
            hidden_count = styled.len() - limit;
            visible = if edge == PaneEdge::Tail {
                styled[styled.len() - limit..].to_vec()
            } else {
                styled[..limit].to_vec()
            };
        }
    }

    if hidden_count > 0 && options.show_hidden_marker {
        let label_fn = options.format_hidden.unwrap_or(default_hidden_label);
        let label = label_fn(hidden_count, visible.len(), hidden_count + visible.len(), edge);
        let hint = if options.show_expand_hint {
            format_expand_hint(theme, Some(options.expanded), Some(true))
        } else {
            String::new()
        };
        let marker = if hint.is_empty() {
            label
        } else {
            format!("{label} {hint}")
        };
        let marker = paint(theme, ThemeColor::Dim, &marker);
        if edge == PaneEdge::Tail {
            visible.insert(0, marker);
        } else {
            visible.push(marker);
        }
    } else if !options.expanded && options.show_expand_hint_when_uncapped {
        let hint = format_expand_hint(theme, Some(false), Some(true));
        if !hint.is_empty() {
            visible.push(hint);
        }
    }

    OutputPaneFormatResult {
        lines: visible,
        hidden_count,
        has_sixel,
    }
}

/// Splice `overlay` into `base` at display column `start_col`, padding or
/// truncating so the result never exceeds `total_width`.
pub fn composite_line_at(
    base: &str,
    overlay: &str,
    start_col: usize,
    overlay_width: usize,
    total_width: usize,
) -> String {
    let after_start = start_col + overlay_width;
    let segments = extract_segments(base, start_col, after_start, total_width.saturating_sub(after_start));
    let trimmed_overlay = slice_by_column(overlay, 0, overlay_width, true);

    let before_pad = start_col.saturating_sub(segments.before_width);
    let overlay_pad = overlay_width.saturating_sub(trimmed_overlay.width);
    let actual_before_width = start_col.max(segments.before_width);
    let actual_overlay_width = overlay_width.max(trimmed_overlay.width);
    let after_target = total_width
        .saturating_sub(actual_before_width)
        .saturating_sub(actual_overlay_width);
    let after_pad = after_target.saturating_sub(segments.after_width);

    let reset = "\x1b[0m";
    let mut result = String::with_capacity(total_width + reset.len() * 2 + overlay.len());
    result.push_str(&segments.before);
    result.extend(std::iter::repeat_n(' ', before_pad));
    result.push_str(reset);
    result.push_str(&trimmed_overlay.text);
    result.extend(std::iter::repeat_n(' ', overlay_pad));
    result.push_str(reset);
    result.push_str(&segments.after);
    result.extend(std::iter::repeat_n(' ', after_pad));

    if visible_width(&result) <= total_width {
        return result;
    }
    slice_by_column(&result, 0, total_width, true).text
}

/// The head and tail of `line` around a `[before_target, skip_start)` column
/// window, each ANSI-aware and width-bounded.
struct Segments {
    before: String,
    before_width: usize,
    after: String,
    after_width: usize,
}

fn extract_segments(line: &str, before_target: usize, skip_start: usize, after_target: usize) -> Segments {
    let mut before = String::new();
    let mut after = String::new();
    let mut column = 0usize;
    for token in scan(line) {
        match token {
            Token::Escape(seq) => {
                if column >= skip_start {
                    after.push_str(seq);
                } else if column < before_target {
                    before.push_str(seq);
                }
            }
            Token::Text(chunk) => {
                for grapheme in chunk.graphemes(true) {
                    let width = UnicodeWidthStr::width(grapheme);
                    if column < before_target {
                        if column + width > before_target {
                            // A wide grapheme straddling the boundary is
                            // dropped, matching a strict column slice.
                        } else {
                            before.push_str(grapheme);
                            column += width;
                            continue;
                        }
                    }
                    if column >= skip_start && visible_width(&after) < after_target {
                        if visible_width(&after) + width > after_target {
                            break;
                        }
                        after.push_str(grapheme);
                        column += width;
                        continue;
                    }
                    column += width;
                }
            }
        }
    }
    let before_width = visible_width(&before);
    let after_width = visible_width(&after);
    Segments {
        before,
        before_width,
        after,
        after_width,
    }
}

// ── Tree guides ─────────────────────────────────────────────────────────────

/// Build indentation and continuing branches for ancestor levels.
///
/// Each `true` marks an ancestor with a following sibling and emits the
/// theme's vertical guide plus two spaces; `false` emits three blank columns.
///
/// Reference: `l/src/render/utils.ts::buildTreePrefix`.
pub fn build_tree_prefix(ancestors: &[bool], theme: &Theme) -> String {
    let tree = theme.tree();
    let mut prefix = String::with_capacity(ancestors.len() * 3);
    for &has_next in ancestors {
        if has_next {
            prefix.push_str(&tree.vertical);
            prefix.push_str("  ");
        } else {
            prefix.push_str("   ");
        }
    }
    prefix
}

/// The branch glyph for a final or continuing tree item.
///
/// Reference: `l/src/render/utils.ts::getTreeBranch`.
pub fn get_tree_branch(is_last: bool, theme: &Theme) -> String {
    let tree = theme.tree();
    if is_last {
        tree.last
    } else {
        tree.branch
    }
}

/// The continuation prefix drawn before a tree item's wrapped lines.
///
/// Reference: `l/src/render/utils.ts::getTreeContinuePrefix`.
pub fn get_tree_continue_prefix(is_last: bool, theme: &Theme) -> String {
    if is_last {
        "   ".to_string()
    } else {
        format!("{}  ", theme.tree().vertical)
    }
}

// ── Render caches ───────────────────────────────────────────────────────────

/// Incremental 64-bit key builder for render-cache keys.
///
/// Deviation: the reference chains `Bun.hash.xxHash64` (seed 0) over each fed
/// value. This port uses FNV-1a 64 because the crate carries no hashing
/// dependency and cache keys need determinism, not collision resistance. The
/// feeds keep the reference encodings: strings are length-prefixed
/// (little-endian u32), integers use little-endian fixed width, booleans hash
/// one byte (`1` = true, `0` = false), and an absent optional hashes a `0xff`
/// sentinel byte.
#[derive(Clone, Debug)]
pub struct Hasher {
    state: u64,
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher {
    /// Start a fresh hash.
    pub fn new() -> Self {
        Self {
            state: FNV_OFFSET_BASIS,
        }
    }

    fn mix(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state ^= u64::from(byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    fn mix_u32(&mut self, value: u32) {
        self.mix(&value.to_le_bytes());
    }

    fn mix_u64(&mut self, value: u64) {
        self.mix(&value.to_le_bytes());
    }

    /// Feed a string tag (the reference's `str` feed, length included).
    pub fn update(&mut self, tag: &str) -> &mut Self {
        self.mix_u32(tag.len() as u32);
        self.mix(tag.as_bytes());
        self
    }

    /// Feed an unsigned 32-bit integer.
    pub fn update_u32(&mut self, value: u32) -> &mut Self {
        self.mix_u32(value);
        self
    }

    /// Feed a 64-bit integer.
    pub fn update_u64(&mut self, value: u64) -> &mut Self {
        self.mix_u64(value);
        self
    }

    /// Feed a boolean as one byte (`1` = true, `0` = false).
    pub fn update_flag(&mut self, value: bool) -> &mut Self {
        self.mix(&[u8::from(value)]);
        self
    }

    /// Feed an optional string; `None` hashes the reference's `0xff` sentinel.
    pub fn update_optional(&mut self, value: Option<&str>) -> &mut Self {
        match value {
            Some(tag) => self.mix(tag.as_bytes()),
            None => self.mix(&[0xff]),
        }
        self
    }

    /// The final 64-bit digest.
    pub fn digest(&self) -> u64 {
        self.state
    }

    /// The low 32 bits of the digest, the compact cache-key form.
    pub fn digest_u32(&self) -> u32 {
        self.state as u32
    }
}

/// Bounded least-recently-used cache for rendered output.
///
/// Entries are keyed by `(hash, key)`: `hash` identifies a renderer (the
/// [`Hasher::digest_u32`] of its tag) and `key` identifies that renderer's
/// inputs, so independent renderers can share one cache without colliding on
/// `key` alone.
///
/// Semantics: setting an existing key replaces its value and marks it most
/// recently used; inserting past `limit` evicts the least recently used entry
/// first. A `get` hit refreshes recency, a miss returns `None` without
/// allocating or disturbing the order, and `has` checks presence without
/// refreshing. A `limit` of zero retains nothing.
///
/// Deviation: the reference keeps one entry per component and uses the
/// external `@oh-my-pi/pi-utils/lru`, which is not vendored here, so the LRU
/// lives in-crate with the semantics above.
pub struct RenderCache<K, V> {
    limit: usize,
    order: VecDeque<(u32, K)>,
    entries: HashMap<(u32, K), V>,
}

impl<K: Clone + Eq + Hash, V> RenderCache<K, V> {
    /// Create a cache holding at most `limit` entries.
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    /// The maximum number of entries retained.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Fetch the value for `(hash, key)`, marking it most recently used.
    pub fn get(&mut self, hash: u32, key: &K) -> Option<&V> {
        let composite = (hash, key.clone());
        if !self.entries.contains_key(&composite) {
            return None;
        }
        self.touch(&composite);
        self.entries.get(&composite)
    }

    /// Insert or replace the value for `(hash, key)`, evicting the least
    /// recently used entry when the cache is full.
    pub fn set(&mut self, hash: u32, key: K, value: V) {
        if self.limit == 0 {
            return;
        }
        let composite = (hash, key);
        if self.entries.insert(composite.clone(), value).is_some() {
            self.touch(&composite);
        } else {
            self.order.push_back(composite);
        }
        while self.order.len() > self.limit {
            match self.order.pop_front() {
                Some(evicted) => {
                    self.entries.remove(&evicted);
                }
                None => break,
            }
        }
    }

    /// Whether `(hash, key)` is cached, without changing recency.
    pub fn has(&self, hash: u32, key: &K) -> bool {
        self.entries.contains_key(&(hash, key.clone()))
    }

    /// Drop every entry.
    pub fn clear(&mut self) {
        self.order.clear();
        self.entries.clear();
    }

    fn touch(&mut self, key: &(u32, K)) {
        if let Some(index) = self.order.iter().position(|entry| entry == key) {
            self.order.remove(index);
        }
        self.order.push_back(key.clone());
    }
}

// ── Preview limits and shared constants ─────────────────────────────────────

/// Preview limits for collapsed/expanded views.
///
/// Reference: `l/src/render/render-utils.ts::PREVIEW_LIMITS`. The reference is a
/// frozen object; the upper-case field names are kept so
/// `PREVIEW_LIMITS.COLLAPSED_LINES` reads the same here.
#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewLimits {
    /// Lines shown in collapsed view.
    pub COLLAPSED_LINES: usize,
    /// Lines shown in expanded view.
    pub EXPANDED_LINES: usize,
    /// Items (files, results) shown in collapsed view.
    pub COLLAPSED_ITEMS: usize,
    /// Definition, type-definition and implementation locations shown collapsed.
    pub LOCATIONS_COLLAPSED: usize,
    /// Output preview lines in collapsed view.
    pub OUTPUT_COLLAPSED: usize,
    /// Output preview lines in expanded view.
    pub OUTPUT_EXPANDED: usize,
    /// Computer script lines shown in collapsed view.
    pub COMPUTER_CODE_COLLAPSED: usize,
    /// Max hunks shown when collapsed (edit tool).
    pub DIFF_COLLAPSED_HUNKS: usize,
    /// Max diff lines shown when collapsed (edit tool).
    pub DIFF_COLLAPSED_LINES: usize,
}

/// The shared preview limits.
pub const PREVIEW_LIMITS: PreviewLimits = PreviewLimits {
    COLLAPSED_LINES: 3,
    EXPANDED_LINES: 12,
    COLLAPSED_ITEMS: 8,
    LOCATIONS_COLLAPSED: 2,
    OUTPUT_COLLAPSED: 3,
    OUTPUT_EXPANDED: 10,
    COMPUTER_CODE_COLLAPSED: 10,
    DIFF_COLLAPSED_HUNKS: 8,
    DIFF_COLLAPSED_LINES: 40,
};

/// Default number of terminal output rows shown before expansion.
pub const DEFAULT_TERMINAL_PREVIEW_LINES: usize = 10;

/// Match a Markdown fenced-code opener and capture its indentation and marker.
pub static FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^( {0,3})([`~]{3,})").expect("valid fence regex"));

/// Shared empty link-target lookup for renderers without interactive links.
pub static EMPTY_LINK_TARGETS: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);

/// Rows reserved for the rest of a collapsed block and the editor/status area.
const PREVIEW_WINDOW_RESERVED_ROWS: i64 = 20;
/// Floor so tiny or unknown viewports still show a useful window.
const PREVIEW_WINDOW_MIN_LINES: i64 = 6;
/// Assumed viewport when rows are unknown (non-TTY, tests).
const PREVIEW_WINDOW_FALLBACK_ROWS: i64 = 30;

/// Tail-window height for collapsed command/code previews.
///
/// Reference: `l/src/render/render-utils.ts::previewWindowRows`.
pub fn preview_window_rows() -> usize {
    let rows = crate::env_lookup("LINES")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(PREVIEW_WINDOW_FALLBACK_ROWS);
    (rows - PREVIEW_WINDOW_RESERVED_ROWS)
        .max(PREVIEW_WINDOW_MIN_LINES)
        .max(0) as usize
}

/// Default display width reserved for a resolved model badge.
pub const FEED_MODEL_BADGE_WIDTH: usize = 30;

static FEED_MODEL_BADGE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Set whether feed rows display their resolved model badge.
pub fn set_feed_model_badge_enabled(enabled: bool) {
    FEED_MODEL_BADGE_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Return whether resolved model badges are enabled.
pub fn is_feed_model_badge_enabled() -> bool {
    FEED_MODEL_BADGE_ENABLED.load(Ordering::Relaxed)
}

/// Compact glyph for a configured thinking level; empty for `inherit`.
pub fn thinking_level_glyph(level: crate::thinking::ConfiguredThinkingLevel, theme: &Theme) -> String {
    use crate::thinking::ConfiguredThinkingLevel;
    match level {
        ConfiguredThinkingLevel::Inherit => String::new(),
        ConfiguredThinkingLevel::Off => theme.status().disabled,
        other => {
            let key = if other == ConfiguredThinkingLevel::Auto {
                "thinking.autoPending".to_string()
            } else {
                format!("thinking.{}", other.as_str())
            };
            let symbol = theme.symbol(&key);
            match symbol.find(' ') {
                Some(space) => symbol[..space].to_string(),
                None => symbol.to_string(),
            }
        }
    }
}

/// Compact feed-row prefix: thinking glyph, sanitized model identity, advisor eye.
///
/// Reference: `l/src/render/render-utils.ts::formatFeedModelBadge`.
pub fn format_feed_model_badge(
    model_identity: Option<&str>,
    thinking_level: Option<crate::thinking::ConfiguredThinkingLevel>,
    advisor: bool,
    theme: &Theme,
    max_width: usize,
) -> String {
    let Some(identity) = model_identity else {
        return String::new();
    };
    let clean = sanitize_badge_text(identity);
    if clean.is_empty() || max_width == 0 {
        return String::new();
    }
    let glyph = thinking_level
        .map(|level| thinking_level_glyph(level, theme))
        .unwrap_or_default();
    let advisor_icon = if advisor {
        theme.icon().advisor
    } else {
        String::new()
    };
    let prefix = if glyph.is_empty() {
        String::new()
    } else {
        format!("{glyph} ")
    };
    let suffix = if advisor_icon.is_empty() {
        String::new()
    } else {
        format!(" {advisor_icon}")
    };
    let model_width = max_width.saturating_sub(visible_width(&prefix) + visible_width(&suffix));
    if model_width < 1 {
        let advisor_width = visible_width(&advisor_icon);
        if !advisor_icon.is_empty() && advisor_width <= max_width {
            let thinking_prefix = if !glyph.is_empty() && visible_width(&prefix) + advisor_width <= max_width {
                prefix
            } else {
                String::new()
            };
            return format!(
                "{}{}",
                paint(theme, ThemeColor::Accent, &thinking_prefix),
                paint(theme, ThemeColor::Dim, &advisor_icon)
            );
        }
        return if !glyph.is_empty() && visible_width(&glyph) <= max_width {
            paint(theme, ThemeColor::Accent, &glyph)
        } else {
            String::new()
        };
    }
    let model = truncate_middle_to_width(&clean, model_width);
    format!(
        "{}{}{}",
        paint(theme, ThemeColor::Accent, &prefix),
        paint(theme, ThemeColor::Dim, &format!("{model}{suffix}"))
    )
}

/// Drop control characters and collapse whitespace for badge text.
fn sanitize_badge_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Truncation limits ────────────────────────────────────────────────────────

/// Truncation lengths for different content types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TruncateLengths {
    /// Short titles, labels.
    pub title: usize,
    /// Medium-length content (messages, previews).
    pub content: usize,
    /// Longer content (code, explanations).
    pub long: usize,
    /// Full line content.
    pub line: usize,
    /// Very short (task previews, badges).
    pub short: usize,
    /// Idle recap status line (~40-word LLM reply).
    pub recap: usize,
}

/// Truncation lengths for different content types.
pub const TRUNCATE_LENGTHS: TruncateLengths = TruncateLengths {
    title: 60,
    content: 80,
    long: 100,
    line: 110,
    short: 40,
    recap: 280,
};

/// Get the first nonblank preview lines, trimmed and width-capped.
pub fn get_preview_lines(
    text: &str,
    max_lines: usize,
    max_line_len: usize,
    ellipsis: Option<Ellipsis>,
) -> Vec<String> {
    let ellipsis = ellipsis.unwrap_or_default();
    text.split('\n')
        .filter(|line| !line.trim().is_empty())
        .take(max_lines)
        .map(|line| truncate_to_width(line.trim(), max_line_len, ellipsis))
        .collect()
}

/// Clip a result row only while collapsed; expanded rows stay whole.
pub fn clip_collapsed(text: &str, max_width: usize, expanded: Option<bool>) -> String {
    if expanded == Some(true) {
        text.to_string()
    } else {
        truncate_to_width(text, max_width, Ellipsis::Unicode)
    }
}

/// Extract the domain from a URL, stripping a `www.` prefix.
pub fn get_domain(url: &str) -> String {
    if let Some((_, rest)) = url.split_once("://") {
        let host = rest
            .split(|character| character == '/' || character == '?' || character == '#')
            .next()
            .unwrap_or("");
        let host = host.rsplit('@').next().unwrap_or(host);
        let host = host.split(':').next().unwrap_or(host);
        if !host.is_empty() {
            return host.strip_prefix("www.").unwrap_or(host).to_string();
        }
    }
    url.to_string()
}

/// Options for [`cap_preview_lines`].
#[derive(Clone, Debug, Default)]
pub struct CapPreviewLinesOptions {
    /// Maximum rows to keep; defaults to the live preview window.
    pub max: Option<usize>,
    /// Expanded views are never capped.
    pub expanded: Option<bool>,
    /// Raw prefix prepended to the overflow marker.
    pub prefix: Option<String>,
    /// Set to `Some(false)` to drop the expand hint.
    pub expand_hint: Option<bool>,
}

/// Cap a pre-rendered preview to a viewport-sized tail window.
pub fn cap_preview_lines(
    lines: &[String],
    theme: &Theme,
    options: CapPreviewLinesOptions,
) -> Vec<String> {
    if options.expanded == Some(true) {
        return lines.to_vec();
    }
    let max = options.max.unwrap_or_else(preview_window_rows);
    if lines.len() <= max {
        return lines.to_vec();
    }
    let visible = if max <= 1 {
        Vec::new()
    } else {
        lines[lines.len() - (max - 1)..].to_vec()
    };
    let hidden = lines.len() - visible.len();
    let hint = if options.expand_hint == Some(false) {
        String::new()
    } else {
        format_expand_hint(theme, Some(false), Some(true))
    };
    let marker = if hint.is_empty() {
        format!("… {hidden} earlier {}", pluralize("line", hidden))
    } else {
        format!("… {hidden} earlier {} {hint}", pluralize("line", hidden))
    };
    let mut out = Vec::with_capacity(visible.len() + 1);
    out.push(format!(
        "{}{}",
        options.prefix.as_deref().unwrap_or(""),
        paint(theme, ThemeColor::Dim, &marker)
    ));
    out.extend(visible);
    out
}

/// Color of a file name or whole URL in a tool body.
pub const FILE_NAME_COLOR: ThemeColor = ThemeColor::CustomMessageLabel;

/// Presentation options for a tool title.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolUITitleOptions {
    /// `Some(false)` disables bold emphasis.
    pub bold: Option<bool>,
}

/// Render a themed tool title with optional bold emphasis.
pub fn format_title(label: &str, theme: &Theme, options: Option<ToolUITitleOptions>) -> String {
    let content = if options.and_then(|options| options.bold) == Some(false) {
        label.to_string()
    } else {
        theme.bold(label)
    };
    paint(theme, ThemeColor::ToolTitle, &content)
}

/// Expand tabs in diagnostic text for terminal display.
pub fn sanitize_diagnostic_display_text(text: &str) -> String {
    replace_tabs(text)
}

/// Diagnostic summary and messages consumed by [`format_diagnostics`].
#[derive(Clone, Debug, Default)]
pub struct DiagnosticsInput {
    /// Whether the diagnostics are errors rather than warnings.
    pub errored: bool,
    /// Optional summary shown beside the header.
    pub summary: String,
    /// Raw diagnostic messages.
    pub messages: Vec<String>,
}

/// Options for [`format_diagnostics`].
#[derive(Clone, Debug, Default)]
pub struct FormatDiagnosticsOptions {
    /// Override the `Diagnostics` header title.
    pub title: Option<String>,
}

fn is_tree_end(
    files: &[(String, Vec<ParsedDiagnostic>)],
    unparsed_len: usize,
    total_diags: usize,
    diags_shown: usize,
    file_idx: isize,
    diag_idx: Option<usize>,
    unparsed_idx: Option<usize>,
) -> bool {
    if total_diags > diags_shown + 1 {
        return false;
    }
    if let Some(unparsed_idx) = unparsed_idx {
        return unparsed_idx == unparsed_len - 1;
    }
    if let Some(diag_idx) = diag_idx {
        let file_idx = file_idx as usize;
        let is_last_diag_in_file = diag_idx == files[file_idx].1.len() - 1;
        let is_last_file = file_idx == files.len() - 1;
        return is_last_diag_in_file && is_last_file && unparsed_len == 0;
    }
    false
}

/// Render diagnostic summaries and optionally expanded messages.
pub fn format_diagnostics(
    diag: &DiagnosticsInput,
    expanded: bool,
    theme: &Theme,
    get_lang_icon: &dyn Fn(&str) -> String,
    options: Option<FormatDiagnosticsOptions>,
) -> String {
    if diag.messages.is_empty() {
        return String::new();
    }

    let mut files: Vec<(String, Vec<ParsedDiagnostic>)> = Vec::new();
    let mut unparsed: Vec<String> = Vec::new();
    for message in &diag.messages {
        if let Some(parsed) = parse_diagnostic_message(message) {
            if let Some((_, diagnostics)) = files
                .iter_mut()
                .find(|(file_path, _)| file_path.as_str() == parsed.file_path)
            {
                diagnostics.push(parsed);
            } else {
                files.push((parsed.file_path.clone(), vec![parsed]));
            }
        } else {
            unparsed.push(sanitize_diagnostic_display_text(message));
        }
    }

    for (_, diagnostics) in &mut files {
        diagnostics.sort_by(|left, right| {
            left.severity
                .rank()
                .cmp(&right.severity.rank())
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.col.cmp(&right.col))
                .then_with(|| left.message.cmp(&right.message))
        });
    }

    let header_icon = if diag.errored {
        paint(theme, ThemeColor::Error, theme.symbol("status.error"))
    } else {
        paint(theme, ThemeColor::Warning, theme.symbol("status.warning"))
    };
    let summary = sanitize_diagnostic_display_text(&diag.summary);
    let summary_tag = if summary.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            paint(theme, ThemeColor::Dim, &format!("({summary})"))
        )
    };
    let title = options
        .and_then(|options| options.title)
        .unwrap_or_else(|| "Diagnostics".to_string());
    let mut output = format!(
        "\n\n{} {}{}",
        header_icon,
        paint(theme, ThemeColor::ToolTitle, &title),
        summary_tag
    );

    let max_diags = if expanded {
        diag.messages.len()
    } else {
        5
    };
    let mut diags_shown = 0usize;
    let total_parsed: usize = files.iter().map(|(_, diagnostics)| diagnostics.len()).sum();
    let total_diags = total_parsed + unparsed.len();
    let tree = theme.tree();

    for file_index in 0..files.len() {
        if diags_shown >= max_diags {
            break;
        }
        let (file_path, diagnostics) = &files[file_index];
        let remaining_diags_in_file = diagnostics.len();
        let remaining_diags_after = files[file_index + 1..]
            .iter()
            .map(|(_, diagnostics)| diagnostics.len())
            .sum::<usize>()
            + unparsed.len();
        let will_show_all_remaining =
            diags_shown + remaining_diags_in_file + remaining_diags_after <= max_diags;
        let is_last_file_node =
            file_index == files.len() - 1 && unparsed.is_empty() && will_show_all_remaining;
        let file_branch = if is_last_file_node {
            &tree.last
        } else {
            &tree.branch
        };
        let file_icon = paint(theme, ThemeColor::Muted, &get_lang_icon(file_path));
        output.push_str(&format!(
            "\n {} {} {}",
            paint(theme, ThemeColor::Dim, file_branch),
            file_icon,
            format_styled_path(file_path, theme)
        ));

        for diag_index in 0..diagnostics.len() {
            if diags_shown >= max_diags {
                break;
            }
            let diagnostic = &diagnostics[diag_index];
            let is_last_diag_in_file = diag_index == diagnostics.len() - 1;
            let at_display_limit = diags_shown + 1 >= max_diags;
            let is_last_visible_in_file = is_last_diag_in_file || at_display_limit;
            let is_very_last = is_tree_end(
                &files,
                unparsed.len(),
                total_diags,
                diags_shown,
                file_index as isize,
                Some(diag_index),
                None,
            );
            let diag_branch = if is_last_file_node {
                if is_last_visible_in_file || is_very_last {
                    format!("  {}", tree.last)
                } else {
                    format!("  {}", tree.branch)
                }
            } else if is_last_visible_in_file || is_very_last {
                format!("{} {}", tree.vertical, tree.last)
            } else {
                format!("{} {}", tree.vertical, tree.branch)
            };
            let severity_icon = match diagnostic.severity {
                DiagnosticSeverity::Error => {
                    paint(theme, ThemeColor::Error, theme.symbol("status.error"))
                }
                DiagnosticSeverity::Warning => {
                    paint(theme, ThemeColor::Warning, theme.symbol("status.warning"))
                }
                DiagnosticSeverity::Info | DiagnosticSeverity::Hint => {
                    paint(theme, ThemeColor::Muted, theme.symbol("status.info"))
                }
            };
            let location = paint(
                theme,
                ThemeColor::Muted,
                &format!(":{}:{}", diagnostic.line, diagnostic.col),
            );
            let code_tag = diagnostic
                .code
                .as_ref()
                .map(|code| paint(theme, ThemeColor::Dim, &format!(" ({code})")))
                .unwrap_or_default();
            let message_color = match diagnostic.severity {
                DiagnosticSeverity::Error => ThemeColor::Error,
                DiagnosticSeverity::Warning => ThemeColor::Warning,
                DiagnosticSeverity::Info | DiagnosticSeverity::Hint => ThemeColor::ToolOutput,
            };
            output.push_str(&format!(
                "\n {} {}{} {}{}",
                paint(theme, ThemeColor::Dim, &diag_branch),
                severity_icon,
                location,
                paint(theme, message_color, &diagnostic.message),
                code_tag
            ));
            diags_shown += 1;
        }
    }

    for unparsed_index in 0..unparsed.len() {
        if diags_shown >= max_diags {
            break;
        }
        let message = &unparsed[unparsed_index];
        let is_very_last = is_tree_end(
            &files,
            unparsed.len(),
            total_diags,
            diags_shown,
            -1,
            None,
            Some(unparsed_index),
        );
        let branch = if is_very_last {
            &tree.last
        } else {
            &tree.branch
        };
        let color = if message.contains("[error]") {
            ThemeColor::Error
        } else if message.contains("[warning]") {
            ThemeColor::Warning
        } else {
            ThemeColor::Dim
        };
        output.push_str(&format!(
            "\n {} {}",
            paint(theme, ThemeColor::Dim, branch),
            paint(theme, color, message)
        ));
        diags_shown += 1;
    }

    if total_diags > diags_shown {
        let remaining = total_diags - diags_shown;
        output.push_str(&format!(
            "\n {} {}",
            paint(theme, ThemeColor::Dim, &tree.last),
            paint(theme, ThemeColor::Muted, &format!("… {remaining} more"))
        ));
        output.push(' ');
        output.push_str(&format_expand_hint(theme, None, None));
    }

    output
}

/// Options for [`truncate_diff_by_hunk`].
#[derive(Clone, Debug, Default)]
pub struct TruncateDiffOptions {
    /// Keep the tail of the diff instead of its head.
    pub from_tail: Option<bool>,
}

/// Result of [`truncate_diff_by_hunk`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TruncateDiffResult {
    /// The retained diff text.
    pub text: String,
    /// Hunks omitted by the caps.
    pub hidden_hunks: usize,
    /// Lines omitted by the caps.
    pub hidden_lines: usize,
}

struct DiffSegment {
    lines: Vec<String>,
    is_change: bool,
    is_ellipsis: bool,
}

fn parse_diff_segments(lines: &[String]) -> Vec<DiffSegment> {
    let mut segments: Vec<DiffSegment> = Vec::new();
    let mut current: Option<DiffSegment> = None;
    for line in lines {
        let is_change = line.starts_with('+') || line.starts_with('-');
        let is_ellipsis = line.trim_start().starts_with("...") || line.trim().is_empty();
        if is_ellipsis {
            if let Some(segment) = current.take() {
                segments.push(segment);
            }
            segments.push(DiffSegment {
                lines: vec![line.clone()],
                is_change: false,
                is_ellipsis: true,
            });
        } else if let Some(segment) = current.as_mut() {
            if segment.is_change == is_change {
                segment.lines.push(line.clone());
            } else {
                let previous = current.take().expect("current segment");
                segments.push(previous);
                current = Some(DiffSegment {
                    lines: vec![line.clone()],
                    is_change,
                    is_ellipsis: false,
                });
            }
        } else {
            current = Some(DiffSegment {
                lines: vec![line.clone()],
                is_change,
                is_ellipsis: false,
            });
        }
    }
    if let Some(segment) = current {
        segments.push(segment);
    }
    segments
}

/// Truncate a unified diff at hunk boundaries within the supplied preview limits.
pub fn truncate_diff_by_hunk(
    diff_text: &str,
    max_hunks: usize,
    max_lines: usize,
    options: Option<TruncateDiffOptions>,
) -> TruncateDiffResult {
    if options.and_then(|options| options.from_tail) == Some(true) {
        let reversed = diff_text
            .split('\n')
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_diff_by_hunk_head(&reversed, max_hunks, max_lines);
        let text = result
            .text
            .split('\n')
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return TruncateDiffResult {
            text,
            hidden_hunks: result.hidden_hunks,
            hidden_lines: result.hidden_lines,
        };
    }
    truncate_diff_by_hunk_head(diff_text, max_hunks, max_lines)
}

fn truncate_diff_by_hunk_head(
    diff_text: &str,
    max_hunks: usize,
    max_lines: usize,
) -> TruncateDiffResult {
    let lines: Vec<String> = if diff_text.is_empty() {
        Vec::new()
    } else {
        diff_text.split('\n').map(str::to_string).collect()
    };
    let total_stats = get_diff_stats(diff_text);
    if lines.len() <= max_lines && total_stats.hunks <= max_hunks {
        return TruncateDiffResult {
            text: diff_text.to_string(),
            hidden_hunks: 0,
            hidden_lines: 0,
        };
    }

    let segments = parse_diff_segments(&lines);
    let change_line_count: usize = segments
        .iter()
        .filter(|segment| segment.is_change)
        .map(|segment| segment.lines.len())
        .sum();

    if change_line_count > max_lines {
        let mut kept: Vec<String> = Vec::new();
        let mut kept_hunks = 0usize;
        for segment in &segments {
            if kept.len() >= max_lines {
                break;
            }
            if segment.is_change {
                if kept_hunks >= max_hunks {
                    break;
                }
                kept_hunks += 1;
            }
            let take = segment.lines.len().min(max_lines - kept.len());
            kept.extend(segment.lines[..take].iter().cloned());
        }
        return TruncateDiffResult {
            text: kept.join("\n"),
            hidden_hunks: total_stats.hunks.saturating_sub(kept_hunks),
            hidden_lines: lines.len().saturating_sub(kept.len()),
        };
    }

    let context_budget = max_lines - change_line_count;
    let total_context_lines: usize = segments
        .iter()
        .filter(|segment| !segment.is_change)
        .map(|segment| segment.lines.len())
        .sum();
    let mut kept: Vec<String> = Vec::new();
    let mut kept_hunks = 0usize;
    let mut kept_source_lines = 0usize;

    if total_context_lines <= context_budget {
        for segment in &segments {
            if segment.is_change {
                if kept_hunks >= max_hunks {
                    break;
                }
                kept_hunks += 1;
            }
            kept.extend(segment.lines.iter().cloned());
            kept_source_lines += segment.lines.len();
        }
    } else {
        let context_ratio = if total_context_lines > 0 {
            context_budget as f64 / total_context_lines as f64
        } else {
            0.0
        };
        let mut remaining_context_budget = context_budget;
        for index in 0..segments.len() {
            let segment = &segments[index];
            if segment.is_change {
                if kept_hunks >= max_hunks {
                    break;
                }
                kept_hunks += 1;
                kept.extend(segment.lines.iter().cloned());
                kept_source_lines += segment.lines.len();
                continue;
            }
            if remaining_context_budget <= 0 {
                continue;
            }
            let allowed_lines = remaining_context_budget.min(
                (segment.lines.len() as f64 * context_ratio)
                    .floor()
                    .max(1.0) as usize,
            );
            let output_start = kept.len();
            let mut source_lines_added = 0usize;
            if segment.is_ellipsis || segment.lines.len() <= allowed_lines {
                let take = allowed_lines.min(segment.lines.len());
                kept.extend(segment.lines[..take].iter().cloned());
                source_lines_added = take;
            } else {
                let is_before_change = segments
                    .get(index + 1)
                    .map(|segment| segment.is_change)
                    .unwrap_or(false);
                let is_after_change = if index > 0 {
                    segments[index - 1].is_change
                } else {
                    false
                };
                if is_before_change && is_after_change {
                    if allowed_lines >= 3 {
                        let source_budget = allowed_lines - 1;
                        let first_count = (source_budget as f64 / 2.0).ceil() as usize;
                        let last_count = source_budget - first_count;
                        kept.extend(segment.lines[..first_count].iter().cloned());
                        kept.push(String::new());
                        if last_count > 0 {
                            kept.extend(
                                segment.lines[segment.lines.len() - last_count..]
                                    .iter()
                                    .cloned(),
                            );
                        }
                        source_lines_added = source_budget;
                    } else {
                        let first_count = (allowed_lines as f64 / 2.0).ceil() as usize;
                        let last_count = allowed_lines - first_count;
                        kept.extend(segment.lines[..first_count].iter().cloned());
                        if last_count > 0 {
                            kept.extend(
                                segment.lines[segment.lines.len() - last_count..]
                                    .iter()
                                    .cloned(),
                            );
                        }
                        source_lines_added = allowed_lines;
                    }
                } else if is_before_change {
                    kept.extend(
                        segment.lines[segment.lines.len() - allowed_lines..]
                            .iter()
                            .cloned(),
                    );
                    source_lines_added = allowed_lines;
                } else if is_after_change {
                    kept.extend(segment.lines[..allowed_lines].iter().cloned());
                    source_lines_added = allowed_lines;
                } else {
                    let take = allowed_lines.min(2);
                    kept.extend(segment.lines[..take].iter().cloned());
                    source_lines_added = take;
                }
            }
            kept_source_lines += source_lines_added;
            remaining_context_budget =
                remaining_context_budget.saturating_sub(kept.len() - output_start);
        }
    }

    TruncateDiffResult {
        text: kept.join("\n"),
        hidden_hunks: total_stats.hunks.saturating_sub(kept_hunks),
        hidden_lines: lines.len().saturating_sub(kept_source_lines),
    }
}

// ── Path utilities ───────────────────────────────────────────────────────────

fn default_home_dir() -> Option<String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|value| value.to_string_lossy().into_owned())
}

fn is_windows_home(home: &str) -> bool {
    let bytes = home.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || home.starts_with("\\\\")
}

fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
        .collect()
}

fn is_path_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\'' | '`' | '(' | '[' | '{' | '=' | ',' | ':' | '<' | '>' | '&' | '|'
        )
}

fn is_path_end(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\'' | '`' | ')' | ']' | '}' | ',' | ';' | ':' | '<' | '>' | '&' | '|'
        )
}

fn inside_url(text: &str, start: usize) -> bool {
    let prefix = &text[..start];
    let tail_start = prefix
        .rfind(char::is_whitespace)
        .map(|index| index + 1)
        .unwrap_or(0);
    prefix[tail_start..].contains("://")
}

/// Replace a leading home directory with a portable tilde prefix.
pub fn shorten_path(file_path: &str, home_dir: Option<&str>) -> String {
    if file_path.is_empty() {
        return String::new();
    }
    let Some(resolved_home) = home_dir.map(str::to_string).or_else(default_home_dir) else {
        return file_path.to_string();
    };
    if resolved_home.is_empty() {
        return file_path.to_string();
    }
    let home = resolved_home.trim_end_matches(['/', '\\']).to_string();
    if home.is_empty() || home == "/" || home == "\\" {
        return file_path.to_string();
    }
    let windows_style = is_windows_home(&home);
    let matches_home = if windows_style {
        file_path.len() >= home.len() && file_path[..home.len()].eq_ignore_ascii_case(&home)
    } else {
        file_path.starts_with(&home)
    };
    if matches_home {
        let next = file_path[home.len()..].chars().next();
        if next.map_or(true, |character| character == '/' || character == '\\') {
            let suffix = file_path[home.len()..].replace('\\', "/");
            return format!("~{suffix}");
        }
    }
    file_path.to_string()
}

/// Shorten embedded home paths in prose.
pub fn shorten_embedded_paths(text: &str, home_dir: Option<&str>, preserve_separators: bool) -> String {
    let Some(resolved_home) = home_dir.map(str::to_string).or_else(default_home_dir) else {
        return text.to_string();
    };
    if resolved_home.len() <= 1 {
        return text.to_string();
    }
    let home = resolved_home.trim_end_matches(['/', '\\']).to_string();
    if home.is_empty() {
        return text.to_string();
    }
    let windows_style = is_windows_home(&home);
    let pattern = if windows_style {
        format!("(?i){}", regex::escape(&home))
    } else {
        regex::escape(&home)
    };
    let Ok(home_re) = Regex::new(&pattern) else {
        return text.to_string();
    };
    let mut shortened = String::with_capacity(text.len());
    let mut last = 0usize;
    for matched in home_re.find_iter(text) {
        let start = matched.start();
        let end = matched.end();
        let previous = text[..start].chars().next_back();
        let next = text[end..].chars().next();
        let boundary_before = previous.map_or(true, is_path_boundary);
        let boundary_after = next.map_or(true, is_path_end);
        if boundary_before && boundary_after && !inside_url(text, start) {
            shortened.push_str(&text[last..start]);
            shortened.push('~');
            last = end;
        }
    }
    shortened.push_str(&text[last..]);
    if preserve_separators {
        return shortened;
    }
    shortened
        .split(' ')
        .map(|segment| {
            if segment.contains('~') {
                segment.replace('\\', "/")
            } else {
                segment.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[index + 1..index + 3], 16) {
                decoded.push(value);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn file_url_path(text: &str) -> Option<String> {
    let rest = text.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    if decoded.starts_with("//") {
        let after = &decoded[2..];
        if let Some((host, path)) = after.split_once('/') {
            return Some(format!("//{host}{path}"));
        }
        return Some(decoded);
    }
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return Some(decoded[1..].to_string());
    }
    Some(decoded)
}

/// Shorten filesystem and command arguments without rewriting literal search patterns.
pub fn shorten_tool_argument_paths(text: &str, key: Option<&str>, home_dir: Option<&str>) -> String {
    if key == Some("url") {
        if let Some(path) = file_url_path(text) {
            return shorten_embedded_paths(&path, home_dir, false);
        }
        return text.to_string();
    }
    match key {
        None | Some("path") | Some("file_path") | Some("command") | Some("task") | Some("prompt") => {
            shorten_embedded_paths(text, home_dir, false)
        }
        _ => text.to_string(),
    }
}

/// Sanitize warning text before showing it in the TUI.
pub fn sanitize_display_warning(text: &str) -> String {
    let cleaned = replace_tabs(&sanitize_text(text))
        .replace(['\r', '\n'], " ");
    shorten_embedded_paths(cleaned.trim(), None, false)
}

/// Sanitize and bound warning text before showing it in the TUI.
pub fn sanitize_display_warnings(warnings: &[String]) -> Vec<String> {
    let mut visible: Vec<String> = warnings
        .iter()
        .take(PREVIEW_LIMITS.COLLAPSED_ITEMS)
        .map(|warning| {
            truncate_to_width(
                &sanitize_display_warning(warning),
                TRUNCATE_LENGTHS.long,
                Ellipsis::Unicode,
            )
        })
        .collect();
    let hidden = warnings.len().saturating_sub(visible.len());
    if hidden > 0 {
        visible.push(format!("… {hidden} more {}", pluralize("warning", hidden)));
    }
    visible
}

fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Format a tool working directory when it differs from the project directory.
pub fn format_tool_working_directory(workdir: &str, project_dir: &str) -> Option<String> {
    if workdir.is_empty() {
        return None;
    }
    let project = normalize_path(std::path::Path::new(project_dir));
    let workdir_path = if std::path::Path::new(workdir).is_absolute() {
        std::path::PathBuf::from(workdir)
    } else {
        project.join(workdir)
    };
    let work = normalize_path(&workdir_path);
    if work == project {
        return None;
    }
    if let Ok(relative) = work.strip_prefix(&project) {
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !relative.is_empty() && !relative.starts_with("..") {
            return Some(replace_tabs(&relative));
        }
    }
    Some(replace_tabs(&shorten_path(
        &work.to_string_lossy(),
        None,
    )))
}

// ── Parse-error helpers ──────────────────────────────────────────────────────

/// Maximum number of parse errors displayed in expanded details.
pub const PARSE_ERRORS_LIMIT: usize = 20;

/// Deduplicate parse errors while preserving their original order.
pub fn dedupe_parse_errors(errors: Option<&[String]>) -> Vec<String> {
    let Some(errors) = errors else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for error in errors {
        if seen.insert(error.clone()) {
            deduped.push(error.clone());
        }
    }
    deduped
}

/// Format deduplicated parse errors with a bounded detail list.
pub fn format_parse_errors(errors: &[String], total: Option<usize>) -> Vec<String> {
    let deduped = dedupe_parse_errors(Some(errors));
    if deduped.is_empty() {
        return Vec::new();
    }
    let full_count = total.unwrap_or(deduped.len());
    let capped = &deduped[..deduped.len().min(PARSE_ERRORS_LIMIT)];
    let header = if full_count > capped.len() {
        format!("Parse issues ({} / {}):", capped.len(), full_count)
    } else {
        "Parse issues:".to_string()
    };
    let mut out = vec![header];
    out.extend(capped.iter().map(|error| format!("- {error}")));
    out
}

/// Cap an upstream parse-error list, preserving the deduplicated total.
pub fn cap_parse_errors(
    errors: Option<&[String]>,
    limit: Option<usize>,
) -> (Vec<String>, usize) {
    let deduped = dedupe_parse_errors(errors);
    let total = deduped.len();
    let limit = limit.unwrap_or(PARSE_ERRORS_LIMIT);
    (deduped.into_iter().take(limit).collect(), total)
}

/// Append the indented bullet list of parse errors to `lines`.
pub fn append_parse_errors_bullet_list(
    lines: &mut Vec<String>,
    parse_errors: Option<&[String]>,
    theme: &Theme,
    total: Option<usize>,
) {
    let Some(parse_errors) = parse_errors else {
        return;
    };
    if parse_errors.is_empty() {
        return;
    }
    let full_count = total.unwrap_or(parse_errors.len());
    for error in parse_errors.iter().take(PARSE_ERRORS_LIMIT) {
        lines.push(paint(theme, ThemeColor::Warning, &format!("  - {error}")));
    }
    if full_count > parse_errors.len().min(PARSE_ERRORS_LIMIT) {
        lines.push(paint(
            theme,
            ThemeColor::Dim,
            &format!("  … {} more", full_count - parse_errors.len().min(PARSE_ERRORS_LIMIT)),
        ));
    }
}

/// Human-readable summary string for the parse-issues count.
pub fn format_parse_errors_count_label(parse_errors: &[String], total: Option<usize>) -> String {
    let full_count = total.unwrap_or(parse_errors.len());
    if full_count > PARSE_ERRORS_LIMIT {
        format!("{PARSE_ERRORS_LIMIT} / {full_count} parse issues")
    } else if full_count == 1 {
        "1 parse issue".to_string()
    } else {
        format!("{full_count} parse issues")
    }
}

/// Normalize a path argument that may be a JSON-encoded string array.
pub fn to_path_list(input: &str) -> Vec<String> {
    let trimmed = input.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return vec![input.to_string()];
    }
    if let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if items.iter().all(|item| item.is_string()) {
            return items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect();
        }
    }
    vec![input.to_string()]
}

/// Options for [`create_cached_component`].
#[derive(Clone, Copy, Debug, Default)]
pub struct CachedComponentOptions {
    /// Horizontal padding applied to every rendered row.
    pub padding_x: usize,
}

struct CachedComponent {
    get_expanded: Box<dyn Fn() -> bool>,
    compute: Box<dyn Fn(usize, bool) -> Vec<String>>,
    padding_x: usize,
    cached: Option<(u64, Vec<String>)>,
}

impl Component for CachedComponent {
    fn render(&mut self, width: u16) -> Vec<String> {
        let expanded = (self.get_expanded)();
        let width = width as usize;
        let key = Hasher::new()
            .update_flag(expanded)
            .update_u32(width as u32)
            .digest();
        if let Some((cached_key, lines)) = &self.cached {
            if *cached_key == key {
                return lines.clone();
            }
        }
        let inner_width = width.saturating_sub(self.padding_x * 2).max(1);
        let mut lines = (self.compute)(inner_width, expanded);
        if self.padding_x > 0 {
            let pad = " ".repeat(self.padding_x);
            lines = lines
                .into_iter()
                .map(|line| format!("{pad}{line}{pad}"))
                .collect();
        }
        self.cached = Some((key, lines.clone()));
        lines
    }

    fn invalidate(&mut self) {
        self.cached = None;
    }

    fn dispose(&mut self) {
        self.cached = None;
    }
}

/// Create a width+expand keyed render cache for search-style tool renderers.
pub fn create_cached_component<F, C>(
    get_expanded: F,
    compute: C,
    options: CachedComponentOptions,
) -> Box<dyn Component>
where
    F: Fn() -> bool + 'static,
    C: Fn(usize, bool) -> Vec<String> + 'static,
{
    Box::new(CachedComponent {
        get_expanded: Box::new(get_expanded),
        compute: Box::new(compute),
        padding_x: options.padding_x,
        cached: None,
    })
}

/// Fit a status header plus body rows to `width`.
pub fn fit_headered_rows(header: &str, rows: &[String], width: usize) -> Vec<String> {
    let mut out = wrap_text_with_ansi(header, width);
    out.extend(
        rows.iter()
            .map(|row| truncate_to_width(row, width, Ellipsis::Omit)),
    );
    out
}

/// Single-slot memo for an expensive rendered string.
///
/// The theme is compared by reference address, matching the reference's
/// `cache.theme === theme` check.
#[derive(Clone, Debug, Default)]
pub struct RenderedStringCache {
    /// Address of the cached theme, or `None` when empty.
    pub theme: Option<usize>,
    /// Cached expansion state.
    pub expanded: bool,
    /// Caller-chosen salt (path/language).
    pub salt: String,
    /// Source content.
    pub content: String,
    /// Cached rendered value.
    pub value: String,
}

/// Create an empty memo for rendered tool strings.
pub fn create_rendered_string_cache() -> RenderedStringCache {
    RenderedStringCache::default()
}

/// Drop the memo so the next lookup re-renders.
pub fn invalidate_rendered_string_cache(cache: &mut RenderedStringCache) {
    cache.theme = None;
}

/// Release every field of the memo.
pub fn release_rendered_string_cache(cache: &mut RenderedStringCache) {
    *cache = RenderedStringCache::default();
}

/// Reuse a rendered string while its theme, expansion, content, and salt match.
pub fn cached_rendered_string<F>(
    cache: Option<&mut RenderedStringCache>,
    theme: &Theme,
    expanded: bool,
    salt: &str,
    content: &str,
    render: F,
) -> String
where
    F: FnOnce() -> String,
{
    let theme_key = std::ptr::from_ref(theme) as usize;
    if let Some(cache) = cache {
        if cache.theme == Some(theme_key)
            && cache.expanded == expanded
            && cache.salt == salt
            && cache.content == content
        {
            return cache.value.clone();
        }
    }
    let value = render();
    if let Some(cache) = cache {
        cache.theme = Some(theme_key);
        cache.expanded = expanded;
        cache.salt = salt.to_string();
        cache.content = content.to_string();
        cache.value = value.clone();
    }
    value
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::render::types::TreeContext;
    use crate::theme::color::{ColorMode, ColorValue};
    use crate::theme::schema::ColorMap;
    use crate::theme::symbols::{NormalizedSpinnerFrames, SymbolPreset};

    /// A resolved theme for renderer tests, mirroring `theme_class`'s sample.
    pub fn sample_theme() -> Theme {
        theme_with_preset(SymbolPreset::Unicode)
    }

    /// A resolved theme using `preset` for symbol-dependent assertions.
    fn theme_with_preset(preset: SymbolPreset) -> Theme {

        let mut fg = ColorMap::new();
        for (key, hex) in [
            ("accent", "#febc38"),
            ("text", "#e6e6e6"),
            ("muted", "#777d88"),
            ("dim", "#5a5f66"),
            ("success", "#89d281"),
            ("error", "#fc3a4b"),
            ("warning", "#e4c00f"),
            ("border", "#3d424a"),
            ("borderAccent", "#0088fa"),
            ("borderMuted", "#3d424a"),
            ("toolTitle", "#e6e6e6"),
            ("toolOutput", "#e6e6e6"),
            ("toolDiffAdded", "#89d281"),
            ("toolDiffRemoved", "#fc3a4b"),
            ("toolDiffContext", "#777d88"),
            ("customMessageLabel", "#b281d6"),
        ] {
            fg.insert(key.to_string(), ColorValue::Hex(hex.to_string()));
        }
        let mut bg = ColorMap::new();
        for (key, hex) in [
            ("selectedBg", "#31363f"),
            ("userMessageBg", "#1b1d22"),
            ("customMessageBg", "#1b1d22"),
            ("toolPendingBg", "#1b1d22"),
            ("toolSuccessBg", "#152a18"),
            ("toolErrorBg", "#2a1518"),
            ("statusLineBg", "#121212"),
        ] {
            bg.insert(key.to_string(), ColorValue::Hex(hex.to_string()));
        }
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("sep.dot".to_string(), " · ".to_string());
        Theme::new(
            &fg,
            &bg,
            ColorMode::TrueColor,
            preset,
            &overrides,
            &NormalizedSpinnerFrames::default(),
            &[],
        )
        .unwrap()
    }


    #[test]
    fn tabs_expand_to_three_spaces() {
        assert_eq!(replace_tabs("a\tb"), "a   b");
        assert_eq!(replace_tabs("plain"), "plain");
    }

    #[test]
    fn flatten_collapses_whitespace_runs() {
        assert_eq!(flatten_line(" a\t b\n c "), "a b c");
        assert_eq!(flatten_line(""), "");
    }

    #[test]
    fn split_lines_takes_text_after_last_cr() {
        assert_eq!(
            split_terminal_output_lines("progress 1\rprogress 2\ndone"),
            vec!["progress 2", "done"]
        );
        assert_eq!(split_terminal_output_lines(""), vec![""]);
    }

    #[test]
    fn carriage_returns_collapse_to_spaces() {
        assert_eq!(sanitize_carriage_returns("a\r\nb\rc"), "a\nb c");
    }

    #[test]
    fn slice_by_column_is_ansi_aware_and_strict() {
        let styled = "\x1b[31mabcdef\x1b[0m";
        let head = slice_by_column(styled, 0, 3, true);
        assert_eq!(head.text, "\x1b[31mabc\x1b[0m");
        assert_eq!(head.width, 3);
        let tail = slice_by_column(styled, 3, 2, true);
        assert_eq!(tail.text, "\x1b[31mde\x1b[0m");
        assert_eq!(tail.width, 2);
    }

    #[test]
    fn middle_truncation_keeps_both_ends() {
        assert_eq!(truncate_middle_to_width("abcdefghij", 5), "ab…ij");
        assert_eq!(truncate_middle_to_width("abc", 5), "abc");
        assert_eq!(truncate_middle_to_width("abc", 1), "…");
        assert_eq!(truncate_middle_to_width("abc", 0), "");
    }

    #[test]
    fn pad_or_truncate_caps_wide_rows() {
        assert_eq!(pad_or_truncate("abc", 5, None), "abc  ");
        assert_eq!(pad_or_truncate("abcdef", 3, None), "abc");
        assert_eq!(pad_or_truncate("", 2, None), "  ");
    }

    #[test]
    fn stabilize_background_reapplies_after_resets() {
        let stabilized = stabilize_background("a\x1b[0mb", Color::Blue);
        assert!(stabilized.contains("\x1b[0m\x1b[44m"));
        assert!(stabilized.ends_with("\x1b[49m"));
    }

    #[test]
    fn coalesce_merges_adjacent_equal_styles() {
        let runs = vec![
            StyledRun {
                style: Style::fg(Color::Red),
                text: "a".into(),
            },
            StyledRun {
                style: Style::fg(Color::Red),
                text: "b".into(),
            },
            StyledRun {
                style: Style::default(),
                text: "c".into(),
            },
        ];
        let merged = coalesce_styles(&runs);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "ab");
        assert_eq!(paint_runs(&runs), "\x1b[31mab\x1b[0mc");
    }

    #[test]
    fn diff_stats_count_hunks_runs() {
        let stats = get_diff_stats("+a\n+b\n ctx\n-c\n+d");
        assert_eq!(stats.added, 3);
        assert_eq!(stats.removed, 1);
        assert_eq!(stats.hunks, 2);
        assert_eq!(stats.lines, 5);
    }

    #[test]
    fn code_frame_gutter_pads_and_combines_marker() {
        assert_eq!(format_code_frame_line("", "12", "x", 2), " 12│x");
        assert_eq!(format_code_frame_line("*", "12", "x", 2), "*12│x");
        assert_eq!(format_code_frame_line(">", "", "x", 2), "  >│x");
    }

    #[test]
    fn path_name_start_handles_trailing_separators() {
        assert_eq!(path_name_start("a/b/c.txt"), 4);
        assert_eq!(path_name_start("a/b/"), 2);
        assert_eq!(path_name_start("c:\\dir\\file.rs"), 7);
        assert_eq!(path_name_start("/"), 0);
    }

    #[test]
    fn sixel_mask_spans_the_whole_payload() {
        let lines = vec![
            "before".to_string(),
            "\x1bPqpayload".to_string(),
            "still".to_string(),
            "\x1b\\after".to_string(),
        ];
        assert_eq!(
            get_sixel_line_mask(&lines),
            vec![false, true, true, true]
        );
    }

    fn tint(text: &str) -> String {
        format!("<{text}>")
    }

    #[test]
    fn build_tree_prefix_selects_a_glyph_per_ancestor() {
        let unicode = sample_theme();
        let ascii = theme_with_preset(SymbolPreset::Ascii);

        assert_eq!(build_tree_prefix(&[], &unicode), "");
        assert_eq!(build_tree_prefix(&[true], &unicode), "│  ");
        assert_eq!(build_tree_prefix(&[false], &unicode), "   ");
        assert_eq!(
            build_tree_prefix(&[true, false, true], &unicode),
            "│     │  "
        );
        assert_eq!(build_tree_prefix(&[false, false], &unicode), "      ");
        assert_eq!(build_tree_prefix(&[true], &ascii), "|  ");
        assert_eq!(
            build_tree_prefix(&[true, false, true], &ascii),
            "|     |  "
        );
    }

    #[test]
    fn tree_guides_round_trip_through_context() {
        let theme = sample_theme();
        let rails = [true, false];
        let last = true;
        let ctx = TreeContext {
            index: 2,
            is_last: last,
            depth: rails.len(),
            theme: theme.clone(),
            prefix: format!(
                "{}{} ",
                build_tree_prefix(&rails, &theme),
                get_tree_branch(last, &theme)
            ),
            continue_prefix: get_tree_continue_prefix(last, &theme),
            prefix_width: None,
        };

        assert_eq!(ctx.prefix, "│     └─ ");
        assert_eq!(ctx.continue_prefix, "   ");
        assert_eq!(
            visible_width(&ctx.prefix),
            rails.len() * 3 + visible_width(&get_tree_branch(last, &theme)) + 1
        );

        let continuing = TreeContext {
            is_last: false,
            prefix: format!(
                "{}{} ",
                build_tree_prefix(&rails, &theme),
                get_tree_branch(false, &theme)
            ),
            continue_prefix: get_tree_continue_prefix(false, &theme),
            ..ctx
        };
        assert_eq!(continuing.prefix, "│     ├─ ");
        assert_eq!(continuing.continue_prefix, "│  ");
        assert!(!continuing.is_last);
    }

    #[test]
    fn pad_to_width_fits_both_directions_and_tints() {
        assert_eq!(pad_to_width("abc", 5, None), "abc  ");
        assert_eq!(pad_to_width("abc", 3, None), "abc");
        assert_eq!(pad_to_width("abc", 0, None), "abc");
        assert_eq!(pad_to_width("abcdef", 3, None), "ab…");
        assert_eq!(pad_to_width("日本", 5, None), "日本 ");
        assert_eq!(pad_to_width("日本語", 3, None), "日…");
        assert_eq!(
            pad_to_width("\x1b[31mabc\x1b[0m", 5, None),
            "\x1b[31mabc\x1b[0m  "
        );
        assert_eq!(pad_to_width("abc", 5, Some(&tint)), "<abc  >");
        assert_eq!(pad_to_width("abcdef", 3, Some(&tint)), "<ab…>");
        assert_eq!(pad_to_width("abc", 0, Some(&tint)), "<abc>");
    }

    #[test]
    fn hasher_is_deterministic_and_tag_sensitive() {
        let find = Hasher::new().update("find").digest_u32();
        assert_eq!(find, Hasher::new().update("find").digest_u32());
        assert_ne!(find, Hasher::new().update("glob").digest_u32());

        let chained = Hasher::new()
            .update_flag(true)
            .update_u32(80)
            .update_optional(Some("expanded"))
            .update("tail")
            .digest_u32();
        assert_eq!(
            chained,
            Hasher::new()
                .update_flag(true)
                .update_u32(80)
                .update_optional(Some("expanded"))
                .update("tail")
                .digest_u32()
        );
        assert_ne!(
            chained,
            Hasher::new()
                .update_flag(false)
                .update_u32(80)
                .update_optional(Some("expanded"))
                .update("tail")
                .digest_u32()
        );
        assert_eq!(
            chained,
            Hasher::new()
                .update_flag(true)
                .update_u32(80)
                .update_optional(Some("expanded"))
                .update("tail")
                .digest() as u32
        );
        assert_ne!(
            Hasher::new().update_optional(None).digest_u32(),
            Hasher::new().update_optional(Some("")).digest_u32()
        );
    }

    #[test]
    fn render_cache_evicts_least_recently_used() {
        let mut cache = RenderCache::new(2);
        assert_eq!(cache.limit(), 2);
        cache.set(1, "a", "alpha");
        cache.set(1, "b", "beta");
        assert_eq!(cache.get(1, &"a"), Some(&"alpha"));
        cache.set(1, "c", "gamma");
        assert!(cache.has(1, &"a"));
        assert!(!cache.has(1, &"b"));
        assert!(cache.has(1, &"c"));
    }

    #[test]
    fn render_cache_normalizes_keys_and_ignores_misses() {
        let mut cache = RenderCache::new(2);
        cache.set(1, "a", 1);
        cache.set(1, "b", 2);
        cache.set(1, "a", 10);
        assert_eq!(cache.get(1, &"a"), Some(&10));
        assert_eq!(cache.get(1, &"missing"), None);
        cache.set(2, "a", 20);
        assert!(cache.has(1, &"a"));
        assert!(!cache.has(1, &"b"));
        assert!(cache.has(2, &"a"));
    }

    #[test]
    fn render_cache_clear_and_zero_limit() {
        let mut cache = RenderCache::new(1);
        cache.set(1, "a", 1);
        assert!(cache.has(1, &"a"));
        cache.clear();
        assert!(!cache.has(1, &"a"));
        assert_eq!(cache.get(1, &"a"), None);

        let mut empty = RenderCache::<&str, i32>::new(0);
        assert_eq!(empty.limit(), 0);
        empty.set(1, "a", 1);
        assert!(!empty.has(1, &"a"));
        assert_eq!(empty.get(1, &"a"), None);
    }

    #[test]
    fn composite_line_at_keeps_the_tail_from_column_zero() {
        let base = "\x1b[4mAlpha Beta\x1b[24m";
        let overlay = "\x1b[1;7ma\x1b[22;27m";
        let merged = composite_line_at(base, overlay, 0, 1, 20);
        assert_eq!(crate::ansi_scan::strip_ansi(&merged).trim_end(), "alpha Beta");
        // A tag landing mid-label must not erase the head before it.
        let merged = composite_line_at(base, "\x1b[1;7mb\x1b[22;27m", 6, 1, 20);
        assert_eq!(crate::ansi_scan::strip_ansi(&merged).trim_end(), "Alpha beta");
    }

    #[test]
    fn disambiguate_display_labels_suffixes_collisions() {
        let raw = vec![
            "Retry\rnow".to_string(),
            "Retry now".to_string(),
            "Other (type your own)".to_string(),
            "Other (type your own)".to_string(),
        ];
        let reserved = vec!["Other (type your own)".to_string()];
        assert_eq!(
            disambiguate_display_labels(&raw, &reserved),
            vec![
                "Retry now".to_string(),
                "Retry now (2)".to_string(),
                "Other (type your own) (2)".to_string(),
                "Other (type your own) (3)".to_string(),
            ]
        );
        assert_eq!(
            disambiguate_display_labels(&["Alpha".to_string()], &["Alpha".to_string()]),
            vec!["Alpha (2)".to_string()]
        );
    }

    #[test]
    fn inline_image_caps_resolve_from_settings_and_viewport() {
        set_inline_image_max_columns(40);
        set_inline_image_max_rows(0);
        assert_eq!(
            resolve_image_options_with_rows(Some(50)),
            ImageOptions {
                max_width_cells: 40,
                max_height_cells: Some(30),
            }
        );
        assert_eq!(
            resolve_image_options_with_rows(None),
            ImageOptions {
                max_width_cells: 40,
                max_height_cells: None,
            }
        );

        set_inline_image_max_rows(10);
        assert_eq!(
            resolve_image_options_with_rows(Some(50)),
            ImageOptions {
                max_width_cells: 40,
                max_height_cells: Some(10),
            }
        );
        assert_eq!(
            resolve_image_options_with_rows(Some(10)),
            ImageOptions {
                max_width_cells: 40,
                max_height_cells: Some(6),
            }
        );
        assert_eq!(
            resolve_image_options_with_rows(None),
            ImageOptions {
                max_width_cells: 40,
                max_height_cells: Some(10),
            }
        );

        set_inline_image_max_columns(100);
        set_inline_image_max_rows(20);
    }

    #[test]
    fn preview_limits_match_the_reference() {
        assert_eq!(PREVIEW_LIMITS.COLLAPSED_LINES, 3);
        assert_eq!(PREVIEW_LIMITS.EXPANDED_LINES, 12);
        assert_eq!(PREVIEW_LIMITS.COLLAPSED_ITEMS, 8);
        assert_eq!(PREVIEW_LIMITS.LOCATIONS_COLLAPSED, 2);
        assert_eq!(PREVIEW_LIMITS.OUTPUT_COLLAPSED, 3);
        assert_eq!(PREVIEW_LIMITS.OUTPUT_EXPANDED, 10);
        assert_eq!(PREVIEW_LIMITS.COMPUTER_CODE_COLLAPSED, 10);
        assert_eq!(PREVIEW_LIMITS.DIFF_COLLAPSED_HUNKS, 8);
        assert_eq!(PREVIEW_LIMITS.DIFF_COLLAPSED_LINES, 40);
        assert_eq!(DEFAULT_TERMINAL_PREVIEW_LINES, 10);
    }

    #[test]
    fn fence_regex_captures_indent_and_marker() {
        let opened = FENCE_RE.captures("  ```rust").expect("fence opener");
        assert_eq!(opened.get(1).map(|matched| matched.as_str()), Some("  "));
        assert_eq!(opened.get(2).map(|matched| matched.as_str()), Some("```"));
        let tilde = FENCE_RE.captures("~~~~").expect("fence opener");
        assert_eq!(tilde.get(1).map(|matched| matched.as_str()), Some(""));
        assert_eq!(tilde.get(2).map(|matched| matched.as_str()), Some("~~~~"));
        assert!(FENCE_RE.captures("    ```").is_none());
        assert!(FENCE_RE.captures("plain").is_none());
    }

    #[test]
    fn empty_link_targets_is_empty() {
        assert!(EMPTY_LINK_TARGETS.is_empty());
        assert!(EMPTY_LINK_TARGETS.get("omp://missing").is_none());
    }

    #[test]
    fn truncate_lengths_match_the_reference() {
        assert_eq!(TRUNCATE_LENGTHS.title, 60);
        assert_eq!(TRUNCATE_LENGTHS.content, 80);
        assert_eq!(TRUNCATE_LENGTHS.long, 100);
        assert_eq!(TRUNCATE_LENGTHS.line, 110);
        assert_eq!(TRUNCATE_LENGTHS.short, 40);
        assert_eq!(TRUNCATE_LENGTHS.recap, 280);
    }

    #[test]
    fn get_domain_strips_www_and_port() {
        assert_eq!(get_domain("https://www.example.com/a"), "example.com");
        assert_eq!(get_domain("http://example.com:8080/x"), "example.com");
        assert_eq!(get_domain("not a url"), "not a url");
    }

    #[test]
    fn to_path_list_parses_json_arrays_only() {
        assert_eq!(
            to_path_list("[\"a.ts\",\"b.ts\"]"),
            vec!["a.ts".to_string(), "b.ts".to_string()]
        );
        assert_eq!(to_path_list("a.ts"), vec!["a.ts".to_string()]);
        assert_eq!(to_path_list("[1]"), vec!["[1]".to_string()]);
    }

    #[test]
    fn parse_error_helpers_dedupe_and_cap() {
        let errors = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(
            dedupe_parse_errors(Some(&errors)),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            format_parse_errors(&errors, Some(3)),
            vec![
                "Parse issues (2 / 3):".to_string(),
                "- a".to_string(),
                "- b".to_string()
            ]
        );
        let (capped, total) = cap_parse_errors(Some(&errors), Some(1));
        assert_eq!(capped, vec!["a".to_string()]);
        assert_eq!(total, 2);
        assert_eq!(format_parse_errors_count_label(&[], None), "0 parse issues");
        assert_eq!(
            format_parse_errors_count_label(&[], Some(21)),
            "20 / 21 parse issues"
        );
    }

    #[test]
    fn shorten_path_replaces_only_home_prefix() {
        assert_eq!(shorten_path("/home/me/a/b", Some("/home/me")), "~/a/b");
        assert_eq!(shorten_path("/home/methere", Some("/home/me")), "/home/methere");
        assert_eq!(shorten_path("/other", None), "/other");
    }

    #[test]
    fn format_diagnostics_groups_parsed_messages() {
        let theme = sample_theme();
        let diag = DiagnosticsInput {
            errored: true,
            summary: "1 error".to_string(),
            messages: vec!["src/a.ts:12:3 [error] [ts] Type mismatch (ts2322)".to_string()],
        };
        let output = format_diagnostics(&diag, false, &theme, &|_| "ts".to_string(), None);
        assert!(output.contains("Diagnostics"));
        assert!(output.contains("src/a.ts"));
        assert!(output.contains("Type mismatch"));
    }
}
