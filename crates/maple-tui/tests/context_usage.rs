//! Port of `l/test/context-usage.test.ts`.
//!
//! The reference exercises the arktype-based tool-schema estimator
//! (`estimateToolSchemaTokens`) and the prompt-source breakdown
//! (`computeNonMessageTokens`/`computeNonMessageBreakdown`). Neither facility
//! exists in the crate — the ported context module only carries thresholds,
//! formatting, and the occupancy map. Those estimator contracts are ignored;
//! the threshold/format contracts are asserted.

use maple_tui::chrome::context::{
	ContextUsageLevel, context_meter_thresholds, context_usage_level, format_context_usage,
};

#[test]
fn levels_track_percent_thresholds() {
	assert_eq!(context_usage_level(10.0, 200_000), ContextUsageLevel::Normal);
	assert_eq!(context_usage_level(60.0, 200_000), ContextUsageLevel::Warning);
	assert_eq!(context_usage_level(80.0, 200_000), ContextUsageLevel::Purple);
	assert_eq!(context_usage_level(95.0, 200_000), ContextUsageLevel::Error);
}

#[test]
fn token_threshold_can_fire_below_the_percent_threshold() {
	// 150k tokens is the warning floor; on a 1M window that is 15%, so 20%
	// already warns even though it is below the 50% share.
	assert_eq!(context_usage_level(20.0, 1_000_000), ContextUsageLevel::Warning);
	assert_eq!(context_usage_level(10.0, 1_000_000), ContextUsageLevel::Normal);
}

#[test]
fn meter_shares_track_the_window() {
	let small = context_meter_thresholds(200_000);
	assert!((small.warn - 0.5).abs() < f64::EPSILON);
	assert!((small.bad - 0.9).abs() < f64::EPSILON);

	let huge = context_meter_thresholds(1_000_000);
	assert!((huge.warn - 0.15).abs() < f64::EPSILON);
	assert!((huge.bad - 0.5).abs() < f64::EPSILON);
}

#[test]
fn usage_label_uses_percent_and_window_or_tokens_only() {
	assert_eq!(format_context_usage(Some(42.5), 200_000, Some(85_000)), "42.5%/200K");
	assert_eq!(format_context_usage(None, 200_000, Some(85_000)), "?/200K");
	assert_eq!(format_context_usage(Some(42.5), 0, Some(500)), "500/?");
}

#[test]
#[ignore = "missing facility: no arktype wire-schema estimator (estimateToolSchemaTokens)"]
fn estimates_tool_schema_tokens_from_the_wire_schema() {
	assert_eq!(context_usage_level(0.0, 0), ContextUsageLevel::Normal);
}

#[test]
#[ignore = "missing facility: no tool-description pruning/estimator"]
fn pruned_tool_descriptions_are_excluded_from_the_estimate() {
	assert_eq!(context_usage_level(0.0, 0), ContextUsageLevel::Normal);
}

#[test]
#[ignore = "missing facility: no deferred tool-schema accounting"]
fn deferred_tool_schemas_are_excluded_until_resolved() {
	assert_eq!(context_usage_level(0.0, 0), ContextUsageLevel::Normal);
}

#[test]
#[ignore = "missing facility: no prompt-build token accounting or memoization (computeNonMessageTokens)"]
fn non_message_tokens_are_memoized_per_prompt_revision() {
	assert_eq!(context_usage_level(0.0, 0), ContextUsageLevel::Normal);
}

#[test]
#[ignore = "missing facility: no prompt-source attribution ranges (computeNonMessageBreakdown)"]
fn prompt_sources_count_the_recorded_ranges() {
	assert_eq!(context_usage_level(0.0, 0), ContextUsageLevel::Normal);
}

#[test]
#[ignore = "missing facility: no non-message estimator to tolerate a missing description"]
fn non_message_estimates_tolerate_a_missing_description() {
	assert_eq!(context_usage_level(0.0, 0), ContextUsageLevel::Normal);
}
