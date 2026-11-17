//! Xdev `xd://` device call/result render-dispatch types and helpers.
//!
//! Reference: `l/src/tools/xdev.ts`.
//!
//! Deviations:
//! - TS wires live TUI/native renderers (`Component`, `Theme`, `Text`,
//!   `Container`, `NativeToolView`, default renderer), renderer lookup,
//!   streaming JSON decode, and mounted-state callbacks. Rust ports the
//!   dispatch contracts and pure helpers: mounted renderer/state shapes,
//!   render-dispatch details, label/activity resolution, inner-args decoding,
//!   queued-call detection, device renderer/description selection, and native
//!   describe-view builders for queued/default/help forwarding plans.
//! - Actual component/native rendering and live renderer lookup callbacks stay
//!   host-side; this module keeps the data and planning helpers needed for a
//!   renderer-completion pass.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::native::describe::text as ntext;
use crate::native::node::{NodeText, Props};
use crate::tools::native_view::{tool_head, NativeToolHead, NativeToolView, ToolHeadDetail};

/// Mounted device display callback identity, independent of execution state.
///
/// In TS these are render/describe callbacks over components/native views.
/// Rust keeps presentation metadata and feature flags so hosts can dispatch to
/// the corresponding renderer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XdevMountedRenderer {
	/// Human label.
	pub label: Option<String>,
	/// Whether `renderCall` exists.
	pub has_render_call: bool,
	/// Whether `renderResult` exists.
	pub has_render_result: bool,
	/// Whether `describeCall` exists.
	pub has_describe_call: bool,
	/// Whether `describeResult` exists.
	pub has_describe_result: bool,
	/// Whether call/result are merged by the renderer.
	pub merge_call_and_result: bool,
}

impl XdevMountedRenderer {
	/// Can render call or result.
	pub fn can_render(&self) -> bool {
		self.has_render_call || self.has_render_result
	}
	/// Can describe call or result.
	pub fn can_describe(&self) -> bool {
		self.has_describe_call || self.has_describe_result
	}
}

/// Tool presentation state supplied by the host's canonical tool map.
#[derive(Clone, Debug, Default)]
pub struct XdevMountedState {
	/// Mounted device names.
	pub mounted_names: HashSet<String>,
	/// Mounted tool presentation metadata keyed by name.
	pub tools: HashMap<String, XdevMountedRenderer>,
}

impl XdevMountedState {
	/// Resolve mounted renderer metadata.
	pub fn get(&self, name: &str) -> Option<&XdevMountedRenderer> {
		self.tools.get(name)
	}
}

/// Result metadata consumed by delegated device rendering.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct XdevRenderDispatch {
	/// Target tool/device name.
	pub tool: String,
	/// Dispatch mode.
	pub mode: String,
	/// Inner args passed to device.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub args: Option<Value>,
	/// Tool tier label (host-specific).
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tier: Option<String>,
	/// Inner result/details forwarded to renderer.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub inner: Option<Value>,
}

/// Compact activity summary for a device write.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XdevActivitySummary {
	/// Display label.
	pub label: String,
	/// Optional detail (`verb object` or prose first line).
	pub detail: Option<String>,
}

/// Minimal render options affecting xdev call preview.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XdevRenderOptions {
	/// Whether execution has started for this write.
	pub execution_started: bool,
	/// Partial/streaming state (informational).
	pub is_partial: bool,
}

/// Call preview kind selected by xdev dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XdevCallPreviewKind {
	/// Queued/planning card before execution starts.
	Queued,
	/// Forward to device renderer call.
	DeviceCall,
	/// Forward to default tool execution renderer.
	DefaultCall,
}

/// Selected renderer source for a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XdevRendererSource {
	Mounted,
	Lookup,
	None,
}

/// Decode (possibly partially streamed) inner args JSON string into display args.
/// Best-effort: invalid/empty/non-object becomes empty map; preserves raw under
/// `__partialJson`.
pub fn decode_inner_args(raw: Option<&str>) -> HashMap<String, Value> {
	let mut out = HashMap::new();
	let Some(raw) = raw.filter(|s| !s.is_empty()) else {
		return out;
	};
	if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) {
		for (k, v) in map {
			out.insert(k, v);
		}
	}
	out.insert("__partialJson".to_string(), Value::String(raw.to_string()));
	out
}

/// Drop streaming-decode bookkeeping key before showing inner args.
pub fn display_device_args(args: &HashMap<String, Value>) -> HashMap<String, Value> {
	let mut out = args.clone();
	out.remove("__partialJson");
	out
}

/// Parse MCP tool name `mcp__<server>__<tool>`.
pub fn parse_mcp_tool_name(name: &str) -> Option<(String, String)> {
	crate::tools::mcp::parse_mcp_tool_name(name)
}

/// Human label for a device write: mounted label, else `server/tool` for MCP names.
pub fn display_device_label(name: &str, mounted: Option<&XdevMountedRenderer>) -> String {
	if let Some(label) = mounted.and_then(|m| m.label.as_ref()) {
		if !label.is_empty() {
			return label.clone();
		}
	}
	if let Some((server, tool)) = parse_mcp_tool_name(name) {
		return format!("{server}/{tool}");
	}
	name.to_string()
}

/// Salient inner-arg keys for compact activity line.
pub const ACTIVITY_VERB_KEYS: &[&str] = &["action", "op", "command"];
/// Salient inner-arg object keys for compact activity line.
pub const ACTIVITY_OBJECT_KEYS: &[&str] = &["query", "symbol", "path", "file", "pattern", "url", "name"];

fn pick_first_line_arg(args: &HashMap<String, Value>, keys: &[&str]) -> Option<String> {
	for key in keys {
		if let Some(Value::String(s)) = args.get(*key) {
			if !s.is_empty() {
				return s.split('\n').next().map(|v| v.to_string());
			}
		}
	}
	None
}

/// Compact `label · verb object` activity summary for a device write.
pub fn xdev_activity_summary(
	name: &str,
	content: Option<&str>,
	mounted: Option<&XdevMountedRenderer>,
) -> XdevActivitySummary {
	let args = decode_inner_args(content);
	let verb = pick_first_line_arg(&args, ACTIVITY_VERB_KEYS);
	let obj = pick_first_line_arg(&args, ACTIVITY_OBJECT_KEYS);
	let mut detail = [verb, obj].into_iter().flatten().collect::<Vec<_>>().join(" ");
	if detail.is_empty() {
		if let Some(raw) = content {
			if !raw.trim_start().starts_with('{') {
				detail = raw.trim().split('\n').next().unwrap_or("").to_string();
			}
		}
	}
	XdevActivitySummary {
		label: display_device_label(name, mounted),
		detail: if detail.is_empty() { None } else { Some(detail) },
	}
}

/// Renderer for a mounted device: mounted custom renderer first if it can render, else lookup.
pub fn resolve_device_renderer<'a>(
	mounted: Option<&'a XdevMountedRenderer>,
	lookup: Option<&'a XdevMountedRenderer>,
) -> (Option<&'a XdevMountedRenderer>, XdevRendererSource) {
	if let Some(m) = mounted {
		if m.can_render() {
			return (Some(m), XdevRendererSource::Mounted);
		}
	}
	if let Some(l) = lookup {
		if l.can_render() {
			return (Some(l), XdevRendererSource::Lookup);
		}
	}
	(None, XdevRendererSource::None)
}

/// Native describer: mounted first if it can describe, else lookup.
pub fn resolve_device_describer<'a>(
	mounted: Option<&'a XdevMountedRenderer>,
	lookup: Option<&'a XdevMountedRenderer>,
) -> (Option<&'a XdevMountedRenderer>, XdevRendererSource) {
	if let Some(m) = mounted {
		if m.can_describe() {
			return (Some(m), XdevRendererSource::Mounted);
		}
	}
	if let Some(l) = lookup {
		if l.can_describe() {
			return (Some(l), XdevRendererSource::Lookup);
		}
	}
	(None, XdevRendererSource::None)
}

/// Determine call preview kind for an `xd://` write.
pub fn xdev_call_preview_kind(
	options: &XdevRenderOptions,
	renderer: Option<&XdevMountedRenderer>,
) -> XdevCallPreviewKind {
	if !options.execution_started {
		return XdevCallPreviewKind::Queued;
	}
	if renderer.and_then(|r| if r.has_render_call { Some(()) } else { None }).is_some() {
		XdevCallPreviewKind::DeviceCall
	} else {
		XdevCallPreviewKind::DefaultCall
	}
}

/// Queued call label.
pub fn queued_call_label(name: &str, mounted: Option<&XdevMountedRenderer>) -> String {
	format!("queued {}", display_device_label(name, mounted))
}

/// Join text content blocks from a tool result.
pub fn dispatch_result_text(blocks: &[Value]) -> String {
	blocks
		.iter()
		.filter_map(|b| {
			if b.get("type").and_then(|t| t.as_str()) == Some("text") {
				b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
			} else {
				None
			}
		})
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join("\n")
}

/// Result forwarding decision for describe/render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdevResultForward {
	/// Help-mode: return plain text body when present.
	pub help_text: Option<String>,
	/// Target renderer exists for execute mode.
	pub has_renderer: bool,
	/// Renderer merges call and result.
	pub merge_call_and_result: bool,
	/// Renderer can describe/render result.
	pub has_result_renderer: bool,
	/// Renderer can describe/render call (for unmerged prepend).
	pub has_call_renderer: bool,
}

/// Plan result forwarding for a dispatch.
pub fn plan_xdev_result_forward(
	dispatch: &XdevRenderDispatch,
	result_text: &str,
	mounted: Option<&XdevMountedRenderer>,
	lookup: Option<&XdevMountedRenderer>,
) -> XdevResultForward {
	if dispatch.mode == "help" {
		return XdevResultForward {
			help_text: if result_text.is_empty() { None } else { Some(result_text.to_string()) },
			has_renderer: false,
			merge_call_and_result: false,
			has_result_renderer: false,
			has_call_renderer: false,
		};
	}
	let (renderer, _) = resolve_device_renderer(mounted, lookup);
	let r = renderer.cloned().unwrap_or_default();
	XdevResultForward {
		help_text: None,
		has_renderer: renderer.is_some(),
		merge_call_and_result: r.merge_call_and_result,
		has_result_renderer: r.has_render_result || r.has_describe_result,
		has_call_renderer: r.has_render_call || r.has_describe_call,
	}
}

/// Native queued call describe view (default-like head/body hint).
pub fn describe_xdev_queued_call(name: &str, content: Option<&str>, mounted: Option<&XdevMountedRenderer>) -> NativeToolView {
	let label = queued_call_label(name, mounted);
	let args = decode_inner_args(content);
	let shown = display_device_args(&args);
	let detail = if shown.is_empty() {
		None
	} else {
		Some(serde_json::to_string(&shown).unwrap_or_else(|_| "{…}".to_string()))
	};
	let details: Vec<ToolHeadDetail> = detail.iter().map(|d| ToolHeadDetail::Text(d.clone())).collect();
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(&label, &details))),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native help-result body view.
pub fn describe_xdev_help_result(text: &str) -> Option<NativeToolView> {
	let t = text.trim();
	if t.is_empty() {
		return None;
	}
	let mut p = Props::new();
	p.set_str("wrap", "word");
	Some(NativeToolView {
		body: Some(vec![ntext(NodeText::Plain(t.to_string()), p)]),
		..NativeToolView::default()
	})
}

/// Simple tool-head view for forwarded/default call/result planning.
pub fn describe_xdev_head(label: &str, detail: Option<&str>) -> NativeToolView {
	let details = detail
		.filter(|d| !d.trim().is_empty())
		.map(|d| vec![ToolHeadDetail::Text(d.trim().to_string())])
		.unwrap_or_default();
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(label, &details))),
		..NativeToolView::default()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decode_inner_args_keeps_partial_json() {
		let args = decode_inner_args(Some("{\"a\":1}"));
		assert_eq!(args.get("a"), Some(&Value::from(1)));
		assert!(args.get("__partialJson").is_some());
		let bad = decode_inner_args(Some("{"));
		assert!(!bad.contains_key("a"));
		assert!(bad.get("__partialJson").is_some());
	}

	#[test]
	fn display_label_mcp() {
		assert_eq!(display_device_label("mcp__codegraph__explore", None), "codegraph/explore");
	}

	#[test]
	fn activity_summary_verb_object() {
		let s = xdev_activity_summary("lsp", Some("{\"action\":\"references\",\"symbol\":\"foo\"}"), None);
		assert_eq!(s.label, "lsp");
		assert_eq!(s.detail.as_deref(), Some("references foo"));
	}

	#[test]
	fn queued_preview_before_execution() {
		let k = xdev_call_preview_kind(&XdevRenderOptions { execution_started: false, is_partial: true }, None);
		assert_eq!(k, XdevCallPreviewKind::Queued);
	}

	#[test]
	fn result_forward_help() {
		let d = XdevRenderDispatch { tool: "x".into(), mode: "help".into(), args: None, tier: None, inner: None };
		let f = plan_xdev_result_forward(&d, "hi", None, None);
		assert_eq!(f.help_text.as_deref(), Some("hi"));
	}

	#[test]
	fn describe_helpers() {
		let q = describe_xdev_queued_call("lsp", Some("{\"action\":\"hover\"}"), None);
		assert!(q.inline.unwrap_or(false));
		let h = describe_xdev_help_result("docs").unwrap();
		assert!(h.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
	}
}
