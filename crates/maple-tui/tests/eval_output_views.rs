//! Port of `l/test/eval-output-views.test.ts`.
//!
//! `split_eval_output` is a pure function and is ported into
//! `tools/eval_execution.rs`; the renderer-entry cases stay ignored because the
//! crate's eval card has no `evalToolRenderer.renderResult`.

use maple_tui::tools::eval_execution::{
	split_eval_output, EvalOutputPart, EvalView, EvalViewEntry,
};
use serde_json::json;

fn json_view(value: serde_json::Value) -> EvalView {
	EvalView::Json(value)
}

fn entry(text: &str, view: EvalView) -> EvalViewEntry {
	EvalViewEntry { text: text.to_string(), view }
}

#[test]
fn keeps_text_around_a_view_with_bordering_newlines_trimmed() {
	let view = json_view(json!({ "a": 1 }));
	let parts = split_eval_output(
		"before\n{'a': 1}\nafter\n",
		Some(&[entry("{'a': 1}", view.clone())]),
	);
	assert_eq!(
		parts,
		vec![
			EvalOutputPart::Text("before".to_string()),
			EvalOutputPart::View(view),
			EvalOutputPart::Text("after".to_string()),
		]
	);
}

#[test]
fn consumes_duplicate_view_texts_in_order() {
	let first = json_view(json!([1]));
	let second = json_view(json!([2]));
	let views = [entry("X", first.clone()), entry("X", second.clone())];
	assert_eq!(
		split_eval_output("X\nmid\nX", Some(&views)),
		vec![
			EvalOutputPart::View(first),
			EvalOutputPart::Text("mid".to_string()),
			EvalOutputPart::View(second),
		]
	);
}

#[test]
fn skips_views_whose_text_is_not_in_the_output() {
	let kept = json_view(json!({ "b": 2 }));
	let views = [
		entry("gone", json_view(json!({ "a": 1 }))),
		entry("B", kept.clone()),
		entry("A", json_view(json!({ "late": true }))),
	];
	assert_eq!(
		split_eval_output("A\nB\n", Some(&views)),
		vec![EvalOutputPart::Text("A".to_string()), EvalOutputPart::View(kept)]
	);
	assert_eq!(
		split_eval_output("plain", None),
		vec![EvalOutputPart::Text("plain".to_string())]
	);
}

#[test]
#[ignore = "missing API: evalToolRenderer.renderResult (table/json view rendering in the eval card)"]
fn keeps_a_table_views_printed_text_beside_a_json_tree() {
}

#[test]
#[ignore = "missing API: evalToolRenderer.renderResult (json tree in place of printed text)"]
fn renders_a_json_view_as_a_tree_in_place_of_its_text() {
}
