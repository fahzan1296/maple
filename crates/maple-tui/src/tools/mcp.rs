//! MCP tool render-model types and helpers.
//!
 //! Reference: `l/src/tools/mcp.ts`.
 //!
 //! Deviations:
//! - JS-runtime-only facilities dropped: TUI `Component`/`Markdown` renderers,
//!   cached/memoized result views, ANSI theme styling, interactive truncation/
//!   expansion UI and markdown-preference runtime state. Module-doc only;
//!   parity provided via plain/native render+describe helpers.
 //! - TS renders TUI components/native views (Markdown, JSON tree, themed cards,
 //!   status lines, truncation styling). Rust ports MCP content/progress/detail
//!   types, name parsing, progress formatting/parsing, visibility helpers, and
//!   exported render/describe helpers.
use serde_json::Value;
use std::collections::BTreeMap;

/// Registry prefix every minted MCP tool name carries.
pub const MCP_TOOL_NAME_PREFIX: &str = "mcp__";

/// MCP text content block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpTextContent {
	pub text: String,
}

/// MCP image content block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpImageContent {
	pub data: String,
	pub mime_type: String,
}

/// MCP resource content block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpResourceContent {
	pub uri: String,
	pub mime_type: Option<String>,
	pub text: Option<String>,
	pub blob: Option<String>,
}

/// Supported MCP result content blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpContent {
	Text(McpTextContent),
	Image(McpImageContent),
	Resource(McpResourceContent),
}

/// Server-reported progress of an in-flight MCP request.
#[derive(Clone, Debug, PartialEq)]
pub struct McpProgress {
	pub progress: f64,
	pub total: Option<f64>,
	pub message: Option<String>,
}

/// Read MCP progress fields from an untrusted JSON value.
pub fn parse_mcp_progress(value: &Value) -> Option<McpProgress> {
	let obj = value.as_object()?;
	let progress = obj.get("progress")?.as_f64()?;
	if !progress.is_finite() {
		return None;
	}
	let total = obj
		.get("total")
		.and_then(|v| v.as_f64())
		.filter(|v| v.is_finite());
	let message = obj.get("message").and_then(|v| v.as_str()).map(|s| s.to_string());
	Some(McpProgress {
		progress,
		total,
		message,
	})
}

fn truncate_chars(s: &str, max: usize) -> String {
	let count = s.chars().count();
	if count <= max {
		s.to_string()
	} else if max <= 1 {
		"…".to_string()
	} else {
		let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
		out.push('…');
		out
	}
}

/// One-line display text for MCP progress.
pub fn format_mcp_progress(progress: &McpProgress, max_width: usize) -> String {
	let message = progress
		.message
		.as_deref()
		.map(|m| m.replace(['\n', '\r', '\t'], " ").split_whitespace().collect::<Vec<_>>().join(" "))
		.unwrap_or_default();
	let amount = if let Some(total) = progress.total {
		format!("{}/{}", trim_num(progress.progress), trim_num(total))
	} else {
		trim_num(progress.progress)
	};
	if message.is_empty() {
		return truncate_chars(&amount, max_width);
	}
	if progress.total.is_none() {
		return truncate_chars(&message, max_width);
	}
	let suffix = format!(" ({amount})");
	let msg_w = max_width.saturating_sub(suffix.chars().count());
	if msg_w > 0 {
		format!("{}{}", truncate_chars(&message, msg_w), suffix)
	} else {
		truncate_chars(&amount, max_width)
	}
}

fn trim_num(n: f64) -> String {
	if (n.fract()).abs() < f64::EPSILON {
		format!("{}", n as i64)
	} else {
		n.to_string()
	}
}

/// MCP result details shared by renderers and consumers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpToolDetails {
	pub server_name: String,
	pub mcp_tool_name: String,
	pub is_error: bool,
	pub raw_content: Vec<McpContent>,
	pub structured_content: Option<BTreeMap<String, Value>>,
	pub mcp_meta: Option<BTreeMap<String, Value>>,
	pub provider: Option<String>,
	pub provider_name: Option<String>,
	pub progress: Option<McpProgress>,
}

/// Parse an MCP tool name back to server and tool components.
pub fn parse_mcp_tool_name(name: &str) -> Option<(String, String)> {
	let rest = name.strip_prefix(MCP_TOOL_NAME_PREFIX)?;
	let idx = rest.find('_')?;
	Some((rest[..idx].to_string(), rest[idx + 1..].to_string()))
}

/// Visible MCP argument entries (drop intent/partial bookkeeping keys).
pub fn visible_mcp_args(args: Option<&serde_json::Map<String, Value>>) -> Vec<(String, Value)> {
	let Some(args) = args else {
		return Vec::new();
	};
	args.iter()
		.filter(|(k, _)| k.as_str() != "i" && k.as_str() != "__partialJson")
		.map(|(k, v)| (k.clone(), v.clone()))
		.collect()
}

/// A JSON value drawn by hosts that render JSON natively.
fn json_node(value: &Value) -> crate::native::node::NativeNode {
	use crate::native::node::{NativeNode, NodeKind, Props};
	let mut props = Props::new();
	props.insert("value", value.clone());
	NativeNode::with_props(NodeKind::Json, props)
}

/// Drop host bookkeeping keys from a JSON result payload.
fn strip_hidden_json(value: Value) -> Value {
	match value {
		Value::Object(mut map) => {
			map.remove("__partialJson");
			Value::Object(map)
		}
		other => other,
	}
}

/// Folded `args` section carrying every visible argument value.
fn args_section(
	args: Option<&serde_json::Map<String, Value>>,
) -> Option<crate::native::node::NativeNode> {
	use crate::native::node::{NativeNode, NodeKind, Props};
	let visible = visible_mcp_args(args);
	if visible.is_empty() {
		return None;
	}
	let mut map = serde_json::Map::new();
	for (key, value) in visible {
		map.insert(key, value);
	}
	let mut props = Props::new();
	props.set_bool("collapsible", true);
	props.set_bool("collapsed", true);
	let mut section = NativeNode::with_props(NodeKind::Section, props);
	section.key = Some("args".to_string());
	section.children.push(json_node(&Value::Object(map)));
	Some(section)
}

/// Plain render of an MCP call.
pub fn render_call(label: &str, args: Option<&serde_json::Map<String, serde_json::Value>>) -> String {
	let visible = visible_mcp_args(args);
	if visible.is_empty() {
		label.to_string()
	} else {
		let preview: Vec<String> = visible
			.iter()
			.take(4)
			.map(|(k, v)| format!("{k}={}", summarize_value(v, 40)))
			.collect();
		format!("{label} {}", preview.join(", "))
	}
}

/// Plain render of an MCP result.
pub fn render_result(output: &str, is_error: bool, details: Option<&McpToolDetails>) -> Vec<String> {
	let mut lines = Vec::new();
	if is_error {
		let msg = output.trim();
		lines.push(if msg.is_empty() { "MCP failed".into() } else { format!("MCP failed: {}", msg.lines().next().unwrap_or(msg)) });
	} else if output.trim().is_empty() {
		lines.push("No output".into());
	} else {
		lines.extend(output.trim_end().lines().take(80).map(|s| s.to_string()));
		let total = output.lines().count();
		if total > 80 {
			lines.push(format!("… {total} lines total"));
		}
	}
	if let Some(d) = details {
		let truncation = d
			.mcp_meta
			.as_ref()
			.and_then(|meta| meta.get("truncation"))
			.and_then(|value| value.as_str());
		if let Some(t) = truncation {
			lines.push(format!("truncated: {t}"));
		}
	}
	lines
}

/// Native describe of an MCP call (exported parity with `describeMCPCall`).
pub fn describe_call(
	args: Option<&serde_json::Map<String, serde_json::Value>>,
	label: &str,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let visible = visible_mcp_args(args);
	let mut details = Vec::new();
	if !visible.is_empty() {
		let preview = visible
			.iter()
			.take(3)
			.map(|(k, v)| format!("{k}={}", summarize_value(v, 24)))
			.collect::<Vec<_>>()
			.join(", ");
		details.push(ToolHeadDetail::Span(Span::styled(preview, "muted")));
	}
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(label, &details))),
		body: args_section(args).map(|section| vec![section]),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native describe of an MCP result (exported parity with `describeMCPResult`).
pub fn describe_result(
	output: &str,
	is_error: bool,
	label: Option<&str>,
	details: Option<&McpToolDetails>,
	args: Option<&serde_json::Map<String, Value>>,
	json: bool,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, footnote_text, note_text, tool_head, NativeToolView, ToolHeadDetail};
	let title = label.unwrap_or("MCP");
	if is_error {
		let msg = output.trim();
		return error_view(title, if msg.is_empty() { "failed" } else { msg }, &[]);
	}
	let head = tool_head(title, &[]);
	let body_text = output.trim_end();
	if body_text.is_empty() {
		let mut body = vec![note_text("No output", "dim", None)];
		if let Some(section) = args_section(args) {
			body.push(section);
		}
		return NativeToolView {
			head: Some(NodeText::Spans(head)),
			body: Some(body),
			..NativeToolView::default()
		};
	}
	let mut p = Props::new();
	p.set_str("wrap", "word");
	let mut body = if json {
		match serde_json::from_str::<Value>(body_text) {
			Ok(parsed) => vec![json_node(&strip_hidden_json(parsed))],
			Err(_) => vec![ntext(NodeText::Plain(body_text.to_string()), p)],
		}
	} else {
		vec![ntext(NodeText::Plain(body_text.to_string()), p)]
	};
	if let Some(d) = details {
		if d
			.mcp_meta
			.as_ref()
			.and_then(|meta| meta.get("truncation"))
			.is_some()
		{
			if let Some(foot) = footnote_text(&vec!["output truncated".into()], None) {
				body.push(foot);
			}
		}
	}
	if let Some(section) = args_section(args) {
		body.push(section);
	}
	NativeToolView {
		head: Some(NodeText::Spans(head)),
		body: Some(body),
		tone: Some(Tone::Success),
		..NativeToolView::default()
	}
}

fn summarize_value(v: &serde_json::Value, max: usize) -> String {
	let s = match v {
		serde_json::Value::String(s) => s.clone(),
		_ => v.to_string(),
	};
	if s.chars().count() <= max {
		s
	} else {
		let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
		out.push('…');
		out
	}
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let mut map = serde_json::Map::new();
		map.insert("q".into(), serde_json::Value::String("hello".into()));
		let c = render_call("mcp__x__y", Some(&map));
		assert!(c.contains("q="));
		let v = describe_call(Some(&map), "mcp__x__y");
		assert!(v.head.is_some());
		let r = render_result("ok", false, None);
		assert_eq!(r[0], "ok");
		let d = describe_result("ok", false, Some("mcp__x__y"), None, Some(&map), false);
		assert!(d.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn parse_tool_name() {
		assert_eq!(
			parse_mcp_tool_name("mcp__srv_tool_x"),
			Some(("srv".into(), "tool_x".into()))
		);
		assert_eq!(parse_mcp_tool_name("nope"), None);
	}

	#[test]
	fn progress_parse_and_format() {
		let p = parse_mcp_progress(&json!({"progress": 2, "total": 5, "message": "hi"})).unwrap();
		assert_eq!(p.progress, 2.0);
		assert_eq!(format_mcp_progress(&p, 80), "hi (2/5)");
		let p2 = McpProgress { progress: 3.0, total: None, message: Some("msg".into()) };
		assert_eq!(format_mcp_progress(&p2, 80), "msg");
	}

	#[test]
	fn visible_args_filters() {
		let mut m = serde_json::Map::new();
		m.insert("i".into(), json!("x"));
		m.insert("__partialJson".into(), json!("y"));
		m.insert("q".into(), json!(1));
		let v = visible_mcp_args(Some(&m));
		assert_eq!(v.len(), 1);
		assert_eq!(v[0].0, "q");
	}
}
