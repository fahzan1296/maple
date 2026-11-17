//! Port of `l/test/tool-execution-preview-coalesce.test.ts` (portable subset).
//!
//! TS asserts `ToolExecutionComponent.updateStreamPreview` batching/coalescing:
//! new preview batches replace the rendered diff, empty batches retain the prior
//! preview, and late preview batches after a terminal result are rejected. The
//! Rust `chat::tool_execution::ToolExecutionComponent` in this tree does not
//! expose an `update_stream_preview` API or an observable preview-batch/render
//! seam with that contract (stream preview handling is internal/absent from the
//! public handle surface inspected), so a faithful count/render assertion would
//! require new infrastructure.
//!
//! This file ports the only directly representable state contract: a settled
//! terminal result makes the card non-partial/finalized in the exposed API, and
//! subsequent state changes that do not mark a new partial frame must not turn
//! it back into a streaming/pending block. This is a narrow, faithful subset of
//! the "reject late streaming work after terminal result" intent.
//!
//! Deviations:
//! - No public `updateStreamPreview`/generation/files preview API and no
//!   render-spy seam; preview replacement/retention/count assertions are not
//!   representable and therefore not claimed here.
//! - No edit-diff rendering path assertions; uses text result content through
//!   `update_result`/`result_text` and lifecycle predicates.

use maple_tui::chat::tool_execution::{
	ToolExecutionComponent, ToolExecutionHandle, ToolExecutionOptions, ToolResultBlock, ToolResultFrame,
};
use maple_tui::theme::{builtin_theme_json, create_theme, CreateThemeOptions};
use serde_json::json;

fn theme() -> maple_tui::theme::Theme {
	let t = builtin_theme_json("dark").expect("dark");
	create_theme(t, CreateThemeOptions::default()).expect("theme")
}

fn text_frame(text: &str, is_error: bool) -> ToolResultFrame {
	ToolResultFrame {
		content: vec![ToolResultBlock {
			kind: "text".to_string(),
			text: Some(text.to_string()),
			data: None,
			mime_type: None,
		}],
		details: None,
		is_error,
	}
}

#[test]
fn terminal_result_remains_final_and_late_non_partial_updates_do_not_reopen_streaming() {
	let mut component = ToolExecutionComponent::new(
		"edit",
		Some(json!({"path": "/tmp/mod.ts", "old_string": "const a = 1;", "new_string": "const a = 2;"})),
		ToolExecutionOptions::default(),
		None,
		theme(),
		Some("call-1".to_string()),
	);
	component.set_args_complete(Some("call-1"));
	component.set_execution_started(Some("call-1"));
	component.update_result(text_frame("final", false), false, Some("call-1"));

	assert!(!component.is_partial());
	assert!(component.is_transcript_block_finalized());
	assert!(component.result_text().contains("final"));

	// A later call/result completion signal must not turn a settled non-partial
	// card back into a streaming block.
	component.set_args_complete(Some("call-1"));
	component.set_execution_started(Some("call-1"));
	assert!(!component.is_partial());
	assert!(component.is_transcript_block_finalized());
	assert!(component.result_text().contains("final"));
	assert!(!component.result_text().contains("late"));
}

#[test]
fn partial_result_stays_pending_until_terminal_frame_arrives() {
	let mut component = ToolExecutionComponent::new(
		"edit",
		Some(json!({"path": "/tmp/mod.ts"})),
		ToolExecutionOptions::default(),
		None,
		theme(),
		Some("call-2".to_string()),
	);
	component.set_execution_started(Some("call-2"));
	component.update_result(text_frame("const a = 2;", false), true, Some("call-2"));
	assert!(component.is_partial());
	assert!(!component.is_transcript_block_finalized());
	assert!(component.result_text().contains("const a = 2;"));

	component.update_result(text_frame("const a = 3;", false), false, Some("call-2"));
	assert!(!component.is_partial());
	assert!(component.is_transcript_block_finalized());
	assert!(component.result_text().contains("const a = 3;"));
	assert!(!component.result_text().contains("const a = 2;"));
}
