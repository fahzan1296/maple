//! Port of `l/test/editor-vim.test.ts`.
//!
//! The reference drives `Editor`'s Vim integration (mode chrome, rendering,
//! history, atomic placeholder tokens). The crate keeps modal editing in the
//! pure [`maple_tui::vim`] state machine and leaves rendering, undo, and token
//! protection to the editor, so these tests assert the commands and mode state
//! the machine emits for the same key runs. Only the atomic-token describe is
//! ignored: `vim.rs` has no token awareness by design.

use maple_tui::vim::{
	VimBuffer, VimCommand, VimInsertRecord, VimMode, VimPosition, VimState,
};

fn buf(lines: &[&str]) -> VimBuffer {
	VimBuffer::new(lines.iter().map(|line| line.to_string()).collect(), 0, 0)
}

fn at(buf: VimBuffer, line: usize, col: usize) -> VimBuffer {
	VimBuffer {
		lines: buf.lines,
		cursor_line: line,
		cursor_col: col,
	}
}

fn pos(line: usize, col: usize) -> VimPosition {
	VimPosition { line, col }
}

/// Feed `key`, returning the commands it produced (empty when the key only
/// extended a pending prefix).
fn key(vim: &mut VimState, buf: &VimBuffer, key: &str) -> Vec<VimCommand> {
	vim.handle_key(key, buf).unwrap_or_default()
}

fn delete(from: VimPosition, to: VimPosition, linewise: bool, insert: bool) -> VimCommand {
	VimCommand::Delete {
		from,
		to,
		linewise,
		insert,
	}
}

// ── disabled by default ──────────────────────────────────────────────────────

#[test]
fn normal_mode_starts_enabled_and_swallows_unknown_keys() {
	let mut vim = VimState::new();
	assert_eq!(vim.mode, VimMode::Normal);
	let buf = buf(&["abc"]);
	assert_eq!(key(&mut vim, &buf, "h"), vec![VimCommand::Move { to: pos(0, 0) }]);
	// `z` is not a Vim key: the machine reports no edit, so a host with Vim off
	// would fall through and type it.
	assert!(key(&mut vim, &buf, "z").is_empty());
}

// ── mode switching ───────────────────────────────────────────────────────────

#[test]
fn insert_entries_report_mode_and_cursor() {
	let mut vim = VimState::new();
	let buf = buf(&["ab"]);
	assert_eq!(
		key(&mut vim, &buf, "i"),
		vec![VimCommand::Mode {
			mode: VimMode::Insert
		}]
	);
	assert_eq!(vim.mode, VimMode::Insert);

	let mut vim = VimState::new();
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 1), "a"),
		vec![
			VimCommand::Mode {
				mode: VimMode::Insert
			},
			VimCommand::Move { to: pos(0, 2) },
		]
	);
}

#[test]
fn escape_leaves_insert_one_grapheme_back() {
	let mut vim = VimState::new();
	let buf = buf(&["abc"]);
	key(&mut vim, &buf, "i");
	let commands = key(&mut vim, &at(buf, 0, 2), "escape");
	assert_eq!(
		commands,
		vec![
			VimCommand::Mode {
				mode: VimMode::Normal
			},
			VimCommand::Move { to: pos(0, 1) },
		]
	);
	assert_eq!(vim.mode, VimMode::Normal);
}

#[test]
fn open_line_below_and_above() {
	let mut vim = VimState::new();
	let buf = buf(&["one"]);
	assert!(matches!(
		key(&mut vim, &buf, "o").as_slice(),
		[VimCommand::OpenLine { below: true }, VimCommand::Mode { mode: VimMode::Insert }]
	));
	let mut vim = VimState::new();
	assert!(matches!(
		key(&mut vim, &buf, "O").as_slice(),
		[VimCommand::OpenLine { below: false }, VimCommand::Mode { mode: VimMode::Insert }]
	));
}

#[test]
fn append_insert_and_line_end_moves() {
	let mut vim = VimState::new();
	let buf = buf(&["ab"]);
	assert_eq!(
		key(&mut vim, &buf, "A"),
		vec![
			VimCommand::Mode {
				mode: VimMode::Insert
			},
			VimCommand::Move { to: pos(0, 2) },
		]
	);
	let mut vim = VimState::new();
	assert_eq!(
		key(&mut vim, &buf, "I"),
		vec![
			VimCommand::Mode {
				mode: VimMode::Insert
			},
			VimCommand::Move { to: pos(0, 0) },
		]
	);
}

// ── motions ──────────────────────────────────────────────────────────────────

#[test]
fn horizontal_and_vertical_motions() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa", "beta"]), 0, 2);
	assert_eq!(key(&mut vim, &buf, "l"), vec![VimCommand::Move { to: pos(0, 3) }]);
	assert_eq!(key(&mut vim, &buf, "h"), vec![VimCommand::Move { to: pos(0, 1) }]);
	assert_eq!(key(&mut vim, &buf, "j"), vec![VimCommand::Move { to: pos(1, 2) }]);
	assert_eq!(
		key(&mut vim, &at(buf, 1, 2), "k"),
		vec![VimCommand::Move { to: pos(0, 2) }]
	);
}

#[test]
fn line_edges_and_word_motions() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta gamma"]);
	assert_eq!(key(&mut vim, &buf, "0"), vec![VimCommand::Move { to: pos(0, 0) }]);
	assert_eq!(key(&mut vim, &buf, "$"), vec![VimCommand::Move { to: pos(0, 14) }]);
	assert_eq!(key(&mut vim, &buf, "w"), vec![VimCommand::Move { to: pos(0, 5) }]);
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 5), "b"),
		vec![VimCommand::Move { to: pos(0, 0) }]
	);
	key(&mut vim, &buf, "3");
	assert_eq!(
		key(&mut vim, &buf, "w"),
		vec![VimCommand::Move { to: pos(0, 14) }]
	);
}

#[test]
fn buffer_edges_with_gg_and_g() {
	let mut vim = VimState::new();
	let buf = buf(&["one", "two", "three"]);
	assert_eq!(
		key(&mut vim, &buf, "G"),
		vec![VimCommand::Move { to: pos(2, 0) }]
	);
	key(&mut vim, &buf, "g");
	assert_eq!(key(&mut vim, &buf, "g"), vec![VimCommand::Move { to: pos(0, 0) }]);
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, "g");
	assert_eq!(key(&mut vim, &buf, "g"), vec![VimCommand::Move { to: pos(1, 0) }]);
}

// ── normal-mode edits ────────────────────────────────────────────────────────

#[test]
fn x_deletes_the_grapheme_under_the_cursor() {
	let mut vim = VimState::new();
	let buf = buf(&["abcd"]);
	assert_eq!(key(&mut vim, &buf, "x"), vec![delete(pos(0, 0), pos(0, 1), false, false)]);
	key(&mut vim, &buf, "2");
	assert_eq!(key(&mut vim, &buf, "x"), vec![delete(pos(0, 0), pos(0, 2), false, false)]);
}

#[test]
fn d_and_c_to_end_of_line() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa beta"]), 0, 5);
	assert_eq!(
		key(&mut vim, &buf, "D"),
		vec![delete(pos(0, 5), pos(0, 9), false, false)]
	);
	let mut vim = VimState::new();
	assert_eq!(
		key(&mut vim, &buf, "C"),
		vec![
			VimCommand::Move { to: pos(0, 5) },
			delete(pos(0, 5), pos(0, 9), false, true),
			VimCommand::Mode {
				mode: VimMode::Insert
			},
		]
	);
	assert_eq!(vim.mode, VimMode::Insert);
}

#[test]
fn dw_and_dd_delete_word_and_line() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta"]);
	key(&mut vim, &buf, "d");
	assert_eq!(key(&mut vim, &buf, "w"), vec![delete(pos(0, 0), pos(0, 5), false, false)]);

	let mut vim = VimState::new();
	let buf = buf(&["foo", "bar"]);
	key(&mut vim, &buf, "d");
	assert_eq!(key(&mut vim, &buf, "d"), vec![delete(pos(0, 0), pos(0, 3), true, false)]);
}

#[test]
fn cw_uses_the_ce_quirk_and_cc_clears_the_line() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta"]);
	key(&mut vim, &buf, "c");
	assert_eq!(
		key(&mut vim, &buf, "w"),
		vec![
			VimCommand::Move { to: pos(0, 0) },
			delete(pos(0, 0), pos(0, 5), false, true),
			VimCommand::Mode {
				mode: VimMode::Insert
			},
		]
	);

	let mut vim = VimState::new();
	let buf = buf(&["alfa beta", "second"]);
	key(&mut vim, &buf, "c");
	assert_eq!(
		key(&mut vim, &buf, "c"),
		vec![
			VimCommand::Move { to: pos(0, 0) },
			delete(pos(0, 0), pos(0, 9), false, true),
			VimCommand::Mode {
				mode: VimMode::Insert
			},
		]
	);
}

#[test]
fn yy_and_p_duplicate_a_line() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa", "beta"]);
	key(&mut vim, &buf, "y");
	assert_eq!(
		key(&mut vim, &buf, "y"),
		vec![
			VimCommand::Yank {
				from: pos(0, 0),
				to: pos(0, 4),
				linewise: true,
			},
			VimCommand::Move { to: pos(0, 0) },
		]
	);
	assert_eq!(
		key(&mut vim, &buf, "p"),
		vec![VimCommand::Paste { after: true, count: 1 }]
	);
}

#[test]
fn u_reports_undo() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa"]);
	assert_eq!(key(&mut vim, &buf, "u"), vec![VimCommand::Undo]);
}

// ── text objects ─────────────────────────────────────────────────────────────

#[test]
fn diw_deletes_the_word_under_the_cursor() {
	let mut vim = VimState::new();
	let buf = at(buf(&["foo bar"]), 0, 1);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "i");
	assert_eq!(key(&mut vim, &buf, "w"), vec![delete(pos(0, 0), pos(0, 3), false, false)]);
}

#[test]
fn daw_takes_the_trailing_whitespace() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa beta gamma"]), 0, 5);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "a");
	assert_eq!(key(&mut vim, &buf, "w"), vec![delete(pos(0, 5), pos(0, 10), false, false)]);
}

#[test]
fn counted_aw_takes_that_many_words() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta gamma"]);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, "a");
	assert_eq!(key(&mut vim, &buf, "w"), vec![delete(pos(0, 0), pos(0, 10), false, false)]);
}

#[test]
fn ciw_replaces_the_word_and_enters_insert() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa beta"]), 0, 1);
	key(&mut vim, &buf, "c");
	key(&mut vim, &buf, "i");
	assert_eq!(
		key(&mut vim, &buf, "w"),
		vec![
			VimCommand::Move { to: pos(0, 0) },
			delete(pos(0, 0), pos(0, 4), false, true),
			VimCommand::Mode {
				mode: VimMode::Insert
			},
		]
	);
}

#[test]
fn quoted_span_objects() {
	let mut vim = VimState::new();
	let buf = at(buf(&["say \"hello there\" now"]), 0, 5);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "i");
	assert_eq!(
		key(&mut vim, &buf, "\""),
		vec![delete(pos(0, 5), pos(0, 16), false, false)]
	);

	let mut vim = VimState::new();
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "a");
	assert_eq!(
		key(&mut vim, &buf, "\""),
		vec![delete(pos(0, 4), pos(0, 18), false, false)]
	);
}

#[test]
fn ci_quote_parks_the_cursor_between_empty_quotes() {
	let mut vim = VimState::new();
	let buf = at(buf(&["say \"\" now"]), 0, 5);
	key(&mut vim, &buf, "c");
	key(&mut vim, &buf, "i");
	let commands = key(&mut vim, &buf, "\"");
	assert_eq!(vim.mode, VimMode::Insert);
	assert!(commands.iter().any(|command| matches!(
		command,
		VimCommand::Mode {
			mode: VimMode::Insert
		}
	)));
}

#[test]
fn bracket_objects_span_pairs_and_lines() {
	let mut vim = VimState::new();
	let buf = at(buf(&["call(one, nest(two), three)"]), 0, 15);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "i");
	assert_eq!(
		key(&mut vim, &buf, "("),
		vec![delete(pos(0, 15), pos(0, 18), false, false)]
	);

	let mut vim = VimState::new();
	let buf = at(buf(&["fn {", "  body", "}", "tail"]), 1, 0);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "a");
	assert_eq!(
		key(&mut vim, &buf, "{"),
		vec![delete(pos(0, 3), pos(2, 1), false, false)]
	);
}

#[test]
fn paragraph_objects_are_linewise() {
	let mut vim = VimState::new();
	let buf = buf(&["one", "two", "", "three"]);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "i");
	assert_eq!(
		key(&mut vim, &buf, "p"),
		vec![delete(pos(0, 0), pos(1, 2), true, false)]
	);

	let mut vim = VimState::new();
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "a");
	assert_eq!(
		key(&mut vim, &buf, "p"),
		vec![delete(pos(0, 0), pos(2, 0), true, false)]
	);
}

#[test]
fn viw_selects_the_word_for_the_next_operator() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa beta"]), 0, 1);
	assert_eq!(
		key(&mut vim, &buf, "v"),
		vec![VimCommand::Mode {
			mode: VimMode::Visual
		}]
	);
	key(&mut vim, &buf, "i");
	assert_eq!(key(&mut vim, &buf, "w"), vec![VimCommand::Move { to: pos(0, 3) }]);
}

// ── visual mode ──────────────────────────────────────────────────────────────

#[test]
fn visual_delete_covers_the_selection() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta"]);
	key(&mut vim, &buf, "v");
	key(&mut vim, &at(buf.clone(), 0, 0), "l");
	key(&mut vim, &at(buf.clone(), 0, 1), "l");
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 2), "l"),
		vec![VimCommand::Move { to: pos(0, 3) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 0, 3), "d"),
		vec![
			delete(pos(0, 0), pos(0, 4), false, false),
			VimCommand::Mode {
				mode: VimMode::Normal
			},
		]
	);
	assert_eq!(vim.mode, VimMode::Normal);
}

#[test]
fn visual_yank_copies_without_editing() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta"]);
	key(&mut vim, &buf, "v");
	key(&mut vim, &at(buf.clone(), 0, 0), "l");
	key(&mut vim, &at(buf.clone(), 0, 1), "l");
	assert_eq!(
		key(&mut vim, &at(buf, 0, 2), "y"),
		vec![
			VimCommand::Yank {
				from: pos(0, 0),
				to: pos(0, 3),
				linewise: false,
			},
			VimCommand::Move { to: pos(0, 0) },
			VimCommand::Mode {
				mode: VimMode::Normal
			},
		]
	);
}

#[test]
fn visual_line_delete_spans_whole_lines() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa", "beta", "gamma"]);
	key(&mut vim, &buf, "V");
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 1, 0), "j"),
		vec![VimCommand::Move { to: pos(1, 0) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 1, 0), "d"),
		vec![
			delete(pos(0, 0), pos(1, 3), true, false),
			VimCommand::Mode {
				mode: VimMode::Normal
			},
		]
	);
}

#[test]
fn escape_leaves_visual_without_editing() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta"]);
	key(&mut vim, &buf, "v");
	key(&mut vim, &at(buf.clone(), 0, 0), "l");
	assert_eq!(
		key(&mut vim, &at(buf, 0, 1), "escape"),
		vec![
			VimCommand::Mode {
				mode: VimMode::Normal
			},
			VimCommand::Move { to: pos(0, 1) },
		]
	);
}

// ── batched input ────────────────────────────────────────────────────────────

#[test]
fn a_batched_run_is_replayed_key_by_key() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta gamma"]);
	assert_eq!(key(&mut vim, &buf, "w"), vec![VimCommand::Move { to: pos(0, 5) }]);
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 5), "w"),
		vec![VimCommand::Move { to: pos(0, 10) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 0, 10), "x"),
		vec![delete(pos(0, 10), pos(0, 11), false, false)]
	);
}

// ── autocomplete hand-off ────────────────────────────────────────────────────

#[test]
fn insert_mode_escape_returns_to_normal_for_the_host() {
	let mut vim = VimState::new();
	let buf = buf(&["/help"]);
	key(&mut vim, &buf, "i");
	assert_eq!(vim.mode, VimMode::Insert);
	// The machine handles the second Escape; a host popup intercepts the first.
	assert!(vim.handle_key("escape", &buf).is_some());
	assert_eq!(vim.mode, VimMode::Normal);
}

// ── protected regions ────────────────────────────────────────────────────────

#[test]
#[ignore = "missing facility: atomic placeholder tokens live in the Editor (atomicTokenPattern); vim.rs is token-unaware by design"]
fn atomic_tokens_are_protected() {
	let mut vim = VimState::new();
	let buf = buf(&["[Image #1, 800x600]!"]);
	assert_eq!(key(&mut vim, &buf, "x"), vec![delete(pos(0, 0), pos(0, 1), false, false)]);
}

// ── toggling ─────────────────────────────────────────────────────────────────

#[test]
fn reset_returns_to_a_clean_normal_state() {
	let mut vim = VimState::new();
	let buf = buf(&["abc"]);
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, "d");
	assert!(vim.pending());
	vim.reset();
	assert_eq!(vim.mode, VimMode::Normal);
	assert!(!vim.pending());
	assert_eq!(vim.pending_text(), "");
}

// ── mode chrome ──────────────────────────────────────────────────────────────

#[test]
fn pending_text_echoes_the_half_typed_command() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa bravo charlie"]);
	assert_eq!(vim.pending_text(), "");
	key(&mut vim, &buf, "2");
	assert_eq!(vim.pending_text(), "2");
	key(&mut vim, &buf, "d");
	assert_eq!(vim.pending_text(), "2d");
	key(&mut vim, &buf, "escape");
	assert_eq!(vim.pending_text(), "");
}

#[test]
fn visual_selection_height_is_anchor_to_cursor() {
	let mut vim = VimState::new();
	let buf = buf(&["one", "two", "three"]);
	key(&mut vim, &buf, "V");
	let anchor = vim.anchor.expect("visual anchor");
	assert_eq!(vim.mode, VimMode::VisualLine);
	assert_eq!(anchor, pos(0, 0));
	key(&mut vim, &at(buf, 1, 0), "j");
	let height = 1 - anchor.line + 1;
	assert_eq!(height, 2);
}

// ── desired column ───────────────────────────────────────────────────────────

#[test]
fn desired_column_survives_a_short_line() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa bravo charlie", "xy", "delta echo foxtrot"]), 0, 12);
	assert_eq!(key(&mut vim, &buf, "j"), vec![VimCommand::Move { to: pos(1, 12) }]);
	assert_eq!(
		key(&mut vim, &at(buf, 1, 12), "j"),
		vec![VimCommand::Move { to: pos(2, 12) }]
	);
}

#[test]
fn horizontal_motion_reanchors_the_column() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa bravo charlie", "xy", "delta echo foxtrot"]), 0, 12);
	key(&mut vim, &buf, "j");
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 1, 12), "0"),
		vec![VimCommand::Move { to: pos(1, 0) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 1, 0), "j"),
		vec![VimCommand::Move { to: pos(2, 0) }]
	);
}

#[test]
fn dollar_is_a_sticky_end_of_line_column() {
	let mut vim = VimState::new();
	let buf = at(buf(&["alfa bravo charlie", "xy", "delta echo foxtrot"]), 0, 0);
	assert_eq!(key(&mut vim, &buf, "$"), vec![VimCommand::Move { to: pos(0, 17) }]);
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 17), "j"),
		vec![VimCommand::Move { to: pos(1, 2) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 1, 2), "j"),
		vec![VimCommand::Move { to: pos(2, 18) }]
	);
}

// ── find and till ────────────────────────────────────────────────────────────

#[test]
fn find_and_till_land_on_the_adjacent_grapheme() {
	let buf = buf(&["abcde"]);
	let mut vim = VimState::new();
	key(&mut vim, &buf, "f");
	assert_eq!(key(&mut vim, &buf, "c"), vec![VimCommand::Move { to: pos(0, 2) }]);

	let mut vim = VimState::new();
	key(&mut vim, &at(buf.clone(), 0, 2), "F");
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 2), "a"),
		vec![VimCommand::Move { to: pos(0, 0) }]
	);

	let mut vim = VimState::new();
	key(&mut vim, &buf, "t");
	assert_eq!(key(&mut vim, &buf, "c"), vec![VimCommand::Move { to: pos(0, 1) }]);

	let mut vim = VimState::new();
	key(&mut vim, &at(buf.clone(), 0, 4), "T");
	assert_eq!(
		key(&mut vim, &at(buf, 0, 4), "c"),
		vec![VimCommand::Move { to: pos(0, 3) }]
	);
}

#[test]
fn counted_find_stays_on_the_line() {
	let mut vim = VimState::new();
	let buf = buf(&["abaca", "zzz"]);
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, "f");
	assert_eq!(key(&mut vim, &buf, "a"), vec![VimCommand::Move { to: pos(0, 4) }]);
	key(&mut vim, &buf, "f");
	assert!(key(&mut vim, &buf, "z").is_empty());
}

#[test]
fn find_includes_the_target_till_stops_short() {
	let mut vim = VimState::new();
	let buf = buf(&["abcde"]);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "f");
	assert_eq!(
		key(&mut vim, &buf, "c"),
		vec![delete(pos(0, 0), pos(0, 3), false, false)]
	);

	let mut vim = VimState::new();
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "t");
	assert_eq!(
		key(&mut vim, &buf, "c"),
		vec![delete(pos(0, 0), pos(0, 2), false, false)]
	);
}

#[test]
fn semicolon_repeats_and_comma_reverses() {
	let mut vim = VimState::new();
	let buf = buf(&["a-b-c"]);
	key(&mut vim, &buf, "f");
	assert_eq!(key(&mut vim, &buf, "-"), vec![VimCommand::Move { to: pos(0, 1) }]);
	assert_eq!(key(&mut vim, &buf, ";"), vec![VimCommand::Move { to: pos(0, 3) }]);
	assert_eq!(key(&mut vim, &buf, ","), vec![VimCommand::Move { to: pos(0, 1) }]);
}

#[test]
fn semicolon_repeats_a_till_past_the_target() {
	let mut vim = VimState::new();
	let buf = buf(&["a,b,c,d"]);
	key(&mut vim, &buf, "t");
	assert!(key(&mut vim, &buf, ",").is_empty());
	assert_eq!(key(&mut vim, &buf, ";"), vec![VimCommand::Move { to: pos(0, 2) }]);
}

#[test]
fn escape_cancels_a_half_typed_find() {
	let mut vim = VimState::new();
	let buf = buf(&["a1b"]);
	key(&mut vim, &buf, "f");
	assert!(vim.pending());
	key(&mut vim, &buf, "escape");
	assert!(!vim.pending());
	key(&mut vim, &buf, "f");
	assert_eq!(key(&mut vim, &buf, "1"), vec![VimCommand::Move { to: pos(0, 1) }]);
}

// ── replace ──────────────────────────────────────────────────────────────────

#[test]
fn r_replaces_the_character_under_the_cursor() {
	let mut vim = VimState::new();
	let buf = at(buf(&["abcd"]), 0, 1);
	key(&mut vim, &buf, "r");
	assert_eq!(
		key(&mut vim, &buf, "X"),
		vec![VimCommand::Replace {
			from: pos(0, 1),
			to: pos(0, 2),
			text: "X".to_string(),
		}]
	);
	assert_eq!(vim.mode, VimMode::Normal);
}

#[test]
fn counted_r_stops_at_the_line_end() {
	let mut vim = VimState::new();
	let buf = buf(&["abcd"]);
	key(&mut vim, &buf, "3");
	key(&mut vim, &buf, "r");
	assert_eq!(
		key(&mut vim, &buf, "z"),
		vec![VimCommand::Replace {
			from: pos(0, 0),
			to: pos(0, 3),
			text: "zzz".to_string(),
		}]
	);
	let buf = at(buf(&["abcd"]), 0, 3);
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, "r");
	assert!(key(&mut vim, &buf, "Q").is_empty());
}

#[test]
fn R_enters_replace_mode() {
	let mut vim = VimState::new();
	let buf = buf(&["abcd"]);
	assert_eq!(
		key(&mut vim, &buf, "R"),
		vec![VimCommand::Mode {
			mode: VimMode::Replace
		}]
	);
	assert_eq!(vim.mode, VimMode::Replace);
}

// ── bracket match ────────────────────────────────────────────────────────────

#[test]
fn percent_jumps_between_nested_pairs() {
	let mut vim = VimState::new();
	let buf = buf(&["(a(b)c)"]);
	assert_eq!(key(&mut vim, &buf, "%"), vec![VimCommand::Move { to: pos(0, 6) }]);
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 0, 6), "%"),
		vec![VimCommand::Move { to: pos(0, 0) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 0, 2), "%"),
		vec![VimCommand::Move { to: pos(0, 4) }]
	);
}

#[test]
fn counted_percent_jumps_to_a_buffer_percentage() {
	let mut vim = VimState::new();
	let buf = buf(&["(a)", "b", "  c", "d"]);
	key(&mut vim, &buf, "5");
	key(&mut vim, &buf, "0");
	assert_eq!(key(&mut vim, &buf, "%"), vec![VimCommand::Move { to: pos(1, 0) }]);
	key(&mut vim, &buf, "7");
	key(&mut vim, &buf, "5");
	assert_eq!(key(&mut vim, &buf, "%"), vec![VimCommand::Move { to: pos(2, 2) }]);
}

#[test]
fn unmatched_percent_stays_put() {
	let mut vim = VimState::new();
	let buf = buf(&["ab(c"]);
	assert!(key(&mut vim, &buf, "%").is_empty());
}

// ── paragraph motions ────────────────────────────────────────────────────────

#[test]
fn paragraph_boundaries_step_forward_and_back() {
	let mut vim = VimState::new();
	let buf = buf(&["aaa", "", "bbb", "", "ccc"]);
	assert_eq!(key(&mut vim, &buf, "}"), vec![VimCommand::Move { to: pos(1, 0) }]);
	assert_eq!(
		key(&mut vim, &at(buf.clone(), 1, 0), "}"),
		vec![VimCommand::Move { to: pos(3, 0) }]
	);
	assert_eq!(
		key(&mut vim, &at(buf, 3, 0), "{"),
		vec![VimCommand::Move { to: pos(1, 0) }]
	);
}

#[test]
fn delete_paragraph_leaves_the_boundary_line() {
	let mut vim = VimState::new();
	let buf = buf(&["aaa", "", "bbb"]);
	key(&mut vim, &buf, "d");
	assert_eq!(
		key(&mut vim, &buf, "}"),
		vec![delete(pos(0, 0), pos(1, 0), false, false)]
	);
}

#[test]
fn delete_paragraph_clears_to_the_buffer_end() {
	let mut vim = VimState::new();
	let buf = buf(&["hello world"]);
	key(&mut vim, &buf, "d");
	assert_eq!(
		key(&mut vim, &buf, "}"),
		vec![delete(pos(0, 0), pos(0, 11), false, false)]
	);
}

// ── join ─────────────────────────────────────────────────────────────────────

#[test]
fn join_reports_the_line_span() {
	let mut vim = VimState::new();
	let buf = buf(&["foo", "  bar"]);
	assert_eq!(
		key(&mut vim, &buf, "J"),
		vec![VimCommand::Join {
			from_line: 0,
			lines: 2,
		}]
	);
	key(&mut vim, &buf, "3");
	assert_eq!(
		key(&mut vim, &buf, "J"),
		vec![VimCommand::Join {
			from_line: 0,
			lines: 3,
		}]
	);
}

// ── indent ───────────────────────────────────────────────────────────────────

#[test]
fn indent_and_outdent_report_line_ranges() {
	let mut vim = VimState::new();
	let buf = buf(&["a", "b"]);
	key(&mut vim, &buf, ">");
	assert_eq!(
		key(&mut vim, &buf, ">"),
		vec![VimCommand::Indent {
			from_line: 0,
			to_line: 0,
			out: false,
		}]
	);
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, ">");
	assert_eq!(
		key(&mut vim, &buf, ">"),
		vec![VimCommand::Indent {
			from_line: 0,
			to_line: 1,
			out: false,
		}]
	);
	key(&mut vim, &buf, "<");
	assert_eq!(
		key(&mut vim, &buf, "<"),
		vec![VimCommand::Indent {
			from_line: 0,
			to_line: 0,
			out: true,
		}]
	);
}

#[test]
fn indent_follows_a_motion_and_a_visual_selection() {
	let mut vim = VimState::new();
	let buf = buf(&["a", "b", "c"]);
	key(&mut vim, &buf, ">");
	assert_eq!(
		key(&mut vim, &at(buf, 0, 0), "j"),
		vec![VimCommand::Indent {
			from_line: 0,
			to_line: 1,
			out: false,
		}]
	);

	let mut vim = VimState::new();
	let buf = buf(&["a", "b"]);
	key(&mut vim, &buf, "V");
	key(&mut vim, &at(buf.clone(), 1, 0), "j");
	assert!(matches!(
		key(&mut vim, &at(buf, 1, 0), ">").as_slice(),
		[VimCommand::Indent { from_line: 0, to_line: 1, out: false }, VimCommand::Mode { mode: VimMode::Normal }]
	));
}

#[test]
fn indent_applies_to_a_text_object() {
	let mut vim = VimState::new();
	let buf = buf(&["one", "two", "", "three"]);
	key(&mut vim, &buf, ">");
	key(&mut vim, &buf, "i");
	assert_eq!(
		key(&mut vim, &buf, "p"),
		vec![VimCommand::Indent {
			from_line: 0,
			to_line: 1,
			out: false,
		}]
	);
}

// ── dot repeat ───────────────────────────────────────────────────────────────

#[test]
fn dot_repeats_an_operator_change() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta gamma"]);
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "w");
	vim.commit_change();
	assert_eq!(
		key(&mut vim, &buf, "."),
		vec![delete(pos(0, 0), pos(0, 4), false, false)]
	);
}

#[test]
fn dot_replaces_the_count_with_the_new_count() {
	let mut vim = VimState::new();
	let buf = buf(&["a b c d e f g h i"]);
	key(&mut vim, &buf, "2");
	key(&mut vim, &buf, "d");
	key(&mut vim, &buf, "w");
	vim.commit_change();
	let buf = buf(&["c d e f g h i"]);
	key(&mut vim, &buf, "3");
	assert_eq!(
		key(&mut vim, &buf, "."),
		vec![delete(pos(0, 0), pos(0, 6), false, false)]
	);
}

#[test]
fn dot_replays_a_changes_typed_text() {
	let mut vim = VimState::new();
	let buf = buf(&["alfa beta gamma"]);
	key(&mut vim, &buf, "c");
	key(&mut vim, &buf, "w");
	vim.record_insert(
		VimInsertRecord {
			text: "new".to_string(),
			..VimInsertRecord::default()
		},
		false,
	);
	key(&mut vim, &buf, "escape");
	let commands = key(&mut vim, &buf, ".");
	assert!(commands.iter().any(|command| matches!(
		command,
		VimCommand::Retype { times: 1, newline: false, overwrite: false }
			if record.text == "new"
	)));
}

#[test]
fn dot_multiplies_an_insert_by_the_count() {
	let mut vim = VimState::new();
	let buf = buf(&["x"]);
	key(&mut vim, &buf, "A");
	vim.record_insert(
		VimInsertRecord {
			text: "!".to_string(),
			..VimInsertRecord::default()
		},
		false,
	);
	key(&mut vim, &buf, "escape");
	key(&mut vim, &buf, "3");
	let commands = key(&mut vim, &buf, ".");
	assert!(commands.iter().any(|command| matches!(
		command,
		VimCommand::Retype { times: 3, newline: false, overwrite: false }
			if record.text == "!"
	)));
}

#[test]
fn dot_repeats_an_open_line_once_per_count() {
	let mut vim = VimState::new();
	let buf = buf(&["a"]);
	key(&mut vim, &buf, "o");
	vim.record_insert(
		VimInsertRecord {
			text: "b".to_string(),
			..VimInsertRecord::default()
		},
		false,
	);
	key(&mut vim, &buf, "escape");
	key(&mut vim, &buf, "2");
	let commands = key(&mut vim, &buf, ".");
	assert!(commands.iter().any(|command| matches!(
		command,
		VimCommand::Retype { times: 2, newline: true, overwrite: false }
			if record.text == "b"
	)));
}

#[test]
fn dot_replays_replace_mode_as_overwrite() {
	let mut vim = VimState::new();
	let buf = buf(&["abcd"]);
	key(&mut vim, &buf, "R");
	vim.record_insert(
		VimInsertRecord {
			text: "xy".to_string(),
			..VimInsertRecord::default()
		},
		false,
	);
	key(&mut vim, &buf, "escape");
	key(&mut vim, &buf, "j");
	key(&mut vim, &buf, "0");
	let commands = key(&mut vim, &buf, ".");
	assert!(commands.iter().any(|command| matches!(
		command,
		VimCommand::Retype { overwrite: true, .. }
	)));
}

#[test]
fn dot_repeats_a_visual_change_over_the_same_shape() {
	let mut vim = VimState::new();
	let buf = buf(&["a", "b", "c", "d", "e"]);
	key(&mut vim, &buf, "V");
	key(&mut vim, &at(buf.clone(), 1, 0), "j");
	key(&mut vim, &at(buf, 1, 0), "d");
	vim.commit_change();
	let buf = buf(&["c", "d", "e"]);
	assert_eq!(
		key(&mut vim, &buf, "."),
		vec![
			delete(pos(0, 0), pos(1, 1), true, false),
			VimCommand::Mode {
				mode: VimMode::Normal
			},
		]
	);
}
