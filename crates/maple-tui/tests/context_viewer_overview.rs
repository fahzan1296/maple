//! Port of `l/test/context-viewer-overview.test.ts`.
//!
//! The reference expects `3.2K`-style abbreviations; the crate's
//! `format_number` groups with commas, so the assertions match the crate.

use maple_tui::overlays::context_viewer_overview::{
	context_snapcompact_lines, ContextBreakdown, SnapcompactBreakdown, SnapcompactSystemPrompt,
	SnapcompactToolResults,
};

fn breakdown_with(snapcompact: Option<SnapcompactBreakdown>) -> ContextBreakdown {
	ContextBreakdown {
		context_window: 200_000,
		used_tokens: 27_929,
		free_tokens: 172_071,
		auto_compact_buffer_tokens: 0,
		threshold_tokens: None,
		snapcompact,
	}
}

#[test]
fn renders_savings_skip_reasons_and_the_wire_total() {
	let snap = SnapcompactBreakdown {
		vision_capable: true,
		system_prompt: Some(SnapcompactSystemPrompt {
			applied: true,
			scope: "all".to_string(),
			text_tokens: 9_768,
			frames: 2,
			image_tokens: 6_600,
			saved_tokens: 3_168,
			reason: None,
		}),
		tool_results: Some(SnapcompactToolResults {
			total: 3,
			swapped: 0,
			text_tokens: 0,
			frames: 0,
			image_tokens: 0,
			saved_tokens: 0,
		}),
		saved_tokens: 3_168,
	};
	let output = context_snapcompact_lines(&breakdown_with(Some(snap))).join("\n");

	assert!(output.contains("Snapcompact (estimated wire savings)"));
	assert!(output.contains("System prompt (all): saves ~3,168 (9,768 text → 2 frames ≈ 6,600)"));
	assert!(output.contains("Tool results: none imaged (3 in history)"));
	// 27,929 logical − 3,168 saved = 24,761 on the wire.
	assert!(output.contains("Next request: ~24,761 tokens on the wire"));
}

#[test]
fn reports_text_only_models_as_inactive() {
	let snap = SnapcompactBreakdown {
		vision_capable: false,
		saved_tokens: 0,
		..Default::default()
	};
	let output = context_snapcompact_lines(&breakdown_with(Some(snap))).join("\n");
	assert!(output.contains("Snapcompact: inactive (model has no image input)"));
}

#[test]
fn omits_the_section_entirely_when_no_snapcompact_setting_is_on() {
	assert!(context_snapcompact_lines(&breakdown_with(None)).is_empty());
}
