//! Port of `l/test/memory-render.test.ts`.
//!
//! The reference asserts on rendered component lines; the crate exposes the
//! pure retain/recall/reflect summaries those lines are built from, so the
//! data-level contract is pinned instead.

use serde_json::json;

use maple_tui::tools::memory::{
	parse_recall_found, retain_contents, summarize_recall, summarize_reflect, summarize_retain,
	truncate_preview, RetainRenderArgs,
};

fn retain_args(items: serde_json::Value) -> Option<RetainRenderArgs> {
	Some(RetainRenderArgs { items: Some(items) })
}

#[test]
fn retain_extracts_one_content_string_per_item_and_keeps_the_summary() {
	let args = retain_args(json!([
		{ "content": "First fact to remember" },
		{ "content": "Second fact to remember" },
		{ "content": "Third fact to remember" }
	]));
	let summary = summarize_retain(args.as_ref(), "3 memories stored.", false).expect("retain ok");
	assert_eq!(
		summary.contents,
		vec![
			"First fact to remember".to_string(),
			"Second fact to remember".to_string(),
			"Third fact to remember".to_string()
		]
	);
	assert_eq!(summary.summary.as_deref(), Some("3 memories stored"));
}

#[test]
fn retain_ignores_transient_non_object_items_while_streaming() {
	let args = retain_args(json!(["noise", { "content": "Kept fact" }, 7, { "content": "   " }]));
	assert_eq!(retain_contents(args.as_ref()), vec!["Kept fact".to_string()]);
}

#[test]
fn retain_without_an_item_array_has_nothing_to_show() {
	assert!(retain_contents(None).is_empty());
	assert!(retain_contents(Some(&RetainRenderArgs { items: Some(json!("pending")) })).is_empty());
}

#[test]
fn long_memory_content_is_truncated_to_a_single_line() {
	let long = "x".repeat(200);
	let truncated = truncate_preview(&long, 60);
	assert_eq!(truncated.chars().count(), 60);
	assert!(truncated.ends_with('…'));
	assert_eq!(truncate_preview("short", 60), "short");
}

#[test]
fn recall_counts_matches_and_hides_the_body_until_expanded() {
	assert_eq!(parse_recall_found("Found 12 relevant memories for \"retry\"."), 12);
	assert_eq!(parse_recall_found("no matches here"), 0);

	let summary = summarize_recall(
		Some("retry"),
		"Found 12 relevant memories for \"retry\".\nfirst body line",
		false,
	)
	.expect("recall ok");
	assert_eq!(summary.found, 12);
	assert_eq!(summary.body, "first body line");
	assert_eq!(summary.query.as_deref(), Some("retry"));
}

#[test]
fn an_empty_recall_yields_no_matches_and_no_body() {
	let summary = summarize_recall(Some("nothing"), "", false).expect("recall ok");
	assert_eq!(summary.found, 0);
	assert!(summary.body.is_empty());

	let error = summarize_recall(Some("q"), "boom", true).expect_err("error");
	assert_eq!(error, "boom");
}

#[test]
fn reflect_splits_the_answer_into_nonempty_lines_under_its_query() {
	let summary = summarize_reflect(Some("why?"), "line one\n\nline two\n", false).expect("reflect ok");
	assert_eq!(summary.answer_lines, vec!["line one".to_string(), "line two".to_string()]);
	assert_eq!(summary.query.as_deref(), Some("why?"));

	let error = summarize_reflect(None, "", true).expect_err("error");
	assert_eq!(error, "Reflect failed");
}
