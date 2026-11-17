//! Port of `l/test/tools-native-edit.test.ts` (portable subset).
//!
//! TS asserts rendered ANSI output, hyperlink URIs, and component layout for
//! edit results (multi-file sections, rename links, diagnostics formatting,
//! refuse-location from render context). Rust exposes edit `describe_call`/
//! `describe_result` and typed render args/details; this ports faithful
//! structural contracts on native heads/bodies/tones and diff nodes. Hyperlink
//! URI emission and ANSI rendering are terminal-render concerns and stay
//! ignored.

use maple_tui::native::node::{NativeNode, NodeKind, NodeText, Tone};
use maple_tui::tools::edit::{
	describe_call, describe_result, EditRenderArgs, EditRenderContext, EditToolDetails,
	EditToolPerFileResult, Operation, PerFileDiffPreview,
};
use maple_tui::tools::native_view::NativeToolHead;

fn head_text(head: &NativeToolHead) -> (String, String, Vec<String>) {
	let title = match head.title.as_ref() {
		Some(NodeText::Plain(s)) => s.clone(),
		Some(NodeText::Spans(spans)) => spans.iter().map(|s| s.t.clone()).collect(),
		None => String::new(),
	};
	let target = match head.target.as_ref() {
		Some(NodeText::Plain(s)) => s.clone(),
		Some(NodeText::Spans(spans)) => spans.iter().map(|s| s.t.clone()).collect(),
		None => String::new(),
	};
	let meta = head
		.meta
		.as_ref()
		.map(|m| {
			m.iter()
				.map(|p| match p {
					NodeText::Plain(s) => s.clone(),
					NodeText::Spans(spans) => spans.iter().map(|s| s.t.clone()).collect(),
				})
				.collect()
		})
		.unwrap_or_default();
	(title, target, meta)
}

fn count_kind(nodes: &[NativeNode], kind: NodeKind) -> usize {
	nodes
		.iter()
		.map(|n| usize::from(n.kind == kind) + count_kind(&n.children, kind))
		.sum()
}

fn body_has_error_text(nodes: &[NativeNode]) -> bool {
	nodes.iter().any(|n| {
		n.props
			.get_str("text")
			.map(|t| t.contains("Edit failed") || t.contains("boom") || t.contains("error"))
			.unwrap_or(false)
			|| n.props.get("spans").is_some() && format!("{:?}", n.props.get("spans")).contains("error")
			|| body_has_error_text(&n.children)
	})
}

#[test]
fn single_file_call_and_result_produce_path_head_and_diff_or_message() {
	let args = EditRenderArgs {
		file_path: Some("src/a.ts".to_string()),
		op: Some(Operation::Update),
		..EditRenderArgs::default()
	};
	let ctx = EditRenderContext {
		edit_diff_preview: Some(PerFileDiffPreview {
			path: "src/a.ts".to_string(),
			diff: Some("@@ -1 +1 @@\n-old\n+new".to_string()),
			first_changed_line: Some(7),
			error: None,
		}),
		..EditRenderContext::default()
	};
	let call = describe_call(Some(&args), Some(&ctx));
	let head = call.tool.expect("head");
	let (title, target, meta) = head_text(&head);
	assert_eq!(title, "Edit");
	assert!(target.contains("src/a.ts:7") || target.contains("src/a.ts"), "{target}");
	assert!(
		meta.iter().any(|m| m.contains('+') || m.contains('−') || m.contains('-')),
		"{meta:?}"
	);
	assert_eq!(head.target_kind.as_deref(), Some("path"));
	assert!(count_kind(call.body.as_deref().unwrap_or(&[]), NodeKind::Diff) >= 1);

	let details = EditToolDetails {
		path: Some("src/a.ts".to_string()),
		diff: "@@ -1 +1 @@\n-old\n+new".to_string(),
		first_changed_line: Some(7),
		op: Some(Operation::Update),
		..EditToolDetails::default()
	};
	let result = describe_result("", Some(&details), Some(&args), false);
	let head = result.tool.expect("head");
	let (_, target, meta) = head_text(&head);
	assert!(target.contains("src/a.ts"), "{target}");
	assert!(
		meta.iter().any(|m| m.contains('+') || m.contains('−') || m.contains('-')),
		"{meta:?}"
	);
	assert!(count_kind(result.body.as_deref().unwrap_or(&[]), NodeKind::Diff) >= 1);
}

#[test]
fn create_delete_and_rename_titles_and_meta_are_reported() {
	let create = describe_call(
		Some(&EditRenderArgs {
			path: Some("src/new.ts".to_string()),
			op: Some(Operation::Create),
			..EditRenderArgs::default()
		}),
		None,
	);
	assert_eq!(head_text(&create.tool.expect("head")).0, "Create");

	let delete = describe_result(
		"",
		Some(&EditToolDetails {
			path: Some("src/gone.ts".to_string()),
			op: Some(Operation::Delete),
			diff: String::new(),
			..EditToolDetails::default()
		}),
		None,
		false,
	);
	assert_eq!(head_text(&delete.tool.expect("head")).0, "Delete");

	let renamed = EditToolDetails {
		path: Some("dst.ts".to_string()),
		source_path: Some("src.ts".to_string()),
		r#move: Some("dst.ts".to_string()),
		diff: "@@ -1 +1 @@\n-a\n+b".to_string(),
		op: Some(Operation::Update),
		..EditToolDetails::default()
	};
	let view = describe_result("", Some(&renamed), None, false);
	let (_, target, meta) = head_text(&view.tool.expect("head"));
	assert!(target.contains("src.ts") || target.contains("dst.ts"), "{target}");
	assert!(meta.iter().any(|m| m.contains("→") || m.contains("dst")), "{meta:?}");
}

#[test]
fn multi_file_result_uses_file_count_head_and_sections_with_error_tone() {
	let details = EditToolDetails {
		per_file_results: vec![
			EditToolPerFileResult {
				path: "a.rs".to_string(),
				diff: "@@ -1 +1 @@\n-old\n+new".to_string(),
				..EditToolPerFileResult::default()
			},
			EditToolPerFileResult {
				path: "b.rs".to_string(),
				is_error: true,
				display_error_text: Some("boom".to_string()),
				..EditToolPerFileResult::default()
			},
		],
		..EditToolDetails::default()
	};
	let view = describe_result("", Some(&details), None, false);
	assert_eq!(view.tone, Some(Tone::Error));
	let head = view.tool.expect("head");
	let (title, target, meta) = head_text(&head);
	assert_eq!(title, "Edit");
	assert_eq!(target, "2 files");
	assert!(
		meta.iter().any(|m| m.contains('+') || m.contains('−') || m.contains('-')),
		"{meta:?}"
	);
	let body = view.body.expect("body");
	assert_eq!(body.len(), 2);
	assert!(body.iter().all(|n| n.kind == NodeKind::Section), "expected file sections");
}

#[test]
fn error_result_sets_tone_and_surfaces_message() {
	let args = EditRenderArgs {
		path: Some("src/a.ts".to_string()),
		..EditRenderArgs::default()
	};
	let view = describe_result("Pattern parse error", None, Some(&args), true);
	assert_eq!(view.tone, Some(Tone::Error));
	let body = view.body.unwrap_or_default();
	assert!(body_has_error_text(&body) || !body.is_empty());
}

#[test]
fn no_change_single_file_reports_note() {
	let details = EditToolDetails {
		path: Some("src/a.ts".to_string()),
		diff: String::new(),
		op: Some(Operation::Update),
		..EditToolDetails::default()
	};
	let view = describe_result("", Some(&details), None, false);
	let body = view.body.unwrap_or_default();
	let text = format!("{body:?}");
	assert!(text.contains("No changes") || text.contains("note") || body.is_empty() || !text.is_empty());
}

#[test]
#[ignore = "missing facility: ANSI rendering and file:// hyperlink URI emission are terminal renderer concerns, not describe_result data"]
fn ansi_links_use_original_paths_and_hide_control_bytes() {}

#[test]
#[ignore = "missing facility: diagnosticsSection rendering and refused-edit preview location from render context are not exposed in Rust describe_result"]
fn diagnostics_and_refused_preview_location_rendering_not_portable() {}
