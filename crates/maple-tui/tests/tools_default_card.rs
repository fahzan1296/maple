//! Port of `l/test/tools-default-card.test.ts`.
//!
//! Rust exposes the generic fallback as pure model/describe helpers in
//! `tools::default_renderer` (status/phase, output mode, native view) rather
//! than a themed terminal text formatter. This ports the faithful subset:
//! output-mode decisions, large/raw fidelity in `describe_default_tool_execution`,
//! JSON safety/preview, status/tone, args summary, and plain status formatting.
//! Cases requiring the TS themed `formatDefaultToolExecution` text body stay
//! ignored with explicit reasons.

use maple_tui::native::node::{NodeKind, NodeText, Tone};
use maple_tui::tools::default_renderer::{
	build_default_tool_status, decide_output_mode, describe_default_tool_execution,
	format_args_inline, format_default_tool_execution_plain, is_tree_safe_json, DefaultOutputMode,
	DefaultRenderOptions, DefaultToolRenderInput, DefaultToolRenderResult, ToolCardPhase,
	ToolUiStatus, NATIVE_ARGS_SUMMARY_CHARS,
};
use maple_tui::tools::native_view::NativePreview;
use serde_json::json;

fn mcp_result(output: &str, expanded: bool, json_mode: bool) -> DefaultToolRenderInput {
	DefaultToolRenderInput {
		label: "srv/mcp_tool".to_string(),
		args: Some(json!({"query": "tokyo"})),
		result: Some(DefaultToolRenderResult {
			output: output.to_string(),
			is_error: false,
			skipped: false,
		}),
		options: DefaultRenderOptions {
			expanded,
			is_partial: false,
			spinner_frame: None,
			json: json_mode,
		},
	}
}

fn body_text(view: &maple_tui::tools::native_view::NativeToolView) -> Option<String> {
	let body = view.body.as_ref()?;
	let first = body.first()?;
	if first.kind != NodeKind::Code && first.kind != NodeKind::Ansi {
		return None;
	}
	first.props.get_str("text").map(str::to_string)
}

#[test]
fn status_and_plain_format_follow_result_state() {
	let ok = mcp_result("row", false, false);
	let (s, p) = build_default_tool_status(&ok);
	assert_eq!(s.icon, ToolUiStatus::Success);
	assert_eq!(p, ToolCardPhase::Success);
	assert_eq!(format_default_tool_execution_plain(&ok), "[success] srv/mcp_tool");

	let skipped = DefaultToolRenderInput {
		label: "srv/mcp_tool".to_string(),
		args: None,
		result: Some(DefaultToolRenderResult {
			output: String::new(),
			is_error: false,
			skipped: true,
		}),
		options: DefaultRenderOptions::default(),
	};
	let (s, p) = build_default_tool_status(&skipped);
	assert_eq!(s.icon, ToolUiStatus::Info);
	assert_eq!(p, ToolCardPhase::Info);
	assert!(s.title_muted);
	assert_eq!(format_default_tool_execution_plain(&skipped), "[info] srv/mcp_tool");

	let err = DefaultToolRenderInput {
		result: Some(DefaultToolRenderResult {
			output: "bad".to_string(),
			is_error: true,
			skipped: false,
		}),
		..ok.clone()
	};
	assert_eq!(build_default_tool_status(&err).0.icon, ToolUiStatus::Error);
	assert_eq!(format_default_tool_execution_plain(&err), "[error] srv/mcp_tool");
}

#[test]
fn args_inline_is_sorted_and_budgeted() {
	let s = format_args_inline(&json!({"b": 2, "a": "x"}), 50).unwrap();
	assert_eq!(s, "a=x, b=2");
	let long = format_args_inline(&json!({"k": "x".repeat(400)}), NATIVE_ARGS_SUMMARY_CHARS).unwrap();
	assert!(long.chars().count() <= NATIVE_ARGS_SUMMARY_CHARS);
}

#[test]
fn output_mode_respects_tree_safe_json_and_unsafe_integers() {
	assert!(is_tree_safe_json("{\"a\":1}"));
	assert!(!is_tree_safe_json("{\"a\":1234567890123456}"));
	assert_eq!(decide_output_mode("{\"a\":1}"), DefaultOutputMode::Json);
	assert_eq!(decide_output_mode("plain"), DefaultOutputMode::Raw);
	assert_eq!(decide_output_mode("   "), DefaultOutputMode::Empty);
}

#[test]
fn large_result_stays_whole_in_native_view_and_keeps_raw_end() {
	// 100 KiB of newline-joined text: past an old 64 KiB construction-time cut.
	let rows: Vec<String> = (0..4096).map(|i| format!("line-{i}-{}", "x".repeat(20))).collect();
	let output = format!("{}\n", rows.join("\n"));
	assert!(output.len() > 64 * 1024);

	let view = describe_default_tool_execution(&mcp_result(&output, false, false));
	let body = view.body.expect("body");
	assert_eq!(body.len(), 1);
	assert_eq!(body[0].kind, NodeKind::Ansi);
	let text = body_text(&view).expect("ansi text");
	assert_eq!(text, output.trim_end());
	assert!(text.len() > 64 * 1024);
	assert!(text.ends_with(&format!("line-4095-{}", "x".repeat(20))));
	assert_eq!(view.preview, Some(NativePreview::Lines(12)));
}

#[test]
fn json_output_becomes_pretty_code_and_unsafe_int_stays_raw() {
	let view = describe_default_tool_execution(&mcp_result("{\"a\":1}", true, false));
	let body = view.body.expect("body");
	assert_eq!(body[0].kind, NodeKind::Code);
	assert_eq!(body[0].props.get_str("language"), Some("json"));
	assert!(body_text(&view).unwrap().contains("\"a\": 1"));

	let unsafe_view = describe_default_tool_execution(&mcp_result(
		"{\"id\": 9223372036854775807}",
		true,
		false,
	));
	let unsafe_body = unsafe_view.body.expect("body");
	assert_eq!(unsafe_body[0].kind, NodeKind::Ansi);
	assert!(body_text(&unsafe_view).unwrap().contains("9223372036854775807"));
}

#[test]
fn json_mode_requests_json_code_node_for_safe_values() {
	let view = describe_default_tool_execution(&mcp_result("{\"query\":\"tokyo\",\"i\":\"kept\"}", true, true));
	let body = view.body.expect("body");
	assert_eq!(body[0].kind, NodeKind::Code);
	assert_eq!(body[0].props.get_str("language"), Some("json"));
	assert!(body_text(&view).unwrap().contains("\"kept\""));
}

#[test]
fn skipped_and_error_tones_and_heads_are_set() {
	let skipped = DefaultToolRenderInput {
		label: "srv/mcp_tool".to_string(),
		args: None,
		result: Some(DefaultToolRenderResult {
			output: String::new(),
			is_error: false,
			skipped: true,
		}),
		options: DefaultRenderOptions::default(),
	};
	let view = describe_default_tool_execution(&skipped);
	assert_eq!(view.tone, Some(Tone::Muted));
	assert_eq!(view.preview, Some(NativePreview::None));
	assert!(matches!(view.head, Some(NodeText::Spans(_))));

	let err = DefaultToolRenderInput {
		label: "srv/mcp_tool".to_string(),
		args: Some(json!({"q":"x"})),
		result: Some(DefaultToolRenderResult {
			output: "boom".to_string(),
			is_error: true,
			skipped: false,
		}),
		options: DefaultRenderOptions::default(),
	};
	let view = describe_default_tool_execution(&err);
	assert_eq!(view.tone, Some(Tone::Error));
	assert!(view.body.is_some());
}

#[test]
fn empty_or_absent_result_has_no_body() {
	let pending = DefaultToolRenderInput {
		label: "srv/mcp_tool".to_string(),
		args: None,
		result: None,
		options: DefaultRenderOptions::default(),
	};
	assert!(describe_default_tool_execution(&pending).body.is_none());

	let empty = DefaultToolRenderInput {
		result: Some(DefaultToolRenderResult {
			output: "   ".to_string(),
			is_error: false,
			skipped: false,
		}),
		..pending
	};
	assert!(describe_default_tool_execution(&empty).body.is_none());
}

#[test]
#[ignore = "missing facility: Rust has no themed full-text formatDefaultToolExecution formatter; plain status is exposed as format_default_tool_execution_plain"]
fn expands_past_collapsed_window_in_themed_text_output() {}

#[test]
#[ignore = "missing facility: Rust describe returns native view nodes, not TS TUI component tree with JSON tree/raw routing parity across all hosts"]
fn routes_json_to_lossless_raw_text_in_every_tree_cut_case() {}
