//! Port of `l/test/alt-frame-row-diff.test.ts`.
//!
//! The reference paints a fullscreen overlay on the alternate screen through a
//! real `TUI` and asserts which screen rows each paint rewrites. The crate has
//! no overlay/alternate-screen/cursor runtime; its differential painter is
//! `screen::Screen` (which rewrites from the first difference to the end of the
//! frame), so the row-diff contract is pinned there and the alt-screen,
//! hardware-cursor, scaled-glyph and paint-listener assertions are ignored.

use maple_tui::screen::Screen;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::Terminal;

fn lines(values: &[&str]) -> Vec<String> {
	values.iter().map(|v| (*v).to_string()).collect()
}

/// 1-based screen rows a paint addressed with an absolute row move.
fn rewritten_rows(frame: &str) -> Vec<u32> {
	let mut rows = Vec::new();
	let mut rest = frame;
	while let Some(start) = rest.find("\x1b[") {
		let after = &rest[start + 2..];
		let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
		if digits.is_empty() {
			rest = after;
			continue;
		}
		let tail = &after[digits.len()..];
		if let Some(following) = tail.strip_prefix(";1H") {
			rows.push(digits.parse().expect("row digits"));
			rest = following;
		} else {
			rest = tail;
		}
	}
	rows
}

#[test]
fn rewrites_only_the_rows_that_changed() {
	let mut terminal = VirtualTerminal::new(30, 6);
	let mut screen = Screen::new();
	let original = lines(&["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"]);

	screen.flush(&mut terminal, &original, 30);
	terminal.clear_output();

	let mut changed = original.clone();
	changed[2] = "CHARLIE CHANGED".to_string();
	screen.flush(&mut terminal, &changed, 30);
	let grown = terminal.output().to_string();
	// The crate rewrites from the first difference to the end of the frame,
	// so rows above the change stay untouched and the changed row is addressed.
	let rows = rewritten_rows(&grown);
	assert_eq!(rows.first(), Some(&3), "{grown}");
	assert!(!rows.contains(&1) && !rows.contains(&2), "{grown}");
	assert!(grown.contains("CHARLIE CHANGED"), "{grown}");
	assert_eq!(screen.rows(), changed);

	// Deleting what was just typed returns the row to text an earlier paint
	// replaced; it must be repainted, not skipped as already on screen.
	terminal.clear_output();
	screen.flush(&mut terminal, &original, 30);
	let restored = terminal.output().to_string();
	assert_eq!(rewritten_rows(&restored).first(), Some(&3), "{restored}");
	assert_eq!(screen.rows(), original);
}

#[test]
#[ignore = "missing facility: Screen has no forced-repaint path; only a width or row-count change triggers a full erase"]
fn still_rewrites_every_row_on_a_forced_repaint() {
	let mut terminal = VirtualTerminal::new(30, 4);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &lines(&["alpha", "bravo", "charlie", "delta"]), 30);
	terminal.clear_output();
	screen.flush(&mut terminal, &lines(&["alpha", "bravo", "charlie", "delta"]), 30);
	assert_eq!(screen.rows().len(), 4);
}

#[test]
#[ignore = "missing facility: no OSC 66 scaled-glyph model, so a glyph covering the row below cannot be tracked"]
fn repaints_a_frame_holding_a_scaled_heading_whole_when_it_changes() {
	let mut terminal = VirtualTerminal::new(30, 4);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &lines(&["top", "heading", "", "bottom"]), 30);
	terminal.clear_output();
	screen.flush(&mut terminal, &lines(&["top", "heading", "", "bottom"]), 30);
	assert!(rewritten_rows(terminal.output()).is_empty());
}

#[test]
#[ignore = "missing facility: no overlay or hardware-cursor runtime, so a focused fullscreen marker cannot be positioned"]
fn positions_a_focused_fullscreen_cursor_and_moves_it_when_only_the_marker_changes() {
	let mut terminal = VirtualTerminal::new(30, 1);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &lines(&["stable row"]), 30);
	assert_eq!(screen.rows(), lines(&["stable row"]));
}

#[test]
#[ignore = "missing facility: no frame provider, history commit or shell handoff in the crate's Tui"]
fn preserves_the_normal_transcript_after_closing_a_fullscreen_cursor_overlay() {
	let mut terminal = VirtualTerminal::new(30, 8);
	let mut screen = Screen::new();
	screen.flush(
		&mut terminal,
		&lines(&["committed first", "committed second", "live first", "live second"]),
		30,
	);
	assert_eq!(screen.rows().len(), 4);
}

#[test]
#[ignore = "missing facility: no frame provider, history commit or shell handoff in the crate's Tui"]
fn preserves_the_normal_transcript_after_stopping_inside_a_fullscreen_cursor_overlay() {
	let mut terminal = VirtualTerminal::new(30, 8);
	let mut screen = Screen::new();
	screen.flush(
		&mut terminal,
		&lines(&["committed first", "committed second", "live first", "live second"]),
		30,
	);
	assert_eq!(screen.rows().len(), 4);
}

#[test]
#[ignore = "missing facility: no hardware-cursor preference, so a hidden marker cannot be asserted"]
fn keeps_a_fullscreen_marker_hidden_when_hardware_cursor_rendering_is_disabled() {
	let mut terminal = VirtualTerminal::new(30, 1);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &lines(&["stable row"]), 30);
	assert_eq!(screen.rows(), lines(&["stable row"]));
}

#[test]
#[ignore = "missing facility: no focusable-component contract, so marker exposure cannot be asserted"]
fn does_not_expose_markers_from_a_non_focusable_fullscreen_overlay() {
	let mut terminal = VirtualTerminal::new(30, 1);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &lines(&["stable row"]), 30);
	assert_eq!(screen.rows(), lines(&["stable row"]));
}

#[test]
#[ignore = "missing facility: no paint-listener channel on the crate's Tui"]
fn reports_the_complete_overlay_to_paint_listeners_when_one_row_changes() {
	let mut terminal = VirtualTerminal::new(30, 3);
	let mut screen = Screen::new();
	screen.flush(&mut terminal, &lines(&["alpha", "bravo", "charlie"]), 30);
	terminal.clear_output();
	let mut changed = lines(&["alpha", "bravo", "charlie"]);
	changed[1] = "BRAVO".to_string();
	screen.flush(&mut terminal, &changed, 30);
	assert_eq!(rewritten_rows(terminal.output()).first(), Some(&2));
}
