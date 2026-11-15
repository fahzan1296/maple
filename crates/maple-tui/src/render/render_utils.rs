//! Shared formatting, truncation, and styling helpers for the render
//! primitives.
//!
//! Every helper here is deterministic and side-effect free: they shape strings
//! for display and never touch I/O. The few process-wide knobs (tight-layout
//! padding, the expand key hint, sixel passthrough) are set once at startup by
//! the embedder; `render` only reads them.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use unicode_segmentation::UnicodeSegmentation;


use crate::ansi_scan::{scan, Token};
use crate::style::{Color, Style};
use crate::theme::{Theme, ThemeBg, ThemeColor};
use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};
use unicode_width::UnicodeWidthStr;

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
    let mut collecting_after = false;
    for token in scan(line) {
        match token {
            Token::Escape(seq) => {
                if collecting_after || column >= skip_start {
                    after.push_str(seq);
                } else if column < before_target {
                    before.push_str(seq);
                }
            }
            Token::Text(chunk) => {
                for grapheme in chunk.graphemes(true) {
                    let width = UnicodeWidthStr::width(grapheme);
                    if !collecting_after && column < before_target {
                        if column + width > before_target {
                            collecting_after = true;
                        } else {
                            before.push_str(grapheme);
                            column += width;
                            continue;
                        }
                    }
                    if collecting_after && column >= skip_start && visible_width(&after) < after_target {
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

#[cfg(test)]
pub mod tests {
    use super::*;
    /// A resolved theme for renderer tests, mirroring `theme_class`'s sample.
    pub fn sample_theme() -> Theme {
        use crate::theme::color::{ColorMode, ColorValue};
        use crate::theme::schema::ColorMap;
        use crate::theme::symbols::{NormalizedSpinnerFrames, SymbolPreset};

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
            SymbolPreset::Unicode,
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
}
