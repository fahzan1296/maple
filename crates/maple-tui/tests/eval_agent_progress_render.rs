//! Port of `l/test/eval-agent-progress-render.test.ts`.
//!
//! Deviations:
//! - The crate has no `evalToolRenderer.renderResult` cell box, so the agent
//!   tree is rendered directly via `render_agent_progress_events`; assertions
//!   about "below the box" become assertions that the progress tree contains
//!   the live rows.
//! - `ThinkingLevel` maps to `crate::thinking::ConfiguredThinkingLevel`.
//! - The feed-badge flag is process-global in both implementations, so tests
//!   that read or write it share a lock instead of racing.

use std::sync::{Mutex, MutexGuard};

use maple_tui::ansi_scan::strip_ansi;
use maple_tui::render::render_utils::{
	set_feed_model_badge_enabled, thinking_level_glyph,
};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::default_theme;
use maple_tui::thinking::ConfiguredThinkingLevel;
use maple_tui::tools::agent_progress::{render_agent_progress_events, AgentProgressEvent};
use maple_tui::tools::agent_tree::AgentStatus;

static BADGE_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
	BADGE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn event(id: &str, status: AgentStatus) -> AgentProgressEvent {
	AgentProgressEvent {
		id: Some(id.to_string()),
		status,
		current_tool: None,
		current_tool_args: None,
		last_intent: None,
		task_preview: None,
		tool_count: None,
		context_tokens: None,
		context_window: None,
		cost: None,
		duration_ms: None,
		resolved_model_identity: None,
		resolved_model: None,
		model: None,
		resolved_thinking_level: None,
		advisor: false,
	}
}

fn render(events: &[AgentProgressEvent], width: usize, badge: bool) -> String {
	let theme = default_theme();
	set_feed_model_badge_enabled(badge);
	render_agent_progress_events(events, &theme, width, 0).join("\n")
}

#[test]
fn draws_running_subagent_with_current_tool_and_intent() {
	let _guard = lock();
	let theme = default_theme();
	let mut ev = event("0-Scout", AgentStatus::Running);
	ev.current_tool = Some("read".to_string());
	ev.current_tool_args = Some("config.ts".to_string());
	ev.last_intent = Some("Reading config".to_string());
	ev.task_preview = Some("investigate the bug".to_string());
	ev.tool_count = Some(4);
	ev.context_tokens = Some(5000);
	ev.context_window = Some(200_000);
	ev.cost = Some(0.03);
	ev.duration_ms = Some(800);
	ev.resolved_model_identity = Some("p/model:high".to_string());
	ev.resolved_thinking_level = Some(ConfiguredThinkingLevel::High);
	ev.advisor = true;

	let below = strip_ansi(&render(std::slice::from_ref(&ev), 120, true));
	assert!(below.contains("0-Scout"));
	assert!(below.contains("read"));
	assert!(below.contains("Reading config"));
	let glyph = thinking_level_glyph(ConfiguredThinkingLevel::High, &theme);
	let advisor = &theme.icon().advisor;
	assert!(
		below.contains(&format!("{glyph} p/model:high {advisor} 0-Scout")),
		"badge row missing: {below:?}"
	);
	assert_eq!(below.matches("p/model:high").count(), 1);
	assert!(!below.contains("p/model:high:high"));
}

#[test]
fn keeps_full_stats_on_a_completed_subagent() {
	let _guard = lock();
	let theme = default_theme();
	let mut ev = event("0-Scout", AgentStatus::Completed);
	ev.tool_count = Some(7);
	ev.context_tokens = Some(8000);
	ev.context_window = Some(200_000);
	ev.cost = Some(0.06);
	ev.duration_ms = Some(1500);
	ev.model = Some("p/model:low:high".to_string());
	ev.resolved_model_identity = Some("p/model:low".to_string());
	ev.resolved_thinking_level = Some(ConfiguredThinkingLevel::High);

	let below = strip_ansi(&render(std::slice::from_ref(&ev), 120, true));
	assert!(below.contains("$0.06"), "cost missing: {below:?}");
	let glyph = thinking_level_glyph(ConfiguredThinkingLevel::High, &theme);
	assert!(below.contains(&format!("{glyph} p/model:low 0-Scout")));
	assert_eq!(below.matches("p/model:low").count(), 1);
	assert!(!below.contains("p/model:low:high"));
	assert!(!below.contains(&theme.icon().advisor));
}

#[test]
fn preserves_legacy_model_fields_without_inferring_thinking_levels() {
	let _guard = lock();
	let theme = default_theme();
	let mut legacy_model = event("LegacyModel", AgentStatus::Running);
	legacy_model.model = Some("p/model:high".to_string());
	legacy_model.resolved_model = Some("p/unused:max".to_string());

	let mut legacy_resolved = event("LegacyResolved", AgentStatus::Completed);
	legacy_resolved.resolved_model = Some("p/model:max".to_string());

	let mut literal_auto = event("LiteralAuto", AgentStatus::Completed);
	literal_auto.resolved_model_identity = Some("p/model:auto".to_string());

	let below = [
		render(std::slice::from_ref(&legacy_model), 120, true),
		render(std::slice::from_ref(&legacy_resolved), 120, true),
		render(std::slice::from_ref(&literal_auto), 120, true),
	]
	.join("\n");
	let below = strip_ansi(&below);

	assert!(below.contains("p/model:high LegacyModel"));
	assert!(below.contains("p/model:max LegacyResolved"));
	assert!(below.contains("p/model:auto LiteralAuto"));
	assert!(!below.contains("p/unused"));
	assert!(!below.contains(&thinking_level_glyph(ConfiguredThinkingLevel::High, &theme)));
	assert!(!below.contains(&thinking_level_glyph(ConfiguredThinkingLevel::Max, &theme)));
}

#[test]
fn hides_the_agent_badge_when_disabled_without_hiding_live_activity() {
	let _guard = lock();
	let theme = default_theme();
	let mut ev = event("0-Scout", AgentStatus::Running);
	ev.model = Some("p/model:high".to_string());
	ev.resolved_thinking_level = Some(ConfiguredThinkingLevel::High);
	ev.advisor = true;
	ev.current_tool = Some("read".to_string());
	ev.last_intent = Some("Reading config".to_string());

	let below = strip_ansi(&render(std::slice::from_ref(&ev), 120, false));
	assert!(below.contains("0-Scout"));
	assert!(below.contains("Reading config"));
	assert!(!below.contains("p/model"));
	assert!(!below.contains(&thinking_level_glyph(ConfiguredThinkingLevel::High, &theme)));
	assert!(!below.contains(&theme.icon().advisor));
}

#[test]
fn reserves_failure_status_for_oversized_ids_even_when_badges_are_disabled() {
	let _guard = lock();
	let mut ev = event(&format!("LongWorker{}", "界".repeat(100)), AgentStatus::Failed);
	ev.resolved_model_identity = Some("provider/model".to_string());
	ev.advisor = true;

	let below = strip_ansi(&render(std::slice::from_ref(&ev), 120, false));
	let row = below.lines().find(|line| line.contains("LongWorker")).expect("id row");
	assert!(row.contains("failed"));
	assert!(!row.contains("provider/model"));
	assert!(visible_width(row) <= 120);
}

#[test]
fn keeps_identities_and_failure_status_within_the_viewport_across_resizes() {
	let _guard = lock();
	let model = "openai-codex/gpt-6-astra";
	let intent = "Reading renderer and framework boundaries";
	let mut failed = event("ArchitectureScout", AgentStatus::Failed);
	failed.resolved_model_identity = Some(model.to_string());
	failed.resolved_thinking_level = Some(ConfiguredThinkingLevel::High);
	failed.advisor = true;
	failed.tool_count = Some(7);
	failed.context_tokens = Some(8000);
	failed.context_window = Some(200_000);
	failed.cost = Some(0.06);
	failed.duration_ms = Some(1500);

	let mut running = event("ResearchScout", AgentStatus::Running);
	running.resolved_model_identity = Some(model.to_string());
	running.current_tool = Some("read".to_string());
	running.last_intent = Some(intent.to_string());

	let mut aborted = event("CancelledScout", AgentStatus::Aborted);
	aborted.resolved_model_identity = Some(format!("{model}/{}", "extended-identity-".repeat(8)));
	aborted.cost = Some(0.09);

	let events = [failed, running, aborted];
	for width in [40, 120, 40] {
		let plain = strip_ansi(&render(&events, width, true));
		for line in plain.lines() {
			assert!(visible_width(line) <= width, "row wider than {width}: {line:?}");
		}
		let first = plain.lines().next().expect("first row");
		assert!(first.contains("ArchitectureScout"));
		assert!(first.contains("failed"));
		assert!(plain.contains("ResearchScout"));
		assert!(plain.contains("CancelledScout"));
		assert!(plain.contains("aborted"));
		assert!(plain.contains("read"));
		if width == 120 {
			assert!(plain.contains(model));
			assert!(plain.contains("$0.06"));
			assert!(plain.contains("$0.09"));
			assert!(plain.contains(intent));
		}
	}
}

#[test]
#[ignore = "missing API: eval-cell Status section / evalToolRenderer.renderResult (non-agent event folding)"]
fn still_folds_non_agent_status_events_into_the_box_status_section() {}