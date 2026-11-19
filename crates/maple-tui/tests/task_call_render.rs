//! Port of `l/test/task-call-render.test.ts` (portable subset).
//!
//! Divergence: TS exercised streaming TUI call preview (component tree, themed
//! badges, markdown body, partial args, hasResult suppression, stepped items,
//! context-before-agents layout). Rust exposes plain/native helpers
//! (`render_call`, `describe_call`) without streaming/component rendering; this
//! subset asserts the exported preview/headline semantics that do exist.

use maple_tui::tools::task::{describe_call, render_call};

#[test]
fn render_call_shows_agent_and_first_task_line_only() {
	let out = render_call(
		Some("reviewer"),
		Some("Review packages/server/src/auth for missing 401 handling.\nReport findings."),
		None,
	);
	assert!(out.starts_with("reviewer") || out.contains("reviewer"));
	assert!(out.contains("Review packages/server/src/auth for missing 401 handling."));
	assert!(!out.contains("Report findings."));
}

#[test]
fn render_call_defaults_agent_name_to_task() {
	let out = render_call(None, Some("do work"), None);
	assert!(out.starts_with("task"));
	assert!(out.contains("do work"));
}

#[test]
fn render_call_batch_count_and_describe_details() {
	let out = render_call(Some("task"), Some("ctx"), Some(2));
	assert!(out.contains("(2 agents)"));
	let view = describe_call(Some("scout"), Some("map the code"), Some(2));
	assert!(view.head.is_some());
}

#[test]
fn render_call_ignores_empty_task_text() {
	let out = render_call(Some("worker"), Some("   \n  "), None);
	assert_eq!(out, "worker");
}