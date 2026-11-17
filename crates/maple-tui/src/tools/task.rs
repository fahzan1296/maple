//! Task/subagent tool render-model types and helpers.
//!
 //! Reference: `l/src/tools/task.ts`.
 //!
 //! Deviations:
//! - JS-runtime-only facilities dropped: TUI `Component`/`Text`/`Markdown`
//!   renderers, cached/memoized rows, theme/spinner/width-aware layout,
//!   streaming progress trees, nested task recursion/cycle UI, subprocess
//!   renderer integration, and interactive expansion state. Module-doc only;
//!   parity provided via plain/native render+describe helpers.
 //! - TS is a very large TUI renderer (~2.6k lines): nested task progress,
 //!   streaming agent rows, markdown/JSON output rendering, reviewer finding
//!   assembly, native describe/memos. Rust ports core data contracts and pure
//!   helper logic plus exported render/describe helpers built on native
//!   describe primitives.
use serde_json::Value;

/// Priority labels for review findings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingPriority {
	P0,
	P1,
	P2,
	P3,
}

impl FindingPriority {
	pub fn parse(s: &str) -> Option<Self> {
		Some(match s {
			"P0" => Self::P0,
			"P1" => Self::P1,
			"P2" => Self::P2,
			"P3" => Self::P3,
			_ => return None,
		})
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::P0 => "P0",
			Self::P1 => "P1",
			Self::P2 => "P2",
			Self::P3 => "P3",
		}
	}
}

/// Finding details (reviewer output).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindingDetails {
	pub title: String,
	pub body: Option<String>,
	pub priority: Option<FindingPriority>,
	pub confidence: Option<f64>,
	pub file_path: Option<String>,
	pub line_start: Option<u32>,
	pub line_end: Option<u32>,
}

/// Parsed finding from untrusted value.
pub fn parse_finding_details(value: &Value) -> Option<FindingDetails> {
	let obj = value.as_object()?;
	let title = obj.get("title").and_then(|v| v.as_str())?.to_string();
	let priority = obj
		.get("priority")
		.and_then(|v| v.as_str())
		.and_then(FindingPriority::parse);
	Some(FindingDetails {
		title,
		body: obj.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()),
		priority,
		confidence: obj.get("confidence").and_then(|v| v.as_f64()),
		file_path: obj.get("file_path").and_then(|v| v.as_str()).map(|s| s.to_string()),
		line_start: obj.get("line_start").and_then(|v| v.as_u64()).map(|n| n as u32),
		line_end: obj.get("line_end").and_then(|v| v.as_u64()).map(|n| n as u32),
	})
}

/// Normalize findings array.
pub fn normalize_findings(value: Option<&Value>) -> Vec<FindingDetails> {
	let Some(arr) = value.and_then(|v| v.as_array()) else {
		return Vec::new();
	};
	arr.iter().filter_map(parse_finding_details).collect()
}

/// Submit-review summary fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubmitReviewDetails {
	pub overall_correctness: Option<String>, // "correct" | "incorrect"
	pub explanation: Option<String>,
	pub confidence: Option<f64>,
}

/// Render yield item (normalized).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderYieldItem {
	pub data: Option<Value>,
	pub r#type: Option<YieldTypeValue>,
	pub status: Option<String>,
	pub use_last_turn: bool,
}

/// Yield type value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YieldTypeValue {
	String(String),
	Array(Vec<String>),
}

/// Normalize `yield` slot into array of yield items.
pub fn normalize_yield_data(value: Option<&Value>) -> Vec<RenderYieldItem> {
	let Some(v) = value else {
		return Vec::new();
	};
	let items: Vec<&Value> = if let Some(arr) = v.as_array() {
		arr.iter().collect()
	} else if v.is_object() {
		vec![v]
	} else {
		return Vec::new();
	};
	let mut out = Vec::new();
	for item in items {
		let Some(obj) = item.as_object() else {
			continue;
		};
		let r#type = match obj.get("type") {
			Some(Value::String(s)) => Some(YieldTypeValue::String(s.clone())),
			Some(Value::Array(a)) => {
				let labels: Option<Vec<String>> = a
					.iter()
					.map(|x| x.as_str().map(|s| s.to_string()))
					.collect();
				labels.map(YieldTypeValue::Array)
			}
			_ => None,
		};
		out.push(RenderYieldItem {
			data: obj.get("data").cloned(),
			r#type,
			status: obj.get("status").and_then(|x| x.as_str()).map(|s| s.to_string()),
			use_last_turn: obj.get("useLastTurn").and_then(|x| x.as_bool()).unwrap_or(false),
		});
	}
	out
}

/// Get render yield labels from type.
pub fn get_render_yield_labels(t: Option<&YieldTypeValue>) -> Vec<String> {
	match t {
		Some(YieldTypeValue::String(s)) => {
			let label = s.trim();
			if label.is_empty() { Vec::new() } else { vec![label.to_string()] }
		}
		Some(YieldTypeValue::Array(v)) => v
			.iter()
			.map(|s| s.trim().to_string())
			.filter(|s| !s.is_empty())
			.collect(),
		None => Vec::new(),
	}
}

/// Task agent run status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskRunStatus {
	Pending,
	Running,
	Completed,
	Failed,
	Cancelled,
}

/// High-level task details snapshot (subset used by renderer logic).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskToolDetails {
	pub status: Option<TaskRunStatus>,
	pub agent: Option<String>,
	pub model: Option<String>,
	pub description: Option<String>,
	pub duration_ms: Option<u64>,
	pub error: Option<String>,
	pub findings: Vec<FindingDetails>,
}

/// Plain render of a task call (agent/task preview).
pub fn render_call(agent: Option<&str>, task: Option<&str>, batch_count: Option<usize>) -> String {
	let who = agent.filter(|s| !s.trim().is_empty()).unwrap_or("task");
	let mut out = who.to_string();
	if let Some(n) = batch_count.filter(|n| *n > 1) {
		out.push_str(&format!(" ({n} agents)"));
	}
	if let Some(t) = task.map(str::trim).filter(|s| !s.is_empty()) {
		let first = t.lines().next().unwrap_or(t);
		out.push_str(": ");
		out.push_str(first);
	}
	out
}

/// Plain render of a task result summary.
pub fn render_result(details: Option<&TaskToolDetails>, output: &str, is_error: bool) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		return vec![if msg.is_empty() { "Task failed".into() } else { format!("Task failed: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	let mut lines = Vec::new();
	if output.trim().is_empty() {
		if lines.is_empty() {
			lines.push("No output".into());
		}
	} else {
		for l in output.trim_end().lines().take(40) {
			lines.push(l.to_string());
		}
		let n = output.lines().count();
		if n > 40 {
			lines.push(format!("… {n} lines total"));
		}
	}
	lines
}

/// Native describe of a task call.
pub fn describe_call(agent: Option<&str>, task: Option<&str>, batch_count: Option<usize>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let title = agent.filter(|s| !s.trim().is_empty()).unwrap_or("Task");
	let mut details = Vec::new();
	if let Some(n) = batch_count.filter(|n| *n > 1) {
		details.push(ToolHeadDetail::Span(Span::styled(format!("{n} agents"), "muted")));
	}
	if let Some(t) = task.map(str::trim).filter(|s| !s.is_empty()) {
		details.push(ToolHeadDetail::Span(Span::styled(t.lines().next().unwrap_or(t).to_string(), "accent")));
	}
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(title, &details))),
		tone: Some(Tone::Accent),
		..NativeToolView::default()
	}
}

/// Native describe of a task result.
pub fn describe_result(
	details: Option<&TaskToolDetails>,
	output: &str,
	is_error: bool,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	if is_error {
		let msg = output.trim();
		return error_view("Task", if msg.is_empty() { "failed" } else { msg }, &[]);
	}
	let head = tool_head("Task", &[]);
	let body_text = output.trim_end();
	if body_text.is_empty() {
		return NativeToolView {
			head: Some(NodeText::Spans(head)),
			body: Some(vec![note_text("No output", "dim", None)]),
			tone: Some(Tone::Success),
			..NativeToolView::default()
		};
	}
	let mut p = Props::new();
	p.set_str("wrap", "word");
	NativeToolView {
		head: Some(NodeText::Spans(head)),
		body: Some(vec![ntext(NodeText::Plain(body_text.to_string()), p)]),
		tone: Some(Tone::Success),
		..NativeToolView::default()
	}
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let c = render_call(Some("scout"), Some("find files\nmore"), None);
		assert!(c.contains("scout"));
		let v = describe_call(Some("scout"), Some("find files"), Some(2));
		assert!(v.head.is_some());
		let lines = render_result(None, "ok", false);
		assert_eq!(lines[0], "ok");
		let r = describe_result(None, "ok", false);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn parse_and_normalize_findings() {
		let v = json!([
			{"title": "bug", "priority": "P1", "confidence": 0.9},
			{"nope": 1}
		]);
		let f = normalize_findings(Some(&v));
		assert_eq!(f.len(), 1);
		assert_eq!(f[0].title, "bug");
		assert_eq!(f[0].priority, Some(FindingPriority::P1));
	}

	#[test]
	fn normalize_yield_data_shapes() {
		let v = json!([{"type": "a", "data": 1, "useLastTurn": true}, {"type": ["x", "y"]}]);
		let items = normalize_yield_data(Some(&v));
		assert_eq!(items.len(), 2);
		assert!(items[0].use_last_turn);
		assert_eq!(get_render_yield_labels(items[1].r#type.as_ref()), vec!["x", "y"]);
		let single = normalize_yield_data(Some(&json!({"type":"z"})));
		assert_eq!(single.len(), 1);
		assert!(normalize_yield_data(Some(&json!(null))).is_empty());
	}
}
