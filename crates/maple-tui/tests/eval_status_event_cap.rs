//! Port of `l/test/eval-status-event-cap.test.ts`.
//!
//! The bounded log (`record_status_event`, `MAX_STATUS_EVENTS`, snapshot
//! protection) is ported into `tools/eval_execution.rs`; the three renderer
//! cases (dropped-count rows, MCP progress text) stay ignored because the crate
//! eval card has no status-event rendering.

use maple_tui::tools::eval_execution::{
	record_status_event, EvalStatusEvent, StatusEventLog, MAX_STATUS_EVENTS,
};
use serde_json::json;

fn event(op: &str) -> EvalStatusEvent {
	EvalStatusEvent {
		op: op.to_string(),
		..Default::default()
	}
}

#[test]
fn keeps_the_newest_discrete_events_and_counts_the_dropped_ones() {
	let mut log = StatusEventLog::default();
	let calls = MAX_STATUS_EVENTS + 1000;
	for index in 0..calls {
		record_status_event(&mut log, event("browser").with_field("detail", format!("call {index}")));
	}

	assert_eq!(log.status_events.len(), MAX_STATUS_EVENTS);
	assert_eq!(log.status_events_elided, 1000);
	assert_eq!(log.status_events[0].fields.get("detail"), Some(&json!("call 1000")));
	assert_eq!(
		log.status_events.last().unwrap().fields.get("detail"),
		Some(&json!(format!("call {}", calls - 1)))
	);
}

#[test]
fn keeps_agent_snapshots_and_the_committed_todo_result_behind_a_flood() {
	let mut log = StatusEventLog::default();
	record_status_event(&mut log, event("todo").with_field("committed", true).with_field("call", "first"));
	record_status_event(&mut log, event("agent").with_field("id", "Scout").with_field("status", "running"));
	for index in 0..MAX_STATUS_EVENTS * 3 {
		record_status_event(&mut log, event("browser").with_field("detail", format!("call {index}")));
	}
	record_status_event(&mut log, event("agent").with_field("id", "Scout").with_field("status", "completed"));

	assert!(log.status_events.contains(&event("todo").with_field("committed", true).with_field("call", "first")));
	let agents: Vec<EvalStatusEvent> = log
		.status_events
		.iter()
		.filter(|existing| existing.op == "agent")
		.cloned()
		.collect();
	assert_eq!(
		agents,
		vec![event("agent").with_field("id", "Scout").with_field("status", "completed")]
	);
	assert_eq!(log.status_events.len(), MAX_STATUS_EVENTS);
}

#[test]
fn bounds_a_loop_of_todo_calls_while_keeping_the_newest_committed_one() {
	let mut log = StatusEventLog::default();
	let calls = 50_000;
	for index in 0..calls {
		let committed = index % 1000 == 0;
		record_status_event(
			&mut log,
			event("todo")
				.with_field("committed", committed)
				.with_field("call", index.to_string()),
		);
	}

	assert_eq!(log.status_events.len(), MAX_STATUS_EVENTS);
	assert_eq!(log.status_events_elided, calls - MAX_STATUS_EVENTS);
	let committed: Vec<&str> = log
		.status_events
		.iter()
		.filter(|existing| existing.fields.get("committed").is_some_and(serde_json::Value::as_bool))
		.map(|existing| existing.fields.get("call").and_then(serde_json::Value::as_str).unwrap_or_default())
		.collect();
	assert_eq!(committed, vec!["49000"]);
}

#[test]
fn replaces_one_mcp_calls_progress_in_place() {
	let mut log = StatusEventLog::default();
	record_status_event(
		&mut log,
		event("mcp_progress").with_field("id", "call-1").with_field("tool", "mcp__search").with_field("progress", 1),
	);
	record_status_event(&mut log, event("read").with_field("chars", 10));
	record_status_event(
		&mut log,
		event("mcp_progress")
			.with_field("id", "call-1")
			.with_field("tool", "mcp__search")
			.with_field("progress", 2)
			.with_field("message", "fetching"),
	);

	assert_eq!(
		log.status_events,
		vec![
			event("mcp_progress")
				.with_field("id", "call-1")
				.with_field("tool", "mcp__search")
				.with_field("progress", 2)
				.with_field("message", "fetching"),
			event("read").with_field("chars", 10),
		]
	);
	assert_eq!(log.status_events_elided, 0);
}

#[test]
fn evicts_old_mcp_snapshots_under_the_cap() {
	let mut log = StatusEventLog::default();
	let protected = [
		event("agent").with_field("id", "Scout").with_field("status", "running"),
		event("judge_batch").with_field("id", "batch").with_field("done", 1).with_field("total", 2),
		event("todo").with_field("committed", true),
	];
	for event in &protected {
		record_status_event(&mut log, event.clone());
	}
	for index in 0..MAX_STATUS_EVENTS * 3 {
		record_status_event(
			&mut log,
			event("mcp_progress")
				.with_field("id", format!("call-{index}"))
				.with_field("tool", "mcp__search")
				.with_field("progress", index),
		);
	}

	assert_eq!(log.status_events.len(), MAX_STATUS_EVENTS);
	assert_eq!(log.status_events_elided, MAX_STATUS_EVENTS * 2 + protected.len());
	for event in &protected {
		assert!(log.status_events.contains(event), "protected event missing: {event:?}");
	}
	assert_eq!(
		log.status_events.last().unwrap().fields.get("id"),
		Some(&json!(format!("call-{}", MAX_STATUS_EVENTS * 3 - 1)))
	);
}

#[test]
#[ignore = "missing API: eval status rendering (dropped-events count in the earlier-events row)"]
fn includes_dropped_events_in_the_rendered_earlier_events_count() {
}

#[test]
#[ignore = "missing API: eval status rendering (dropped count when only agent snapshots remain)"]
fn shows_the_dropped_count_when_only_agent_snapshots_remain() {
}

#[test]
#[ignore = "missing API: eval status rendering (MCP tool/progress text with shortened home paths)"]
fn shows_the_mcp_tool_and_progress_text() {
}
