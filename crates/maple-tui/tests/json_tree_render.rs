//! Port of the `formatArgsInline` cases in `l/test/json-tree-render.test.ts`.
//!
//! The reference's `describeJsonTree` collapses containers into structural tree
//! rows; the crate has no such renderer (`describe_json_tree` is a text dump), so
//! the tree-row tests are not portable and one ignored test names the facility.

use maple_tui::text_shape::visible_width;
use maple_tui::tools::json_tree::{InlineFormatOptions, format_args_inline};

fn inline(args: serde_json::Value, max_width: usize) -> String {
	let object = args.as_object().expect("object args").clone();
	format_args_inline(&object, max_width, &InlineFormatOptions::default())
}

#[test]
fn a_trailing_scalar_grows_into_the_available_width_instead_of_a_fixed_cap() {
	let note = "x".repeat(200);
	let narrow = inline(serde_json::json!({ "severity": "concern", "note": note }), 40);
	let wide = inline(serde_json::json!({ "severity": "concern", "note": note }), 120);
	assert!(visible_width(&wide) > visible_width(&narrow) + 40);
	assert!(visible_width(&narrow) <= 40);
	assert!(visible_width(&wide) <= 120);
}

#[test]
fn every_key_stays_visible_even_when_a_leading_value_is_long() {
	let out = inline(
		serde_json::json!({ "path": "x".repeat(200), "pattern": "needle", "limit": 5 }),
		80,
	);
	assert!(out.contains("path="));
	assert!(out.contains("pattern="));
	assert!(out.contains("limit="));
	assert!(visible_width(&out) <= 80);
}

#[test]
fn short_values_render_fully_without_truncation_markers() {
	assert_eq!(
		inline(serde_json::json!({ "a": "x", "b": 5, "c": true }), 80),
		"a=\"x\", b=5, c=true"
	);
}

#[test]
fn hidden_meta_keys_are_skipped() {
	assert_eq!(
		inline(
			serde_json::json!({ "i": "noise", "__partialJson": "{}", "path": "src/foo.ts" }),
			80,
		),
		"path=\"src/foo.ts\""
	);
}

#[test]
#[ignore = "missing facility: describeJsonTree structural tree renderer"]
fn tree_rows_collapse_containers_and_align_indices() {
	let counts = serde_json::json!([
		[[3, "daemon_wire_decode"], 121],
		[[6, "vt"], 98],
	]);
	let described = maple_tui::tools::json_tree::describe_json_tree(&counts, 2, 10);
	assert!(described.props.get_str("text").is_some());
}
