//! Inline output capture, artifact spill, and byte/line/middle truncation.
//!
//! Ported from `l/src/tools/streaming-output.ts`.
//!
//! Deviations:
//! - The reference is async (`async enforceInlineByteCap`, `async dump`,
//!   `Bun.FileSink`, `WritableStream`); the port is synchronous and writes the
//!   artifact with `std::fs::File`. `create_input()` (a WHATWG
//!   `WritableStream`) has no Rust equivalent and is omitted; callers push
//!   `&str` chunks through [`OutputSink::push`] or [`TailBuffer::append`].
//! - `onChunk` throttling has no background timer in Rust: a chunk inside the
//!   throttle window is coalesced and flushed on the next push, `preview`, or
//!   `dump`.
//! - `Buffer.byteLength(text, "utf-8")` is `text.len()` in Rust; JavaScript's
//!   UTF-16 `String.length` is a `char` count here.
//! - `materializeString` is a no-op in Rust (strings own their bytes).

use std::fs::File;
use std::time::Instant;

use crate::render::sixel::{is_sixel_passthrough_enabled, sanitize_with_sixel_passthrough};
use crate::tools::native_view::ContentBlock;
use crate::tools::shell_viz::sanitize_text;

// =============================================================================
// Constants
// =============================================================================

/// Default maximum number of inline output lines.
pub const DEFAULT_MAX_LINES: usize = 3000;
/// Default inline output budget in UTF-8 bytes.
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024;
/// Default UTF-8 byte budget per match line.
pub const DEFAULT_MAX_COLUMN: usize = 512;

/// Default artifact-on-disk cap for [`OutputSink`]: 16 MiB.
pub const ARTIFACT_DEFAULT_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Default head budget; the remainder becomes the rolling tail window.
pub const ARTIFACT_DEFAULT_HEAD_BYTES: usize = 3 * 1024 * 1024;
/// Upper bound on how far the rolling artifact tail may outgrow its budget
/// between trims.
const ARTIFACT_TAIL_MAX_SLACK_BYTES: usize = 4 * 1024 * 1024;

/// Maximum number of pending chunks [`TailBuffer`] joins before compacting.
const MAX_PENDING: usize = 10;

const NL: char = '\n';
const CR: char = '\r';
const ELLIPSIS: &str = "…";

// =============================================================================
// Interfaces
// =============================================================================

/// First failed artifact I/O operation; safe to persist without exposing
/// filesystem errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputArtifactError {
    /// Opening the artifact failed.
    Open,
    /// Writing failed.
    Write,
    /// Flushing failed.
    Flush,
    /// Closing failed.
    End,
}

impl OutputArtifactError {
    /// Wire name of the failure.
    pub fn as_str(self) -> &'static str {
        match self {
            OutputArtifactError::Open => "open",
            OutputArtifactError::Write => "write",
            OutputArtifactError::Flush => "flush",
            OutputArtifactError::End => "end",
        }
    }
}

impl std::fmt::Display for OutputArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Captured output with inline truncation and artifact storage metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputSummary {
    /// Composed inline output.
    pub output: String,
    /// Whether anything was dropped.
    pub truncated: bool,
    /// Total source lines.
    pub total_lines: usize,
    /// Total source bytes.
    pub total_bytes: usize,
    /// Lines kept inline.
    pub output_lines: usize,
    /// Bytes kept inline.
    pub output_bytes: usize,
    /// Bytes elided from the middle when head-retain mode is active.
    pub elided_bytes: Option<usize>,
    /// Lines elided from the middle when head-retain mode is active.
    pub elided_lines: Option<usize>,
    /// Bytes dropped by the per-line column cap.
    pub column_dropped_bytes: Option<usize>,
    /// Number of distinct lines that hit the per-line column cap.
    pub column_truncated_lines: Option<usize>,
    /// 1-indexed first and last lines the column cap cut.
    pub column_truncated_range: Option<ColumnRange>,
    /// Configured per-line column cap in effect, when > 0.
    pub column_max: Option<usize>,
    /// Artifact ID for internal URL access when truncated.
    pub artifact_id: Option<String>,
    /// Bytes the artifact cap dropped from the saved file's middle.
    pub artifact_elided_bytes: Option<usize>,
    /// Full raw output was not completely saved.
    pub artifact_error: Option<OutputArtifactError>,
}

/// Inclusive 1-indexed line range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnRange {
    /// First line.
    pub first: usize,
    /// Last line.
    pub last: usize,
}

/// Inline capture, artifact storage, and streaming callback options.
#[derive(Default)]
pub struct OutputSinkOptions {
    /// Path of the spill file.
    pub artifact_path: Option<String>,
    /// Artifact id reported to the model.
    pub artifact_id: Option<String>,
    /// Total inline body budget (bytes).
    pub spill_threshold: Option<usize>,
    /// Bytes kept in addition to the rolling tail window (head retention).
    pub head_bytes: Option<usize>,
    /// Per-line byte cap.
    pub max_columns: Option<usize>,
    /// Live preview callback.
    pub on_chunk: Option<Box<dyn FnMut(&str)>>,
    /// Minimum milliseconds between `on_chunk` calls.
    pub chunk_throttle_ms: Option<u64>,
    /// Cap on bytes written to the artifact file.
    pub artifact_max_bytes: Option<usize>,
    /// Bytes reserved for the artifact head window.
    pub artifact_head_bytes: Option<usize>,
}

/// Retained text and measurements describing a truncated output window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TruncationResult {
    /// Retained content.
    pub content: String,
    /// Whether the input was truncated.
    pub truncated: Option<bool>,
    /// What bounded the window.
    pub truncated_by: Option<TruncateBy>,
    /// Total source lines.
    pub total_lines: usize,
    /// Total source bytes.
    pub total_bytes: usize,
    /// Lines retained.
    pub output_lines: Option<usize>,
    /// Bytes retained.
    pub output_bytes: Option<usize>,
    /// Bytes elided from the middle.
    pub elided_bytes: Option<usize>,
    /// Lines elided from the middle.
    pub elided_lines: Option<usize>,
    /// Exact source-line counts retained before the middle marker.
    pub head_lines: Option<usize>,
    /// Exact source-line counts retained after the middle marker.
    pub tail_lines: Option<usize>,
    /// True when either retained side is a partial byte range of a source line.
    pub partial_byte_windows: Option<bool>,
    /// True when the retained suffix ends with a partial final source line.
    pub last_line_partial: Option<bool>,
    /// The first line exceeds the byte limit.
    pub first_line_exceeds_limit: Option<bool>,
}

/// What bounded a truncation window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruncateBy {
    /// Line cap.
    Lines,
    /// Byte cap.
    Bytes,
    /// Middle elision.
    Middle,
}

/// Line and byte budgets for head, tail, or middle truncation.
#[derive(Clone, Copy, Debug, Default)]
pub struct TruncationOptions {
    /// Maximum number of lines (default: 3000).
    pub max_lines: Option<usize>,
    /// Maximum number of bytes (default: 50KB).
    pub max_bytes: Option<usize>,
    /// Bytes reserved for the head window of middle truncation.
    pub max_head_bytes: Option<usize>,
    /// Lines reserved for the head window of middle truncation.
    pub max_head_lines: Option<usize>,
}

/// Result from byte-level truncation helpers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteTruncationResult {
    /// Retained text.
    pub text: String,
    /// Retained UTF-8 byte count.
    pub bytes: usize,
}

/// A string or byte slice input.
#[derive(Clone, Copy, Debug)]
pub enum ByteInput<'a> {
    /// UTF-8 text.
    Text(&'a str),
    /// Raw bytes.
    Bytes(&'a [u8]),
}

/// Which end of the input a byte window keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruncateMode {
    /// Keep the head.
    Head,
    /// Keep the tail.
    Tail,
}

/// Options for [`enforce_inline_byte_cap`].
#[derive(Default)]
pub struct InlineByteCapOptions {
    /// Inline byte budget. Defaults to [`DEFAULT_MAX_BYTES`].
    pub max_bytes: Option<usize>,
    /// Persist the full text as a session artifact; returns the artifact id.
    pub save_artifact: Option<Box<dyn Fn(&str) -> Option<String>>>,
}

/// Context included in a notice for tail-retained output.
#[derive(Clone, Debug, Default)]
pub struct TailTruncationNoticeOptions {
    /// Full output path.
    pub full_output_path: Option<String>,
    /// Original content.
    pub original_content: Option<String>,
    /// Suffix appended inside the brackets.
    pub suffix: Option<String>,
}

/// File coordinates included in a notice for head-retained output.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeadTruncationNoticeOptions {
    /// First shown line.
    pub start_line: Option<usize>,
    /// Total file lines.
    pub total_file_lines: Option<usize>,
}

/// A single line truncation result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineTruncation {
    /// Retained text.
    pub text: String,
    /// Whether the line was cut.
    pub was_truncated: bool,
}

/// A match found in column-capped output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnCappedMatch {
    /// Start byte offset.
    pub start: usize,
    /// End byte offset.
    pub end: usize,
}

/// Options for [`OutputSink::push`].
#[derive(Clone, Debug, Default)]
pub struct PushOptions {
    /// Bounded inline substitute for the in-memory buffer and live preview.
    pub inline: Option<String>,
    /// Keep the inline substitute out of the live callback.
    pub emit_inline: Option<bool>,
}

/// A streaming update payload for [`stream_tail_updates`].
#[derive(Clone, Debug)]
pub struct ToolUpdate<TDetails> {
    /// Text content blocks.
    pub content: Vec<ContentBlock>,
    /// Tool details.
    pub details: TDetails,
}

// =============================================================================
// Low-level helpers
// =============================================================================

fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|byte| *byte == b'\n').count()
}

fn utf8_boundary_forward(bytes: &[u8], mut index: usize) -> usize {
    if index > bytes.len() {
        index = bytes.len();
    }
    while index < bytes.len() && (bytes[index] & 0xc0) == 0x80 {
        index += 1;
    }
    index
}

fn utf8_boundary_backward(bytes: &[u8], cut: usize) -> usize {
    let mut index = cut.min(bytes.len());
    if index >= bytes.len() {
        return bytes.len();
    }
    while index > 0 && (bytes[index] & 0xc0) == 0x80 {
        index -= 1;
    }
    index
}

fn truncate_bytes_windowed(
    data: ByteInput<'_>,
    max_bytes: usize,
    mode: TruncateMode,
) -> ByteTruncationResult {
    if max_bytes == 0 {
        return ByteTruncationResult {
            text: String::new(),
            bytes: 0,
        };
    }

    let bytes: &[u8] = match data {
        ByteInput::Text(text) => text.as_bytes(),
        ByteInput::Bytes(bytes) => bytes,
    };
    if bytes.len() <= max_bytes {
        return ByteTruncationResult {
            text: decode_utf8(bytes),
            bytes: bytes.len(),
        };
    }
    match mode {
        TruncateMode::Head => {
            let end = utf8_boundary_backward(bytes, max_bytes);
            if end == 0 {
                return ByteTruncationResult {
                    text: String::new(),
                    bytes: 0,
                };
            }
            ByteTruncationResult {
                text: decode_utf8(&bytes[..end]),
                bytes: end,
            }
        }
        TruncateMode::Tail => {
            let start = utf8_boundary_forward(bytes, bytes.len() - max_bytes);
            ByteTruncationResult {
                text: decode_utf8(&bytes[start..]),
                bytes: bytes.len() - start,
            }
        }
    }
}

fn decode_utf8(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Truncate a string/buffer to fit within a byte limit, keeping the tail.
pub fn truncate_tail_bytes(data: ByteInput<'_>, max_bytes: usize) -> ByteTruncationResult {
    truncate_bytes_windowed(data, max_bytes, TruncateMode::Tail)
}

/// Truncate a string/buffer to fit within a byte limit, keeping the head.
pub fn truncate_head_bytes(data: ByteInput<'_>, max_bytes: usize) -> ByteTruncationResult {
    truncate_bytes_windowed(data, max_bytes, TruncateMode::Head)
}

/// Truncate a single line to `max_chars`, appending `…` if truncated.
pub fn truncate_line(line: &str, max_chars: usize) -> LineTruncation {
    if line.chars().count() <= max_chars {
        return LineTruncation {
            text: line.to_string(),
            was_truncated: false,
        };
    }
    let mut text: String = line.chars().take(max_chars).collect();
    text.push('…');
    LineTruncation {
        text,
        was_truncated: true,
    }
}

/// Truncate a single line to a UTF-8 byte budget, appending `…` if truncated.
pub fn truncate_line_bytes(line: &str, max_bytes: usize) -> LineTruncation {
    if line.len() <= max_bytes {
        return LineTruncation {
            text: line.to_string(),
            was_truncated: false,
        };
    }
    let head = truncate_head_bytes(ByteInput::Text(line), max_bytes.saturating_sub(3)).text;
    LineTruncation {
        text: format!("{head}{ELLIPSIS}"),
        was_truncated: true,
    }
}

/// Leading characters of a text's first line [`find_column_capped`] searches
/// for.
const CAPPED_PROBE_CHARS: usize = 16;

/// Where `text` shows in `output` written through an [`OutputSink`] with
/// `max_columns`, searching from `from`.
pub fn find_column_capped(
    output: &str,
    text: &str,
    from: usize,
) -> Option<ColumnCappedMatch> {
    if text.is_empty() {
        return None;
    }
    let lines: Vec<&str> = text.split(NL).collect();
    let first = lines.first().copied().unwrap_or("");
    let probe = first.chars().take(CAPPED_PROBE_CHARS).collect::<String>();
    if probe.is_empty() {
        return None;
    }
    let mut search = from;
    while let Some(offset) = output[search..].find(&probe) {
        let at = search + offset;
        if let Some(end) = capped_lines_end(output, at, &lines) {
            return Some(ColumnCappedMatch { start: at, end });
        }
        search = at + 1;
    }
    None
}

fn capped_lines_end(output: &str, at: usize, lines: &[&str]) -> Option<usize> {
    let mut pos = at;
    for (index, line) in lines.iter().enumerate() {
        let last = index == lines.len() - 1;
        if output[pos..].starts_with(line) {
            let end = pos + line.len();
            if last {
                if end == output.len() || output.as_bytes().get(end) == Some(&b'\n') {
                    return Some(end);
                }
                return None;
            }
            if output.as_bytes().get(end) == Some(&b'\n') {
                pos = end + 1;
                continue;
            }
            return None;
        }
        let eol = output[pos..]
            .find(NL)
            .map(|offset| pos + offset)
            .unwrap_or(output.len());
        if eol < pos + ELLIPSIS.len() {
            return None;
        }
        let marker_start = eol - ELLIPSIS.len();
        if &output[marker_start..eol] != ELLIPSIS || !line.starts_with(&output[pos..marker_start]) {
            return None;
        }
        if last {
            return Some(eol);
        }
        if eol == output.len() {
            return None;
        }
        pos = eol + 1;
    }
    Some(pos)
}

// =============================================================================
// Content truncation
// =============================================================================

/// Build a no-truncation result.
pub fn no_trunc_result(content: &str, total_lines: Option<usize>, total_bytes: Option<usize>) -> TruncationResult {
    let total_lines = total_lines.unwrap_or_else(|| count_newlines(content) + 1);
    let total_bytes = total_bytes.unwrap_or(content.len());
    TruncationResult {
        content: content.to_string(),
        total_lines,
        total_bytes,
        ..TruncationResult::default()
    }
}

/// Truncate content from the head (keep first N lines/bytes).
pub fn truncate_head(content: &str, options: TruncationOptions) -> TruncationResult {
    let max_lines = options.max_lines.unwrap_or(DEFAULT_MAX_LINES);
    let max_bytes = options.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let total_bytes = content.len();
    let total_lines = count_newlines(content) + 1;

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return no_trunc_result(content, Some(total_lines), Some(total_bytes));
    }

    let mut included_lines = 0usize;
    let mut bytes_used = 0usize;
    let mut cut_index = 0usize;
    let mut cursor = 0usize;
    let mut truncated_by = TruncateBy::Lines;

    while included_lines < max_lines {
        let nl = content[cursor..].find(NL).map(|offset| cursor + offset);
        let line_end = nl.unwrap_or(content.len());
        let sep_bytes = if included_lines > 0 { 1 } else { 0 };
        let remaining = max_bytes.saturating_sub(bytes_used + sep_bytes);
        if remaining == 0 && max_bytes < bytes_used + sep_bytes {
            truncated_by = TruncateBy::Bytes;
            break;
        }
        let line = &content[cursor..line_end];
        if line.chars().count() > remaining {
            truncated_by = TruncateBy::Bytes;
            if included_lines == 0 {
                return TruncationResult {
                    content: String::new(),
                    truncated: Some(true),
                    truncated_by: Some(TruncateBy::Bytes),
                    total_lines,
                    total_bytes,
                    output_lines: Some(0),
                    output_bytes: Some(0),
                    last_line_partial: Some(false),
                    first_line_exceeds_limit: Some(true),
                    ..TruncationResult::default()
                };
            }
            break;
        }
        let line_bytes = line.len();
        if line_bytes > remaining {
            truncated_by = TruncateBy::Bytes;
            if included_lines == 0 {
                return TruncationResult {
                    content: String::new(),
                    truncated: Some(true),
                    truncated_by: Some(TruncateBy::Bytes),
                    total_lines,
                    total_bytes,
                    output_lines: Some(0),
                    output_bytes: Some(0),
                    last_line_partial: Some(false),
                    first_line_exceeds_limit: Some(true),
                    ..TruncationResult::default()
                };
            }
            break;
        }
        bytes_used += sep_bytes + line_bytes;
        included_lines += 1;
        cut_index = line_end;
        match nl {
            Some(nl) => cursor = nl + 1,
            None => break,
        }
    }

    if included_lines >= max_lines && bytes_used <= max_bytes {
        truncated_by = TruncateBy::Lines;
    }

    TruncationResult {
        content: content[..cut_index].to_string(),
        truncated: Some(true),
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines: Some(included_lines),
        output_bytes: Some(bytes_used),
        last_line_partial: Some(false),
        first_line_exceeds_limit: Some(false),
        ..TruncationResult::default()
    }
}

/// Truncate content from the tail (keep last N lines/bytes).
pub fn truncate_tail(content: &str, options: TruncationOptions) -> TruncationResult {
    let max_lines = options.max_lines.unwrap_or(DEFAULT_MAX_LINES);
    let max_bytes = options.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let total_bytes = content.len();
    let total_lines = count_newlines(content) + 1;

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return no_trunc_result(content, Some(total_lines), Some(total_bytes));
    }

    let mut included_lines = 0usize;
    let mut bytes_used = 0usize;
    let mut start_index = content.len();
    let mut end = content.len();
    let mut truncated_by = TruncateBy::Lines;

    while included_lines < max_lines {
        let nl = content[..end].rfind(NL);
        let line_start = nl.map(|index| index + 1).unwrap_or(0);
        let sep_bytes = if included_lines > 0 { 1 } else { 0 };
        let remaining = max_bytes.saturating_sub(bytes_used + sep_bytes);
        if remaining == 0 && max_bytes < bytes_used + sep_bytes {
            truncated_by = TruncateBy::Bytes;
            break;
        }
        let line = &content[line_start..end];
        let mut partial_line: Option<ByteTruncationResult> = None;
        if line.chars().count() > max_bytes {
            truncated_by = TruncateBy::Bytes;
            let window_start = line_start.max(end.saturating_sub(remaining));
            let window = &content[window_start..end];
            partial_line = Some(truncate_tail_bytes(ByteInput::Text(window), remaining));
        } else {
            let line_bytes = line.len();
            if line_bytes > remaining {
                truncated_by = TruncateBy::Bytes;
                if line_bytes > max_bytes {
                    partial_line = Some(truncate_tail_bytes(ByteInput::Text(line), remaining));
                }
            } else {
                bytes_used += sep_bytes + line_bytes;
                included_lines += 1;
                start_index = line_start;
                match nl {
                    Some(nl) => end = nl,
                    None => break,
                }
                continue;
            }
        }
        if let Some(partial) = partial_line {
            if partial.bytes > 0 || included_lines == 0 {
                let has_complete_lines = included_lines > 0;
                let separator = if has_complete_lines && partial.bytes > 0 { NL } else { '\0' };
                let complete_tail = if has_complete_lines { &content[start_index..] } else { "" };
                let mut output = partial.text.clone();
                if separator != '\0' {
                    output.push(separator);
                }
                output.push_str(complete_tail);
                return TruncationResult {
                    content: output,
                    truncated: Some(true),
                    truncated_by: Some(TruncateBy::Bytes),
                    total_lines,
                    total_bytes,
                    output_lines: Some(included_lines + usize::from(partial.bytes > 0)),
                    output_bytes: Some(bytes_used + usize::from(separator != '\0') + partial.bytes),
                    partial_byte_windows: Some(partial.bytes > 0),
                    last_line_partial: Some(partial.bytes > 0 && !has_complete_lines),
                    first_line_exceeds_limit: Some(false),
                    ..TruncationResult::default()
                };
            }
        }
        break;
    }

    if included_lines >= max_lines && bytes_used <= max_bytes {
        truncated_by = TruncateBy::Lines;
    }

    TruncationResult {
        content: content[start_index..].to_string(),
        truncated: Some(true),
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines: Some(included_lines),
        output_bytes: Some(bytes_used),
        last_line_partial: Some(false),
        first_line_exceeds_limit: Some(false),
        ..TruncationResult::default()
    }
}

/// Format the inline marker substituted for an elided middle region.
pub fn format_middle_elision_marker(elided_lines: usize, elided_bytes: usize) -> String {
    if elided_lines <= 1 {
        format!("[…{elided_bytes}B elided…]")
    } else {
        format!("[…{elided_lines}ln elided…]")
    }
}

/// Truncate content keeping a head window and a tail window, eliding the
/// middle.
pub fn truncate_middle(content: &str, options: TruncationOptions) -> TruncationResult {
    let max_bytes = options.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let max_lines = options.max_lines.unwrap_or(DEFAULT_MAX_LINES);
    let head_bytes = options.max_head_bytes.unwrap_or(max_bytes / 2);
    let tail_bytes = max_bytes.saturating_sub(head_bytes);
    let head_lines = options.max_head_lines.unwrap_or((max_lines / 2).max(1));
    let tail_lines = max_lines.saturating_sub(head_lines);
    let total_bytes = content.len();
    let total_lines = count_newlines(content) + 1;

    if total_bytes <= max_bytes && total_lines <= max_lines {
        return no_trunc_result(content, Some(total_lines), Some(total_bytes));
    }
    if head_bytes == 0 || head_lines == 0 {
        return truncate_tail(
            content,
            TruncationOptions {
                max_bytes: Some(if tail_bytes == 0 { max_bytes } else { tail_bytes }),
                max_lines: Some(if tail_lines == 0 { max_lines } else { tail_lines }),
                ..TruncationOptions::default()
            },
        );
    }
    if tail_bytes == 0 || tail_lines == 0 {
        return truncate_head(
            content,
            TruncationOptions {
                max_bytes: Some(head_bytes),
                max_lines: Some(head_lines),
                ..TruncationOptions::default()
            },
        );
    }

    let head = truncate_head(
        content,
        TruncationOptions {
            max_bytes: Some(head_bytes),
            max_lines: Some(head_lines),
            ..TruncationOptions::default()
        },
    );
    let tail = truncate_tail(
        content,
        TruncationOptions {
            max_bytes: Some(tail_bytes),
            max_lines: Some(tail_lines),
            ..TruncationOptions::default()
        },
    );
    let head_lines_kept = head.output_lines.unwrap_or(0);
    let tail_lines_kept = tail.output_lines.unwrap_or(0);
    let head_bytes_kept = head.output_bytes.unwrap_or(head.content.len());
    let tail_bytes_kept = tail.output_bytes.unwrap_or(tail.content.len());
    let tail_has_partial_window = tail.partial_byte_windows == Some(true) || tail.last_line_partial == Some(true);

    if head_lines_kept == 0 || head.first_line_exceeds_limit == Some(true) || tail_lines_kept == 0 {
        let use_byte_head = head_lines_kept == 0 || head.first_line_exceeds_limit == Some(true);
        let byte_head = if use_byte_head {
            truncate_head_bytes(ByteInput::Text(content), head_bytes.min(total_bytes))
        } else {
            ByteTruncationResult {
                text: head.content.clone(),
                bytes: head_bytes_kept,
            }
        };
        let actual_head_lines = if use_byte_head { 1 } else { head_lines_kept };
        let remaining_bytes = total_bytes.saturating_sub(byte_head.bytes);
        let use_byte_tail = tail_lines_kept == 0;
        let byte_tail = if use_byte_tail {
            truncate_tail_bytes(ByteInput::Text(content), tail_bytes.min(remaining_bytes))
        } else {
            let limited = truncate_tail(
                content,
                TruncationOptions {
                    max_bytes: Some(tail_bytes.min(remaining_bytes)),
                    max_lines: Some(tail_lines),
                    ..TruncationOptions::default()
                },
            );
            ByteTruncationResult {
                text: limited.content.clone(),
                bytes: limited.output_bytes.unwrap_or(limited.content.len()),
            }
        };
        let actual_tail_lines = if use_byte_tail { 1 } else { tail_lines_kept.min(tail_lines) };
        let elided_bytes = total_bytes.saturating_sub(byte_head.bytes + byte_tail.bytes);
        if elided_bytes == 0 {
            return no_trunc_result(content, Some(total_lines), Some(total_bytes));
        }
        let marker = format_middle_elision_marker(0, elided_bytes);
        let composed = format!("{}\n{}\n{}", byte_head.text, marker, byte_tail.text);
        return TruncationResult {
            content: composed.clone(),
            truncated: Some(true),
            truncated_by: Some(TruncateBy::Middle),
            total_lines,
            total_bytes,
            output_lines: Some(actual_head_lines + actual_tail_lines + 1),
            output_bytes: Some(composed.len()),
            elided_lines: Some(total_lines.saturating_sub(actual_head_lines + actual_tail_lines)),
            head_lines: Some(actual_head_lines),
            tail_lines: Some(actual_tail_lines),
            partial_byte_windows: Some(use_byte_head || use_byte_tail || tail_has_partial_window),
            elided_bytes: Some(elided_bytes),
            last_line_partial: tail.last_line_partial,
            first_line_exceeds_limit: Some(false),
        };
    }

    if head_lines_kept + tail_lines_kept >= total_lines && !tail_has_partial_window {
        return no_trunc_result(content, Some(total_lines), Some(total_bytes));
    }

    let elided_lines = total_lines - head_lines_kept - tail_lines_kept;
    let elided_bytes = total_bytes.saturating_sub(head_bytes_kept + tail_bytes_kept);
    let marker = format_middle_elision_marker(elided_lines, elided_bytes);
    let composed = format!("{}\n{}\n{}", head.content, marker, tail.content);
    let marker_bytes = marker.len();
    TruncationResult {
        content: composed.clone(),
        truncated: Some(true),
        truncated_by: Some(TruncateBy::Middle),
        total_lines,
        total_bytes,
        output_lines: Some(head_lines_kept + tail_lines_kept + 1),
        output_bytes: Some(head_bytes_kept + tail_bytes_kept + marker_bytes + 2),
        elided_lines: Some(elided_lines),
        elided_bytes: Some(elided_bytes),
        head_lines: Some(head_lines_kept),
        tail_lines: Some(tail_lines_kept),
        partial_byte_windows: Some(tail_has_partial_window),
        last_line_partial: tail.last_line_partial,
        first_line_exceeds_limit: Some(false),
    }
}

// =============================================================================
// Inline byte cap
// =============================================================================

fn trim_head_to_line_boundary(text: &str) -> &str {
    match text.rfind(NL) {
        Some(index) if index > 0 => &text[..index],
        _ => text,
    }
}

fn trim_tail_to_line_boundary(text: &str) -> &str {
    match text.find(NL) {
        Some(index) if index != text.len() - 1 => &text[index + 1..],
        _ => text,
    }
}

/// Final-defense inline size guard for tool results.
pub fn enforce_inline_byte_cap(text: &str, options: &InlineByteCapOptions) -> String {
    let max_bytes = options.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    if max_bytes == 0 || text.len() <= max_bytes {
        return text.to_string();
    }
    let head_window = truncate_head_bytes(ByteInput::Text(text), max_bytes * 6 / 10);
    let head = trim_head_to_line_boundary(&head_window.text);
    let tail_window = truncate_tail_bytes(ByteInput::Text(text), max_bytes / 4);
    let tail = trim_tail_to_line_boundary(&tail_window.text);
    let elided_bytes = text
        .len()
        .saturating_sub(head.len() + tail.len());
    let marker = format!("[…{elided_bytes}B elided…]");
    let mut composed = format!("{head}\n{marker}\n{tail}");
    if let Some(save) = &options.save_artifact {
        if let Some(artifact_id) = save(text) {
            if !composed.ends_with(NL) {
                composed.push(NL);
            }
            composed.push_str(&format!("[raw output: artifact://{artifact_id}]"));
        }
    }
    composed
}

// =============================================================================
// TailBuffer
// =============================================================================

/// Byte-bounded rolling text window with lazy chunk joining.
pub struct TailBuffer {
    max_bytes: usize,
    pending: Vec<String>,
    pos: usize,
    head_bytes: usize,
    head_clean: bool,
    rest_well_formed: bool,
}

impl TailBuffer {
    /// A buffer holding at most `max_bytes` bytes of tail.
    pub fn new(max_bytes: usize) -> Self {
        TailBuffer {
            max_bytes,
            pending: Vec::new(),
            pos: 0,
            head_bytes: 0,
            head_clean: true,
            rest_well_formed: true,
        }
    }

    /// Append a chunk, trimming the window back to budget.
    pub fn append(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.max_bytes == 0 {
            self.set_head("", 0, true);
            return;
        }
        let n = text.len();
        if n >= self.max_bytes {
            let truncated = truncate_tail_bytes(ByteInput::Text(text), self.max_bytes);
            let clean = truncated.text.len() == truncated.text.chars().count() || truncated.text.is_char_boundary(0);
            self.set_head(&truncated.text, truncated.bytes, clean);
            return;
        }
        if self.pending.is_empty() {
            self.set_head(text, n, true);
        } else {
            self.pos += n;
            self.pending.push(text.to_string());
            if self.pending.len() > MAX_PENDING {
                self.compact();
            }
        }
        if self.pos > self.max_bytes * 2 {
            self.trim_to(self.max_bytes);
        }
    }

    /// The current tail text.
    pub fn text(&mut self) -> String {
        self.trim_to(self.max_bytes);
        self.flush()
    }

    /// The current tail byte count.
    pub fn bytes(&mut self) -> usize {
        self.trim_to(self.max_bytes);
        self.pos
    }

    fn set_head(&mut self, text: &str, bytes: usize, clean: bool) {
        self.pending.clear();
        self.pending.push(text.to_string());
        self.pos = bytes;
        self.head_bytes = bytes;
        self.head_clean = clean;
        self.rest_well_formed = true;
    }

    fn compact(&mut self) {
        let joined = self.pending.concat();
        self.pending.clear();
        self.pending.push(joined);
        self.head_bytes = self.pos;
        self.head_clean = self.head_clean && self.rest_well_formed;
        self.rest_well_formed = true;
    }

    fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        if self.pending.len() > 1 {
            self.compact();
        }
        self.pending[0].clone()
    }

    fn trim_to(&mut self, max: usize) {
        if max == 0 {
            self.set_head("", 0, true);
            return;
        }
        if self.pos <= max {
            return;
        }
        let rest_bytes = self.pos - self.head_bytes;
        if rest_bytes < max && self.rest_well_formed {
            let head = self.pending[0].clone();
            let keep = max - rest_bytes;
            let drop = self.head_bytes - keep;
            if head.len() == self.head_bytes {
                self.pending[0] = head[drop..].to_string();
                self.head_bytes = keep;
            } else if self.head_clean && drop <= keep {
                let mut dropped = 0usize;
                let mut index = 0usize;
                for ch in head.char_indices() {
                    if dropped >= drop {
                        index = ch.0;
                        break;
                    }
                    dropped += ch.1.len_utf8();
                    index = ch.0 + ch.1.len_utf8();
                }
                self.pending[0] = head[index..].to_string();
                self.head_bytes -= dropped;
            } else {
                let truncated = truncate_tail_bytes(ByteInput::Text(&head), keep);
                self.head_clean = truncated.text.is_char_boundary(0);
                self.pending[0] = truncated.text;
                self.head_bytes = truncated.bytes;
                self.pos = self.head_bytes + rest_bytes;
                return;
            }
            self.head_clean = true;
            self.pos = self.head_bytes + rest_bytes;
            return;
        }
        let joined = self.flush();
        let truncated = truncate_tail_bytes(ByteInput::Text(&joined), max);
        self.set_head(&truncated.text, truncated.bytes, true);
    }
}

// =============================================================================
// OutputSink
// =============================================================================

struct FileState {
    path: String,
    artifact_id: Option<String>,
    file: Option<File>,
}

/// Captures bounded inline output while optionally preserving a full artifact.
pub struct OutputSink {
    buffer: String,
    buffer_bytes: usize,
    tail_untrimmed: bool,
    head: String,
    head_bytes: usize,
    head_lines: usize,
    head_retention_disabled: bool,
    total_lines: usize,
    total_bytes: usize,
    saw_data: bool,
    truncated: bool,
    last_chunk_time: Option<Instant>,
    pending_chunk: String,
    pending_carriage_return: bool,

    current_line_bytes: usize,
    column_ellipsis_added: bool,
    column_dropped_bytes: usize,
    column_truncated_lines: usize,
    column_first_cut_line: usize,
    column_last_cut_line: usize,
    column_cut_lines_unmapped: bool,

    file: Option<FileState>,
    pending_file_writes: Option<Vec<String>>,
    file_ready: bool,
    finalized: bool,
    artifact_error: Option<OutputArtifactError>,

    artifact_path: Option<String>,
    artifact_id: Option<String>,
    spill_threshold: usize,
    head_limit: usize,
    on_chunk: Option<Box<dyn FnMut(&str)>>,
    chunk_throttle_ms: u64,
    max_columns: usize,

    artifact_max_bytes: usize,
    artifact_head_budget: usize,
    artifact_tail_budget: usize,
    artifact_tail_slack: usize,
    artifact_head_bytes_written: usize,
    artifact_head_closed: bool,
    artifact_tail_ring: String,
    artifact_tail_ring_bytes: usize,
    artifact_tail_incoming_bytes: usize,
    artifact_elided_bytes: usize,
}

impl Default for OutputSink {
    fn default() -> Self {
        OutputSink::new(OutputSinkOptions::default())
    }
}

impl OutputSink {
    /// A sink with the given options.
    pub fn new(options: OutputSinkOptions) -> Self {
        let spill_threshold = options.spill_threshold.unwrap_or(DEFAULT_MAX_BYTES);
        let max_columns = options.max_columns.unwrap_or(0);
        let head_limit = options
            .head_bytes
            .unwrap_or(0)
            .min(spill_threshold / 2);
        let artifact_max_bytes = options
            .artifact_max_bytes
            .unwrap_or(ARTIFACT_DEFAULT_MAX_BYTES);
        let head_bytes_requested = options
            .artifact_head_bytes
            .unwrap_or(ARTIFACT_DEFAULT_HEAD_BYTES.min(artifact_max_bytes / 2));
        let artifact_head_budget = head_bytes_requested.min(artifact_max_bytes);
        let artifact_tail_budget = artifact_max_bytes - artifact_head_budget;
        OutputSink {
            buffer: String::new(),
            buffer_bytes: 0,
            tail_untrimmed: false,
            head: String::new(),
            head_bytes: 0,
            head_lines: 0,
            head_retention_disabled: false,
            total_lines: 0,
            total_bytes: 0,
            saw_data: false,
            truncated: false,
            last_chunk_time: None,
            pending_chunk: String::new(),
            pending_carriage_return: false,
            current_line_bytes: 0,
            column_ellipsis_added: false,
            column_dropped_bytes: 0,
            column_truncated_lines: 0,
            column_first_cut_line: 0,
            column_last_cut_line: 0,
            column_cut_lines_unmapped: false,
            file: None,
            pending_file_writes: None,
            file_ready: false,
            finalized: false,
            artifact_error: None,
            artifact_path: options.artifact_path.clone(),
            artifact_id: options.artifact_id.clone(),
            spill_threshold,
            head_limit,
            on_chunk: options.on_chunk,
            chunk_throttle_ms: options.chunk_throttle_ms.unwrap_or(0),
            max_columns,
            artifact_max_bytes,
            artifact_head_budget,
            artifact_tail_budget,
            artifact_tail_slack: artifact_tail_budget.min(ARTIFACT_TAIL_MAX_SLACK_BYTES),
            artifact_head_bytes_written: 0,
            artifact_head_closed: false,
            artifact_tail_ring: String::new(),
            artifact_tail_ring_bytes: 0,
            artifact_tail_incoming_bytes: 0,
            artifact_elided_bytes: 0,
        }
    }

    /// Push a chunk of output.
    pub fn push(&mut self, chunk: &str, options: Option<PushOptions>) {
        if self.finalized {
            return;
        }
        let normalized = self.normalize_carriage_returns(chunk);
        let sanitized = sanitize_with_sixel_passthrough(
            &normalized,
            is_sixel_passthrough_enabled(),
            sanitize_text,
        );
        let options = options.unwrap_or_default();
        let substituted = options.inline.is_some();
        let inline_chunk = match &options.inline {
            Some(inline) => sanitize_text(inline),
            None => sanitized.clone(),
        };
        self.store(&sanitized, &inline_chunk, substituted);

        if let Some(callback) = self.on_chunk.as_mut() {
            if options.emit_inline != Some(false) && !inline_chunk.is_empty() {
                let now = Instant::now();
                let due = self
                    .last_chunk_time
                    .map_or(true, |last| {
                        now.duration_since(last).as_millis() as u64 >= self.chunk_throttle_ms
                    });
                if due {
                    self.last_chunk_time = Some(now);
                    let merged = std::mem::take(&mut self.pending_chunk);
                    let merged = format!("{merged}{inline_chunk}");
                    callback(&merged);
                } else {
                    self.pending_chunk.push_str(&inline_chunk);
                }
            }
        }
    }

    fn store(&mut self, chunk: &str, inline_chunk: &str, substituted: bool) {
        let raw_bytes = chunk.len();
        self.total_bytes += raw_bytes;
        let first_line = if substituted { 0 } else { self.total_lines + 1 };
        if !chunk.is_empty() {
            self.saw_data = true;
            self.total_lines += count_newlines(chunk);
        }
        let inline_bytes = if substituted { inline_chunk.len() } else { raw_bytes };
        let capped = if self.max_columns > 0 {
            self.apply_column_cap(inline_chunk, first_line)
        } else {
            inline_chunk.to_string()
        };
        let capped_bytes = if capped == inline_chunk {
            inline_bytes
        } else {
            capped.len()
        };
        let capped_this_chunk = capped_bytes < inline_bytes;
        if substituted {
            self.truncated = true;
        }

        if self.artifact_path.is_some()
            && (self.file.is_some() || substituted || capped_this_chunk || self.will_overflow(capped_bytes))
        {
            self.write_to_file(chunk);
        }
        if capped_bytes == 0 {
            return;
        }

        let mut tail_chunk = capped.clone();
        let mut tail_bytes = capped_bytes;
        if self.head_limit > 0 && !self.head_retention_disabled && self.head_bytes < self.head_limit {
            let room = self.head_limit - self.head_bytes;
            if capped_bytes <= room {
                self.head.push_str(&capped);
                self.head_bytes += capped_bytes;
                self.head_lines += count_newlines(&capped);
                return;
            }
            let head_slice = truncate_head_bytes(ByteInput::Text(&capped), room);
            if head_slice.bytes > 0 {
                self.head.push_str(&head_slice.text);
                self.head_bytes += head_slice.bytes;
                self.head_lines += count_newlines(&head_slice.text);
                let consumed = head_slice.text.len();
                tail_chunk = capped[consumed..].to_string();
                tail_bytes = capped_bytes - head_slice.bytes;
            }
        }
        self.push_tail(&tail_chunk, tail_bytes);
    }

    fn apply_column_cap(&mut self, chunk: &str, first_line: usize) -> String {
        if chunk.is_empty() {
            return chunk.to_string();
        }
        if !self.column_ellipsis_added && self.fits_column_cap(chunk) {
            return chunk.to_string();
        }
        let max = self.max_columns;
        let mut parts = String::new();
        let mut cursor = 0usize;
        let mut line = first_line;
        while cursor < chunk.len() {
            let nl_index = chunk[cursor..].find(NL).map(|offset| cursor + offset);
            let seg_end = nl_index.unwrap_or(chunk.len());
            if seg_end > cursor {
                let segment = &chunk[cursor..seg_end];
                if self.column_ellipsis_added {
                    self.column_dropped_bytes += segment.len();
                } else {
                    let seg_bytes = segment.len();
                    let remaining = max.saturating_sub(self.current_line_bytes);
                    if seg_bytes <= remaining {
                        parts.push_str(segment);
                        self.current_line_bytes += seg_bytes;
                    } else {
                        let ellipsis_bytes = ELLIPSIS.len();
                        let head_room = remaining.saturating_sub(ellipsis_bytes);
                        let mut kept = String::new();
                        let mut kept_bytes = 0usize;
                        if head_room > 0 {
                            let sliced = truncate_head_bytes(ByteInput::Text(segment), head_room);
                            kept = sliced.text;
                            kept_bytes = sliced.bytes;
                            parts.push_str(&kept);
                        }
                        parts.push_str(ELLIPSIS);
                        self.column_dropped_bytes += seg_bytes - kept_bytes;
                        self.column_truncated_lines += 1;
                        if line > 0 {
                            if self.column_first_cut_line == 0 {
                                self.column_first_cut_line = line;
                            }
                            self.column_last_cut_line = line;
                        } else {
                            self.column_cut_lines_unmapped = true;
                        }
                        self.current_line_bytes += kept_bytes + ellipsis_bytes;
                        self.column_ellipsis_added = true;
                    }
                }
            }
            let Some(nl_index) = nl_index else {
                break;
            };
            parts.push(NL);
            self.current_line_bytes = 0;
            self.column_ellipsis_added = false;
            cursor = nl_index + 1;
            if line > 0 {
                line += 1;
            }
        }
        parts
    }

    fn fits_column_cap(&mut self, chunk: &str) -> bool {
        let max = self.max_columns;
        let mut room = max.saturating_sub(self.current_line_bytes);
        let mut cursor = 0usize;
        loop {
            let nl_index = chunk[cursor..].find(NL).map(|offset| cursor + offset);
            let seg_end = nl_index.unwrap_or(chunk.len());
            if (seg_end - cursor) * 3 > room {
                return false;
            }
            let Some(nl_index) = nl_index else {
                break;
            };
            cursor = nl_index + 1;
            room = max;
        }
        if cursor == 0 {
            self.current_line_bytes += chunk.len();
        } else {
            self.current_line_bytes = if cursor == chunk.len() {
                0
            } else {
                chunk[cursor..].len()
            };
        }
        true
    }

    fn will_overflow(&self, data_bytes: usize) -> bool {
        self.buffer_bytes + data_bytes > self.spill_threshold.saturating_sub(self.head_bytes)
    }

    fn push_tail(&mut self, chunk: &str, data_bytes: usize) {
        if data_bytes == 0 {
            return;
        }
        let threshold = self.spill_threshold.saturating_sub(self.head_bytes);
        let will_overflow = self.buffer_bytes + data_bytes > threshold;
        if !will_overflow {
            self.buffer.push_str(chunk);
            self.buffer_bytes += data_bytes;
            return;
        }
        self.truncated = true;
        if data_bytes >= threshold {
            let truncated = truncate_tail_bytes(ByteInput::Text(chunk), threshold);
            self.buffer = truncated.text;
            self.buffer_bytes = truncated.bytes;
            self.tail_untrimmed = false;
            return;
        }
        self.buffer.push_str(chunk);
        self.buffer_bytes += data_bytes;
        self.tail_untrimmed = true;
        if self.buffer_bytes > threshold * 2 {
            self.trim_tail();
        }
    }

    fn trim_tail(&mut self) {
        if !self.tail_untrimmed {
            return;
        }
        self.tail_untrimmed = false;
        let threshold = self.spill_threshold.saturating_sub(self.head_bytes);
        if self.buffer_bytes <= threshold {
            return;
        }
        let truncated = truncate_tail_bytes(ByteInput::Text(&self.buffer), threshold);
        self.buffer = truncated.text;
        self.buffer_bytes = truncated.bytes;
    }

    fn normalize_carriage_returns(&mut self, text: &str) -> String {
        if text.is_empty() || (!self.pending_carriage_return && !text.contains(CR)) {
            return text.to_string();
        }
        let mut cursor = 0usize;
        let mut normalized = String::new();
        if self.pending_carriage_return {
            self.pending_carriage_return = false;
            normalized.push(NL);
            if text.starts_with(NL) {
                cursor = 1;
            }
        }
        while cursor < text.len() {
            let Some(offset) = text[cursor..].find(CR) else {
                normalized.push_str(&text[cursor..]);
                break;
            };
            let carriage_return = cursor + offset;
            normalized.push_str(&text[cursor..carriage_return]);
            if carriage_return == text.len() - 1 {
                self.pending_carriage_return = true;
                break;
            }
            normalized.push(NL);
            cursor = if text[carriage_return + 1..].starts_with(NL) {
                carriage_return + 2
            } else {
                carriage_return + 1
            };
        }
        normalized
    }

    fn write_to_file(&mut self, chunk: &str) {
        if self.artifact_error.is_some() {
            return;
        }
        if self.file_ready && self.file.is_some() {
            self.emit_to_sink(chunk);
            return;
        }
        if self.pending_file_writes.is_none() {
            self.pending_file_writes = Some(vec![chunk.to_string()]);
            self.create_file_sink();
        } else if let Some(pending) = self.pending_file_writes.as_mut() {
            pending.push(chunk.to_string());
        }
    }

    fn emit_to_sink(&mut self, chunk: &str) {
        if self.file.is_none() || self.artifact_error.is_some() || chunk.is_empty() {
            return;
        }
        if self.artifact_max_bytes == 0 {
            self.write_artifact(chunk);
            return;
        }
        let chunk_bytes = chunk.len();
        let room = if self.artifact_head_closed {
            0
        } else {
            self.artifact_head_budget - self.artifact_head_bytes_written
        };
        if room >= chunk_bytes {
            self.write_artifact(chunk);
            self.artifact_head_bytes_written += chunk_bytes;
            return;
        }
        let mut overflow = chunk.to_string();
        if room > 0 {
            let head_slice = truncate_head_bytes(ByteInput::Text(chunk), room);
            if head_slice.bytes > 0 {
                self.write_artifact(&head_slice.text);
                self.artifact_head_bytes_written += head_slice.bytes;
            }
            self.artifact_head_closed = true;
            overflow = chunk[head_slice.text.len()..].to_string();
        }
        if overflow.is_empty() || self.artifact_tail_budget == 0 {
            if !overflow.is_empty() {
                self.artifact_tail_incoming_bytes += overflow.len();
            }
            return;
        }
        self.push_artifact_tail(&overflow);
    }

    fn push_artifact_tail(&mut self, chunk: &str) {
        if self.artifact_error.is_some() {
            return;
        }
        let chunk_bytes = chunk.len();
        self.artifact_tail_incoming_bytes += chunk_bytes;
        let budget = self.artifact_tail_budget;
        if chunk_bytes >= budget {
            let truncated = truncate_tail_bytes(ByteInput::Text(chunk), budget);
            self.artifact_tail_ring = truncated.text;
            self.artifact_tail_ring_bytes = truncated.bytes;
            return;
        }
        self.artifact_tail_ring.push_str(chunk);
        self.artifact_tail_ring_bytes += chunk_bytes;
        if self.artifact_tail_ring_bytes > budget + self.artifact_tail_slack {
            self.trim_artifact_tail();
        }
    }

    fn trim_artifact_tail(&mut self) {
        if self.artifact_tail_ring_bytes <= self.artifact_tail_budget {
            return;
        }
        let truncated = truncate_tail_bytes(ByteInput::Text(&self.artifact_tail_ring), self.artifact_tail_budget);
        self.artifact_tail_ring = truncated.text;
        self.artifact_tail_ring_bytes = truncated.bytes;
    }

    fn record_artifact_error(&mut self, operation: OutputArtifactError) {
        if self.artifact_error.is_none() {
            self.artifact_error = Some(operation);
        }
        self.pending_file_writes = None;
        self.artifact_tail_ring.clear();
        self.artifact_tail_ring_bytes = 0;
    }

    fn write_artifact(&mut self, chunk: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if self.artifact_error.is_some() {
            return;
        }
        let Some(handle) = file.file.as_mut() else {
            return;
        };
        use std::io::Write;
        if handle.write_all(chunk.as_bytes()).is_err() {
            self.record_artifact_error(OutputArtifactError::Write);
        }
    }

    fn create_file_sink(&mut self) {
        let Some(path) = self.artifact_path.clone() else {
            return;
        };
        if self.file_ready || self.artifact_error.is_some() {
            return;
        }
        match File::create(&path) {
            Ok(handle) => {
                self.file = Some(FileState {
                    path,
                    artifact_id: self.artifact_id.clone(),
                    file: Some(handle),
                });
                self.file_ready = true;
                if !self.head.is_empty() {
                    let head = std::mem::take(&mut self.head);
                    self.emit_to_sink(&head);
                    self.head = head;
                }
                if !self.buffer.is_empty() {
                    let buffer = std::mem::take(&mut self.buffer);
                    self.emit_to_sink(&buffer);
                    self.buffer = buffer;
                }
                if let Some(pending) = self.pending_file_writes.take() {
                    for chunk in pending {
                        self.emit_to_sink(&chunk);
                    }
                }
            }
            Err(_) => self.record_artifact_error(OutputArtifactError::Open),
        }
    }

    /// Replace the in-memory buffer with `text`.
    pub fn replace(&mut self, text: &str) {
        self.current_line_bytes = 0;
        self.column_ellipsis_added = false;
        self.column_dropped_bytes = 0;
        self.column_truncated_lines = 0;
        self.column_cut_lines_unmapped = true;
        let capped = if self.max_columns > 0 {
            self.apply_column_cap(text, 0)
        } else {
            text.to_string()
        };
        self.buffer = capped.clone();
        self.buffer_bytes = capped.len();
        self.tail_untrimmed = false;
        self.head.clear();
        self.head_bytes = 0;
        self.head_lines = 0;
        self.head_retention_disabled = true;
        self.total_bytes = text.len();
        self.total_lines = count_newlines(text);
        self.saw_data = !text.is_empty();
        self.truncated = false;
        self.pending_chunk.clear();
        self.pending_carriage_return = false;
    }

    /// Current inline view without finalizing.
    pub fn preview(&mut self) -> String {
        self.compose_body().body
    }

    fn compose_body(&mut self) -> ComposedBody {
        self.trim_tail();
        let total_lines = if self.saw_data { self.total_lines + 1 } else { 0 };
        let head_bytes = self.head_bytes;
        let tail_buf = self.buffer.clone();
        let tail_bytes = self.buffer_bytes;
        let head_lines = self.head_lines + usize::from(head_bytes > 0 && !self.head.ends_with(NL));
        let tail_lines = if !tail_buf.is_empty() {
            count_newlines(&tail_buf) + 1
        } else {
            0
        };
        let effective_total_bytes = self.total_bytes.saturating_sub(self.column_dropped_bytes);

        if head_bytes > 0 && effective_total_bytes > head_bytes + tail_bytes {
            let head_cut = if self.head.ends_with(NL) {
                None
            } else {
                self.head.rfind(NL)
            };
            let (head, kept_head_bytes, kept_head_lines) = match head_cut {
                Some(cut) => {
                    let part = &self.head[..=cut];
                    (part, part.len(), count_newlines(part))
                }
                None => (self.head.as_str(), head_bytes, head_lines),
            };
            let tail_cut = tail_buf.find(NL);
            let (tail, kept_tail_bytes, kept_tail_lines) = match tail_cut {
                Some(cut) if cut < tail_buf.len() - 1 => {
                    let part = &tail_buf[cut + 1..];
                    (part, part.len(), count_newlines(part) + 1)
                }
                _ => (tail_buf.as_str(), tail_bytes, tail_lines),
            };
            let kept_tail_bytes = if std::ptr::eq(tail, &tail_buf[..]) {
                tail_bytes
            } else {
                tail.len()
            };
            let kept_head_lines = if head.len() == self.head.len() {
                head_lines
            } else {
                count_newlines(head)
            };
            let kept_tail_lines = if tail.len() == tail_buf.len() {
                tail_lines
            } else {
                count_newlines(tail) + 1
            };
            let elided_bytes = effective_total_bytes.saturating_sub(kept_head_bytes + kept_tail_bytes);
            let elided_lines = total_lines.saturating_sub(kept_head_lines + kept_tail_lines);
            let marker = format_middle_elision_marker(elided_lines, elided_bytes);
            let marker_bytes = marker.len();
            let head_sep = if head.ends_with(NL) { "" } else { "\n" };
            let tail_sep = if tail.starts_with(NL) { "" } else { "\n" };
            let body = format!("{head}{head_sep}{marker}{tail_sep}{tail}");
            let output_bytes = kept_head_bytes + marker_bytes + kept_tail_bytes + head_sep.len() + tail_sep.len();
            return ComposedBody {
                body,
                output_bytes,
                output_lines: kept_head_lines + 1 + kept_tail_lines,
                elided_bytes: Some(elided_bytes),
                elided_lines: Some(elided_lines),
            };
        }
        if head_bytes > 0 {
            let body = format!("{}{}", self.head, tail_buf);
            let output_lines = if body.is_empty() { 0 } else { count_newlines(&body) + 1 };
            return ComposedBody {
                body,
                output_bytes: head_bytes + tail_bytes,
                output_lines,
                elided_bytes: None,
                elided_lines: None,
            };
        }
        ComposedBody {
            body: tail_buf,
            output_bytes: tail_bytes,
            output_lines: tail_lines,
            elided_bytes: None,
            elided_lines: None,
        }
    }

    /// Compose the visible output and finalize the artifact.
    pub fn dump(&mut self, notice: Option<&str>) -> OutputSummary {
        if self.pending_carriage_return {
            self.pending_carriage_return = false;
            self.push("\n", None);
        }
        let notice_line = notice.map(|notice| format!("[{notice}]\n")).unwrap_or_default();
        self.flush_pending_chunk();
        let total_lines = if self.saw_data { self.total_lines + 1 } else { 0 };
        self.finalize_file();
        let composed = self.compose_body();
        let mut truncated = self.truncated;
        if composed.elided_bytes.is_some() {
            truncated = true;
        }
        let artifact_id = if self.artifact_error.is_some() {
            None
        } else {
            self.file.as_ref().and_then(|file| file.artifact_id.clone())
        };
        OutputSummary {
            output: format!("{notice_line}{}", composed.body),
            truncated,
            total_lines,
            total_bytes: self.total_bytes,
            output_lines: composed.output_lines,
            output_bytes: composed.output_bytes,
            elided_bytes: composed.elided_bytes,
            elided_lines: composed.elided_lines,
            column_dropped_bytes: if self.column_dropped_bytes > 0 {
                Some(self.column_dropped_bytes)
            } else {
                None
            },
            column_truncated_lines: if self.column_truncated_lines > 0 {
                Some(self.column_truncated_lines)
            } else {
                None
            },
            column_max: if self.column_truncated_lines > 0 {
                Some(self.max_columns)
            } else {
                None
            },
            column_truncated_range: if self.column_first_cut_line > 0 && !self.column_cut_lines_unmapped {
                Some(ColumnRange {
                    first: self.column_first_cut_line,
                    last: self.column_last_cut_line,
                })
            } else {
                None
            },
            artifact_id,
            artifact_elided_bytes: if self.artifact_error.is_some()
                || self.artifact_elided_bytes == 0
                || self
                    .file
                    .as_ref()
                    .and_then(|file| file.artifact_id.as_ref())
                    .is_none()
            {
                None
            } else {
                Some(self.artifact_elided_bytes)
            },
            artifact_error: self.artifact_error,
        }
    }

    fn flush_pending_chunk(&mut self) {
        if self.pending_chunk.is_empty() {
            return;
        }
        let merged = std::mem::take(&mut self.pending_chunk);
        if let Some(callback) = self.on_chunk.as_mut() {
            self.last_chunk_time = Some(Instant::now());
            callback(&merged);
        }
    }

    fn finalize_file(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        self.flush_artifact_tail_if_capped();
        if let Some(file) = self.file.as_mut() {
            if let Some(mut handle) = file.file.take() {
                use std::io::Write;
                if handle.flush().is_err() && self.artifact_error.is_none() {
                    self.artifact_error = Some(OutputArtifactError::Flush);
                }
                drop(handle);
            }
        }
    }

    fn flush_artifact_tail_if_capped(&mut self) {
        if self.file.is_none() || self.artifact_error.is_some() || self.artifact_max_bytes == 0 {
            return;
        }
        self.trim_artifact_tail();
        let tail_bytes = self.artifact_tail_ring_bytes;
        let dropped_bytes = self.artifact_tail_incoming_bytes.saturating_sub(tail_bytes);
        if tail_bytes == 0 && dropped_bytes == 0 {
            return;
        }
        if dropped_bytes > 0 {
            self.artifact_elided_bytes = dropped_bytes;
            let head_written = self.artifact_head_bytes_written;
            let total_capped = head_written + self.artifact_tail_incoming_bytes;
            let head_sep = if head_written > 0 { "\n" } else { "" };
            let tail_sep = if tail_bytes > 0 && !self.artifact_tail_ring.starts_with(NL) {
                "\n"
            } else {
                ""
            };
            let notice = format!(
                "{head_sep}[ARTIFACT TRUNCATED: kept first {} + last {} of {}; {} elided from the middle]{tail_sep}",
                format_bytes(head_written),
                format_bytes(tail_bytes),
                format_bytes(total_capped),
                format_bytes(dropped_bytes)
            );
            self.write_artifact(&notice);
        }
        if tail_bytes > 0 {
            let tail = self.artifact_tail_ring.clone();
            self.write_artifact(&tail);
        }
    }

    /// Release the artifact descriptor.
    pub fn dispose(&mut self) {
        self.finalize_file();
    }
}

struct ComposedBody {
    body: String,
    output_bytes: usize,
    output_lines: usize,
    elided_bytes: Option<usize>,
    elided_lines: Option<usize>,
}

/// Format a byte count to a human-readable string (`512B`, `1.5KB`, …).
pub fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1}KB", bytes as f64 / 1024.0);
    }
    if bytes < 1024 * 1024 * 1024 {
        return format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0));
    }
    format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

// =============================================================================
// Truncation notice formatting
// =============================================================================

/// Format a truncation notice for tail-truncated output (bash, python, ssh).
/// Returns empty string if not truncated.
pub fn format_tail_truncation_notice(
    truncation: &TruncationResult,
    options: &TailTruncationNoticeOptions,
) -> String {
    if truncation.truncated != Some(true) {
        return String::new();
    }
    let full_output_part = options
        .full_output_path
        .as_ref()
        .map(|path| format!(". Full output: {path}"))
        .unwrap_or_default();
    let suffix = options.suffix.clone().unwrap_or_default();
    let start_line = truncation
        .total_lines
        .saturating_sub(truncation.output_lines.unwrap_or(truncation.total_lines))
        + 1;
    let end_line = truncation.total_lines;
    let notice = if truncation.last_line_partial == Some(true) {
        let mut last_line_size_part = String::new();
        if let Some(original) = &options.original_content {
            let last_nl = original.rfind(NL);
            let last_line = match last_nl {
                Some(index) => &original[index + 1..],
                None => original.as_str(),
            };
            last_line_size_part = format!(" (line is {})", format_bytes(last_line.len()));
        }
        format!(
            "[Showing last {} of line {end_line}{last_line_size_part}{full_output_part}{suffix}]",
            format_bytes(truncation.output_bytes.unwrap_or(truncation.total_bytes))
        )
    } else if truncation.partial_byte_windows == Some(true) {
        format!(
            "[Showing last {} across lines {start_line}-{end_line} of {}; line {start_line} is partial{full_output_part}{suffix}]",
            format_bytes(truncation.output_bytes.unwrap_or(truncation.total_bytes)),
            truncation.total_lines
        )
    } else {
        format!("[Showing lines {start_line}-{end_line} of {end_line}{full_output_part}{suffix}]")
    };
    format!("\n\n{notice}")
}

/// Format a truncation notice for head-truncated output (read tool).
/// Returns empty string if not truncated.
pub fn format_head_truncation_notice(
    truncation: &TruncationResult,
    options: &HeadTruncationNoticeOptions,
) -> String {
    if truncation.truncated != Some(true) {
        return String::new();
    }
    let start_line_display = options.start_line.unwrap_or(1);
    let total_file_lines = options.total_file_lines.unwrap_or(truncation.total_lines);
    let end_line_display = start_line_display + truncation.output_lines.unwrap_or(truncation.total_lines) - 1;
    let next_offset = end_line_display + 1;
    let notice =
        format!("[Showing lines {start_line_display}-{end_line_display} of {total_file_lines}. Use :{next_offset} to continue]");
    format!("\n\n{notice}")
}

/// Build an `on_chunk` handler that appends to a [`TailBuffer`] and emits a
/// streaming update.
pub fn stream_tail_updates<'a, TDetails, F>(
    tail_buffer: &'a mut TailBuffer,
    mut on_update: Option<F>,
) -> impl FnMut(&str) + 'a
where
    TDetails: Default,
    F: FnMut(ToolUpdate<TDetails>) + 'a,
{
    move |chunk: &str| {
        tail_buffer.append(chunk);
        if let Some(callback) = on_update.as_mut() {
            callback(ToolUpdate {
                content: vec![ContentBlock::text(tail_buffer.text())],
                details: TDetails::default(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_matches_reference_examples() {
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1536), "1.5KB");
        assert_eq!(format_bytes(2 * 1024 * 1024 + 3 * 1024 * 1024 / 10), "2.3MB");
    }

    #[test]
    fn truncate_head_keeps_whole_lines() {
        let result = truncate_head(
            "one\ntwo\nthree\n",
            TruncationOptions {
                max_lines: Some(2),
                ..TruncationOptions::default()
            },
        );
        assert_eq!(result.content, "one\ntwo");
        assert_eq!(result.output_lines, Some(2));
        assert_eq!(result.truncated_by, Some(TruncateBy::Lines));
    }

    #[test]
    fn truncate_tail_keeps_last_lines() {
        let result = truncate_tail(
            "one\ntwo\nthree\n",
            TruncationOptions {
                max_lines: Some(2),
                ..TruncationOptions::default()
            },
        );
        assert_eq!(result.content, "two\nthree");
        assert_eq!(result.output_lines, Some(2));
    }

    #[test]
    fn truncate_middle_marks_elided_lines() {
        let content = "a\nb\nc\nd\ne\n";
        let result = truncate_middle(
            content,
            TruncationOptions {
                max_lines: Some(2),
                max_bytes: Some(10_000),
                ..TruncationOptions::default()
            },
        );
        assert!(result.truncated_by == Some(TruncateBy::Middle));
        assert!(result.content.contains("elided"));
        assert!(result.elided_lines.unwrap() >= 1);
    }

    #[test]
    fn truncate_head_bytes_respects_utf8_boundaries() {
        let text = "éééé"; // 8 bytes, 4 chars
        let head = truncate_head_bytes(ByteInput::Text(text), 5);
        assert_eq!(head.text, "éé"); // 4 bytes
        assert_eq!(head.bytes, 4);
        let tail = truncate_tail_bytes(ByteInput::Text(text), 5);
        assert_eq!(tail.text, "éé");
    }

    #[test]
    fn truncate_line_appends_ellipsis() {
        let cut = truncate_line("abcdef", 3);
        assert_eq!(cut.text, "abc…");
        assert!(cut.was_truncated);
        assert_eq!(truncate_line("abc", 3).was_truncated, false);
    }

    #[test]
    fn truncate_line_bytes_reserves_marker_room() {
        let cut = truncate_line_bytes("abcdef", 4);
        assert_eq!(cut.text, "a…");
        assert!(cut.was_truncated);
    }

    #[test]
    fn tail_buffer_rolls_to_budget() {
        let mut buffer = TailBuffer::new(6);
        buffer.append("hello ");
        buffer.append("world!");
        assert_eq!(buffer.text(), "world!");
        assert_eq!(buffer.bytes(), 6);
    }

    #[test]
    fn tail_buffer_empty_after_zero_budget() {
        let mut buffer = TailBuffer::new(0);
        buffer.append("abc");
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn column_cap_marks_and_drops() {
        let mut sink = OutputSink::new(OutputSinkOptions {
            max_columns: Some(5),
            ..OutputSinkOptions::default()
        });
        sink.push("abcdefghij\nok\n", None);
        let summary = sink.dump(None);
        assert_eq!(summary.output, "ab…\nok");
        assert_eq!(summary.column_truncated_lines, Some(1));
        assert!(summary.column_dropped_bytes.unwrap() >= 4);
    }

    #[test]
    fn head_retention_composes_middle_marker() {
        let mut sink = OutputSink::new(OutputSinkOptions {
            spill_threshold: Some(12),
            head_bytes: Some(6),
            ..OutputSinkOptions::default()
        });
        sink.push("header\nsecond\nthird\n", None);
        let summary = sink.dump(None);
        assert!(summary.truncated);
        assert!(summary.elided_bytes.is_some());
        assert!(summary.output.contains("elided"));
    }

    #[test]
    fn enforce_inline_byte_cap_keeps_head_and_tail() {
        let text = "head\n".to_string() + &"x".repeat(200) + "\ntail";
        let capped = enforce_inline_byte_cap(
            &text,
            &InlineByteCapOptions {
                max_bytes: Some(64),
                ..InlineByteCapOptions::default()
            },
        );
        assert!(capped.len() <= 90);
        assert!(capped.contains("head"));
        assert!(capped.contains("tail"));
        assert!(capped.contains("elided"));
    }

    #[test]
    fn sink_replace_realigns_counters() {
        let mut sink = OutputSink::new(OutputSinkOptions::default());
        sink.push("old\n", None);
        sink.replace("new\ncontent\n");
        let summary = sink.dump(None);
        assert_eq!(summary.total_lines, 2);
        assert_eq!(summary.output, "new\ncontent");
    }

    #[test]
    fn stream_tail_updates_emits_buffer_text() {
        let mut buffer = TailBuffer::new(100);
        let mut seen: Vec<String> = Vec::new();
        {
            let mut handler = stream_tail_updates::<serde_json::Value, _>(&mut buffer, Some(|update| {
                seen.push(crate::tools::native_view::result_text(
                    &crate::tools::native_view::ToolRenderResult {
                        content: update.content,
                        details: None,
                        is_error: None,
                    },
                ));
            }));
            handler("one");
            handler("two");
        }
        assert_eq!(seen, vec!["one".to_string(), "onetwo".to_string()]);
    }

    #[test]
    fn find_column_capped_locates_uncapped_text() {
        let output = "prefix\nhello world\nsuffix";
        let found = find_column_capped(output, "hello world", 0).unwrap();
        assert_eq!(&output[found.start..found.end], "hello world");
    }

    #[test]
    fn tail_notice_reports_partial_line() {
        let truncation = TruncationResult {
            truncated: Some(true),
            total_lines: 1,
            total_bytes: 100,
            output_lines: Some(1),
            output_bytes: Some(10),
            last_line_partial: Some(true),
            ..TruncationResult::default()
        };
        let notice = format_tail_truncation_notice(
            &truncation,
            &TailTruncationNoticeOptions {
                original_content: Some("x".repeat(100)),
                ..TailTruncationNoticeOptions::default()
            },
        );
        assert!(notice.contains("line 1"));
        assert!(notice.contains("line is 100B"));
    }

    #[test]
    fn head_notice_names_next_offset() {
        let truncation = TruncationResult {
            truncated: Some(true),
            total_lines: 100,
            output_lines: Some(10),
            ..TruncationResult::default()
        };
        let notice = format_head_truncation_notice(&truncation, &HeadTruncationNoticeOptions::default());
        assert_eq!(
            notice,
            "\n\n[Showing lines 1-10 of 100. Use :11 to continue]"
        );
    }
}
