//! Line-range selector grammar and parsing.
//!
 //! Reference: `l/src/tools/line-ranges.ts`.
 //!
 //! Deviations:
 //! - The reference throws `ToolError` from `parseLineRangeChunk` and
 //!   `parseLineRanges`; Rust has no exceptions, so those parsers return
 //!   `Result<Option<_, _>, ToolError>` (`Ok(None)` is the reference's `null`,
 //!   `Err` its throw).
 //! - The reference's non-empty tuple return `[LineRange, ...]LineRange[]` is a
 //!   `Vec<LineRange>`; a `Vec` can also be empty, which the reference's
 //!   destructuring would reject, so [`merge_line_ranges`] returns an empty
 //!   vector for empty input.
 //! - `parseLineRanges` has a default `bare = "onward"`; Rust has no default
 //!   arguments, so [`BareLineMode`] implements [`Default`] as `Onward` and
 //!   callers pass it explicitly.
 //! - A `\d+` chunk that overflows `u64` has no JavaScript `parseInt` analogue
 //!   (which yields an imprecise float); it parses as `Ok(None)`.
//! - Render/describe helpers are plain/native; themed variants take `&Theme`
//!   for label painting (theming available in-crate, not a JS-runtime facility).
use regex::Regex;
use std::sync::LazyLock;

use crate::tools::tool_errors::ToolError;

/// Shared line-range grammar for selector recognition and parsing.
pub const LINE_RANGE_CHUNK_SOURCE: &str = r"L?(\d+)(?:(\.\.|[-+])L?(\d+)?)?";

static LINE_RANGE_CHUNK_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(&format!("(?i)^{LINE_RANGE_CHUNK_SOURCE}$")).expect("valid line-range regex")
});

/// Inclusive line range describing one selector segment (e.g. `50-100`,
/// `301-`, or `50+10`). `end_line` is `None` for open-ended ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
	/// First selected line (1-indexed).
	pub start_line: u64,
	/// Last selected line, or `None` when the range reaches EOF.
	pub end_line: Option<u64>,
}

/// What a bare `N` selects in a one-chunk list: lines `N` onward (`:50`), or
/// the single line `N`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BareLineMode {
	/// `N` selects from line `N` onward.
	#[default]
	Onward,
	/// `N` selects only line `N`.
	Single,
}

/// Parse a single `N`, `N-M`, `N-`, `N+K`, or `..`-aliased (`N..M`, `N..`)
/// chunk. `Ok(None)` when `sel` is not a line-range chunk; [`ToolError`] on
/// invalid bounds.
pub fn parse_line_range_chunk(sel: &str) -> Result<Option<LineRange>, ToolError> {
	parse_chunk(sel, false)
}

/// `pin_bare` makes a separator-less `N` the single line `N` instead of "from
/// `N` onward".
fn parse_chunk(sel: &str, pin_bare: bool) -> Result<Option<LineRange>, ToolError> {
	let Some(captures) = LINE_RANGE_CHUNK_RE.captures(sel) else {
		return Ok(None);
	};
	let raw_start: u64 = match captures[1].parse() {
	 Ok(value) => value,
		Err(_) => return Ok(None),
	};
	if raw_start < 1 {
		return Err(ToolError::new(
			"Line selector 0 is invalid; lines are 1-indexed. Use :1.",
			None,
		));
	}
	// `..` is a forgiving alias for `-` (e.g. `2724..2727` == `2724-2727`).
	let sep = match captures.get(2).map(|m| m.as_str()) {
		Some("..") => Some("-"),
		other => other,
	};
	let rhs = captures
		.get(3)
		.map(|m| m.as_str().parse::<u64>().unwrap_or(0));
	let mut raw_end: Option<u64> = None;
	match sep {
		Some("+") => {
			let count = rhs.unwrap_or(0);
			if rhs.is_none() || count < 1 {
				return Err(ToolError::new(
					format!("Invalid range {raw_start}+{count}: count must be >= 1."),
					None,
				));
			}
			raw_end = Some(raw_start.saturating_add(count).saturating_sub(1));
		}
		Some("-") => {
			// `301-` is shorthand for "from 301 onward" — equivalent to a lone
			// bare `301`.
			if let Some(end) = rhs {
				if end < raw_start {
					return Err(ToolError::new(
						format!("Invalid range {raw_start}-{end}: end must be >= start."),
						None,
					));
				}
				raw_end = Some(end);
			}
		}
		_ if pin_bare => raw_end = Some(raw_start),
		_ => {}
	}
	Ok(Some(LineRange {
		start_line: raw_start,
		end_line: raw_end,
	}))
}

/// Parse a comma-separated list of line ranges (e.g. `5-16,960-973` or
/// `19,59`). Returns the ranges in ascending order with overlapping/adjacent
/// ranges merged so downstream consumers can stream the file in a single
/// forward pass per range. `Ok(None)` when no chunk parses.
pub fn parse_line_ranges(
	sel: &str,
	bare: BareLineMode,
) -> Result<Option<Vec<LineRange>>, ToolError> {
	let chunks: Vec<&str> = sel.split(',').collect();
	// A lone `:50` means "from line 50" unless `bare` pins it. Inside a comma
	// list, a bare number is that one line; otherwise `:19,59` collapses to
	// "from 19 through EOF".
	let pin_bare = bare == BareLineMode::Single || chunks.len() > 1;
	let mut parsed: Vec<LineRange> = Vec::new();
	for chunk in chunks {
		let Some(range) = parse_chunk(chunk, pin_bare)? else {
			return Ok(None);
		};
		parsed.push(range);
	}
	if parsed.is_empty() {
		return Ok(None);
	}
	Ok(Some(merge_line_ranges(&parsed)))
}

/// Sort non-empty `ranges` by start line and merge the ones that overlap or
/// touch; an open-ended range (`end_line` `None`) reaches EOF and absorbs
/// every later one.
pub fn merge_line_ranges(ranges: &[LineRange]) -> Vec<LineRange> {
	if ranges.is_empty() {
		return Vec::new();
	}
	let mut sorted = ranges.to_vec();
	sorted.sort_by_key(|range| range.start_line);
	let mut merged: Vec<LineRange> = Vec::with_capacity(sorted.len());
	for current in sorted {
		let Some(last) = merged.last().copied() else {
			merged.push(current);
			continue;
		};
		// An open-ended range reaches EOF; nothing after it can extend it.
		let Some(last_end) = last.end_line else {
			break;
		};
		// Merge when current starts within (or immediately after) the last range.
		if current.start_line <= last_end.saturating_add(1) {
			if current.end_line.is_none() || current.end_line.is_some_and(|end| end > last_end) {
				let index = merged.len() - 1;
				merged[index] = LineRange {
					start_line: last.start_line,
					end_line: current.end_line,
				};
			}
			continue;
		}
		merged.push(current);
	}
	merged
}

/// Extract the line-range component from a read-tool selector that may also
/// carry a verbatim/index display mode (`raw`, `conflicts`) — alone or
/// compounded with a range (`raw:50-100`, `50-100:raw`). Returns the parsed
/// ranges when the selector names any, otherwise `None` (pure
/// `raw`/`conflicts`/none).
pub fn selector_line_ranges(
	sel: Option<&str>,
) -> Result<Option<Vec<LineRange>>, ToolError> {
	let Some(sel) = sel else {
		return Ok(None);
	};
	let chunks: Vec<&str> = sel.split(':').collect();
	let bare = if chunks
		.iter()
		.any(|chunk| chunk.eq_ignore_ascii_case("raw"))
	{
		BareLineMode::Single
	} else {
		BareLineMode::Onward
	};
	for chunk in chunks {
		let lower = chunk.to_lowercase();
		if lower == "raw" || lower == "conflicts" {
			continue;
		}
		if let Some(ranges) = parse_line_ranges(chunk, bare)? {
			return Ok(Some(ranges));
		}
	}
	Ok(None)
}

/// Format a line range as a compact selector string.
pub fn format_line_range(r: &LineRange) -> String {
	match r.end_line {
		None => format!("{}-", r.start_line),
		Some(e) if e == r.start_line => format!("{}", r.start_line),
		Some(e) => format!("{}-{}", r.start_line, e),
	}
}

/// Plain render of parsed line ranges.
pub fn render_result(ranges: &[LineRange]) -> Vec<String> {
	if ranges.is_empty() { vec!["(no ranges)".into()] } else { vec![ranges.iter().map(format_line_range).collect::<Vec<_>>().join(", ")] }
}

/// Themed render of parsed line ranges.
pub fn render_result_themed(ranges: &[LineRange], theme: &crate::theme::Theme) -> Vec<String> {
	if ranges.is_empty() { return vec![theme.fg("dim", "(no ranges)").unwrap_or_else(|_| "(no ranges)".into())]; }
	let parts: Vec<String> = ranges.iter().map(|r| {
		let s = format_line_range(r);
		theme.fg("accent", &s).unwrap_or(s)
	}).collect();
	let sep = theme.fg("dim", ", ").unwrap_or_else(|_| ", ".into());
	vec![parts.join(&sep)]
}

/// Native describe of a line-range parse/call.
pub fn describe_call(selector: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let details = selector.filter(|s| !s.is_empty()).map(|s| vec![ToolHeadDetail::Span(Span::styled(s.to_string(), "accent"))]).unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("Line ranges", &details))), ..NativeToolView::default() }
}

/// Native describe of parsed line ranges.
pub fn describe_result(ranges: &[LineRange]) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let label = if ranges.is_empty() { "none".into() } else { ranges.iter().map(format_line_range).collect::<Vec<_>>().join(", ") };
	let head = tool_head("Line ranges", &[ToolHeadDetail::Span(Span::styled(label.clone(), "accent"))]);
	let mut p = Props::new();
	p.set_str("wrap", "word");
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ntext(NodeText::Plain(label), p)]), ..NativeToolView::default() }
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn format_and_describe() {
		let r = LineRange { start_line: 5, end_line: Some(10) };
		assert_eq!(format_line_range(&r), "5-10");
		let lines = render_result(&[r]);
		assert_eq!(lines[0], "5-10");
		let v = describe_call(Some("5-10"));
		assert!(v.head.is_some());
		let d = describe_result(&[r]);
		assert!(d.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	fn range(start: u64, end: Option<u64>) -> LineRange {
		LineRange {
			start_line: start,
			end_line: end,
		}
	}

	#[test]
	fn parses_single_chunks() {
		let cases: &[(&str, LineRange)] = &[
			("50-100", range(50, Some(100))),
			("301-", range(301, None)),
			("50+10", range(50, Some(59))),
			("1", range(1, None)),
			("L50-60", range(50, Some(60))),
			("l7", range(7, None)),
			("2724..2727", range(2724, Some(2727))),
			("2724..", range(2724, None)),
		];
		for (sel, expected) in cases {
			assert_eq!(
				parse_line_range_chunk(sel).unwrap(),
				Some(*expected),
				"chunk {sel}"
			);
		}
	}

	#[test]
	fn rejects_non_chunks() {
		for sel in ["", "abc", "50-100-200", "-5", "50..", "1.5"] {
			assert_eq!(parse_line_range_chunk(sel).unwrap(), None, "chunk {sel}");
		}
	}

	#[test]
	fn invalid_bounds_throw() {
		let cases: &[(&str, &str)] = &[
			(
				"0",
				"Line selector 0 is invalid; lines are 1-indexed. Use :1.",
			),
			("50+", "Invalid range 50+0: count must be >= 1."),
			("50+0", "Invalid range 50+0: count must be >= 1."),
			("100-50", "Invalid range 100-50: end must be >= start."),
		];
		for (sel, message) in cases {
			let error = parse_line_range_chunk(sel).unwrap_err();
			assert_eq!(error.message(), *message, "chunk {sel}");
			assert_eq!(error.render(), *message, "chunk {sel}");
		}
	}

	#[test]
	fn parses_lists_and_merges() {
		let cases: &[(&str, BareLineMode, Vec<LineRange>)] = &[
			("50", BareLineMode::Onward, vec![range(50, None)]),
			("50", BareLineMode::Single, vec![range(50, Some(50))]),
			("19,59", BareLineMode::Onward, vec![range(19, Some(19)), range(59, Some(59))]),
			(
				"5-16,960-973",
				BareLineMode::Onward,
				vec![range(5, Some(16)), range(960, Some(973))],
			),
			("5-10,8-12", BareLineMode::Onward, vec![range(5, Some(12))]),
			("5-10,11-12", BareLineMode::Onward, vec![range(5, Some(12))]),
			("5-10,12-14", BareLineMode::Onward, vec![range(5, Some(14))]),
			("5-10,20-30", BareLineMode::Onward, vec![range(5, Some(10)), range(20, Some(30))]),
			("10-20,5-", BareLineMode::Onward, vec![range(5, None)]),
			("301-,50", BareLineMode::Onward, vec![range(50, Some(50)), range(301, None)]),
		];
		for (sel, bare, expected) in cases {
			assert_eq!(
				parse_line_ranges(sel, *bare).unwrap(),
				Some(expected.clone()),
				"list {sel}"
			);
		}
	}

	#[test]
	fn list_rejects_bad_chunks() {
		assert_eq!(parse_line_ranges("5-10,abc", BareLineMode::Onward).unwrap(), None);
		assert!(parse_line_ranges("0,5", BareLineMode::Onward).is_err());
	}

	#[test]
	fn merge_handles_empty_and_unsorted() {
		assert!(merge_line_ranges(&[]).is_empty());
		assert_eq!(
			merge_line_ranges(&[range(20, Some(30)), range(1, Some(5))]),
			vec![range(1, Some(5)), range(20, Some(30))]
		);
	}

	#[test]
	fn selector_accepts_display_modes() {
		let cases: &[(&str, Option<Vec<LineRange>>)] = &[
			("50-100", Some(vec![range(50, Some(100))])),
			("50", Some(vec![range(50, None)])),
			("raw:50-100", Some(vec![range(50, Some(100))])),
			("50-100:raw", Some(vec![range(50, Some(100))])),
			("raw:50", Some(vec![range(50, Some(50))])),
			("conflicts:10-20", Some(vec![range(10, Some(20))])),
			("raw", None),
			("conflicts", None),
			("", None),
			("foo", None),
		];
		for (sel, expected) in cases {
			assert_eq!(selector_line_ranges(Some(sel)).unwrap(), *expected, "selector {sel}");
		}
		assert_eq!(selector_line_ranges(None).unwrap(), None);
	}

	#[test]
	fn selector_propagates_invalid_bounds() {
		assert!(selector_line_ranges(Some("0-5")).is_err());
	}
}
