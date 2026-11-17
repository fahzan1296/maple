//! Port of `l/test/task-progress-render.test.ts` (portable subset).
//!
//! Divergence: TS tested live task progress row rendering (path shortening in
//! args/intents, stage sanitization, model/advisor badges, width/resize behavior,
//! stats/cost/context formatting) against themed TUI components. Rust task module
//! does not expose progress-row renderer APIs; this subset asserts exported
//! call/result formatting and yield submission helpers that underpin task
//! summaries/progress payload handling.

use maple_tui::tools::task::{render_call, render_result};
use maple_tui::tools::yield_tool::{
	read_submission, render_call as yield_render_call, render_result as yield_render_result, size_meta,
	YieldRenderArgs, YieldResultView, YieldTypeArg,
};
use serde_json::json;

#[test]
fn task_render_call_preserves_visible_preview_and_multiline_truncation() {
	let out = render_call(Some("task"), Some("Inspect ~/private/file\nsecond line"), None);
	assert!(out.contains("Inspect ~/private/file"));
	assert!(!out.contains("second line"));
}

#[test]
fn task_render_result_truncates_long_output_with_line_count() {
	let output = (1..=45).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
	let lines = render_result(None, &output, false);
	assert_eq!(lines.len(), 41); // 40 lines + summary
	assert!(lines[0].contains("line 1"));
	assert!(lines[39].contains("line 40"));
	assert!(lines[40].contains("45 lines total"));
}

#[test]
fn yield_submission_helpers_capture_size_target_and_notes() {
	assert_eq!(size_meta(Some(&json!([1, 2, 3]))).as_deref(), Some("3 items"));
	assert_eq!(size_meta(Some(&json!({"a": 1}))).as_deref(), Some("1 field"));

	let args = YieldRenderArgs {
		r#type: Some(YieldTypeArg::Array(vec!["a".into(), "b".into()])),
		..Default::default()
	};
	let pending = read_submission(Some(&args), None);
	assert_eq!(pending.target.as_deref(), Some("a, b"));
	assert!(!pending.settled);

	let settled = read_submission(Some(&args), Some(&YieldResultView::default()));
	assert!(settled.settled);
	assert_eq!(
		settled.note.as_deref(),
		Some("Takes the last message as this section")
	);
	assert!(yield_render_call(Some(&args), Some(&YieldResultView::default())).contains("Submit Result"));
	assert_eq!(yield_render_result(Some(&YieldRenderArgs { data: Some(json!("hi")), ..Default::default() }), None)[0], "hi");
}