//! Port of `l/test/compaction-divider.test.ts`.
//!
//! Deviations: the reference's `formatNumber` comma-grouping became the crate's
//! compact `format_tokens` (`48.6K`), so the missing-usage case asserts the
//! sentence shape rather than `48,573`; the pi-tui same-reference render cache
//! has no crate equivalent (see the ignored test).

use maple_tui::chat::compaction_summary_message::CompactionSummaryMessageComponent;
use maple_tui::chat::messages::CompactionSummaryMessage;
use maple_tui::component::Component;
use maple_tui::text_shape::visible_width;
use maple_tui::theme::default_theme;

const SUMMARY: &str = "Earlier the user fixed the login TTL bug.";

fn message(tokens_before: usize, tokens_after: Option<usize>, method: Option<&str>) -> CompactionSummaryMessage {
	CompactionSummaryMessage {
		summary: SUMMARY.to_string(),
		tokens_before,
		tokens_after,
		method: method.map(str::to_string),
		warning: None,
		images: Vec::new(),
	}
}

fn component(message: CompactionSummaryMessage) -> CompactionSummaryMessageComponent {
	CompactionSummaryMessageComponent::new(message, default_theme())
}

#[test]
fn collapsed_is_a_single_full_width_divider_carrying_the_expand_affordance() {
	let mut component = component(message(84_000, None, None));
	let lines = component.render(80);
	assert_eq!(lines.len(), 3, "breathing room above and below the rule");
	let rule = maple_tui::strip_ansi(&lines[1]);
	assert!(rule.contains("compacted"), "{rule}");
	assert_eq!(visible_width(&rule), 80, "the rule spans the full width");
	assert!(!rule.contains(SUMMARY), "the summary body stays hidden");
}

#[test]
fn names_the_compaction_method_and_the_before_to_after_amounts_on_the_divider() {
	let mut component = component(message(256_000, Some(20_000), Some("remote")));
	let rule = maple_tui::strip_ansi(&component.render(80)[1]);
	assert!(rule.contains("remote-compacted"), "{rule}");
	assert!(rule.contains("256K→20K"), "{rule}");
}

#[test]
fn labels_a_handoff_method_compaction_as_handed_off() {
	let mut component = component(message(84_000, None, Some("handoff")));
	let rule = maple_tui::strip_ansi(&component.render(80)[1]);
	assert!(rule.contains("handed-off"), "{rule}");
	assert!(!rule.contains('→'), "no amount badge without tokensAfter: {rule}");
}

#[test]
fn does_not_render_missing_pre_compaction_usage_as_a_literal_zero() {
	let mut component = component(message(0, Some(48_573), Some("handoff")));
	component.inner_mut().set_expanded(true);
	let text = maple_tui::strip_ansi(&component.render(80).join("\n"));
	assert!(text.contains("Compacted to"), "{text}");
	assert!(!text.contains("Compacted from 0"), "{text}");
}

#[test]
fn expanded_reveals_the_summary_and_snapcompact_frame_count_below_the_divider() {
	let mut summary = message(84_000, None, None);
	summary.images = vec!["frame-1".to_string()];
	let mut component = component(summary);
	component.inner_mut().set_expanded(true);
	let text = maple_tui::strip_ansi(&component.render(80).join("\n"));
	assert!(text.contains("compacted"), "{text}");
	assert!(text.contains(SUMMARY), "{text}");
	assert!(text.contains("tokens"), "{text}");
	assert!(text.contains("snapcompact frame attached"), "{text}");
}

#[test]
fn degrades_to_a_bare_label_when_the_viewport_is_too_narrow_for_a_framed_rule() {
	let mut component = component(message(84_000, None, None));
	let lines = component.render(10);
	assert!(maple_tui::strip_ansi(&lines[1]).contains("compacted"));
}

#[test]
#[ignore = "missing facility: the crate's Component::render returns fresh rows; there is no same-reference memo to assert"]
fn honors_the_same_reference_render_cache_and_busts_it_on_expansion_toggle() {
	let mut component = component(message(84_000, None, None));
	let first = component.render(80);
	let second = component.render(80);
	assert_eq!(
		visible_width(&maple_tui::strip_ansi(&first.concat())),
		visible_width(&maple_tui::strip_ansi(&second.concat()))
	);
}
