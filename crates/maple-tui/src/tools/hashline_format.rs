//! Hashline file format: header delimiters, addressable line numbering, and
//! content fingerprints.
 //! Reference: `l/src/tools/hashline-format.ts`.
 //!
 //! The reference delegates `formatHashlineHeader`,
 //! `formatNumberedLines`, `stripHashlinePrefixes`,
 //! `isReadTruncationNotice`, and `computeFileHash` to `@oh-my-pi/pi-natives`
 //! functions that are **not** part of `l/` and are not vendored in this
 //! workspace. Only the constants and the two pure helpers
 //! (`formatNumberedLine`, `splitAddressableFileLines`) are specified by the
 //! reference; the remaining functions here are best-effort equivalents
 //! derived from their documented format and are flagged `best_effort` below.
 //! They are deterministic and dependency-free but are not byte-identical to
 //! the native store.
 //!
 //! Deviations:
 //! - `HASHLINE_TAG_SUFFIX_RE` is a `Regex` in the reference; Rust `Regex` is
 //!   not `const`, so it is a [`LazyLock`] static with the same pattern.
 //! - `format_numbered_lines`' `startLine?` default is `1`, chosen from the
 //!   documented 1-indexed line numbering.
//! - Render/describe helpers take `&Theme` where theming adds value (line
//!   numbers/gutters/header painted); theming is in-crate, not JS-runtime-only.
use regex::Regex;
use std::sync::LazyLock;

/// Opening delimiter of a hashline file header.
pub const HL_FILE_PREFIX: &str = "[";
/// Closing delimiter of a hashline file header.
pub const HL_FILE_SUFFIX: &str = "]";
/// Separator between the file path and content hash.
pub const HL_FILE_HASH_SEP: &str = "#";
/// Number of characters in a displayed file hash.
pub const HL_FILE_HASH_LENGTH: usize = 4;
/// A trailing `#TAG` file hash exactly as the hashline store writes it
/// (uppercase hex), so a name's own `#dead` or `#v2` ending survives
/// stripping.
pub static HASHLINE_TAG_SUFFIX_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"#[0-9A-F]{4}$").expect("valid hashline tag regex"));
/// Keyword identifying a file move operation.
pub const HL_MOVE_KEYWORD: &str = "MV";
/// Keyword identifying a file removal operation.
pub const HL_REM_KEYWORD: &str = "REM";
/// Separator between a line number and its body.
pub const HL_LINE_BODY_SEP: &str = ":";

/// Format a file path and content hash as a hashline header.
///
/// Best-effort native delegate: `[path#TAG]`.
pub fn format_hashline_header(path: &str, tag: &str) -> String {
	format!("{HL_FILE_PREFIX}{path}{HL_FILE_HASH_SEP}{tag}{HL_FILE_SUFFIX}")
}

/// Prefix each addressable line with its file line number, starting at
/// `start_line` (default `1`), joined by newlines.
///
/// Best-effort native delegate.
pub fn format_numbered_lines(text: &str, start_line: Option<u64>) -> String {
	let start = start_line.unwrap_or(1);
	split_addressable_file_lines(text)
		.iter()
		.enumerate()
		.map(|(index, line)| format_numbered_line(start + index as u64, line))
		.collect::<Vec<_>>()
		.join("\n")
}

/// Prefix one line with its file line number.
pub fn format_numbered_line(line_number: u64, line: &str) -> String {
	format!("{line_number}{HL_LINE_BODY_SEP}{line}")
}

/// Split file lines without treating a trailing newline as an empty address.
pub fn split_addressable_file_lines(text: &str) -> Vec<String> {
	let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
	if lines.last().is_some_and(|line| line.is_empty()) {
		lines.pop();
	}
	lines
}

/// Remove model-facing hashline anchors from copied file content: drop header
/// rows (`[path#TAG]`) and one leading `N:` line-number prefix per row.
///
/// Best-effort native delegate.
pub fn strip_hashline_prefixes(lines: &[String]) -> Vec<String> {
	lines
		.iter()
		.filter(|line| !is_hashline_header(line))
		.map(|line| match line.find(HL_LINE_BODY_SEP) {
			Some(index) if index > 0 && line[..index].bytes().all(|b| b.is_ascii_digit()) => {
				line[index + HL_LINE_BODY_SEP.len()..].to_string()
			}
			_ => line.clone(),
		})
		.collect()
}

/// Whether `line` is a `[path#TAG]` hashline header.
fn is_hashline_header(line: &str) -> bool {
	let trimmed = line.trim_end();
	let Some(inner) = trimmed
		.strip_prefix(HL_FILE_PREFIX)
		.and_then(|rest| rest.strip_suffix(HL_FILE_SUFFIX))
	else {
		return false;
	};
	match inner.rsplit_once(HL_FILE_HASH_SEP) {
		Some((_, tag)) => {
			tag.len() == HL_FILE_HASH_LENGTH
				&& tag.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
		}
		None => false,
	}
}

/// Whether a row is a truncation notice emitted by `read`.
///
/// Best-effort native delegate: a bracketed row whose text mentions a
/// truncation.
pub fn is_read_truncation_notice(line: &str) -> bool {
	let trimmed = line.trim();
	let lower = trimmed.to_lowercase();
	trimmed.starts_with('[') && lower.contains("truncat")
}

/// Compute the native hashline content fingerprint.
///
/// Best-effort native delegate: FNV-1a 32-bit over the UTF-8 bytes, rendered
/// as four uppercase hex characters.
pub fn compute_file_hash(text: &str) -> String {
	let mut hash: u32 = 0x811c_9dc5;
	for byte in text.bytes() {
		hash ^= byte as u32;
		hash = hash.wrapping_mul(0x0100_0193);
	}
	format!("{:04X}", (hash >> 16) & 0xFFFF)
}

/// Render numbered lines with themed line-number gutter.
pub fn render_numbered_lines_themed(text: &str, start_line: Option<u64>, theme: &crate::theme::Theme) -> String {
	let start = start_line.unwrap_or(1);
	split_addressable_file_lines(text)
		.into_iter()
		.enumerate()
		.map(|(i, line)| {
			let n = start + i as u64;
			let num = theme.fg("dim", &n.to_string()).unwrap_or_else(|_| n.to_string());
			let sep = theme.fg("dim", HL_LINE_BODY_SEP).unwrap_or_else(|_| HL_LINE_BODY_SEP.to_string());
			format!("{num}{sep}{line}")
		})
		.collect::<Vec<_>>()
		.join("\n")
}

/// Render a hashline header with themed path/tag.
pub fn render_header_themed(path: &str, tag: &str, theme: &crate::theme::Theme) -> String {
	let pfx = theme.fg("dim", HL_FILE_PREFIX).unwrap_or_else(|_| HL_FILE_PREFIX.to_string());
	let sfx = theme.fg("dim", HL_FILE_SUFFIX).unwrap_or_else(|_| HL_FILE_SUFFIX.to_string());
	let sep = theme.fg("dim", HL_FILE_HASH_SEP).unwrap_or_else(|_| HL_FILE_HASH_SEP.to_string());
	let path_s = theme.fg("accent", path).unwrap_or_else(|_| path.to_string());
	let tag_s = theme.fg("muted", tag).unwrap_or_else(|_| tag.to_string());
	format!("{pfx}{path_s}{sep}{tag_s}{sfx}")
}

/// Plain render helpers (call/result) for hashline formatting views.
pub fn render_call(path: Option<&str>) -> String {
	match path.filter(|s| !s.is_empty()) { Some(p) => format!("hashline {p}"), None => "hashline".into() }
}

pub fn render_result(text: &str, start_line: Option<u64>) -> Vec<String> {
	format_numbered_lines(text, start_line).lines().map(|s| s.to_string()).collect()
}

/// Native describe call.
pub fn describe_call(path: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let details = path.filter(|s| !s.is_empty()).map(|p| vec![ToolHeadDetail::Span(Span::styled(p.to_string(), "accent"))]).unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("Hashline", &details))), ..NativeToolView::default() }
}

/// Native describe result (numbered content).
pub fn describe_result(path: Option<&str>, text: &str, start_line: Option<u64>) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::code;
	use crate::native::node::{NodeText, Props, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let details = path.filter(|s| !s.is_empty()).map(|p| vec![ToolHeadDetail::Span(Span::styled(p.to_string(), "accent"))]).unwrap_or_default();
	let head = tool_head("Hashline", &details);
	let body = format_numbered_lines(text, start_line);
	let mut p = Props::new();
	if let Some(path) = path.filter(|s| !s.is_empty()) { p.set_str("path", path); }
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![code(body, p)]), ..NativeToolView::default() }
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn render_and_describe() {
		let r = render_result("a\nb", Some(1));
		assert_eq!(r[0], "1:a");
		let c = describe_call(Some("f.rs"));
		assert!(c.head.is_some());
		let d = describe_result(Some("f.rs"), "x", Some(10));
		assert!(d.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn header_uses_delimiters() {
		assert_eq!(format_hashline_header("src/main.ts", "DEAD"), "[src/main.ts#DEAD]");
		assert_eq!(format_hashline_header("a", "00FF"), "[a#00FF]");
	}

	#[test]
	fn numbered_line_joins_number_and_body() {
		assert_eq!(format_numbered_line(1, "fn main() {}"), "1:fn main() {}");
		assert_eq!(format_numbered_line(42, ""), "42:");
	}

	#[test]
	fn numbered_lines_start_at_one_by_default() {
		assert_eq!(format_numbered_lines("a\nb\nc", None), "1:a\n2:b\n3:c");
		assert_eq!(format_numbered_lines("a\nb", Some(10)), "10:a\n11:b");
	}

	#[test]
	fn numbered_lines_ignore_trailing_newline() {
		assert_eq!(format_numbered_lines("a\nb\n", None), "1:a\n2:b");
		assert_eq!(format_numbered_lines("", None), "");
	}

	#[test]
	fn split_drops_only_the_trailing_empty_address() {
		assert_eq!(split_addressable_file_lines("a\nb"), vec!["a", "b"]);
		assert_eq!(split_addressable_file_lines("a\nb\n"), vec!["a", "b"]);
		assert_eq!(split_addressable_file_lines("a\n\n"), vec!["a", ""]);
		assert_eq!(split_addressable_file_lines(""), Vec::<String>::new());
	}

	#[test]
	fn tag_suffix_matches_uppercase_hex_only() {
		assert!(HASHLINE_TAG_SUFFIX_RE.is_match("notes#ABCD"));
		assert!(HASHLINE_TAG_SUFFIX_RE.is_match("notes#0A2F"));
		assert!(!HASHLINE_TAG_SUFFIX_RE.is_match("notes#dead"));
		assert!(!HASHLINE_TAG_SUFFIX_RE.is_match("notes#v2"));
		assert!(!HASHLINE_TAG_SUFFIX_RE.is_match("notes#ABC"));
		assert!(!HASHLINE_TAG_SUFFIX_RE.is_match("notes#ABCDE"));
		assert!(!HASHLINE_TAG_SUFFIX_RE.is_match("notes"));
	}

	#[test]
	fn strip_removes_headers_and_line_prefixes() {
		let lines = vec![
			"[src/main.ts#DEAD]".to_string(),
			"1:fn main() {}".to_string(),
			"2:  let x = 1;".to_string(),
			"plain".to_string(),
		];
		assert_eq!(
			strip_hashline_prefixes(&lines),
			vec!["fn main() {}", "  let x = 1;", "plain"]
		);
	}

	#[test]
	fn strip_keeps_names_with_hash_suffixes() {
		let lines = vec!["notes#dead".to_string(), "notes#v2".to_string()];
		assert_eq!(strip_hashline_prefixes(&lines), lines);
	}

	#[test]
	fn truncation_notice_detection() {
		assert!(is_read_truncation_notice("[... 12 lines truncated ...]"));
		assert!(is_read_truncation_notice("  [output truncated]"));
		assert!(!is_read_truncation_notice("truncated"));
		assert!(!is_read_truncation_notice("[1: text]"));
	}

	#[test]
	fn file_hash_is_four_uppercase_hex_chars() {
		let hash = compute_file_hash("hello");
		assert_eq!(hash.len(), HL_FILE_HASH_LENGTH);
		assert!(hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase()));
		assert_eq!(compute_file_hash("hello"), hash);
		assert_ne!(compute_file_hash("hello"), compute_file_hash("world"));
	}
}
