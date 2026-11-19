//! Port of `l/test/task-nested-live-render.test.ts` (portable subset).
//!
//! Divergence: TS rendered live nested task progress trees, inflight snapshots,
//! extracted completed sub-results, cycle guards, stats formatting, and
//! completion summaries in TUI components. Rust `tools::task` currently lacks
//! `AgentProgress`/`SingleResult`/nested live rendering APIs; this subset ports
//! the data-normalization contracts exposed for task/yield payloads
//! (`normalize_yield_data`, finding/review field parsing) that nested/finalized
//! task results carry.

use maple_tui::tools::task::{get_render_yield_labels, normalize_findings, normalize_yield_data, parse_finding_details, YieldTypeValue};
use serde_json::json;

#[test]
fn normalize_yield_data_accepts_array_object_and_rejects_primitives() {
	let arr = normalize_yield_data(Some(&json!([
		{"type": "summary", "data": "ok", "useLastTurn": true},
		{"type": ["details"], "data": {"a": 1}}
	])));
	assert_eq!(arr.len(), 2);
	assert!(arr[0].use_last_turn);
	assert_eq!(get_render_yield_labels(arr[1].r#type.as_ref()), vec!["details".to_string()]);

	let single = normalize_yield_data(Some(&json!({"type": "z", "data": 1})));
	assert_eq!(single.len(), 1);
	assert!(matches!(&single[0].r#type, Some(YieldTypeValue::String(s)) if s == "z"));

	assert!(normalize_yield_data(Some(&json!("not-an-array"))).is_empty());
	assert!(normalize_yield_data(Some(&json!(42))).is_empty());
	assert!(normalize_yield_data(None).is_empty());
}

#[test]
fn parse_and_normalize_review_findings_from_untrusted_values() {
	let finding = parse_finding_details(&json!({
		"title": "Handle null response",
		"body": "Null response crashes rendering.",
		"priority": "P1",
		"confidence": 0.8,
		"file_path": "src/review.ts",
		"line_start": 42,
		"line_end": 42
	}))
	.expect("finding");
	assert_eq!(finding.title, "Handle null response");
	assert_eq!(finding.file_path.as_deref(), Some("src/review.ts"));
	assert_eq!(finding.line_start, Some(42));

	let normalized = normalize_findings(Some(&json!([
		{"title": "bug", "priority": "P1", "confidence": 0.9},
		{"nope": 1},
		"bad"
	])));
	assert_eq!(normalized.len(), 1);
	assert_eq!(normalized[0].title, "bug");
}