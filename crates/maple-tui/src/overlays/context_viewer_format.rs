//! Context viewer formatting helpers.
//!
//! Reference: `l/src/overlays/context-viewer-format.ts`.
//!
//! Deviations:
//! - `theme`/ANSI helpers (`fg`, `bold`, `styledSymbol`, `visibleWidth`,
//!   `wrapTextWithAnsi`, `truncateToWidth`) and `shortenPath`/`formatNumber`
//!   utilities are not imported from the TS runtime. This port provides the
//!   pure data/shape helpers and returns plain text/spans without ANSI styling.
//! - `safeText`, `safeContent`, `safeLine` sanitization is simplified to
//!   trimming and stripping NULs; TS uses `sanitizeUserVisibleText`.
//! - `CONTEXT_PRESENTATION` descriptors are expected from
//!   `crate::chrome::context_breakdown` or equivalent host; callers supply
//!   category color/symbol where needed in higher-level rendering.

use crate::chrome::context_breakdown::{ContextCategoryId, ContextThinkingGroup, ContextViewEntry};

/// Preview row limits for overflow content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewLimits {
	pub output_collapsed: usize,
	pub output_expanded: usize,
	pub markdown: usize,
	pub diff: usize,
}

/// Default preview limits (matches TS `PREVIEW_LIMITS`).
pub const PREVIEW_LIMITS: PreviewLimits = PreviewLimits {
	output_collapsed: 10,
	output_expanded: 50,
	markdown: 50,
	diff: 50,
};

/// Timestamp format fields (logical; formatting left to caller).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampFormat {
	pub year: &'static str,
	pub month: &'static str,
	pub day: &'static str,
	pub hour: &'static str,
	pub minute: &'static str,
	pub second: &'static str,
}

pub const TIMESTAMP_FORMAT: TimestampFormat = TimestampFormat {
	year: "numeric",
	month: "2-digit",
	day: "2-digit",
	hour: "2-digit",
	minute: "2-digit",
	second: "2-digit",
};

fn strip_nul(s: &str) -> String {
	s.replace('\0', "")
}

/// Safe text for display (simplified).
#[must_use]
pub fn safe_text(text: &str) -> String {
	strip_nul(text).trim_end().to_string()
}

/// Safe content for display (simplified).
#[must_use]
pub fn safe_content(text: &str) -> String {
	strip_nul(text)
}

/// Safe single-line text.
#[must_use]
pub fn safe_line(text: &str) -> String {
	safe_text(text).replace('\n', " ")
}

/// Content text of an entry, limited to `limit` characters.
#[must_use]
pub fn entry_body_text(entry: &ContextViewEntry, limit: usize) -> String {
	let body = entry.text.trim_end();
	let limit = limit.min(body.len());
	safe_content(&body[..limit])
}

/// Token label of an entry or group.
#[must_use]
pub fn token_estimate_tokens(tokens: usize, interrupted: bool, grouped: bool) -> String {
	if grouped {
		return "—".to_string();
	}
	if interrupted {
		return "N/A · Interrupted".to_string();
	}
	if tokens == 1 {
		"1 token".to_string()
	} else {
		format!("{tokens} tokens")
	}
}

/// Token label for an item (entry or group conceptually).
#[must_use]
pub fn token_estimate_entry(entry: &ContextViewEntry, grouped: bool) -> String {
	token_estimate_tokens(entry.tokens, entry.interrupted, grouped)
}

/// `[label] N tokens` as plain text (ANSI omitted in Rust port).
#[must_use]
pub fn entry_header_line(entry: &ContextViewEntry, _category: ContextCategoryId) -> String {
	format!("[{}] {}", safe_line(&entry.label), token_estimate_entry(entry, false))
}

/// Heading for a shared thinking group: label plus token estimate.
#[must_use]
pub fn group_header_line(group: &ContextThinkingGroup, interrupted: bool) -> String {
	format!(
		"{} · {}",
		safe_line(&group.label),
		token_estimate_tokens(group.tokens, interrupted, false)
	)
}

/// Truncation marker under a preview.
#[must_use]
pub fn preview_hint(selected: bool, confirm_key: Option<&str>) -> String {
	if selected {
		if let Some(k) = confirm_key {
			format!("  … more · {k} View Content")
		} else {
			"  … more · View Content".to_string()
		}
	} else {
		"  … more".to_string()
	}
}

/// Metadata rows for an entry.
#[must_use]
pub fn entry_metadata(
	entry: &ContextViewEntry,
	with_group: bool,
	timestamp: Option<String>,
) -> Vec<String> {
	let mut lines = Vec::new();
	if with_group {
		if let Some(g) = &entry.thinking_group {
			lines.push(format!("{} · {}", safe_line(&g.label), token_estimate_tokens(g.tokens, false, false)));
		}
	}
	let mut info_parts: Vec<String> = Vec::new();
	if let Some(info) = &entry.info {
		if !info.is_empty() {
			info_parts.push(info.clone());
		}
	}
	if let Some(path) = &entry.path {
		if !path.is_empty() {
			info_parts.push(path.clone());
		}
	}
	let info = safe_text(&info_parts.join(" · "));
	if !info.is_empty() {
		lines.push(info);
	}
	if let Some(call_id) = &entry.call_id {
		let call_id = safe_text(call_id);
		if !call_id.is_empty() {
			lines.push(call_id);
		}
	}
	if let Some(ts) = timestamp {
		if !ts.is_empty() {
			lines.push(ts);
		}
	}
	lines
}

/// Preview row budget for an entry list.
#[must_use]
pub fn preview_rows(has_group: bool, height: usize) -> usize {
	let reserved = if has_group { 3 } else { 2 };
	height.saturating_sub(reserved).clamp(1, PREVIEW_LIMITS.output_expanded)
}

/// Options for entry preview rendering.
#[derive(Clone, Debug)]
pub struct EntryPreviewOptions {
	pub width: usize,
	pub rows: usize,
	pub source_width: usize,
}

/// Preview result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPreview {
	pub lines: Vec<String>,
	pub complete: bool,
}

/// Dim metadata rows, then indented body, cut to `rows`.
/// Simplified wrapping: splits by newlines and hard-cuts by width budget.
#[must_use]
pub fn entry_preview(
	entry: &ContextViewEntry,
	options: &EntryPreviewOptions,
) -> EntryPreview {
	let width = options.width.max(1);
	let rows = options.rows;
	let max_chars = options.source_width.max(1).saturating_mul(rows).saturating_mul(2);
	let metadata_text = entry_metadata(entry, false, None).join("\n");
	let metadata_len = metadata_text.len().min(max_chars);
	let body = entry.text.trim_end();
	let body_budget = max_chars.saturating_sub(metadata_len).saturating_sub(if metadata_len > 0 { 1 } else { 0 });
	let body_len = body.len().min(body_budget);
	let mut lines: Vec<String> = Vec::new();
	let mut complete = metadata_len == metadata_text.len() && body_len == body.len();

	let push_block = |lines: &mut Vec<String>, complete: &mut bool, value: String, prefix: &str| {
		if value.is_empty() {
			return;
		}
		for paragraph in value.split('\n') {
			let remaining = rows.saturating_sub(lines.len());
			if remaining == 0 {
				*complete = false;
				return;
			}
			let content_width = width.saturating_sub(prefix.len()).max(1);
			// naive wrap: chunk paragraph
			if paragraph.is_empty() {
				lines.push(prefix.to_string());
				continue;
			}
			let chars: Vec<char> = paragraph.chars().collect();
			let mut start = 0;
			while start < chars.len() && rows.saturating_sub(lines.len()) > 0 {
				let end = (start + content_width).min(chars.len());
				let segment: String = chars[start..end].iter().collect();
				lines.push(format!("{prefix}{segment}"));
				start = end;
			}
			if start < chars.len() {
				*complete = false;
				return;
			}
		}
	};

	push_block(
		&mut lines,
		&mut complete,
		safe_content(&metadata_text[..metadata_len]),
		"",
	);
	push_block(
		&mut lines,
		&mut complete,
		entry_body_text(entry, body_len),
		"  ",
	);
	EntryPreview { lines, complete }
}

#[cfg(test)]
mod tests {
	use super::*;

	fn sample_entry(text: &str) -> ContextViewEntry {
		ContextViewEntry {
			id: "e1".into(),
			label: "Msg".into(),
			tokens: 3,
			text: text.into(),
			info: None,
			path: None,
			call_id: None,
			timestamp: None,
			source_markers: None,
			interrupted: false,
			thinking_group: None,
		}
	}

	#[test]
	fn token_estimate_grouped_and_interrupted() {
		assert_eq!(token_estimate_tokens(5, false, true), "—");
		assert_eq!(token_estimate_tokens(5, true, false), "N/A · Interrupted");
		assert_eq!(token_estimate_tokens(1, false, false), "1 token");
		assert_eq!(token_estimate_tokens(2, false, false), "2 tokens");
	}

	#[test]
	fn entry_header_line_plain() {
		let e = sample_entry("hi");
		assert_eq!(entry_header_line(&e, ContextCategoryId::SystemPrompt), "[Msg] 3 tokens");
	}

	#[test]
	fn preview_rows_accounts_for_group() {
		assert_eq!(preview_rows(false, 10), 8);
		assert_eq!(preview_rows(true, 10), 7);
		assert_eq!(preview_rows(false, 1), 1);
	}

	#[test]
	fn entry_preview_truncates_when_over_rows() {
		let e = sample_entry("line1\nline2\nline3\nline4\nline5");
		let out = entry_preview(&e, &EntryPreviewOptions { width: 80, rows: 2, source_width: 80 });
		assert_eq!(out.lines.len(), 2);
		assert!(!out.complete);
	}
}
