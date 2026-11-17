//! Port of `l/test/destructive-reset-clear-order.test.ts`.
//!
//! The reference asserts that a destructive reset writes `ED2` (`CSI 2J`)
//! before `ED3` (`CSI 3J`) so tmux's clear-pushes-screen-into-history cannot
//! refill scrollback. The Rust port has no clear-scrollback request: the only
//! screen clear is `VirtualTerminal::clear_screen` (`ESC[2J ESC[H`), and no
//! renderer option emits `ESC[3J` in any order.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal as _;

#[test]
#[ignore = "missing API: no destructive-reset/clear-scrollback request emitting ESC[2J then ESC[3J"]
fn emits_ed2_before_ed3_so_tmux_history_cannot_survive_the_wipe() {}

#[test]
fn virtual_terminal_clear_screen_emits_only_ed2() {
	let mut term = VirtualTerminal::new(80, 10);
	term.clear_screen();
	let output = term.output();
	assert!(output.contains("\x1b[2J"), "{output:?}");
	assert!(!output.contains("\x1b[3J"), "{output:?}");
}
