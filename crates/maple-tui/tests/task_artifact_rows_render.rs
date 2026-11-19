//! Port of `l/test/task-artifact-rows-render.test.ts` (portable subset).
//!
//! Divergence: TS asserted TUI task renderer artifact rows (`Patch:`/`Branch:`/
//! `Nested patch:` labels, root-patch hiding) on themed components. Rust
//! `tools::task` currently exposes data/normalization plus plain/native
//! call/result helpers, not isolation-artifact row rendering; no equivalent
//! artifact-row API exists. This subset asserts real exported call/result/describe
//! behavior for labeled task summaries.

use maple_tui::tools::task::{describe_call, describe_result, render_call, render_result};

#[test]
fn render_call_surfaces_agent_and_first_task_line() {
	let out = render_call(Some("reviewer"), Some("Review auth\nReport findings."), None);
	assert!(out.contains("reviewer"));
	assert!(out.contains("Review auth"));
	assert!(!out.contains("Report findings."));
}

#[test]
fn render_call_includes_batch_agent_count() {
	let out = render_call(Some("task"), Some("work"), Some(2));
	assert!(out.contains("2 agents"));
}

#[test]
fn describe_call_includes_head_and_details() {
	let view = describe_call(Some("scout"), Some("find files"), Some(3));
	assert!(view.head.is_some());
	assert!(view.tone.is_some());
}

#[test]
fn render_and_describe_result_handle_empty_error_and_output() {
	let ok = render_result(None, "done", false);
	assert_eq!(ok.first().map(String::as_str), Some("done"));
	let err = render_result(None, "boom\nmore", true);
	assert!(err[0].contains("Task failed"));
	assert!(err[0].contains("boom"));
	let view = describe_result(None, "ok", false);
	assert!(view.body.is_some());
}