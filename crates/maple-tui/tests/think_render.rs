//! Port of `l/test/think-render.test.ts` (portable subset).
//!
//! Divergence: TS asserted `thinkToolRenderer.renderCall` produced themed TUI
//! components with `thinkingText` color/italic styling. Rust `tools::think`
//! exposes pure native description (`describe_think_call`) without
//! theme/component rendering; this subset asserts inline/body/streaming behavior.

use maple_tui::tools::think::{describe_think_call, describe_think_result, ThinkRenderArgs};

#[test]
fn renders_non_empty_thoughts_as_inline_markdown_body() {
	let view = describe_think_call(
		&ThinkRenderArgs {
			thoughts: Some("Cache the parsed config, then check invalidation.".into()),
		},
		true,
	);
	assert!(view.inline);
	assert_eq!(
		view.body_markdown.as_deref(),
		Some("Cache the parsed config, then check invalidation.")
	);
	assert!(!view.streaming);
}

#[test]
fn empty_or_missing_thoughts_have_no_body() {
	let empty = describe_think_call(&ThinkRenderArgs { thoughts: Some(String::new()) }, true);
	assert!(empty.body_markdown.is_none());
	assert!(!empty.streaming);

	let missing = describe_think_call(&ThinkRenderArgs::default(), true);
	assert!(missing.inline);
	assert!(missing.body_markdown.is_none());
	assert!(!missing.streaming);
}

#[test]
fn streaming_flag_follows_args_complete_and_result_not_rendered() {
	let streaming = describe_think_call(&ThinkRenderArgs { thoughts: Some("hmm".into()) }, false);
	assert!(streaming.streaming);
	assert_eq!(streaming.body_markdown.as_deref(), Some("hmm"));
	assert!(describe_think_result().is_none());
}