//! Port of `l/test/task-render.test.ts` (portable subset).
//!
//! Divergence: TS covered the full task tool TUI renderer (streaming/finalized
//! trees, expansion, findings/yield/reviewer assembly, badges, nested results,
//! markdown/JSON rendering, ANSI layout). Rust `tools::task` exposes data
//! normalization and plain/native call/result helpers only; this subset asserts
//! those exported behaviors.

use maple_tui::tools::task::{
	describe_call, describe_result, get_render_yield_labels, normalize_findings, normalize_yield_data,
	render_call, render_result, FindingPriority,
};
use serde_json::json;

#[test]
fn render_call_formats_agent_task_and_batch() {
	assert_eq!(render_call(Some("scout"), Some("find files\nmore"), None), "scout: find files");
	assert_eq!(render_call(None, Some("work"), None), "task: work");
	assert!(render_call(Some("task"), Some("work"), Some(3)).contains("(3 agents)"));
}

#[test]
fn render_and_describe_result_cover_success_and_error_paths() {
	assert_eq!(render_result(None, "ok", false), vec!["ok".to_string()]);
	assert_eq!(render_result(None, "   ", false), vec!["No output".to_string()]);
	let err = render_result(None, "bad\nlater", true);
	assert!(err[0].starts_with("Task failed: bad"));
	assert!(describe_result(None, "ok", false).body.is_some());
	assert!(describe_call(Some("task"), Some("x"), None).head.is_some());
}

#[test]
fn yield_and_finding_normalization_cover_common_shapes() {
	let items = normalize_yield_data(Some(&json!([
		{"type": ["summary"], "data": "first", "status": "success"},
		{"type": "final", "data": "done"},
	])));
	assert_eq!(items.len(), 2);
	assert_eq!(get_render_yield_labels(items[0].r#type.as_ref()), vec!["summary".to_string()]);
	assert_eq!(get_render_yield_labels(items[1].r#type.as_ref()), vec!["final".to_string()]);

	let findings = normalize_findings(Some(&json!([
		{"title": "Null", "priority": "P0", "confidence": 1.0},
		{"missing_title": true}
	])));
	assert_eq!(findings.len(), 1);
	assert_eq!(findings[0].priority, Some(FindingPriority::P0));
}