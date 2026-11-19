//! Provider/error block formatting.
//!
//! Reference: `l/src/chrome/error-block.ts`.
//!
//! Deviations:
//! - `shortenEmbeddedPaths` and `TRUNCATE_LENGTHS` from the reference render
//!   utils are not present in the crate; this port performs tab/CRLF/newline
//!   sanitization and whitespace collapse, and truncates by display width using
//!   [`truncate_to_width`]. Embedded home-path shortening is best-effort: a
//!   literal `home_dir` prefix is replaced with `~` when provided.
//! - Styling/theme access: [`format_error_block`] takes an explicit `dim_style`
//!   closure for the hidden-lines marker and `expand_hint` text instead of
//!   reading process-global theme/keybinding state.

use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// Indent for every row after the first, so continuations hang under the prefix.
const CONTINUATION_INDENT: &str = "  ";

/// Best-effort embedded home-path shortening: replace `home_dir` prefixes with
/// `~`.
fn shorten_embedded_paths(text: &str, home_dir: Option<&str>) -> String {
	match home_dir {
		Some(home) if !home.is_empty() => text.replace(home, "~"),
		_ => text.to_string(),
	}
}

/// Plain, bounded error text to sanitize before applying terminal styles.
pub fn sanitize_error_line(error: &str, max_width: usize, home_dir: Option<&str>) -> String {
	let normalized = error.replace("\r\n", "\n").replace('\r', "\n");
	let text = shorten_embedded_paths(&normalized.replace('\t', "   "), home_dir);
	let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
	let collapsed = if collapsed.is_empty() {
		"Unknown error".to_string()
	} else {
		collapsed
	};
	truncate_to_width(&collapsed, max_width, Ellipsis::Omit)
}

/// Format a provider error into wrapped rows for a block of `content_width`
/// cells.
///
/// Lines wrap to the render width instead of being cut at a fixed column.
/// Blank lines are dropped. When rows are cut, a dim `… +N more lines (...)`
/// row follows; pass `usize::MAX` as `max_rows` to keep every row. `style_line`
/// colors logical line `index` and supplies line 0's prefix.
pub fn format_error_block<F, D>(
	message: &str,
	content_width: usize,
	max_rows: usize,
	style_line: F,
	dim_style: D,
	expand_hint: &str,
) -> String
where
	F: Fn(&str, usize) -> String,
	D: Fn(&str) -> String,
{
	let mut lines: Vec<String> = message
		.replace('\t', "   ")
		.split('\n')
		.map(|line| line.trim().to_string())
		.filter(|line| !line.is_empty())
		.collect();
	if lines.is_empty() {
		lines.push("Unknown error".to_string());
	}
	let wrap_width = content_width
		.saturating_sub(visible_width(CONTINUATION_INDENT))
		.max(1);
	let mut rows: Vec<String> = Vec::new();
	for (index, line) in lines.iter().enumerate() {
		let styled = style_line(line, index);
		for row in wrap_text_with_ansi(&styled, wrap_width) {
			if rows.is_empty() {
				rows.push(row);
			} else {
				rows.push(format!("{CONTINUATION_INDENT}{row}"));
			}
		}
	}
	if rows.len() > max_rows {
		let hidden = rows.len() - max_rows;
		rows.truncate(max_rows);
		let plural = if hidden == 1 { "" } else { "s" };
		let marker = format!(
			"{CONTINUATION_INDENT}… +{hidden} more line{plural} ({expand_hint} to expand)"
		);
		rows.push(dim_style(&marker));
	}
	rows.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sanitize_error_line_collapses_and_bounds() {
		let msg = sanitize_error_line("  bad\t\r\n thing  ", 80, None);
		assert_eq!(msg, "bad thing");
		assert_eq!(sanitize_error_line("   ", 80, None), "Unknown error");
	}

	#[test]
	fn sanitize_error_line_shortens_home_prefix() {
		let msg = sanitize_error_line("/home/u/file.txt failed", 80, Some("/home/u"));
		assert!(msg.contains("~/file.txt"));
	}

	#[test]
	fn format_error_block_wraps_and_marks_hidden_rows() {
		let out = format_error_block(
			"Error: hello world from wrap\n\nsecond",
			12,
			2,
			|line, idx| {
				if idx == 0 {
					format!("ERR {line}")
				} else {
					line.to_string()
				}
			},
			|s| format!("<{s}>"),
			"ctrl+o",
		);
		let lines: Vec<&str> = out.lines().collect();
		assert!(!lines.is_empty());
		assert!(lines.last().unwrap().contains("more line"));
		assert!(lines.last().unwrap().contains("ctrl+o"));
	}

	#[test]
	fn format_error_block_unbounded_keeps_rows() {
		let out = format_error_block(
			"a\nb\nc",
			40,
			usize::MAX,
			|line, _| line.to_string(),
			|s| s.to_string(),
			"ctrl+o",
		);
		assert_eq!(out.lines().count(), 3);
		assert!(!out.contains("more line"));
	}
}
