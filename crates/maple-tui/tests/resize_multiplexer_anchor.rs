//! Port of `l/test/resize-multiplexer-anchor.test.ts` (portable subset).
//!
//! The reference drives tmux's CPR probe transaction: stash/restore of input,
//! late-reply retirement, retry windows and the conservative pull bound. The
//! crate has no CPR probe, input stash or resize transaction, so those cases
//! have no counterpart. The portable subset asserts the two pure seams the
//! transaction would drive: [`Multiplexer::host_owns_grid_on_resize`], which
//! decides whether the TUI takes the in-place resize path, and the headless
//! [`VirtualTerminal`] resize/scrollback model.

use maple_tui::multiplexers::Multiplexer;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;
use maple_tui::tui::Tui;

#[test]
fn tmux_and_screen_hosts_own_the_grid_on_resize() {
	for host in [
		Multiplexer::Tmux,
		Multiplexer::Screen,
		Multiplexer::Zellij,
		Multiplexer::Herdr,
		Multiplexer::Wmux,
	] {
		assert!(host.host_owns_grid_on_resize(), "{}", host.name());
	}
}

#[test]
fn plain_terminals_leave_the_grid_to_the_application() {
	for host in [
		Multiplexer::ITerm2,
		Multiplexer::AppleTerminal,
		Multiplexer::Kitty,
		Multiplexer::WezTerm,
		Multiplexer::WindowsTerminal,
		Multiplexer::Warp,
		Multiplexer::Ghostty,
		Multiplexer::Orca,
	] {
		assert!(!host.host_owns_grid_on_resize(), "{}", host.name());
	}
}

#[test]
fn synchronized_output_is_honored_except_by_zellij() {
	assert!(Multiplexer::Tmux.honors_synchronized_output());
	assert!(Multiplexer::Screen.honors_synchronized_output());
	assert!(!Multiplexer::Zellij.honors_synchronized_output());
}

#[test]
fn resize_updates_the_reported_viewport_size() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.resize(40, 5);
	assert_eq!(terminal.viewport_size(), (40, 5));
	assert_eq!(terminal.viewport_height(), 5);
	assert_eq!(terminal.columns(), 40);
	assert_eq!(terminal.rows(), 5);
}

#[test]
fn shrink_truncates_the_live_viewport_from_the_bottom() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.set_viewport(&["one", "two", "three"]);
	terminal.resize(20, 2);
	assert_eq!(terminal.viewport_rows(), vec!["one", "two"]);
}

#[test]
fn grow_pads_the_live_viewport_with_empty_rows() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.set_viewport(&["one", "two"]);
	terminal.resize(20, 4);
	let rows = terminal.viewport_rows();
	assert_eq!(rows.len(), 4);
	assert_eq!(rows[0], "one");
	assert_eq!(rows[1], "two");
	assert!(rows[2].is_empty());
	assert!(rows[3].is_empty());
}

#[test]
fn resize_clamps_a_scroll_offset_to_the_scrollback() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.append_scrollback(&["old-1", "old-2"]);
	terminal.scroll_to(99);
	assert_eq!(terminal.viewport_offset(), 2);
	terminal.resize(20, 1);
	assert_eq!(terminal.viewport_offset(), 2);
}

#[test]
fn clear_scrollback_resets_the_scroll_offset() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.append_scrollback(&["old-1"]);
	terminal.scroll_to(1);
	terminal.clear_scrollback();
	assert_eq!(terminal.viewport_offset(), 0);
	assert!(terminal.scrollback().is_empty());
}

#[test]
fn scrolling_off_the_live_viewport_shows_a_scrollback_window() {
	let mut terminal = VirtualTerminal::new(20, 2);
	terminal.append_scrollback(&["old-1", "old-2", "old-3"]);
	terminal.set_viewport(&["live-1", "live-2"]);
	terminal.scroll_to(1);
	assert_eq!(terminal.viewport_rows(), vec!["old-3", "live-1"]);
	terminal.scroll_to(2);
	assert_eq!(terminal.viewport_rows(), vec!["old-2", "old-3"]);
	terminal.scroll_to(0);
	assert_eq!(terminal.viewport_rows(), vec!["live-1", "live-2"]);
}

#[test]
fn renders_a_headless_frame_on_the_virtual_terminal() {
	let mut tui = Tui::new(Box::new(VirtualTerminal::new(20, 3)));
	tui.render_now();
	let paint = tui.last_paint().expect("paint");
	assert_eq!(paint.columns, 20);
	assert_eq!(paint.rows, 3);
	assert_eq!(paint.viewport.len(), 3);
	tui.stop().expect("stop");
}
