//! Resolution device names, payloads, and preview helpers.
//!
 //! Reference: `l/src/tools/resolve.ts`.
 //!
 //! Deviations:
//! - JS-runtime-only facilities dropped: TUI `Component`/`Text` renderers,
//!   `OwnerMemo`/cached result views, ANSI/theme styling, and streaming
//!   preview component behavior. Module-doc only; parity provided via plain/
//!   native render+describe helpers.
 //! - TS exports TUI renderers/components (`renderResolutionDeviceCall`,
 //!   `renderDeviceCallPreview`, `resolveRenderer`) and native describe views
 //!   built on `Theme`, `NativeToolView`, spans/nodes. Rust ports device
//!   identifiers, data types, plain preview/head derivation helpers, and
//!   exported render/describe helpers.
 //! - `sourceResultDetails` is `Option<serde_json::Value>`.

use serde_json::Value;
/// Device name for applying a staged action.
pub const RESOLVE_DEVICE_NAME: &str = "resolve";

/// Device name for discarding a staged action.
pub const REJECT_DEVICE_NAME: &str = "reject";

/// Device name for submitting a plan.
pub const PROPOSE_DEVICE_NAME: &str = "propose";

/// Plain-text staged-action device names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionDeviceName {
	/// Apply/resolve.
	Resolve,
	/// Discard/reject.
	Reject,
	/// Propose/submit plan.
	Propose,
}

impl ResolutionDeviceName {
	/// String device name.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Resolve => RESOLVE_DEVICE_NAME,
			Self::Reject => REJECT_DEVICE_NAME,
			Self::Propose => PROPOSE_DEVICE_NAME,
		}
	}

	/// Preview title for this device.
	pub fn preview_title(self) -> &'static str {
		match self {
			Self::Propose => "Propose",
			Self::Reject => "Reject",
			Self::Resolve => "Resolve",
		}
	}
}

/// Resolution applied to a staged action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveAction {
	/// Apply/proceed.
	Apply,
	/// Discard/reject.
	Discard,
}

impl ResolveAction {
	/// Wire/string form used in TS details.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Apply => "apply",
			Self::Discard => "discard",
		}
	}
}

/// Details payload carried on a resolve/reject dispatch result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolveDetails {
	/// Applied action.
	pub action: Option<ResolveAction>,
	/// User/model reason.
	pub reason: String,
	/// Source tool name.
	pub source_tool_name: Option<String>,
	/// Human label.
	pub label: Option<String>,
	/// Source result details.
	pub source_result_details: Option<Value>,
}

/// Invoker input for queued pending-preview handlers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolveInvocation {
	/// Requested action.
	pub action: Option<ResolveAction>,
	/// Reason text.
	pub reason: String,
}

/// First content line of a pending device write, plain text for preview/head.
pub fn preview_first_line(content: Option<&str>) -> Option<String> {
	let body = content?;
	let trimmed = body.trim();
	if trimmed.is_empty() {
		return None;
	}
	let first = trimmed.split('\n').next().unwrap_or("").trim();
	if first.is_empty() {
		None
	} else {
		Some(first.replace('\t', " "))
	}
}

/// Preview title and first content line for a resolution-device write.
pub fn describe_resolution_device_call(
	device: ResolutionDeviceName,
	content: Option<&str>,
) -> (String, Option<String>) {
	(
		device.preview_title().to_string(),
		preview_first_line(content),
	)
}

/// Whether an `xd://` device name is one of the plain-text resolution devices.
pub fn is_resolution_device_name(name: &str) -> bool {
	matches!(name, RESOLVE_DEVICE_NAME | REJECT_DEVICE_NAME | PROPOSE_DEVICE_NAME)
}

/// Parse a resolution device name.
pub fn parse_resolution_device_name(name: &str) -> Option<ResolutionDeviceName> {
	match name {
		RESOLVE_DEVICE_NAME => Some(ResolutionDeviceName::Resolve),
		REJECT_DEVICE_NAME => Some(ResolutionDeviceName::Reject),
		PROPOSE_DEVICE_NAME => Some(ResolutionDeviceName::Propose),
		_ => None,
	}
}

/// Call head description fields for resolve renderer (`title`, action, reason).
pub fn describe_resolve_call(args: &ResolveInvocation) -> (String, Option<String>, Option<String>) {
	let action = args.action.map(|a| a.as_str().to_string());
	let reason = {
		let t = args.reason.trim();
		if t.is_empty() {
			None
		} else {
			Some(t.to_string())
		}
	};
	("Resolve".to_string(), action, reason)
}

/// Result summary for the resolve renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveResultSummary {
	/// Verb (`Accept`, `Failed`, `Discard`).
	pub verb: String,
	/// Tone name (`success`, `warning`, `error`).
	pub tone: String,
	/// Source tool label split out of the human label.
	pub source_label: Option<String>,
	/// Summary label without the source badge.
	pub summary_label: String,
	/// Reason body.
	pub reason: String,
}

/// Derive a plain summary of a resolve result.
pub fn describe_resolve_result(details: Option<&ResolveDetails>, is_error: bool) -> ResolveResultSummary {
	let label = details
		.and_then(|d| d.label.as_deref())
		.unwrap_or("pending action")
		.replace('\t', " ");
	let reason = details
		.and_then(|d| {
			let t = d.reason.trim();
			if t.is_empty() {
				None
			} else {
				Some(t.replace('\t', " "))
			}
		})
		.unwrap_or_else(|| "No reason provided".to_string());
	let action = details
		.and_then(|d| d.action)
		.unwrap_or(ResolveAction::Apply);
	let is_apply = matches!(action, ResolveAction::Apply) && !is_error;
	let verb = if is_apply {
		"Accept"
	} else if matches!(action, ResolveAction::Apply) {
		"Failed"
	} else {
		"Discard"
	}
	.to_string();
	let tone = if is_error {
		"error"
	} else if is_apply {
		"success"
	} else {
		"warning"
	}
	.to_string();
	let separator = ": ";
	let (source_label, summary_label) = if let Some(idx) = label.find(separator) {
		let src = label[..idx].trim();
		let sum = label[idx + separator.len()..].trim();
		(
			if src.is_empty() {
				None
			} else {
				Some(src.to_string())
			},
			if sum.is_empty() {
				label
			} else {
				sum.to_string()
			},
		)
	} else {
		(None, label)
	};
	ResolveResultSummary {
		verb,
		tone,
		source_label,
		summary_label,
		reason,
	}
}

/// Plain render of a resolution-device call.
pub fn render_call(device: ResolutionDeviceName, content: Option<&str>) -> String {
	let (title, preview) = describe_resolution_device_call(device, content);
	match preview {
		Some(p) if !p.is_empty() => format!("{title}: {p}"),
		_ => title,
	}
}

/// Plain render of a resolve/reject result.
pub fn render_result(details: Option<&ResolveDetails>, is_error: bool) -> Vec<String> {
	let s = describe_resolve_result(details, is_error);
	let mut lines = vec![format!("{}: {}", s.summary_label, s.verb)];
	if !s.reason.trim().is_empty() {
		lines.extend(s.reason.lines().take(40).map(|l| l.to_string()));
	}
	lines
}

/// Native describe of a resolution-device call.
pub fn describe_call(device: ResolutionDeviceName, content: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let (title, preview) = describe_resolution_device_call(device, content);
	let details = preview
		.filter(|p| !p.is_empty())
		.map(|p| vec![ToolHeadDetail::Span(Span::styled(p, "muted"))])
		.unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head(&title, &details))), tone: Some(Tone::Accent), ..NativeToolView::default() }
}

/// Native describe of a resolve/reject/propose result.
pub fn describe_result(details: Option<&ResolveDetails>, is_error: bool) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	let s = describe_resolve_result(details, is_error);
	let head_text = match &s.source_label {
		Some(src) => format!("{} [{}]", s.summary_label, src),
		None => s.summary_label.clone(),
	};
	if is_error {
		let msg = if s.reason.trim().is_empty() { "failed".to_string() } else { s.reason.clone() };
		return error_view(&head_text, &msg, &[]);
	}
	let mut head_details = Vec::new();
	head_details.push(ToolHeadDetail::Span(Span::styled(s.verb.clone(), "accent")));
	let head = tool_head(&head_text, &head_details);
	if s.reason.trim().is_empty() {
		return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("ok", "dim", None)]), tone: Some(Tone::Success), ..NativeToolView::default() };
	}
	let mut p = Props::new();
	p.set_str("wrap", "word");
	let tone = match s.tone.as_str() {
		"warning" => Tone::Warning,
		_ => Tone::Success,
	};
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ntext(NodeText::Plain(s.reason.clone()), p)]), tone: Some(tone), ..NativeToolView::default() }
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let c = render_call(ResolutionDeviceName::Resolve, Some("ship it"));
		assert!(c.contains("Resolve"));
		let v = describe_call(ResolutionDeviceName::Reject, Some("no"));
		assert!(v.head.is_some());
		let d = ResolveDetails { action: Some(ResolveAction::Apply), reason: "ok".into(), ..Default::default() };
		let lines = render_result(Some(&d), false);
		assert!(!lines.is_empty());
		let r = describe_result(Some(&d), false);
		assert!(r.body.is_some() || r.head.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn device_name_helpers() {
		assert!(is_resolution_device_name("resolve"));
		assert!(is_resolution_device_name("reject"));
		assert!(is_resolution_device_name("propose"));
		assert!(!is_resolution_device_name("other"));
		assert_eq!(
			parse_resolution_device_name("reject"),
			Some(ResolutionDeviceName::Reject)
		);
	}

	#[test]
	fn preview_first_line() {
		assert_eq!(preview_first_line(Some("  a\nb ")), Some("a".into()));
		assert_eq!(preview_first_line(Some(" \n ")), None);
	}

	#[test]
	fn resolve_result_summary_apply_success() {
		let d = ResolveDetails {
			action: Some(ResolveAction::Apply),
			reason: "ok".into(),
			label: Some("Tool: do thing".into()),
			..Default::default()
		};
		let s = describe_resolve_result(Some(&d), false);
		assert_eq!(s.verb, "Accept");
		assert_eq!(s.tone, "success");
		assert_eq!(s.source_label.as_deref(), Some("Tool"));
		assert_eq!(s.summary_label, "do thing");
	}

	#[test]
	fn resolve_result_summary_failed_apply() {
		let d = ResolveDetails {
			action: Some(ResolveAction::Apply),
			reason: String::new(),
			..Default::default()
		};
		let s = describe_resolve_result(Some(&d), true);
		assert_eq!(s.verb, "Failed");
		assert_eq!(s.tone, "error");
		assert_eq!(s.reason, "No reason provided");
	}
}
