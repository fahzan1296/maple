//! Port of `l/test/status-line-loop.test.ts`.
//!
//! The loop arm of `mode_segment` renders from `LoopMode`; expectations are built
//! from the same `FallbackTheme`. The reference mocks `Date.now`, which the crate
//! replaces with the deterministic `SegmentContext::now` clock.

use maple_tui::status_line::schema::SegmentId;
use maple_tui::status_line::segments::{
	render_segment_with, LoopCondition, LoopLimit, LoopMode, LoopState, SegmentClock,
	SegmentContext,
};
use maple_tui::status_line::{FallbackTheme, StatusTheme};

const NOW_MS: u64 = 1_784_299_200_000; // 2026-07-17T12:00:00Z

fn create_context(loop_mode: Option<LoopMode>) -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.width = 120;
	ctx.loop_mode = loop_mode;
	ctx.now = Some(SegmentClock {
		hour: 12,
		minute: 0,
		second: 0,
		epoch_ms: NOW_MS,
	});
	ctx
}

fn with_icon(icon: &str, text: &str) -> String {
	if icon.is_empty() {
		text.to_string()
	} else {
		format!("{icon} {text}")
	}
}

#[test]
fn warns_that_the_next_prompt_becomes_the_repeated_loop_body() {
	let theme = FallbackTheme::new();
	let ctx = create_context(Some(LoopMode {
		state: LoopState::Waiting,
		limit: Some(LoopLimit::Iterations {
			initial: 10,
			remaining: 10,
		}),
		condition: None,
	}));
	let rendered = render_segment_with(SegmentId::Mode, &theme, &ctx);
	assert_eq!(
		rendered.content,
		theme.fg(
			"customMessageLabel",
			&with_icon(&theme.symbol("icon.loop"), "Loop: next prompt repeats 10/10")
		)
	);
}

#[test]
fn shows_the_live_remaining_duration_while_a_loop_is_running() {
	let theme = FallbackTheme::new();
	let ctx = create_context(Some(LoopMode {
		state: LoopState::Running,
		limit: Some(LoopLimit::Duration {
			deadline_ms: NOW_MS + 90_000,
		}),
		condition: None,
	}));
	let rendered = render_segment_with(SegmentId::Mode, &theme, &ctx);
	assert_eq!(
		rendered.content,
		theme.fg(
			"customMessageLabel",
			&with_icon(&theme.symbol("icon.loop"), "Loop running 1m30s left")
		)
	);
}

#[test]
fn distinguishes_a_paused_loop_from_an_active_loop() {
	let theme = FallbackTheme::new();
	let ctx = create_context(Some(LoopMode {
		state: LoopState::Paused,
		limit: None,
		condition: None,
	}));
	let rendered = render_segment_with(SegmentId::Mode, &theme, &ctx);
	let icon = if theme.symbol("icon.pause").is_empty() {
		theme.symbol("icon.loop")
	} else {
		theme.symbol("icon.pause")
	};
	assert_eq!(
		rendered.content,
		theme.fg("warning", &with_icon(&icon, "Loop paused"))
	);
}

#[test]
fn shows_the_gating_condition_alongside_the_remaining_iterations() {
	let theme = FallbackTheme::new();
	let ctx = create_context(Some(LoopMode {
		state: LoopState::Running,
		limit: Some(LoopLimit::Iterations {
			initial: 3,
			remaining: 3,
		}),
		condition: Some(LoopCondition {
			command: "bun test".to_string(),
			until: true,
		}),
	}));
	let rendered = render_segment_with(SegmentId::Mode, &theme, &ctx);
	assert_eq!(
		rendered.content,
		theme.fg(
			"customMessageLabel",
			&with_icon(&theme.symbol("icon.loop"), "Loop running 3/3 until: bun test")
		)
	);
}

#[test]
fn bounds_a_long_condition_command() {
	let theme = FallbackTheme::new();
	let ctx = create_context(Some(LoopMode {
		state: LoopState::Running,
		limit: None,
		condition: Some(LoopCondition {
			command: format!("bun test {}", "x".repeat(200)),
			until: false,
		}),
	}));
	let rendered = render_segment_with(SegmentId::Mode, &theme, &ctx);
	assert!(
		rendered
			.content
			.contains(&with_icon(&theme.symbol("icon.loop"), "Loop running while: bun test x")),
		"{:?}",
		rendered.content
	);
	assert!(maple_tui::text_shape::plain_width(&rendered.content) < 60);
}
