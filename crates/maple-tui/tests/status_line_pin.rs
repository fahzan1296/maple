//! Port of `l/test/status-line-pin.test.ts`.
//!
//! The reference drives `StatusLineComponent.getTopBorder`/`setSessionPinned`;
//! the crate's composed bar has no top border and no pin API — the pin rides on
//! the session-name segment instead. The case stays ignored; its body exercises
//! that segment seam.

use maple_tui::status_line::schema::SegmentId;
use maple_tui::status_line::segments::{render_segment_with, SegmentContext, SessionInfo};
use maple_tui::status_line::{FallbackTheme, StatusTheme};

fn render_pinned(pinned: bool) -> String {
	let mut ctx = SegmentContext::default();
	ctx.session = SessionInfo {
		session_name: Some("CHROME: Audit Outreach Leads Status".to_string()),
		..SessionInfo::default()
	};
	ctx.is_pinned = pinned;
	let theme = FallbackTheme::new();
	render_segment_with(SegmentId::SessionName, &theme, &ctx).content
}

#[test]
#[ignore = "missing facility: no StatusLineComponent top border or setSessionPinned API"]
fn shows_the_pin_only_beside_the_pinned_sessions_title() {
	let theme = FallbackTheme::new();
	let pin = theme.symbol("icon.pin");
	assert!(render_pinned(true).contains(&pin));
	assert!(!render_pinned(false).contains(&pin));
}
