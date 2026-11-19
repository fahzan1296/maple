//! Output truncation/diagnostics metadata and notice formatting helpers.
//!
//! Reference: `l/src/tools/output-meta.ts`.
//!
//! Deviations:
//! - Formatting helpers that depend on `grouped-file-output`, render theme,
//!   and rich UI utils are not ported; pure metadata types and text/notice
//!   helpers are provided.
//! - `format_bytes` is local (no render-utils dependency).

use std::sync::LazyLock;

use regex::Regex;

/// Truncation metadata for the output notice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TruncationMeta {
	/// Truncation direction.
	pub direction: Option<String>,
	/// What caused truncation.
	pub truncated_by: Option<String>,
	/// Total source lines.
	pub total_lines: usize,
	/// Total source bytes.
	pub total_bytes: usize,
	/// Lines kept in output.
	pub output_lines: usize,
	/// Bytes kept in output.
	pub output_bytes: usize,
	/// Max bytes cap.
	pub max_bytes: Option<usize>,
	/// Line range shown (1-indexed, inclusive).
	pub shown_range: Option<LineRangeShown>,
	/// Head range shown for middle truncation.
	pub head_range: Option<LineRangeShown>,
	/// Tail range shown for middle truncation.
	pub tail_range: Option<LineRangeShown>,
	/// Bytes elided from the middle.
	pub elided_bytes: Option<usize>,
	/// Lines elided from the middle.
	pub elided_lines: Option<usize>,
	/// Artifact ID if full output was saved.
	pub artifact_id: Option<String>,
	/// Bytes the artifact cap dropped from the saved file's middle.
	pub artifact_elided_bytes: Option<usize>,
	/// Next offset for pagination (head truncation only).
	pub next_offset: Option<usize>,
	/// Single shown line is a byte-capped preview of one oversized line.
	pub partial_line: bool,
}

/// Inclusive 1-indexed line range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineRangeShown {
	/// Start line.
	pub start: usize,
	/// End line.
	pub end: usize,
}

/// Source resolution info for the output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceMeta {
	/// Filesystem path.
	Path(String),
	/// URL.
	Url(String),
	/// Internal URL.
	Internal(String),
	/// Aggregate report.
	Report(String),
}

/// LSP diagnostic info (for edit/write tools).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticMeta {
	/// Summary string.
	pub summary: String,
	/// Messages.
	pub messages: Vec<String>,
}

/// Match/result/head limit notices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LimitsMeta {
	/// Match limit reached.
	pub match_limit: Option<LimitReached>,
	/// Result limit reached.
	pub result_limit: Option<LimitReachedOptionalSuggestion>,
	/// Head limit reached.
	pub head_limit: Option<LimitReached>,
	/// Column truncation details.
	pub column_truncated: Option<ColumnTruncated>,
}

/// Reached limit with suggestion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LimitReached {
	/// Reached value.
	pub reached: usize,
	/// Suggested larger value.
	pub suggestion: usize,
}

/// Reached limit with optional suggestion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LimitReachedOptionalSuggestion {
	/// Reached value.
	pub reached: usize,
	/// Suggested larger value.
	pub suggestion: Option<usize>,
}

/// Column truncation metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnTruncated {
	/// Max column cap.
	pub max_column: usize,
	/// Unit.
	pub unit: Option<String>,
	/// Artifact id.
	pub artifact_id: Option<String>,
	/// Artifact elided bytes.
	pub artifact_elided_bytes: Option<usize>,
	/// Affected line span.
	pub lines: Option<ColumnLines>,
	/// Selector base.
	pub selector_base: Option<String>,
	/// Raw read byte cap for oversized first cut line.
	pub raw_byte_cap: Option<usize>,
}

/// First/last cut lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnLines {
	/// First line.
	pub first: usize,
	/// Last line.
	pub last: usize,
}

/// Bash minimizer rewrite metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MinimizedMeta {
	/// Native filter label.
	pub filter: String,
	/// Raw input bytes.
	pub input_bytes: usize,
	/// Minimized output bytes.
	pub output_bytes: usize,
	/// Artifact holding raw capture.
	pub artifact_id: String,
}

/// Structured metadata for tool outputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputMeta {
	/// Truncation metadata.
	pub truncation: Option<TruncationMeta>,
	/// Artifact capture error.
	pub artifact_error: Option<String>,
	/// Source metadata.
	pub source: Option<SourceMeta>,
	/// Output is a bounded paged source view.
	pub paged_source: bool,
	/// Diagnostics metadata.
	pub diagnostics: Option<DiagnosticMeta>,
	/// Limits metadata.
	pub limits: Option<LimitsMeta>,
	/// Minimizer metadata.
	pub minimized: Option<MinimizedMeta>,
}

const RAW_OUTPUT_ARTIFACT_PREFIX: &str = "[raw output: artifact://";
const RAW_OUTPUT_ARTIFACT_SUFFIX: &str = "]";

static COLUMN_TRUNCATION_NOTICE_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^\[Some lines truncated to -?\d+(?:\.\d+)?(?:e[+-]?\d+)? (?:chars|bytes)(?:\. .*)?\]$")
		.expect("valid regex")
});
static COLUMN_TRUNCATION_IN_NOTICE_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^\[.*\. Some lines truncated to -?\d+(?:\.\d+)?(?:e[+-]?\d+)? (?:chars|bytes)(?:\. .*)?\]$")
		.expect("valid regex")
});

/// Format a recoverable output artifact link.
pub fn format_full_output_reference(artifact_id: &str, artifact_elided_bytes: Option<usize>) -> String {
	if let Some(elided) = artifact_elided_bytes {
		if elided > 0 {
			return format!(
				"Read artifact://{artifact_id} for a head/tail sample of the output; {} from its middle was not saved",
				format_bytes(elided)
			);
		}
	}
	format!("Read artifact://{artifact_id} for full output")
}

/// Format truncation ranges and recovery hints.
pub fn format_truncation_meta_notice(
	truncation: &TruncationMeta,
	source: Option<&SourceMeta>,
) -> String {
	let artifact_reference = truncation.artifact_id.as_ref().map(|artifact_id| {
		if let Some(SourceMeta::Report(value)) = source {
			format!("Read artifact://{artifact_id} for full report ({value})")
		} else {
			format_full_output_reference(artifact_id, truncation.artifact_elided_bytes)
		}
	});

	if truncation.direction.as_deref() == Some("middle") {
		let total_lines = truncation.total_lines;
		let elided_bytes = truncation
			.elided_bytes
			.unwrap_or_else(|| truncation.total_bytes.saturating_sub(truncation.output_bytes));
		let elided_lines = truncation
			.elided_lines
			.unwrap_or_else(|| total_lines.saturating_sub(truncation.output_lines));
		let head_part = truncation
			.head_range
			.as_ref()
			.map(|range| format!("lines {}-{}", range.start, range.end))
			.unwrap_or_default();
		let tail_part = truncation
			.tail_range
			.as_ref()
			.map(|range| format!("{}-{}", range.start, range.end))
			.unwrap_or_default();
		let mut notice = if !head_part.is_empty() && !tail_part.is_empty() {
			format!(
				"Showing {head_part} and {tail_part} of {total_lines}; {elided_lines} middle line{} ({}) elided",
				if elided_lines == 1 { "" } else { "s" },
				format_bytes(elided_bytes)
			)
		} else if elided_bytes > 0 {
			format!(
				"Showing head and tail bytes of {total_lines} line{}; {} elided",
				if total_lines == 1 { "" } else { "s" },
				format_bytes(elided_bytes)
			)
		} else {
			format!(
				"Showing {} of {} lines; middle elided",
				truncation.output_lines.min(total_lines),
				total_lines
			)
		};
		if let Some(next_offset) = truncation.next_offset {
			notice.push_str(&format!(". Use :{next_offset} to continue"));
		}
		if let Some(reference) = artifact_reference {
			notice.push_str(&format!(". {reference}"));
		}
		return notice;
	}

	if truncation.partial_line {
		let line = truncation
			.shown_range
			.as_ref()
			.map(|range| range.start)
			.unwrap_or(1);
		let mut notice = format!(
			"Showing line {line} (partial, {} of {}) of {}",
			format_bytes(truncation.output_bytes),
			format_bytes(truncation.total_bytes),
			truncation.total_lines
		);
		if let Some(reference) = artifact_reference {
			notice.push_str(&format!(". {reference}"));
		}
		return notice;
	}

	let mut notice = match truncation.shown_range.as_ref() {
		Some(range) if range.end >= range.start => {
			format!("Showing lines {}-{} of {}", range.start, range.end, truncation.total_lines)
		}
		_ => format!(
			"Showing {} of {} lines",
			truncation.output_lines, truncation.total_lines
		),
	};
	if truncation.truncated_by.as_deref() == Some("bytes") {
		let max_bytes = truncation.max_bytes.unwrap_or(truncation.output_bytes);
		notice.push_str(&format!(" ({} limit)", format_bytes(max_bytes)));
	}
	if let Some(next_offset) = truncation.next_offset {
		notice.push_str(&format!(". Use :{next_offset} to continue"));
	}
	if let Some(reference) = artifact_reference {
		notice.push_str(&format!(". {reference}"));
	}
	notice
}

/// Describe an incomplete artifact capture.
pub fn format_artifact_error_notice(error: &str) -> String {
	format!("Full output was not saved completely (artifact {error} failed)")
}

/// Remove the trailing bash raw-output artifact footer while preserving its id.
pub fn strip_raw_output_artifact_notice(text: &str) -> (String, Option<String>) {
	let trimmed = text.trim_end_matches('\n');
	let start = trimmed.rfind('\n').map(|i| i + 1).unwrap_or(0);
	let line = &trimmed[start..];
	if !line.starts_with(RAW_OUTPUT_ARTIFACT_PREFIX) || !line.ends_with(RAW_OUTPUT_ARTIFACT_SUFFIX) {
		return (text.to_string(), None);
	}
	let id_start = RAW_OUTPUT_ARTIFACT_PREFIX.len();
	let id_end = line.len() - RAW_OUTPUT_ARTIFACT_SUFFIX.len();
	if id_start == id_end || !line[id_start..id_end].chars().all(|c| c.is_ascii_digit()) {
		return (text.to_string(), None);
	}
	let id = line[id_start..id_end].to_string();
	let mut out = trimmed[..start].to_string();
	while out.ends_with('\n') {
		out.pop();
	}
	(out, Some(id))
}

/// Whether `line` is a column-cap notice emitted by the formatter.
pub fn is_column_truncation_notice(line: &str) -> bool {
	COLUMN_TRUNCATION_NOTICE_RE.is_match(line.trim())
}

/// Whether bracketed notice carries the column-cap clause.
pub fn has_column_truncation_clause(line: &str) -> bool {
	let trimmed = line.trim();
	COLUMN_TRUNCATION_NOTICE_RE.is_match(trimmed) || COLUMN_TRUNCATION_IN_NOTICE_RE.is_match(trimmed)
}

/// Format bytes as a human-readable size (decimal-ish, compact).
pub fn format_bytes(bytes: usize) -> String {
	const KB: f64 = 1024.0;
	const MB: f64 = 1024.0 * 1024.0;
	const GB: f64 = 1024.0 * 1024.0 * 1024.0;
	let b = bytes as f64;
	if b >= GB {
		format!("{:.1} GB", b / GB)
	} else if b >= MB {
		format!("{:.1} MB", b / MB)
	} else if b >= KB {
		format!("{:.1} KB", b / KB)
	} else {
		format!("{bytes} B")
	}
}

/// Strip the last literal notice or matching final line.
pub fn strip_trailing_notice(text: &str, notice: &str, trim_result: bool) -> String {
	let Some(idx) = text.rfind(notice) else {
		return text.to_string();
	};
	let mut start = idx;
	let mut end = idx + notice.len();
	if trim_result {
		if start > 0 && text.as_bytes()[start - 1] == b'\n' {
			start -= 1;
		}
		if end < text.len() && text.as_bytes()[end] == b'\n' {
			end += 1;
		}
	}
	let mut out = String::with_capacity(text.len() - (end - start));
	out.push_str(&text[..start]);
	out.push_str(&text[end..]);
	if trim_result {
		out.trim_end().to_string()
	} else {
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn artifact_reference_with_elision() {
		let s = format_full_output_reference("42", Some(2048));
		assert!(s.contains("artifact://42"));
		assert!(s.contains("head/tail sample"));
	}

	#[test]
	fn artifact_reference_plain() {
		assert_eq!(
			format_full_output_reference("7", None),
			"Read artifact://7 for full output"
		);
	}

	#[test]
	fn strip_raw_artifact_footer() {
		let (s, id) = strip_raw_output_artifact_notice("hello\n[raw output: artifact://123]");
		assert_eq!(s, "hello");
		assert_eq!(id.as_deref(), Some("123"));
	}

	#[test]
	fn column_notice_detection() {
		assert!(is_column_truncation_notice(
			"[Some lines truncated to 768 chars]"
		));
		assert!(has_column_truncation_clause(
			"[Showing lines 1-4 of 5. Some lines truncated to 768 chars. Read artifact://1]"
		));
		assert!(!is_column_truncation_notice("[Other notice]"));
	}

	#[test]
	fn format_bytes_units() {
		assert_eq!(format_bytes(512), "512 B");
		assert_eq!(format_bytes(2048), "2.0 KB");
	}

	#[test]
	fn strip_trailing_notice_literal() {
		let out = strip_trailing_notice("a\n[notice]\n", "[notice]", true);
		assert_eq!(out, "a");
	}
}
