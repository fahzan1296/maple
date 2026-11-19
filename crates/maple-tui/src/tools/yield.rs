//! Submission/yield tool render-model types and pure helpers.
//!
 //! Reference: `l/src/tools/yield.ts`.
 //!
 //! Deviations:
//! - JS-runtime-only facilities dropped: TUI `Component` renderers,
//!   `OwnerMemo`/cached views, ANSI/theme styling, interactive expansion,
//!   JSON tree/output-pane rendering components. Module-doc only; parity
//!   provided via plain/native render+describe helpers.
 //! - The TS module is a transcript renderer (TUI `Component`, `Theme`,
 //!   `OwnerMemo`, native describe nodes, JSON tree rendering, output pane).
 //!   Those rendering systems are not ported here; this file provides the
//!   argument/result data shapes, pure submission-derivation logic
//!   (`read_submission`, `size_meta`), and exported render/describe helpers.
 //! - `unknown` data is `serde_json::Value` (`Option<Value>` where absent).
use serde_json::Value;

/// Title shown for the yield tool.
pub const TITLE: &str = "Submit Result";

/// Collapsed rows of string data (a report) in the ANSI card.
pub const TEXT_ROWS_COLLAPSED: usize = 4;

/// Yield `type` as sent/rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YieldTypeArg {
	/// Single terminal type label.
	String(String),
	/// Incremental section labels.
	Array(Vec<String>),
}

/// `yield` arguments as sent; strict-mode providers may send omitted optionals
/// as `null` (represented as `None`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YieldRenderArgs {
	/// Section/result type.
	pub r#type: Option<YieldTypeArg>,
	/// Submitted data.
	pub data: Option<Value>,
	/// Reported error.
	pub error: Option<String>,
	/// 1-based workpool item number.
	pub key: Option<u64>,
}

/// Minimal settled result view needed to derive submission display state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YieldResultView {
	/// Settled details (normalized), when not an error.
	pub details: Option<YieldResultDetails>,
	/// Whether the tool errored.
	pub is_error: bool,
	/// Fallback text (e.g. from result content).
	pub result_text: Option<String>,
}

/// Settled yield details used by the renderer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YieldResultDetails {
	/// Normalized type.
	pub r#type: Option<YieldTypeArg>,
	/// Normalized data.
	pub data: Option<Value>,
	/// Status (`"aborted"` recognized).
	pub status: Option<String>,
	/// Error message.
	pub error: Option<String>,
	/// Accepted although it failed output schema.
	pub schema_overridden: bool,
}

/// One `yield` call as its card shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Submission {
	/// Section labels or workpool item; absent for a whole result.
	pub target: Option<String>,
	/// Accepted/normalized data once settled, else data as sent.
	pub data: Option<Value>,
	/// Failure reported or rejection reason.
	pub error: Option<String>,
	/// Whether settled.
	pub settled: bool,
	/// Accepted although schema-overridden.
	pub overridden: bool,
	/// Note for settled calls without data.
	pub note: Option<String>,
}

fn is_incremental(t: Option<&YieldTypeArg>) -> bool {
	matches!(t, Some(YieldTypeArg::Array(v)) if !v.is_empty())
}

fn join_labels(v: &[String]) -> String {
	v.join(", ")
}

/// Head fact for container data (`"13 fields"`, `"4 items"`).
pub fn size_meta(data: Option<&Value>) -> Option<String> {
	match data {
		Some(Value::Array(a)) => Some(format_count("item", a.len())),
		Some(Value::Object(o)) => Some(format_count("field", o.len())),
		_ => None,
	}
}

fn format_count(kind: &str, n: usize) -> String {
	if n == 1 {
		format!("1 {kind}")
	} else {
		format!("{n} {kind}s")
	}
}

/// Derive display submission state from args and optional settled result.
pub fn read_submission(
	args: Option<&YieldRenderArgs>,
	result: Option<&YieldResultView>,
) -> Submission {
	let details = result
		.filter(|r| !r.is_error)
		.and_then(|r| r.details.as_ref());
	let r#type = details
		.and_then(|d| d.r#type.as_ref())
		.or_else(|| args.and_then(|a| a.r#type.as_ref()));
	let incremental = is_incremental(r#type);
	let data = details
		.and_then(|d| d.data.clone())
		.or_else(|| args.and_then(|a| a.data.clone()));

	let mut error = args.and_then(|a| a.error.clone()).filter(|s| !s.is_empty());
	if let Some(res) = result {
		if res.is_error {
			error = res
				.result_text
				.clone()
				.filter(|s| !s.is_empty())
				.or(Some(format!("{TITLE} failed")));
		} else if details.and_then(|d| d.status.as_deref()) == Some("aborted") {
			error = details
				.and_then(|d| d.error.clone().filter(|s| !s.is_empty()))
				.or_else(|| res.result_text.clone().filter(|s| !s.is_empty()))
				.or(Some("Aborted".to_string()));
		}
	}

	let settled = result.is_some();
	let overridden = details.map(|d| d.schema_overridden).unwrap_or(false);
	let target = args.and_then(|a| a.key).map(|k| format!("item {k}")).or_else(|| {
		match r#type {
			Some(YieldTypeArg::Array(v)) if !v.is_empty() => Some(join_labels(v)),
			_ => None,
		}
	});
	let note = if settled && error.is_none() && data.is_none() {
		Some(if incremental {
			"Takes the last message as this section".to_string()
		} else {
			"Ends with the sections submitted earlier, or the last message".to_string()
		})
	} else {
		None
	};

	Submission {
		target,
		data,
		error,
		settled,
		overridden,
		note,
	}
}

/// Plain render of a yield call/submission.
pub fn render_call(args: Option<&YieldRenderArgs>, result: Option<&YieldResultView>) -> String {
	let s = read_submission(args, result);
	if let Some(err) = s.error {
		if !err.trim().is_empty() { return format!("{TITLE}: error"); }
	}
	if let Some(t) = s.target.as_ref().filter(|x| !x.is_empty()) { return format!("{TITLE}: {t}"); }
	if let Some(n) = s.note.as_ref().filter(|x| !x.is_empty()) { return format!("{TITLE}: {n}"); }
	if let Some(m) = size_meta(s.data.as_ref()) { return format!("{TITLE}: {m}"); }
	TITLE.to_string()
}

/// Plain render of a yield result.
pub fn render_result(args: Option<&YieldRenderArgs>, result: Option<&YieldResultView>) -> Vec<String> {
	let s = read_submission(args, result);
	let mut lines = Vec::new();
	if let Some(err) = s.error {
		lines.push(if err.trim().is_empty() { "error".into() } else { format!("error: {}", err.lines().next().unwrap_or(&err)) });
		return lines;
	}
	if let Some(d) = s.data {
		match d {
			Value::String(t) => {
				if t.trim().is_empty() { lines.push("(empty)".into()); } else { lines.extend(t.lines().take(60).map(|l| l.to_string())); }
			}
			other => {
				let text = serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string());
				lines.extend(text.lines().take(60).map(|l| l.to_string()));
			}
		}
	} else {
		lines.push("No data".into());
	}
	lines
}

/// Native describe of a yield call.
pub fn describe_call(args: Option<&YieldRenderArgs>, result: Option<&YieldResultView>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let s = read_submission(args, result);
	let mut details = Vec::new();
	if let Some(t) = s.target.as_ref().filter(|x| !x.is_empty()) {
		details.push(ToolHeadDetail::Span(Span::styled(t.clone(), "accent")));
	} else if let Some(m) = size_meta(s.data.as_ref()) {
		details.push(ToolHeadDetail::Span(Span::styled(m, "muted")));
	}
	let tone = if s.error.is_some() { Some(Tone::Error) } else { Some(Tone::Success) };
	NativeToolView { head: Some(NodeText::Spans(tool_head(TITLE, &details))), tone, ..NativeToolView::default() }
}

/// Native describe of a yield result.
pub fn describe_result(args: Option<&YieldRenderArgs>, result: Option<&YieldResultView>) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	let s = read_submission(args, result);
	if let Some(err) = s.error {
		let msg = if err.trim().is_empty() { "failed" } else { err.trim() };
		return error_view(TITLE, msg, &[]);
	}
	let mut details = Vec::new();
	if let Some(t) = s.target.as_ref().filter(|x| !x.is_empty()) {
		details.push(ToolHeadDetail::Span(Span::styled(t.clone(), "accent")));
	} else if let Some(m) = size_meta(s.data.as_ref()) {
		details.push(ToolHeadDetail::Span(Span::styled(m, "muted")));
	}
	let head = tool_head(TITLE, &details);
	if let Some(d) = s.data {
		let body_text = match d {
			Value::String(t) => t,
			other => serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string()),
		};
		if body_text.trim().is_empty() {
			return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("(empty)", "dim", None)]), ..NativeToolView::default() };
		}
		let mut p = Props::new();
		p.set_str("wrap", "word");
		return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ntext(NodeText::Plain(body_text), p)]), tone: Some(Tone::Success), ..NativeToolView::default() };
	}
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("No data", "dim", None)]), tone: Some(Tone::Success), ..NativeToolView::default() }
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let args = YieldRenderArgs { data: Some(Value::String("hi".into())), ..Default::default() };
		let c = render_call(Some(&args), None);
		assert!(c.contains(TITLE));
		let v = describe_call(Some(&args), None);
		assert!(v.head.is_some());
		let lines = render_result(Some(&args), None);
		assert_eq!(lines[0], "hi");
		let r = describe_result(Some(&args), None);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn size_meta_counts() {
		assert_eq!(size_meta(Some(&json!([1, 2, 3]))), Some("3 items".into()));
		assert_eq!(size_meta(Some(&json!({"a": 1}))), Some("1 field".into()));
		assert_eq!(size_meta(Some(&json!("x"))), None);
	}

	#[test]
	fn read_submission_pending_incremental_target() {
		let args = YieldRenderArgs {
			r#type: Some(YieldTypeArg::Array(vec!["a".into(), "b".into()])),
			data: Some(json!("hi")),
			..Default::default()
		};
		let s = read_submission(Some(&args), None);
		assert_eq!(s.target.as_deref(), Some("a, b"));
		assert!(!s.settled);
		assert!(s.note.is_none());
	}

	#[test]
	fn read_submission_workpool_key_target() {
		let args = YieldRenderArgs {
			key: Some(2),
			data: Some(json!(1)),
			..Default::default()
		};
		let s = read_submission(Some(&args), None);
		assert_eq!(s.target.as_deref(), Some("item 2"));
	}

	#[test]
	fn read_submission_settled_no_data_note_incremental() {
		let args = YieldRenderArgs {
			r#type: Some(YieldTypeArg::Array(vec!["sec".into()])),
			..Default::default()
		};
		let res = YieldResultView::default();
		let s = read_submission(Some(&args), Some(&res));
		assert!(s.settled);
		assert_eq!(
			s.note.as_deref(),
			Some("Takes the last message as this section")
		);
	}

	#[test]
	fn read_submission_error_result() {
		let res = YieldResultView {
			is_error: true,
			result_text: Some("boom".into()),
			..Default::default()
		};
		let s = read_submission(None, Some(&res));
		assert_eq!(s.error.as_deref(), Some("boom"));
		assert!(s.settled);
	}
}
