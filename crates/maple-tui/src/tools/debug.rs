//! Debug tool render-model types and helpers.
//!
//! Reference: `l/src/tools/debug.ts`.
//!
//! Deviations:
//! - TS renders TUI components/native views with `Theme`, framed cards,
//!   `OwnerMemo`, ANSI sanitization via `@oh-my-pi/pi-utils` and shared render
//!   utils. Rust ports snapshot/args/details types and pure formatting/describe
//!   helpers; sanitization is best-effort (tab/newline collapse, control-char
//!   stripping). Themed card/component rendering is host concern for renderer
//!   completion pass.
//! - Native describe helpers build semantic nodes via `tools/native_view` and
//!   `native/describe`.

use crate::native::describe::{ansi, kv, node, text as ntext};
use crate::native::node::{NodeKind, NodeText, Props};
use crate::tools::native_view::{error_view, note_text, result_text, tool_head, NativeToolView, ToolHeadDetail};

/// Display fields captured from a debugger session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugSessionSnapshot {
	/// Session id.
	pub id: String,
	/// Adapter name.
	pub adapter: String,
	/// Status.
	pub status: String,
	/// Working directory.
	pub cwd: String,
	/// Program.
	pub program: Option<String>,
	/// Stop reason.
	pub stop_reason: Option<String>,
	/// Frame name.
	pub frame_name: Option<String>,
	/// Instruction pointer reference.
	pub instruction_pointer_reference: Option<String>,
	/// Source path.
	pub source_path: Option<String>,
	/// Line.
	pub line: Option<u32>,
	/// Column.
	pub column: Option<u32>,
	/// Configuration done pending.
	pub needs_configuration_done: bool,
	/// Exit code.
	pub exit_code: Option<i32>,
}

/// Debug execution metadata consumed by the transcript.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugToolDetails {
	/// Action name.
	pub action: String,
	/// Success flag.
	pub success: bool,
	/// Snapshot.
	pub snapshot: Option<DebugSessionSnapshot>,
}

/// Debug arguments used to describe a pending request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugRenderArgs {
	/// Action.
	pub action: Option<String>,
	/// Program.
	pub program: Option<String>,
	/// File.
	pub file: Option<String>,
	/// Line.
	pub line: Option<u32>,
	/// Function.
	pub function: Option<String>,
	/// Expression.
	pub expression: Option<String>,
	/// Command.
	pub command: Option<String>,
	/// Memory reference.
	pub memory_reference: Option<String>,
	/// Instruction reference.
	pub instruction_reference: Option<String>,
	/// Data id.
	pub data_id: Option<String>,
	/// Name.
	pub name: Option<String>,
}

/// Formats a debugger stop location as a source path and coordinates.
pub fn format_location(snapshot: Option<&DebugSessionSnapshot>) -> Option<String> {
	let snap = snapshot?;
	let path = snap.source_path.as_deref()?;
	let line = snap.line?;
	Some(match snap.column {
		Some(col) => format!("{path}:{line}:{col}"),
		None => format!("{path}:{line}"),
	})
}

/// Formats the debugger session snapshot for model and terminal output.
pub fn format_session_snapshot(snapshot: &DebugSessionSnapshot) -> Vec<String> {
	let mut lines = vec![
		format!("Session {}", snapshot.id),
		format!("Adapter: {}", snapshot.adapter),
		format!("Status: {}", snapshot.status),
		format!("CWD: {}", snapshot.cwd),
	];
	if let Some(program) = &snapshot.program {
		lines.push(format!("Program: {program}"));
	}
	if let Some(stop_reason) = &snapshot.stop_reason {
		lines.push(format!("Stop reason: {stop_reason}"));
	}
	if let Some(frame) = &snapshot.frame_name {
		lines.push(format!("Frame: {frame}"));
	}
	if let Some(ip) = &snapshot.instruction_pointer_reference {
		lines.push(format!("Instruction pointer: {ip}"));
	}
	if let Some(loc) = format_location(Some(snapshot)) {
		lines.push(format!("Location: {loc}"));
	}
	if snapshot.needs_configuration_done {
		lines.push("Configuration: pending configurationDone; set breakpoints, then continue.".into());
	}
	if let Some(code) = snapshot.exit_code {
		lines.push(format!("Exit code: {code}"));
	}
	lines
}

/// Best-effort single-line sanitize for model-authored arg text.
pub fn sanitize_debug_arg(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for ch in value.chars() {
		match ch {
			'\n' | '\r' | '\t' => out.push(' '),
			c if c.is_control() => {}
			c => out.push(c),
		}
	}
	out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Highest-precedence identifying arg for a debug call head.
pub fn debug_call_target(args: &DebugRenderArgs) -> Option<String> {
	if let Some(p) = &args.program {
		return Some(p.clone());
	}
	if let (Some(file), Some(line)) = (&args.file, args.line) {
		return Some(format!("{file}:{line}"));
	}
	args.function
		.clone()
		.or_else(|| args.expression.clone())
		.or_else(|| args.command.clone())
		.or_else(|| args.memory_reference.clone())
		.or_else(|| args.instruction_reference.clone())
		.or_else(|| args.data_id.clone())
		.or_else(|| args.name.clone())
}

fn truncate_chars(s: &str, max: usize) -> String {
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

/// Summarize a debug call (title-length truncation default 72 chars).
pub fn summarize_debug_call(args: &DebugRenderArgs) -> String {
	let action = args
		.action
		.as_deref()
		.map(|a| sanitize_debug_arg(&a.replace('_', " ")))
		.unwrap_or_else(|| "request".to_string());
	if let Some(target) = debug_call_target(args) {
		format!("{action} {}", truncate_chars(&sanitize_debug_arg(&target), 72))
	} else {
		action
	}
}

/// Untruncated `[action, target]` for native heads.
pub fn debug_call_parts(args: Option<&DebugRenderArgs>, fallback_action: &str) -> (String, Option<String>) {
	let action = sanitize_debug_arg(
		&args
			.and_then(|a| a.action.as_deref())
			.unwrap_or(fallback_action)
			.replace('_', " "),
	);
	let target = args.and_then(debug_call_target).map(|t| sanitize_debug_arg(&t));
	(action, target)
}

/// Plain debug result summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugResultSummary {
	/// Action.
	pub action: String,
	/// Target.
	pub target: Option<String>,
	/// Error flag.
	pub is_error: bool,
	/// Error/output text when relevant.
	pub text: Option<String>,
	/// Session snapshot rows.
	pub session_rows: Vec<String>,
}

/// Describe debug result as plain summary.
pub fn describe_debug_result_plain(
	is_error: bool,
	output: &str,
	details: Option<&DebugToolDetails>,
	args: Option<&DebugRenderArgs>,
) -> DebugResultSummary {
	let fallback_action = details.map(|d| d.action.as_str()).unwrap_or("debug");
	let (action, target) = debug_call_parts(args, fallback_action);
	if is_error {
		return DebugResultSummary {
			action,
			target,
			is_error: true,
			text: Some(if output.trim().is_empty() {
				"Debug failed".to_string()
			} else {
				output.to_string()
			}),
			session_rows: Vec::new(),
		};
	}
	let session_rows = details
		.and_then(|d| d.snapshot.as_ref())
		.map(format_session_snapshot)
		.unwrap_or_default();
	DebugResultSummary {
		action,
		target,
		is_error: false,
		text: if output.is_empty() { None } else { Some(output.to_string()) },
		session_rows,
	}
}

/// Native describe for a debug call.
pub fn describe_debug_call(args: &DebugRenderArgs) -> NativeToolView {
	let summary = summarize_debug_call(args);
	NativeToolView {
		head: Some(NodeText::Spans(tool_head("Debug", &[ToolHeadDetail::Text(summary)]))),
		..NativeToolView::default()
	}
}

/// Native describe for a debug result.
pub fn describe_debug_result_native(
	is_error: bool,
	output: &str,
	details: Option<&DebugToolDetails>,
	args: Option<&DebugRenderArgs>,
) -> NativeToolView {
	let summary = describe_debug_result_plain(is_error, output, details, args);
	let head_details = [
		Some(ToolHeadDetail::Text(summary.action.clone())),
		summary.target.clone().map(ToolHeadDetail::Text),
	]
	.into_iter()
	.flatten()
	.collect::<Vec<_>>();
	let head = Some(NodeText::Spans(tool_head("Debug", &head_details)));
	if summary.is_error {
		return NativeToolView {
			..error_view(
				"Debug",
				summary.text.as_deref().unwrap_or("Debug failed"),
				&[Some(summary.action), summary.target],
			)
		};
	}

	let mut body = Vec::new();
	if !summary.session_rows.is_empty() {
		let pairs: Vec<(String, NodeText)> = summary
			.session_rows
			.iter()
			.filter_map(|line| {
				let (k, v) = line.split_once(": ")?;
				Some((k.to_string(), NodeText::Plain(v.to_string())))
			})
			.collect();
		if let Some(grid) = kv(pairs) {
			body.push(grid);
		} else {
			body.push(note_text(&summary.session_rows.join("\n"), "muted", None));
		}
	}
	if let Some(text) = summary.text.as_deref().filter(|t| !t.trim().is_empty()) {
		let mut p = Props::new();
		p.set_bool("follow", true);
		body.push(ansi(text, p));
	}
	if body.is_empty() {
		body.push(note_text("(no output)", "muted", None));
	}

	NativeToolView { head, body: Some(body), ..NativeToolView::default() }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_location_variants() {
		let mut snap = DebugSessionSnapshot {
			source_path: Some("a.rs".into()),
			line: Some(10),
			..Default::default()
		};
		assert_eq!(format_location(Some(&snap)), Some("a.rs:10".into()));
		snap.column = Some(2);
		assert_eq!(format_location(Some(&snap)), Some("a.rs:10:2".into()));
	}

	#[test]
	fn summarize_call_prefers_program() {
		let args = DebugRenderArgs {
			action: Some("launch".into()),
			program: Some("./app".into()),
			file: Some("x.rs".into()),
			line: Some(1),
			..Default::default()
		};
		let s = summarize_debug_call(&args);
		assert!(s.contains("launch"));
		assert!(s.contains("./app"));
	}

	#[test]
	fn call_parts_untruncated_target() {
		let args = DebugRenderArgs {
			action: Some("set_breakpoint".into()),
			file: Some("src/main.rs".into()),
			line: Some(42),
			..Default::default()
		};
		let (action, target) = debug_call_parts(Some(&args), "debug");
		assert_eq!(action, "set breakpoint");
		assert_eq!(target.as_deref(), Some("src/main.rs:42"));
	}

	#[test]
	fn session_snapshot_lines() {
		let snap = DebugSessionSnapshot {
			id: "s1".into(),
			adapter: "lldb".into(),
			status: "stopped".into(),
			cwd: "/w".into(),
			stop_reason: Some("breakpoint".into()),
			source_path: Some("a.rs".into()),
			line: Some(3),
			..Default::default()
		};
		let lines = format_session_snapshot(&snap);
		assert!(lines.iter().any(|l| l.starts_with("Session s1")));
		assert!(lines.iter().any(|l| l == "Stop reason: breakpoint"));
		assert!(lines.iter().any(|l| l == "Location: a.rs:3"));
	}

	#[test]
	fn native_describe_call_and_result() {
		let args = DebugRenderArgs { action: Some("pause".into()), ..Default::default() };
		let call = describe_debug_call(&args);
		assert!(call.head.is_some());
		let snap = DebugSessionSnapshot {
			id: "s1".into(),
			adapter: "dap".into(),
			status: "stopped".into(),
			cwd: "/".into(),
			..Default::default()
		};
		let details = DebugToolDetails { action: "pause".into(), success: true, snapshot: Some(snap) };
		let res = describe_debug_result_native(false, "ok", Some(&details), Some(&args));
		assert!(res.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
	}
}
