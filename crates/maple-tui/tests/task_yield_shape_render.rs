//! Port of `l/test/task-yield-shape-render.test.ts` (portable subset).
//!
//! Divergence: TS validated task renderer handling of malformed/typed/incremental
//! yield shapes in live progress and final result TUI branches. Rust task module
//! provides yield normalization/label helpers and yield tool submission/render
//! helpers rather than TUI task cards; this subset ports the core yield-shape
//! contracts (single object vs array, primitives ignored, typed labels, last-turn
//! notes) with real assertions on exported APIs.

use maple_tui::tools::task::{get_render_yield_labels, normalize_yield_data, YieldTypeValue};
use maple_tui::tools::yield::{
	read_submission, render_call, render_result, YieldRenderArgs, YieldResultDetails, YieldResultView,
	YieldTypeArg,
};
use serde_json::json;

#[test]
fn normalize_yield_data_tolerates_single_object_array_and_primitives() {
	let single = normalize_yield_data(Some(&json!({"data": {"overall_correctness": "correct"}, "status": "success"})));
	assert_eq!(single.len(), 1);
	assert_eq!(
		single[0].data.as_ref().and_then(|d| d.get("overall_correctness")).and_then(|v| v.as_str()),
		Some("correct")
	);

	let canonical = normalize_yield_data(Some(&json!([{"data": {"overall_correctness": "correct"}, "status": "success"}])));
	assert_eq!(canonical.len(), 1);
	assert_eq!(canonical[0].status.as_deref(), Some("success"));

	assert!(normalize_yield_data(Some(&json!("not-an-array"))).is_empty());
	assert!(normalize_yield_data(Some(&json!(42))).is_empty());
}

#[test]
fn typed_yield_labels_and_incremental_target_are_preserved() {
	let items = normalize_yield_data(Some(&json!([
		{"type": ["summary"], "data": "first note", "status": "success"},
		{"type": ["summary", "details"], "data": {"ok": true}, "status": "success"},
		{"type": "final", "data": "done", "status": "success"}
	])));
	assert_eq!(items.len(), 3);
	assert_eq!(get_render_yield_labels(items[0].r#type.as_ref()), vec!["summary"]);
	assert_eq!(get_render_yield_labels(items[1].r#type.as_ref()), vec!["summary", "details"]);
	assert_eq!(get_render_yield_labels(items[2].r#type.as_ref()), vec!["final"]);

	let args = YieldRenderArgs {
		r#type: Some(YieldTypeArg::Array(vec!["summary".into(), "details".into()])),
		..Default::default()
	};
	let sub = read_submission(Some(&args), None);
	assert_eq!(sub.target.as_deref(), Some("summary, details"));
	assert!(render_call(Some(&args), None).contains("Submit Result"));
}

#[test]
fn settled_incremental_and_terminal_notes_follow_result_shape() {
	let incremental = YieldRenderArgs {
		r#type: Some(YieldTypeArg::Array(vec!["notes".into()])),
		..Default::default()
	};
	let settled_inc = read_submission(Some(&incremental), Some(&YieldResultView::default()));
	assert_eq!(settled_inc.note.as_deref(), Some("Takes the last message as this section"));

	let terminal = YieldRenderArgs::default();
	let settled_term = read_submission(Some(&terminal), Some(&YieldResultView::default()));
	assert_eq!(
		settled_term.note.as_deref(),
		Some("Ends with the sections submitted earlier, or the last message")
	);

	let err = YieldResultView {
		is_error: true,
		result_text: Some("boom".into()),
		..Default::default()
	};
	let failed = read_submission(Some(&terminal), Some(&err));
	assert_eq!(failed.error.as_deref(), Some("boom"));
	assert!(render_result(Some(&terminal), Some(&err))[0].contains("error"));

	let overridden = YieldResultView {
		details: Some(YieldResultDetails {
			data: Some(json!("done")),
			schema_overridden: true,
			..Default::default()
		}),
		..Default::default()
	};
	let sub = read_submission(Some(&terminal), Some(&overridden));
	assert!(sub.overridden);
	assert_eq!(render_result(Some(&terminal), Some(&overridden))[0], "done");
}