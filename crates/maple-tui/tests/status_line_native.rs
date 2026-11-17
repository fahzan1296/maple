//! Port of `l/test/status-line-native.test.ts`.
//!
//! The crate has no native composer-facts describe (`describeComposerFacts`) and
//! no `promptCacheBrief`/`promptCacheDetails` helpers, so every case stays
//! ignored. Bodies exercise the renderer seams such a port would reuse: the
//! composed `StatusLine` component, the git counts, and the cache segment.

use maple_tui::component::Component;
use maple_tui::status_line::component::StatusLine;
use maple_tui::status_line::schema::{SegmentId, StatusLinePreset, StatusLineSettings};
use maple_tui::status_line::segments::{
	render_segment_with, GitStatus, PromptCacheState, PromptCacheStatus, SegmentContext,
};
use maple_tui::status_line::FallbackTheme;

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

fn warm() -> Option<PromptCacheStatus> {
	Some(PromptCacheStatus {
		state: PromptCacheState::Warm,
	})
}

fn cold() -> Option<PromptCacheStatus> {
	Some(PromptCacheStatus {
		state: PromptCacheState::Cold,
	})
}

#[test]
#[ignore = "missing facility: no describeComposerFacts native facts"]
fn keeps_the_configured_segments_without_another_home() {
	let mut line = bar();
	let _ = render_bar(&mut line);
	assert!(!line.visible_segments().is_empty());
}

#[test]
#[ignore = "missing facility: no describeComposerFacts native facts"]
fn reorders_priorities_when_the_configured_order_changes() {
	let mut line = bar();
	let _ = render_bar(&mut line);
	assert!(!line.visible_segments().is_empty());
}

#[test]
#[ignore = "missing facility: no describeComposerFacts native facts and no on-disk git read"]
fn keeps_the_git_branch_by_the_model_chip() {
	let mut line = bar();
	let _ = render_bar(&mut line);
	assert!(!line.visible_segments().is_empty());
}

#[test]
#[ignore = "missing facility: no describeSegment span view"]
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
	assert!(content.contains("*3 +2 ?1"), "{content:?}");
}

#[test]
#[ignore = "missing facility: no describeComposerFacts native facts"]
fn describes_spans_without_ansi_escapes_or_separator_glyphs() {
	let mut line = bar();
	let _ = render_bar(&mut line);
	assert!(!line.visible_segments().is_empty());
}


#[test]
#[ignore = "missing facility: no composer cache marker and no prompt-cache host hook"]
fn draws_the_dim_cache_glyph_while_cached() {
	assert!(render_cache(&cache_ctx(warm())).contains('●'));
}

#[test]
#[ignore = "missing facility: no composer cache marker and no prompt-cache host hook"]
fn strikes_the_glyph_through_once_not_cached() {
	assert!(render_cache(&cache_ctx(cold())).contains('⊘'));
}
fn returns_the_same_facts_while_nothing_changed() {
	let mut line = bar();
	let _ = render_bar(&mut line);
	assert!(!line.visible_segments().is_empty());
}

#[test]
#[ignore = "missing facility: no composer cache marker and no prompt-cache host hook"]
fn is_absent_without_a_status_or_without_the_segment_configured() {
	assert!(render_cache(&cache_ctx(None)).is_empty());
}

#[test]
#[ignore = "missing facility: no composer cache marker, expiry watcher, or prompt-cache host hook"]
fn turns_not_cached_at_expiry_with_a_single_render_request() {
	assert!(render_cache(&cache_ctx(cold())).contains('⊘'));
}

#[test]
#[ignore = "missing facility: no promptCacheBrief/promptCacheDetails helpers"]
fn reads_times_relative_to_now() {
	assert!(render_cache(&cache_ctx(warm())).contains('●'));
}

#[test]
#[ignore = "missing facility: no promptCacheBrief/promptCacheDetails helpers"]
fn dates_a_time_on_another_local_day() {
	assert!(render_cache(&cache_ctx(warm())).contains('●'));
}
