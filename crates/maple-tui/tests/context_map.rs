//! Port of `l/test/context-map.test.ts`.

use maple_tui::chrome::context::{plan_context_map, ContextMapCell, ContextSegmentId};

fn cell(id: ContextSegmentId, full: bool) -> ContextMapCell {
	ContextMapCell { id, full }
}

#[test]
fn chooses_the_largest_overlap_and_distinguishes_split_cells() {
	assert_eq!(
		plan_context_map(
			&[
				(ContextSegmentId::SystemPrompt, 7),
				(ContextSegmentId::ToolCalls, 8),
				(ContextSegmentId::Free, 25),
			],
			40,
			4,
			1
		),
		vec![
			cell(ContextSegmentId::SystemPrompt, true),
			cell(ContextSegmentId::ToolCalls, false),
			cell(ContextSegmentId::Free, true),
			cell(ContextSegmentId::Free, true),
		]
	);
}

#[test]
fn clamps_over_window_segments_without_a_negative_free_region() {
	// The reference also passes a negative `free` token count; the port's
	// `u64` segments cannot represent it, and the clamp drops it anyway.
	assert_eq!(
		plan_context_map(
			&[(ContextSegmentId::SystemPrompt, 75), (ContextSegmentId::ToolCalls, 75)],
			100,
			4,
			1
		),
		vec![
			cell(ContextSegmentId::SystemPrompt, true),
			cell(ContextSegmentId::SystemPrompt, true),
			cell(ContextSegmentId::SystemPrompt, true),
			cell(ContextSegmentId::ToolCalls, true),
		]
	);
}

#[test]
fn rejects_unknown_capacity_or_unusable_geometry() {
	for (window, columns, rows) in [(0, 16, 16), (100, 0, 16), (100, 16, 0)] {
		assert!(plan_context_map(&[(ContextSegmentId::Free, 100)], window, columns, rows).is_empty());
	}
}
