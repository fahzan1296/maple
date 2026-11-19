//! Default/fallback tool renderer model and helpers.
//!
//! Reference: `l/src/tools/default-renderer.ts`.
//!
//! Deviations:
//! - TS renders terminal/TUI components and native describe nodes (JSON tree,
//!   ANSI output pane, themed status lines/cards). Rust ports the input/result
//!   data shapes and pure decision/formatting helpers (status/phase selection,
//!   inline args formatting, JSON-tree-safety checks, output mode selection)
//!   plus native describe helpers for the generic fallback card.
//! - Width/theming/component rendering is host responsibility for renderer
//!   completion pass.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::native::describe::{ansi, code, text as ntext};
use crate::native::node::{NodeText, Props};
use crate::tools::native_view::{tool_head, NativePreview, NativeToolView, ToolHeadDetail};

/// Inputs rendered by the fallback card used when a tool has no bespoke renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultToolRenderInput {
	/// Human-readable tool label.
	pub label: String,
	/// Tool arguments.
	pub args: Option<Value>,
	/// Settled/streaming result.
	pub result: Option<DefaultToolRenderResult>,
	/// Expansion/lifecycle state.
	pub options: DefaultRenderOptions,
}

/// Settled or streaming result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefaultToolRenderResult {
	/// Output text.
	pub output: String,
	/// Error flag.
	pub is_error: bool,
	/// Skipped placeholder flag.
	pub skipped: bool,
}

/// Expansion/lifecycle state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefaultRenderOptions {
	/// Expanded view.
	pub expanded: bool,
	/// Partial/streaming.
	pub is_partial: bool,
	/// Spinner frame present/animating.
	pub spinner_frame: Option<usize>,
	/// Native JSON view requested.
	pub json: bool,
}

/// Status icon kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolUiStatus {
	Pending,
	Running,
	Info,
	Success,
	Error,
}

/// Card phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCardPhase {
	Partial,
	Running,
	Info,
	Error,
	Success,
}

/// Status line state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLineState {
	/// Icon.
	pub icon: ToolUiStatus,
	/// Title.
	pub title: String,
	/// Muted title when skipped.
	pub title_muted: bool,
}

/// Build status and phase for a default tool render.
pub fn build_default_tool_status(input: &DefaultToolRenderInput) -> (StatusLineState, ToolCardPhase) {
	let result = input.result.as_ref();
	let (icon, phase) = if input.options.is_partial {
		if input.options.spinner_frame.is_some() {
			(ToolUiStatus::Running, ToolCardPhase::Running)
		} else {
			(ToolUiStatus::Pending, ToolCardPhase::Partial)
		}
	} else if result.map(|r| r.skipped).unwrap_or(false) {
		(ToolUiStatus::Info, ToolCardPhase::Info)
	} else if result.map(|r| r.is_error).unwrap_or(false) {
		(ToolUiStatus::Error, ToolCardPhase::Error)
	} else {
		(ToolUiStatus::Success, ToolCardPhase::Success)
	};
	(
		StatusLineState {
			icon,
			title: input.label.clone(),
			title_muted: result.map(|r| r.skipped).unwrap_or(false),
		},
		phase,
	)
}

/// Format object args inline as `k=v, ...` (character budget approximation).
pub fn format_args_inline(args: &Value, budget_chars: usize) -> Option<String> {
	let obj = args.as_object()?;
	if obj.is_empty() {
		return None;
	}
	let map: BTreeMap<&String, &Value> = obj.iter().collect();
	let mut parts = Vec::new();
	for (k, v) in map {
		let vstr = match v {
			Value::String(s) => s.clone(),
			Value::Number(n) => n.to_string(),
			Value::Bool(b) => b.to_string(),
			Value::Null => "null".to_string(),
			Value::Array(_) => "[…]".to_string(),
			Value::Object(_) => "{…}".to_string(),
		};
		parts.push(format!("{k}={vstr}"));
	}
	let joined = parts.join(", ");
	if joined.chars().count() <= budget_chars {
		Some(joined)
	} else if budget_chars <= 1 {
		Some("…".to_string())
	} else {
		let mut out: String = joined.chars().take(budget_chars.saturating_sub(1)).collect();
		out.push('…');
		Some(out)
	}
}

/// Serialized JSON past this size keeps raw view (64 KiB).
pub const JSON_TREE_MAX_CHARS: usize = 64 * 1024;

/// Inline args summary budget in characters.
pub const NATIVE_ARGS_SUMMARY_CHARS: usize = 160;

/// An integer literal of 16+ digits may exceed safe integer precision.
pub fn has_unsafe_integer(text: &str) -> bool {
	let bytes = text.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		let b = bytes[i];
		if b.is_ascii_digit() {
			let start_ok = i == 0
				|| (!(bytes[i - 1] as char).is_ascii_alphanumeric()
					&& bytes[i - 1] != b'.'
					&& bytes[i - 1] != b'_');
			if start_ok {
				let mut j = i;
				while j < bytes.len() && bytes[j].is_ascii_digit() {
					j += 1;
				}
				let len = j - i;
				let end_ok = j == bytes.len()
					|| (!(bytes[j] as char).is_ascii_alphanumeric()
						&& bytes[j] != b'.'
						&& bytes[j] != b'_');
				if len >= 16 && end_ok {
					return true;
				}
				i = j;
				continue;
			}
		}
		i += 1;
	}
	false
}

/// Whether text can be rendered as a JSON tree without losing raw fidelity.
pub fn is_tree_safe_json(text: &str) -> bool {
	(text.starts_with('{') || text.starts_with('['))
		&& text.len() <= JSON_TREE_MAX_CHARS
		&& !has_unsafe_integer(text)
}

/// Output rendering mode decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultOutputMode {
	/// No output.
	Empty,
	/// Render as JSON tree/value.
	Json,
	/// Render raw text.
	Raw,
}

/// Decide output mode given output text.
pub fn decide_output_mode(text: &str) -> DefaultOutputMode {
	let trimmed = text.trim_end();
	if trimmed.is_empty() {
		return DefaultOutputMode::Empty;
	}
	if is_tree_safe_json(trimmed) && serde_json::from_str::<Value>(trimmed).is_ok() {
		DefaultOutputMode::Json
	} else {
		DefaultOutputMode::Raw
	}
}

/// Native describe view of the generic fallback card.
pub fn describe_default_tool_execution(input: &DefaultToolRenderInput) -> NativeToolView {
	let (_status, _phase) = build_default_tool_status(input);
	let is_skipped = input.result.as_ref().map(|r| r.skipped).unwrap_or(false);
	let is_error = input.result.as_ref().map(|r| r.is_error).unwrap_or(false);
	let args_summary = input
		.args
		.as_ref()
		.and_then(|a| format_args_inline(a, NATIVE_ARGS_SUMMARY_CHARS))
		.or_else(|| input.args.as_ref().filter(|a| !a.is_null() && !a.as_object().is_some_and(|o| o.is_empty())).map(|a| {
			let s = a.to_string();
			if s.chars().count() <= NATIVE_ARGS_SUMMARY_CHARS {
				s
			} else {
				let mut out: String = s.chars().take(NATIVE_ARGS_SUMMARY_CHARS.saturating_sub(1)).collect();
				out.push('…');
				out
			}
		}));
	let head_details = args_summary.into_iter().map(ToolHeadDetail::Text).collect::<Vec<_>>();
	let head = Some(NodeText::Spans(tool_head(&input.label, &head_details)));

	if is_skipped {
		return NativeToolView {
			head,
			tone: Some(crate::native::node::Tone::Muted),
			preview: Some(NativePreview::None),
			..NativeToolView::default()
		};
	}

	let Some(result) = &input.result else {
		return NativeToolView { head, ..NativeToolView::default() };
	};
	let trimmed = result.output.trim_end();
	if trimmed.is_empty() {
		return NativeToolView { head, ..NativeToolView::default() };
	}

	let mut view = NativeToolView {
		head,
		preview: Some(NativePreview::Lines(12)),
		..NativeToolView::default()
	};
	if is_error {
		view.tone = Some(crate::native::node::Tone::Error);
	}

	if input.options.json {
		if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
			view.body = Some(vec![code(
				serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| trimmed.to_string()),
				{
					let mut p = Props::new();
					p.set_str("language", "json");
					p
				},
			)]);
			return view;
		}
	}

	match decide_output_mode(trimmed) {
		DefaultOutputMode::Json => {
			if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
				view.body = Some(vec![code(
					serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| trimmed.to_string()),
					{
						let mut p = Props::new();
						p.set_str("language", "json");
						p
					},
				)]);
			} else {
				view.body = Some(vec![ansi(trimmed.to_string(), Props::new())]);
			}
		}
		DefaultOutputMode::Raw => {
			view.body = Some(vec![ansi(trimmed.to_string(), Props::new())]);
		}
		DefaultOutputMode::Empty => {}
	}
	view
}

/// Format a plain one-line status for default tool execution.
pub fn format_default_tool_execution_plain(input: &DefaultToolRenderInput) -> String {
	let (status, _phase) = build_default_tool_status(input);
	let icon = match status.icon {
		ToolUiStatus::Pending => "pending",
		ToolUiStatus::Running => "running",
		ToolUiStatus::Info => "info",
		ToolUiStatus::Success => "success",
		ToolUiStatus::Error => "error",
	};
	format!("[{icon}] {}", status.title)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn status_phase_selection() {
		let input = DefaultToolRenderInput {
			label: "X".into(),
			args: None,
			result: Some(DefaultToolRenderResult {
				output: "ok".into(),
				is_error: false,
				skipped: false,
			}),
			options: DefaultRenderOptions::default(),
		};
		let (s, p) = build_default_tool_status(&input);
		assert_eq!(s.icon, ToolUiStatus::Success);
		assert_eq!(p, ToolCardPhase::Success);

		let skipped = DefaultToolRenderInput {
			result: Some(DefaultToolRenderResult {
				output: String::new(),
				is_error: false,
				skipped: true,
			}),
			options: DefaultRenderOptions::default(),
			..input.clone()
		};
		let (s, p) = build_default_tool_status(&skipped);
		assert_eq!(s.icon, ToolUiStatus::Info);
		assert!(s.title_muted);
		assert_eq!(p, ToolCardPhase::Info);
	}

	#[test]
	fn args_inline_and_budget() {
		let v = json!({"b": 2, "a": "x"});
		let s = format_args_inline(&v, 50).unwrap();
		assert!(s.contains("a=x"));
		assert!(s.contains("b=2"));
	}

	#[test]
	fn output_mode_and_unsafe_int() {
		assert!(is_tree_safe_json("{\"a\":1}"));
		assert!(!is_tree_safe_json("{\"a\":1234567890123456}"));
		assert_eq!(decide_output_mode("{\"a\":1}"), DefaultOutputMode::Json);
		assert_eq!(decide_output_mode("plain"), DefaultOutputMode::Raw);
	}

	#[test]
	fn describe_default_exec() {
		let input = DefaultToolRenderInput {
			label: "Tool".into(),
			args: Some(json!({"k":"v"})),
			result: Some(DefaultToolRenderResult { output: "{\"a\":1}".into(), is_error: false, skipped: false }),
			options: DefaultRenderOptions::default(),
		};
		let v = describe_default_tool_execution(&input);
		assert!(v.head.is_some());
		assert!(v.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
		assert!(format_default_tool_execution_plain(&input).contains("Tool"));
	}
}
