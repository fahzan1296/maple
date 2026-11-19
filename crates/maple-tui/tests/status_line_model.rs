//! Port of `l/test/status-line-model.test.ts`.
//!
//! Covers the stream badge, the advisor roster badge, and the compact thinking
//! level in `model_segment`. Expectations are built from the same `FallbackTheme`
//! the renderer uses, mirroring how the reference derives them from `theme`.

use maple_tui::status_line::schema::SegmentId;
use maple_tui::status_line::segments::{
	render_segment_with, AdvisorInfo, AdvisorOverview, ModelInfo, SegmentContext, SessionInfo,
	StreamStatus,
};
use maple_tui::status_line::{FallbackTheme, StatusTheme};

fn model_context(advisor_active: bool) -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.width = 120;
	ctx.session = SessionInfo {
		model: ModelInfo {
			id: Some("test-model".to_string()),
			name: Some("Test Model".to_string()),
			..ModelInfo::default()
		},
		..SessionInfo::default()
	};
	if advisor_active {
		ctx.session.advisor = Some(AdvisorOverview {
			configured: true,
			advisors: vec![AdvisorInfo {
				status: "running".to_string(),
				yielded: false,
			}],
		});
	}
	ctx
}

fn thinking_context(compact: bool) -> SegmentContext {
	let mut ctx = model_context(false);
	ctx.compact_thinking_level = compact;
	ctx.session.model.thinking = true;
	ctx.session.model.thinking_level = Some("high".to_string());
	ctx
}

#[test]
fn renders_the_live_viewer_badge_only_while_attached() {
	let theme = FallbackTheme::new();
	let mut ctx = model_context(false);
	ctx.stream = Some(StreamStatus { viewers: 7 });
	assert_eq!(
		render_segment_with(SegmentId::Stream, &theme, &ctx).content,
		theme.fg("thinkingHigh", "● LIVE 7")
	);

	ctx.stream = None;
	let rendered = render_segment_with(SegmentId::Stream, &theme, &ctx);
	assert!(!rendered.visible);
	assert_eq!(rendered.content, "");
}

#[test]
fn appends_a_success_colored_advisor_symbol_when_all_advisors_run() {
	let theme = FallbackTheme::new();
	let rendered = render_segment_with(SegmentId::Model, &theme, &model_context(true));
	assert!(rendered.content.contains("Test Model"));
	assert!(rendered
		.content
		.contains(&theme.fg("success", &format!(" {}", theme.symbol("icon.advisor")))));
}

#[test]
fn colors_the_badge_by_the_worst_roster_status() {
	let theme = FallbackTheme::new();
	let mut ctx = model_context(true);
	ctx.session.advisor = Some(AdvisorOverview {
		configured: true,
		advisors: vec![
			AdvisorInfo {
				status: "running".to_string(),
				yielded: false,
			},
			AdvisorInfo {
				status: "quota_exhausted".to_string(),
				yielded: false,
			},
		],
	});
	assert!(render_segment_with(SegmentId::Model, &theme, &ctx)
		.content
		.contains(&theme.fg("warning", &format!(" {}", theme.symbol("icon.advisor")))));

	ctx.session.advisor = Some(AdvisorOverview {
		configured: true,
		advisors: vec![
			AdvisorInfo {
				status: "error".to_string(),
				yielded: false,
			},
			AdvisorInfo {
				status: "quota_exhausted".to_string(),
				yielded: false,
			},
		],
	});
	assert!(render_segment_with(SegmentId::Model, &theme, &ctx)
		.content
		.contains(&theme.fg("error", &format!(" {}", theme.symbol("icon.advisor")))));
}

#[test]
fn closes_the_eye_once_every_advisor_has_yielded_its_review() {
	let theme = FallbackTheme::new();
	let mut ctx = model_context(true);
	ctx.session.advisor = Some(AdvisorOverview {
		configured: true,
		advisors: vec![AdvisorInfo {
			status: "running".to_string(),
			yielded: true,
		}],
	});
	let rendered = render_segment_with(SegmentId::Model, &theme, &ctx).content;
	assert!(rendered.contains(&theme.fg("success", &format!(" {}", theme.symbol("icon.advisorClosed")))));
	assert_ne!(theme.symbol("icon.advisorClosed"), theme.symbol("icon.advisor"));
	assert!(!rendered.contains(theme.symbol("icon.advisor")));
}

#[test]
fn keeps_the_eye_open_while_any_advisor_may_still_comment() {
	let theme = FallbackTheme::new();
	let mut ctx = model_context(true);
	ctx.session.advisor = Some(AdvisorOverview {
		configured: true,
		advisors: vec![
			AdvisorInfo {
				status: "running".to_string(),
				yielded: true,
			},
			AdvisorInfo {
				status: "running".to_string(),
				yielded: false,
			},
		],
	});
	let rendered = render_segment_with(SegmentId::Model, &theme, &ctx).content;
	assert!(rendered.contains(&theme.fg("success", &format!(" {}", theme.symbol("icon.advisor")))));
	assert_ne!(theme.symbol("icon.advisorClosed"), theme.symbol("icon.advisor"));
	assert!(!rendered.contains(theme.symbol("icon.advisorClosed")));
}

#[test]
fn omits_the_badge_when_the_advisor_is_inactive() {
	let theme = FallbackTheme::new();
	let rendered = render_segment_with(SegmentId::Model, &theme, &model_context(false));
	assert!(rendered.content.contains("Test Model"));
	assert!(!rendered.content.contains(theme.symbol("icon.advisor")));
}

#[test]
fn trails_the_level_as_a_dot_suffix_when_compact_mode_is_off() {
	let theme = FallbackTheme::new();
	let rendered = render_segment_with(SegmentId::Model, &theme, &thinking_context(false));
	let display = theme.symbol("thinking.high");
	let icon = theme.symbol("icon.model");
	assert_eq!(
		rendered.content,
		format!(
			{}{}",
			theme.fg(
				"statusLineModel",
				&if icon.is_empty() {
					"Test Model".to_string()
				} else {
					format!("{icon} Test Model")
				}
			),
			theme.fg("statusLineModel", &format!("{}{}", theme.sep("dot"), display))
		)
	);
}

#[test]
fn swaps_the_model_icon_for_the_level_glyph_and_drops_the_suffix_when_compact() {
	let theme = FallbackTheme::new();
	let rendered = render_segment_with(SegmentId::Model, &theme, &thinking_context(true));
	let display = theme.symbol("thinking.high");
	let glyph = match display.find(' ') {
		Some(index) => &display[..index],
		None => display.as_str(),
	};
	assert_eq!(
		rendered.content,
		theme.fg("statusLineModel", &format!("{glyph} Test Model"))
	);
	assert!(!rendered.content.contains(theme.sep("dot")));
}
