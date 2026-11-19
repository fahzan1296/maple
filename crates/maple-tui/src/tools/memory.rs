//! Long-term memory tool render-model types and helpers.
//!
//! Reference: `l/src/tools/memory.ts`.
//!
//! Deviations:
//! - TS renders TUI components/native views with `Theme`, `OwnerMemo`,
//!   markdown/text nodes, cached components. Rust ports argument/result data
//!   shapes, pure helpers for retain content extraction, recall/reflect summary
//!   parsing, collapsed preview budgeting, plain render-line builders, and
//!   native-view describe helpers for renderer-completion integration.
//! - `unknown`/`items` payloads use `serde_json::Value`.

use serde_json::Value;

use crate::native::describe::{item as nitem, list as nlist, md, node, span as nspan, text as ntext};
use crate::native::node::{NodeKind, NodeText, Props, Span, Tone};
use crate::native::spans::plain_text;
use crate::tools::native_view::{error_view, note_text, tool_head, NativePreview, NativeToolView};

/// Number of memories accepted by retain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryRetainDetails {
	pub count: usize,
}

/// Retain call args.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RetainRenderArgs {
	pub items: Option<Value>,
}

/// Recall/reflect query args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryRenderArgs {
	pub query: Option<String>,
}

/// Replace tabs with spaces (preview sanitization parity).
pub fn replace_tabs(s: &str) -> String {
	s.replace('\t', " ")
}

/// Extract trimmed, non-empty string contents from retain items.
pub fn retain_contents(args: Option<&RetainRenderArgs>) -> Vec<String> {
	let Some(items) = args.and_then(|a| a.items.as_ref()).and_then(|v| v.as_array()) else {
		return Vec::new();
	};
	let mut out = Vec::new();
	for item in items {
		let Some(obj) = item.as_object() else { continue };
		let Some(content) = obj.get("content").and_then(|v| v.as_str()) else { continue };
		let trimmed = replace_tabs(content.trim());
		if !trimmed.is_empty() {
			out.push(trimmed);
		}
	}
	out
}

/// Collapsed item preview limit.
pub const PREVIEW_LIMITS_COLLAPSED_ITEMS: usize = 6;
/// Expanded output line cap for recall/reflect bodies.
pub const PREVIEW_LIMITS_OUTPUT_EXPANDED: usize = 200;
/// Collapsed output line cap for reflect.
pub const PREVIEW_LIMITS_OUTPUT_COLLAPSED: usize = 4;

/// Truncate string to `max` chars with unicode ellipsis.
pub fn truncate_preview(s: &str, max: usize) -> String {
	let count = s.chars().count();
	if count <= max {
		s.to_string()
	} else if max <= 1 {
		"…".to_string()
	} else {
		let mut out: String = s.chars().take(max - 1).collect();
		out.push('…');
		out
	}
}

/// Parse recall "Found N relevant..." count from result text.
pub fn parse_recall_found(text: &str) -> usize {
	let text = text.trim_start();
	let Some(rest) = text.strip_prefix("Found ") else { return 0 };
	let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
	if digits.is_empty() { return 0; }
	if !rest[digits.len()..].starts_with(" relevant") { return 0; }
	digits.parse().unwrap_or(0)
}

/// Recall result summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallSummary {
	pub query: Option<String>,
	pub found: usize,
	pub body: String,
}

/// Build recall summary.
pub fn summarize_recall(query: Option<&str>, result_text: &str, is_error: bool) -> Result<RecallSummary, String> {
	let output = result_text.trim().to_string();
	if is_error {
		return Err(if output.is_empty() { "Recall failed".to_string() } else { output });
	}
	let found = parse_recall_found(&output);
	let body = if found == 0 {
		String::new()
	} else if let Some(pos) = output.find('\n') {
		output[pos + 1..].trim_start_matches('\n').to_string()
	} else {
		String::new()
	};
	Ok(RecallSummary {
		query: query.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
		found,
		body,
	})
}

/// Retain result summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainSummary {
	pub contents: Vec<String>,
	pub summary: Option<String>,
}

/// Build retain summary.
pub fn summarize_retain(
	args: Option<&RetainRenderArgs>,
	result_text: &str,
	is_error: bool,
) -> Result<RetainSummary, String> {
	if is_error {
		let t = result_text.trim();
		return Err(if t.is_empty() { "Retain failed".to_string() } else { t.to_string() });
	}
	let contents = retain_contents(args);
	let summary = {
		let t = result_text.trim().trim_end_matches('.').to_string();
		if t.is_empty() { None } else { Some(t) }
	};
	Ok(RetainSummary { contents, summary })
}

/// Reflect result summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectSummary {
	pub query: Option<String>,
	pub answer_lines: Vec<String>,
}

/// Build reflect summary.
pub fn summarize_reflect(query: Option<&str>, result_text: &str, is_error: bool) -> Result<ReflectSummary, String> {
	let answer = result_text.trim().to_string();
	if is_error {
		return Err(if answer.is_empty() { "Reflect failed".to_string() } else { answer });
	}
	let answer_lines = answer.lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect();
	Ok(ReflectSummary {
		query: query.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
		answer_lines,
	})
}

/// Query header plain label: `Title: query`.
pub fn query_header_plain(title: &str, query: Option<&str>, meta: Option<&str>) -> String {
	let q = replace_tabs(query.unwrap_or("").trim());
	let q = if q.is_empty() { None } else { Some(truncate_preview(&q, 80)) };
	match (q, meta) {
		(Some(q), Some(m)) => format!("{title}: {q} · {m}"),
		(Some(q), None) => format!("{title}: {q}"),
		(None, Some(m)) => format!("{title} · {m}"),
		(None, None) => title.to_string(),
	}
}

/// Plain retain render lines (frameless).
pub fn retain_render_lines(contents: &[String], header: &str, expanded: bool) -> Vec<String> {
	let mut lines = vec![header.to_string()];
	let limit = if expanded { contents.len() } else { PREVIEW_LIMITS_COLLAPSED_ITEMS };
	let shown = contents.iter().take(limit);
	for content in shown {
		lines.push(format!("  • {}", truncate_preview(content, 120)));
	}
	let remaining = contents.len().saturating_sub(limit.min(contents.len()));
	if remaining > 0 {
		lines.push(format!("  … {remaining} more"));
	}
	lines
}

/// Native describe for retain call/result.
pub fn describe_retain(contents: &[String], summary: Option<&str>, is_error: bool, error_text: Option<&str>) -> NativeToolView {
	if is_error {
		return NativeToolView { ..error_view("Retain", error_text.unwrap_or("Retain failed"), &[summary.map(|s| s.to_string())]) };
	}
	let head_details = summary.map(|s| vec![crate::tools::native_view::ToolHeadDetail::Text(s.to_string())]).unwrap_or_default();
	let body = if contents.is_empty() {
		vec![]
	} else {
		let items: Vec<crate::native::node::NativeNode> = contents
			.iter()
			.enumerate()
			.map(|(i, content)| {
				let mut p = Props::new();
				p.insert("label", serde_json::Value::String(plain_text(content)));
				nitem(i.to_string(), p)
			})
			.collect();
		vec![nlist(items, Props::new(), None)]
	};
	NativeToolView {
		head: Some(NodeText::Spans(tool_head("Retain", &head_details))),
		inline: Some(true),
		body: Some(body),
		preview: Some(NativePreview::Lines(PREVIEW_LIMITS_COLLAPSED_ITEMS)),
		..NativeToolView::default()
	}
}

/// Native describe for recall call.
pub fn describe_recall_call(query: Option<&str>) -> NativeToolView {
	let details = query.map(|q| vec![crate::tools::native_view::ToolHeadDetail::Text(q.trim().to_string())]).unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("Recall", &details))), inline: Some(true), ..NativeToolView::default() }
}

/// Native describe for recall result.
pub fn describe_recall_result(query: Option<&str>, result_text: &str, is_error: bool) -> NativeToolView {
	if is_error {
		return NativeToolView { ..error_view("Recall", &result_text.trim().if_empty("Recall failed"), &[query.map(|s| s.to_string())]) };
	}
	let found = parse_recall_found(result_text.trim());
	if found == 0 {
		let details = [
			query.map(|q| q.trim().to_string()),
			Some("no matches".to_string()),
		].into_iter().flatten().map(crate::tools::native_view::ToolHeadDetail::Text).collect::<Vec<_>>();
		return NativeToolView { head: Some(NodeText::Spans(tool_head("Recall", &details))), tone: Some(Tone::Warning), inline: Some(true), ..NativeToolView::default() };
	}
	let details = [
		query.map(|q| q.trim().to_string()),
		Some(format!("{found} found")),
	].into_iter().flatten().map(crate::tools::native_view::ToolHeadDetail::Text).collect::<Vec<_>>();
	let body_text = if let Some(pos) = result_text.trim().find('\n') {
		result_text.trim()[pos + 1..].trim_start_matches('\n').lines().take(PREVIEW_LIMITS_OUTPUT_EXPANDED).collect::<Vec<_>>().join("\n")
	} else { String::new() };
	let body = if body_text.trim().is_empty() {
		vec![]
	} else {
		let mut p = Props::new();
		p.set_str("wrap", "word");
		vec![ntext(NodeText::Spans(vec![Span::styled(body_text, "muted")]), p)]
	};
	NativeToolView { head: Some(NodeText::Spans(tool_head("Recall", &details))), inline: Some(true), body: Some(body), preview: Some(NativePreview::Lines(0)), ..NativeToolView::default() }
}

/// Native describe for reflect call.
pub fn describe_reflect_call(query: Option<&str>) -> NativeToolView {
	let details = query.map(|q| vec![crate::tools::native_view::ToolHeadDetail::Text(q.trim().to_string())]).unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("Reflect", &details))), inline: Some(true), ..NativeToolView::default() }
}

/// Native describe for reflect result.
pub fn describe_reflect_result(query: Option<&str>, result_text: &str, is_error: bool) -> NativeToolView {
	let answer = result_text.trim().to_string();
	if is_error {
		return NativeToolView { ..error_view("Reflect", if answer.is_empty() { "Reflect failed" } else { &answer }, &[query.map(|s| s.to_string())]) };
	}
	let details = query.map(|q| vec![crate::tools::native_view::ToolHeadDetail::Text(q.trim().to_string())]).unwrap_or_default();
	let shown = answer.lines().take(PREVIEW_LIMITS_OUTPUT_EXPANDED).collect::<Vec<_>>().join("\n").trim().to_string();
	let body = if shown.is_empty() {
		vec![]
	} else {
		let mut p = Props::new();
		p.set_str("role", "omp.memory.reflect");
		vec![md(shown, p)]
	};
	NativeToolView { head: Some(NodeText::Spans(tool_head("Reflect", &details))), inline: Some(true), body: Some(body), preview: Some(NativePreview::Lines(PREVIEW_LIMITS_OUTPUT_COLLAPSED)), ..NativeToolView::default() }
}

trait IfEmpty {
	fn if_empty(self, fallback: &str) -> String;
}
impl IfEmpty for &str {
	fn if_empty(self, fallback: &str) -> String {
		if self.trim().is_empty() { fallback.to_string() } else { self.to_string() }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn retain_contents_extracts() {
		let args = RetainRenderArgs { items: Some(json!([{ "content": "  hello\tworld  " }, { "content": "   " }, { "nope": 1 }, "bad"])) };
		assert_eq!(retain_contents(Some(&args)), vec!["hello world".to_string()]);
	}

	#[test]
	fn parse_recall_found_count() {
		assert_eq!(parse_recall_found("Found 3 relevant memories:\n- a"), 3);
		assert_eq!(parse_recall_found("Found 0 relevant memories."), 0);
		assert_eq!(parse_recall_found("Nope"), 0);
	}

	#[test]
	fn summarize_recall_strips_header() {
		let s = summarize_recall(Some("q"), "Found 2 relevant things:\n\nline1\nline2", false).unwrap();
		assert_eq!(s.found, 2);
		assert!(s.body.contains("line1"));
	}

	#[test]
	fn summarize_retain_strips_period() {
		let s = summarize_retain(None, "2 memories stored.", false).unwrap();
		assert_eq!(s.summary.as_deref(), Some("2 memories stored"));
	}

	#[test]
	fn summarize_reflect_filters_empty_lines() {
		let s = summarize_reflect(Some("why"), "a\n\nb\n  \n", false).unwrap();
		assert_eq!(s.answer_lines, vec!["a".to_string(), "b".to_string()]);
	}

	#[test]
	fn render_and_describe_helpers() {
		let lines = retain_render_lines(&["m1".into(), "m2".into()], "Retain · 2 memories stored", false);
		assert!(lines[0].contains("Retain"));
		let v = describe_retain(&["m1".into()], Some("2 memories stored"), false, None);
		assert!(v.inline.unwrap_or(false));
		let r = describe_recall_result(Some("q"), "Found 1 relevant:\n- x", false);
		assert!(r.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
	}
}
