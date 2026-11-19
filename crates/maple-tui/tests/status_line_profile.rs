//! Port of `l/test/status-line-profile.test.ts`.
//!
//! The reference mocks a global `getActiveProfile()`; the crate carries the
//! active profile on `SessionInfo.profile`, so the tag cases are real. The
//! native `describeSegment` case stays ignored: the crate has no segment
//! describe view.

use maple_tui::status_line::schema::{BrandOptions, SegmentId, SegmentOptions};
use maple_tui::status_line::segments::{render_segment_with, SegmentContext};
use maple_tui::status_line::{FallbackTheme, StatusTheme};

fn theme() -> FallbackTheme {
	FallbackTheme::new()
}

/// Brand content is wrapped in the dim fg and a trailing reset; strip both.
fn plain(content: &str) -> &str {
	let dim = theme().fg_ansi("dim");
	content
		.strip_prefix(&dim)
		.and_then(|rest| rest.strip_suffix("\x1b[39m"))
		.unwrap_or(content)
}

fn brand(profile: Option<&str>) -> String {
	let mut ctx = SegmentContext::default();
	ctx.session.profile = profile.map(str::to_string);
	plain(&render_segment_with(SegmentId::Brand, &theme(), &ctx).content).to_string()
}

#[test]
fn tags_the_brand_with_a_named_profile_and_leaves_the_default_profile_bare() {
	assert_eq!(brand(None), "π");
	assert_eq!(brand(Some("work")), "π-work");
}

#[test]
fn keeps_the_tag_beside_the_spinner_while_a_turn_runs() {
	let mut ctx = SegmentContext::default();
	ctx.session.profile = Some("work".to_string());
	ctx.now = Some(maple_tui::status_line::segments::SegmentClock { epoch_ms: 0 });
	ctx.turn_elapsed_ms = Some(92_000);
	let content = render_segment_with(SegmentId::Brand, &theme(), &ctx).content;
	assert_eq!(plain(&content), "⠋-work 1m");
}

#[test]
fn drops_the_tag_everywhere_when_brand_show_profile_is_false() {
	let mut options = SegmentOptions::default();
	options.brand = Some(BrandOptions {
		show_profile: Some(false),
	});

	let mut idle = SegmentContext::default();
	idle.session.profile = Some("work".to_string());
	idle.options = options.clone();
	assert_eq!(
		plain(&render_segment_with(SegmentId::Brand, &theme(), &idle).content),
		"π"
	);

	let mut running = SegmentContext::default();
	running.session.profile = Some("work".to_string());
	running.options = options;
	running.now = Some(maple_tui::status_line::segments::SegmentClock { epoch_ms: 0 });
	running.turn_elapsed_ms = Some(92_000);
	assert_eq!(
		plain(&render_segment_with(SegmentId::Brand, &theme(), &running).content),
		"⠋ 1m"
	);
}

#[test]
#[ignore = "missing facility: no native describe view for status-line segments in the crate"]
fn names_the_profile_in_the_native_status_bar() {
	let _ = brand(Some("work"));
}
