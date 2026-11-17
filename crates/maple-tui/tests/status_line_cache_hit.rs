//! Port of `l/test/status-line-cache-hit.test.ts`.
//!
//! The hit-rate math lives in `cache_hit_segment`; every case is a real
//! assertion against `FallbackTheme`, matching the reference's theme-derived
//! expectations. ANSI wraps the percentage but the digits stay contiguous.

use maple_tui::status_line::schema::SegmentId;
use maple_tui::status_line::segments::{render_segment_with, SegmentContext, UsageStats};
use maple_tui::status_line::FallbackTheme;

fn ctx_with(usage: UsageStats) -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.usage_stats = usage;
	ctx
}

#[test]
fn computes_hit_rate_over_the_full_prompt_deepseek_miss_lives_in_input() {
	let theme = FallbackTheme::new();
	let mut usage = UsageStats::default();
	usage.cache_read = 800;
	usage.cache_write = 0;
	usage.input = 200;
	let result = render_segment_with(SegmentId::CacheHit, &theme, &ctx_with(usage));
	assert!(result.visible);
	assert!(result.content.contains("80.00%"), "{}", result.content);
}

#[test]
fn counts_uncached_input_in_the_denominator_alongside_cache_write() {
	let theme = FallbackTheme::new();
	let mut usage = UsageStats::default();
	usage.cache_read = 600;
	usage.cache_write = 300;
	usage.input = 100;
	let result = render_segment_with(SegmentId::CacheHit, &theme, &ctx_with(usage));
	assert!(result.visible);
	assert!(result.content.contains("60.00%"), "{}", result.content);
}

#[test]
fn is_hidden_until_there_is_a_cache_read_even_with_uncached_input() {
	let theme = FallbackTheme::new();
	let mut usage = UsageStats::default();
	usage.input = 5_000;
	let result = render_segment_with(SegmentId::CacheHit, &theme, &ctx_with(usage));
	assert!(!result.visible);
	assert_eq!(result.content, "");
}
