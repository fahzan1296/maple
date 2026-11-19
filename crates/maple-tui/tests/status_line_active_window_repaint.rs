//! Port of `l/test/status-line-active-window-repaint.test.ts`.
//!
//! The reference drives `StatusLineComponent`'s active-window repaint ticker on
//! fake timers, re-arming on focus return and native-surface revocation. The
//! crate's `status_line::component` is a headless renderer with no ticker,
//! focus tracking or native surface, so all cases stay ignored; the bodies
//! exercise the headless render seam such a port would reuse.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

fn render_headless() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	tui.stop().expect("stop");
}

#[test]
#[ignore = "missing facility: no active-window repaint ticker in the crate's status line"]
fn repaints_on_its_own_cadence_while_a_turn_window_is_open_with_no_loader_running() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no idle silence, re-arm or dispose lifecycle in the crate's status line"]
fn stays_silent_while_idle_re_arms_for_the_next_turn_and_stops_on_dispose() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no focus-return re-arm in the crate's status line"]
fn re_arms_when_focus_returns_to_a_session_whose_turn_is_still_running() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no native-surface revocation re-arm in the crate's status line"]
fn re_arms_when_a_native_surface_that_suppressed_the_ticker_is_revoked_mid_turn() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no spinner-clock cadence coupling in the crate's status line"]
fn follows_the_spinner_clock_a_slower_or_static_spinner_stops_the_4_hz_wakeups() {
	render_headless();
}

#[test]
#[ignore = "missing facility: no spinner-clock boundary tick math in the crate's status line"]
fn ticks_a_whole_number_of_spinner_frames_apart_on_the_clock_boundaries_whenever_the_turn_started() {
	render_headless();
}
