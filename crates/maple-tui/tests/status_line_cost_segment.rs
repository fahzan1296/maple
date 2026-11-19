//! Port of `l/test/status-line-cost-segment.test.ts`.
//!
//! Rust status-line billing is data-driven (no advisor-subscription probe
//! callback / model-registry hooks in the segment API), so this asserts the
//! observable billing text and visibility rules exposed by
//! `status_line::segments::{billing_summary, render_segment, SegmentId,
//! PricingPeriod}` and `SegmentContext`.

use maple_tui::status_line::schema::SegmentId;
use maple_tui::status_line::segments::{
	billing_summary, render_segment, ModelInfo, PricingPeriod, SegmentClock, SegmentContext,
	SessionInfo, UsageStats,
};
use maple_tui::status_line::FallbackTheme;

fn strip_ansi(s: &str) -> String {
	maple_tui::strip_ansi(s)
}

fn cost_ctx() -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.spinner_interval_ms = 80;
	ctx
}

#[test]
fn hides_without_spend_tariff_or_subscription() {
	let ctx = cost_ctx();
	assert!(billing_summary(&ctx, &FallbackTheme::new()).is_none());
	let seg = render_segment(SegmentId::Cost, &ctx);
	assert!(!seg.visible);
}

#[test]
fn shows_plain_session_cost() {
	let mut ctx = cost_ctx();
	ctx.usage_stats.cost = 0.5;
	let text = strip_ansi(&render_segment(SegmentId::Cost, &ctx).content);
	assert!(text.contains("$0.50"), "{text}");
	assert!(!text.contains("(+"));
}

#[test]
fn shows_scheduled_tariff_without_repricing_accumulated_spend() {
	let mut ctx = cost_ctx();
	ctx.usage_stats.cost = 1.25;
	ctx.session.pricing_period = Some(PricingPeriod::Peak);
	assert_eq!(billing_summary(&ctx, &FallbackTheme::new()).as_deref(), Some("$1.25 ↑"));
	ctx.session.pricing_period = Some(PricingPeriod::OffPeak);
	assert_eq!(billing_summary(&ctx, &FallbackTheme::new()).as_deref(), Some("$1.25 ↓"));
	// History and accumulated spend stay put; only the active tariff changes.
	assert_eq!(ctx.usage_stats.cost, 1.25);
	ctx.session.pricing_period = None;
	assert_eq!(billing_summary(&ctx, &FallbackTheme::new()).as_deref(), Some("$1.25"));
}

#[test]
fn shows_scheduled_tariff_before_first_paid_request_but_hides_ordinary_zero() {
	let mut ctx = cost_ctx();
	ctx.session.pricing_period = Some(PricingPeriod::OffPeak);
	let seg = render_segment(SegmentId::Cost, &ctx);
	assert!(seg.visible);
	assert_eq!(strip_ansi(&seg.content), "$0.00 ↓");
	ctx.session.pricing_period = None;
	assert!(!render_segment(SegmentId::Cost, &ctx).visible);
}

#[test]
fn keeps_tariff_adjacent_to_primary_spend_before_credits_and_advisor_billing() {
	let mut ctx = cost_ctx();
	ctx.usage_stats.cost = 1.25;
	ctx.usage_stats.premium_requests = 2.0;
	ctx.session.using_subscription = true;
	ctx.session.pricing_period = Some(PricingPeriod::Peak);
	ctx.session.advisor_cost = 0.5;
	let rendered = billing_summary(&ctx, &FallbackTheme::new()).expect("billing");
	assert!(rendered.contains("1.25"), "{rendered}");
	assert!(rendered.contains("↑"), "{rendered}");
	assert!(rendered.contains("★ 2"), "{rendered}");
	assert!(rendered.contains("0.50"), "{rendered}");
	assert!(!rendered.contains("↓"), "{rendered}");
	let up = rendered.find("↑").unwrap();
	let star = rendered.find('★').unwrap();
	let adv = rendered.rfind("0.50").unwrap();
	assert!(rendered.find("1.25").unwrap() < up && up < star && star < adv, "{rendered}");
}

#[test]
fn splits_subagent_spend_out_of_session_cost() {
	let theme = FallbackTheme::new();

	let mut ctx = cost_ctx();
	ctx.usage_stats.cost = 0.5;
	ctx.usage_stats.subagent_cost = Some(0.12);
	assert_eq!(billing_summary(&ctx, &theme).as_deref(), Some("$0.38 (+0.12)"));

	// Tree total (grandchildren/running/async agents) wins when larger.
	ctx.subagent_tree_cost = Some(0.4);
	assert_eq!(billing_summary(&ctx, &theme).as_deref(), Some("$0.38 (+0.40)"));

	// Lagging tree total never drops below task results.
	ctx.subagent_tree_cost = Some(0.05);
	assert_eq!(billing_summary(&ctx, &theme).as_deref(), Some("$0.38 (+0.12)"));

	// Subscription spend keeps icon/unit only on the session's own amount.
	let mut ctx = cost_ctx();
	ctx.session.using_subscription = true;
	ctx.usage_stats.cost = 0.38;
	ctx.subagent_tree_cost = Some(0.2);
	let rendered = billing_summary(&ctx, &theme).expect("billing");
	assert!(rendered.contains("(+0.20)"), "{rendered}");
	assert!(rendered.starts_with("(sub) ") || rendered.starts_with("S"), "{rendered}");
	assert!(!rendered.ends_with("(+0.20)$"), "{rendered}");

	// No subagent spend: no suffix.
	let mut ctx = cost_ctx();
	ctx.usage_stats.cost = 0.38;
	assert_eq!(billing_summary(&ctx, &theme).as_deref(), Some("$0.38"));
}

#[test]
fn never_repeats_billing_unit_symbol_across_session_and_advisor_spend() {
	let theme = FallbackTheme::new();

	let mut ctx = cost_ctx();
	ctx.usage_stats.cost = 0.42;
	ctx.subagent_tree_cost = Some(0.1);
	ctx.session.advisor_cost = 0.08;
	let metered = billing_summary(&ctx, &theme).expect("billing");
	assert_eq!(metered.matches('$').count(), 1, "{metered}");
	assert!(metered.starts_with("$0.42 (+0.10) + "), "{metered}");
	assert!(metered.contains("0.08"), "{metered}");

	// Differently billed advisor spend keeps its own unit.
	let mut ctx = cost_ctx();
	ctx.session.using_subscription = true;
	ctx.usage_stats.cost = 0.42;
	ctx.session.advisor_cost = 0.08;
	let rendered = billing_summary(&ctx, &theme).expect("billing");
	assert!(rendered.contains("$0.08") || rendered.contains("S0.08"), "{rendered}");
	assert!(rendered.matches('$').count() >= 1, "{rendered}");
}

#[test]
fn advisor_spend_only_still_renders() {
	let mut ctx = cost_ctx();
	ctx.session.advisor_cost = 0.25;
	let rendered = billing_summary(&ctx, &FallbackTheme::new()).expect("billing");
	assert!(strip_ansi(&render_segment(SegmentId::Cost, &ctx).content).contains("0.25"));
	assert!(rendered.contains("0.25"), "{rendered}");
}

// Keep types referenced so the port stays tied to real API surfaces.
#[allow(dead_code)]
fn _type_surface(_: SegmentClock, _: SessionInfo, _: UsageStats, _: ModelInfo) {}
