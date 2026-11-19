//! Port of `l/test/unsynchronized-row-moves.test.ts`.
//!
//! Without synchronized output the terminal repaints by moving the cursor and
//! rewriting rows, so a row move is only safe when the rows it leaves behind are
//! already in scrollback. `VirtualTerminal` is the crate's model of that
//! behaviour: writes advance the cursor row, `move_by` emits the cursor move,
//! and scrollback is an explicit bounded buffer.

use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;

#[test]
fn newlines_advance_the_cursor_without_touching_scrollback() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.write("a\nb\nc\n");

	assert_eq!(terminal.cursor_row(), 3);
	assert!(terminal.scrollback().is_empty());
	assert_eq!(terminal.output(), "a\nb\nc\n");
}

#[test]
fn move_by_walks_the_cursor_and_emits_the_cursor_move() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.write("a\nb\n");
	terminal.move_by(-1);

	assert_eq!(terminal.cursor_row(), 1);
	assert!(terminal.output().ends_with("\x1b[1B"));

	terminal.move_by(2);
	assert_eq!(terminal.cursor_row(), 3);
	assert!(terminal.output().ends_with("\x1b[2B"));
}

#[test]
fn moving_above_the_top_row_clamps_at_zero() {
	let mut terminal = VirtualTerminal::new(20, 3);
	terminal.write("a\n");
	terminal.move_by(-5);

	assert_eq!(terminal.cursor_row(), 0);
	assert!(terminal.output().ends_with("\x1b[5B"));
}

#[test]
fn append_scrollback_is_bounded_by_the_scrollback_limit() {
	let mut terminal = VirtualTerminal::new(10, 2);
	let rows: Vec<String> = (0..4097).map(|index| format!("r{index}")).collect();
	terminal.append_scrollback(&rows);

	assert_eq!(terminal.scrollback().len(), 4096);
	assert_eq!(terminal.scrollback()[0], "r1");
	assert_eq!(terminal.scrollback()[4095], "r4096");
}

#[test]
fn viewport_rows_follow_the_scroll_offset() {
	let mut terminal = VirtualTerminal::new(10, 3);
	terminal.set_viewport(&["a".to_string(), "b".to_string(), "c".to_string()]);
	terminal.append_scrollback(&["s1".to_string(), "s2".to_string()]);

	assert_eq!(terminal.viewport_rows(), vec!["a", "b", "c"]);
	terminal.scroll_to(1);
	assert_eq!(terminal.viewport_offset(), 1);
	assert_eq!(terminal.viewport_rows(), vec!["s2", "a", "b"]);

	terminal.scroll_to(99);
	assert_eq!(terminal.viewport_offset(), 2);
	assert_eq!(terminal.viewport_rows(), vec!["s1", "s2", "a"]);

	terminal.scroll_by(-1);
	assert_eq!(terminal.viewport_offset(), 1);
	terminal.scroll_by(-5);
	assert_eq!(terminal.viewport_offset(), 0);
	assert_eq!(terminal.viewport_rows(), vec!["a", "b", "c"]);
}

#[test]
fn resize_truncates_the_viewport_and_clamps_the_offset() {
	let mut terminal = VirtualTerminal::new(10, 3);
	terminal.set_viewport(&["a".to_string(), "b".to_string(), "c".to_string()]);
	terminal.append_scrollback(&["s1".to_string(), "s2".to_string()]);
	terminal.scroll_to(2);

	terminal.resize(10, 2);
	assert_eq!(terminal.viewport_size(), (10, 2));
	assert_eq!(terminal.viewport_offset(), 2);
	assert_eq!(terminal.viewport_rows(), vec!["s1", "s2"]);

	terminal.clear_scrollback();
	assert!(terminal.scrollback().is_empty());
	assert_eq!(terminal.viewport_offset(), 0);
	assert_eq!(terminal.viewport_rows(), vec!["a", "b"]);
}
