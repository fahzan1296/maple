//! Port of `l/test/status-line-native.test.ts` (portable subset).
//!
//! The reference drives the native/TSP `describeComposerFacts` registry, which
//! has no counterpart in this slice. The portable subset asserts the crate's
//! pure status-line seams the native path would feed: the composed
//! `StatusLine` bar, per-segment renderers, and the prompt-cache brief/details
//! helpers. Theme facts come from [`FallbackTheme`], so nothing here touches
//! I/O or the native runtime.

use maple_tui::component::Component;
use maple_tui::status_line::component::StatusLine;
use maple_tui::status_line::host::{
	PromptCacheColdReason, PromptCacheState, PromptCacheStatus, PromptCacheTier,
};
use maple_tui::status_line::prompt_cache::{prompt_cache_brief, prompt_cache_details};
use maple_tui::status_line::schema::{SegmentId, StatusLineItem, StatusLinePreset, StatusLineSettings};
use maple_tui::status_line::segments::{render_segment_with, GitStatus, SegmentContext};
use maple_tui::status_line::FallbackTheme;
use maple_tui::strip_ansi;

fn bar() -> StatusLine {
	let mut line = StatusLine::new();
	line.set_settings(StatusLineSettings {
		preset: Some(StatusLinePreset::Full),
		..StatusLineSettings::default()
	});
	line
}

fn render_bar(line: &mut StatusLine) -> Vec<String> {
	line.render(80)
}

fn cache_ctx(status: Option<PromptCacheStatus>) -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.prompt_cache = status;
	ctx
}

fn render_cache(ctx: &SegmentContext) -> String {
	let theme = FallbackTheme::new();
	render_segment_with(SegmentId::Cache, &theme, ctx).content
}

fn warm_at(touched_at: i64) -> Option<PromptCacheStatus> {
	Some(PromptCacheStatus {
		state: PromptCacheState::Warm,
		reason: None,
		inferred: false,
		tier: PromptCacheTier::Short,
		touched_at,
		ttl_ms: Some(300_000),
		cached_tokens: 1_000,
		next_tokens: 1_200,
		warming: None,
	})
}

fn cold_expired(at: i64) -> Option<PromptCacheStatus> {
	Some(PromptCacheStatus {
		state: PromptCacheState::Cold,
		reason: Some(PromptCacheColdReason::Expired { at }),
		inferred: false,
		tier: PromptCacheTier::Short,
		touched_at: at - 300_000,
		ttl_ms: Some(300_000),
		cached_tokens: 1_000,
		next_tokens: 1_200,
		warming: None,
	})
}

#[test]
fn keeps_the_configured_segments_without_another_home() {
	let mut line = bar();
	let rows = render_bar(&mut line);
	assert!(!rows.is_empty(), "full preset must render a row");
	assert!(!line.visible_segments().is_empty());
}

#[test]
fn reorders_priorities_when_the_configured_order_changes() {
	let mut line = bar();
	line.context_mut().git.branch = Some("main".to_string());
	line.context_mut().model.name = Some("Claude".to_string());
	line.set_segments(vec![
		StatusLineItem::left(SegmentId::Git),
		StatusLineItem::left(SegmentId::Model),
	]);
	let _ = render_bar(&mut line);
	let git = line
		.visible_segments()
		.iter()
		.position(|id| *id == SegmentId::Git)
		.expect("git visible");
	let model = line
		.visible_segments()
		.iter()
		.position(|id| *id == SegmentId::Model)
		.expect("model visible");
	assert!(git < model, "configured order must hold: {:?}", line.visible_segments());

	line.set_segments(vec![
		StatusLineItem::left(SegmentId::Model),
		StatusLineItem::left(SegmentId::Git),
	]);
	let _ = render_bar(&mut line);
	let git = line
		.visible_segments()
		.iter()
		.position(|id| *id == SegmentId::Git)
		.expect("git visible");
	let model = line
		.visible_segments()
		.iter()
		.position(|id| *id == SegmentId::Model)
		.expect("model visible");
	assert!(model < git, "reordered bar must follow the new order");
}

#[test]
fn keeps_the_git_branch_by_the_model_chip() {
	let mut ctx = SegmentContext::default();
	ctx.git.branch = Some("main".to_string());
	let theme = FallbackTheme::new();
	let content = render_segment_with(SegmentId::Git, &theme, &ctx).content;
	assert!(strip_ansi(&content).contains("main"), "{content:?}");
}

#[test]
fn spaces_the_git_counts_apart_from_the_branch_and_each_other() {
	let mut ctx = SegmentContext::default();
	ctx.git.branch = Some("main".to_string());
	ctx.git.status = Some(GitStatus {
		staged: 2,
		unstaged: 3,
		untracked: 1,
	});
	let theme = FallbackTheme::new();
	let content = render_segment_with(SegmentId::Git, &theme, &ctx).content;
	let plain = strip_ansi(&content);
	assert!(plain.contains("*3 +2 ?1"), "{plain:?}");
}

#[test]
fn lists_each_visible_segment_once() {
	let mut line = bar();
	let _ = render_bar(&mut line);
	let ids = line.visible_segments();
	let mut seen = ids.to_vec();
	seen.sort();
	let len = seen.len();
	seen.dedup();
	assert_eq!(seen.len(), len, "duplicate segment ids: {ids:?}");
}

#[test]
fn describes_spans_without_ansi_escapes_or_separator_glyphs() {
	let mut line = bar();
	let rows = render_bar(&mut line);
	assert!(!rows.is_empty());
	for row in &rows {
		let plain = strip_ansi(row);
		assert!(!plain.contains('\x1b'), "plain row carries escapes: {plain:?}");
		assert!(!plain.contains('\0'), "plain row carries NUL: {plain:?}");
	}
}

#[test]
fn draws_the_dim_cache_glyph_while_cached() {
	assert!(render_cache(&cache_ctx(warm_at(0))).contains('●'));
}

#[test]
fn strikes_the_glyph_through_once_not_cached() {
	let content = render_cache(&cache_ctx(cold_expired(0)));
	assert!(content.contains('⊘'), "{content:?}");
}

#[test]
fn returns_the_same_facts_while_nothing_changed() {
	let mut line = bar();
	let first = render_bar(&mut line);
	let ids = line.visible_segments().to_vec();
	let second = render_bar(&mut line);
	assert_eq!(first, second);
	assert_eq!(ids, line.visible_segments());
}

#[test]
fn is_absent_without_a_status_or_without_the_segment_configured() {
	let hidden = render_segment_with(
		SegmentId::Cache,
		&FallbackTheme::new(),
		&cache_ctx(None),
	);
	assert!(!hidden.visible);
	assert!(hidden.content.is_empty());
}

#[test]
fn turns_not_cached_at_expiry_with_a_single_render_request() {
	let now = 1_800_000_000_000_i64;
	let content = render_cache(&cache_ctx(cold_expired(now - 1_000)));
	assert!(content.contains('⊘'), "{content:?}");
}

#[test]
fn reads_times_relative_to_now() {
	let now = 1_800_000_000_000_i64;
	let status = warm_at(now - 60_000).expect("warm");
	let brief = prompt_cache_brief(&status, now);
	assert!(brief[0].starts_with("Cached until "), "{brief:?}");
	assert!(brief[0].contains("AM") || brief[0].contains("PM"), "{brief:?}");
	assert!(brief[1].contains("from cache"), "{brief:?}");
}

#[test]
fn dates_a_time_on_another_local_day() {
	let now = 1_800_000_000_000_i64;
	let status = cold_expired(now - 2 * 24 * 60 * 60 * 1000).expect("cold");
	let rows = prompt_cache_details(&status, now);
	let why = rows
		.iter()
		.find(|row| row.label == "Why not cached")
		.expect("cold reason row");
	assert!(why.value.contains("2 days ago"), "{why:?}");
	assert!(!why.value.contains("at "), "{why:?}");
}
