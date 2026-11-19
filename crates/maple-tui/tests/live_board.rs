//! Port of `l/test/live-board.test.ts`.
//!
//! `chrome::live_board::LiveBoard` is ported; expectations are adapted to the
//! crate's single-write paint batching (the reference writes each escape and
//! line separately). The cleanse-preview case stays ignored: the crate's
//! `CleanseBoardModel` has no `agent_finished` preview builder.

use maple_tui::chrome::live_board::{LiveBoard, LiveBoardOutput};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::symbols::{SpinnerType, SymbolPreset};
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};
use std::cell::RefCell;
use std::rc::Rc;

struct RecordingOutput {
	is_tty: bool,
	columns: Option<u16>,
	rows: Option<u16>,
	writes: Rc<RefCell<Vec<String>>>,
}

impl LiveBoardOutput for RecordingOutput {
	fn is_tty(&self) -> bool {
		self.is_tty
	}
	fn columns(&self) -> Option<u16> {
		self.columns
	}
	fn rows(&self) -> Option<u16> {
		self.rows
	}
	fn write(&mut self, text: &str) {
		self.writes.borrow_mut().push(text.to_string());
	}
}

fn recording(is_tty: bool) -> (RecordingOutput, Rc<RefCell<Vec<String>>>) {
	let writes = Rc::new(RefCell::new(Vec::new()));
	(
		RecordingOutput {
			is_tty,
			columns: Some(80),
			rows: Some(24),
			writes: writes.clone(),
		},
		writes,
	)
}

fn ascii_theme() -> Theme {
	let mut json = builtin_theme_json("dark").expect("embedded dark theme");
	json.symbols.get_or_insert_with(Default::default).preset = Some(SymbolPreset::Ascii);
	create_theme(json, CreateThemeOptions::default()).expect("ascii theme resolves")
}

#[test]
fn honors_the_active_ascii_spinner_preset() {
	let theme = ascii_theme();
	let (output, writes) = recording(true);
	let mut board =
		LiveBoard::new(|spinner, _width| vec![format!("{spinner} working")], Box::new(output), &theme);
	board.repaint();
	board.close();

	let frames = theme.get_spinner_frames(SpinnerType::Activity);
	let all: String = writes.borrow().concat();
	assert!(all.contains(&format!("{} working", frames[0])), "{all:?}");
	assert!(!all.contains(&['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'][..]));
}

#[test]
fn repaints_in_place_with_cursor_up_bookkeeping_and_interleaves_log_lines() {
	let theme = ascii_theme();
	let (output, writes) = recording(true);
	let lines = vec!["one".to_string(), "two".to_string()];
	let mut board = LiveBoard::new(
		move |_spinner, _width| lines.clone(),
		Box::new(output),
		&theme,
	);

	board.repaint();
	let first = writes.borrow()[0].clone();
	assert!(first.contains("one"), "{first:?}");
	assert!(first.contains("two"), "{first:?}");
	assert!(first.contains("\x1b[?25l"), "{first:?}");

	board.repaint();
	assert!(writes.borrow()[1].starts_with("\x1b[2A"), "{:?}", writes.borrow()[1]);

	board.log("done two");
	let logged = writes.borrow();
	// Clear (cursor-up + erase-below), the permanent line, then the repaint.
	assert!(logged[2].starts_with("\x1b[2A"), "{:?}", logged[2]);
	assert!(logged[2].contains("\x1b[0J"), "{:?}", logged[2]);
	assert_eq!(logged[3], "done two\n");
	assert!(logged[4].contains("one"), "{:?}", logged[4]);

	board.close();
	assert!(writes.borrow().concat().ends_with("\x1b[?25h"));
}

#[test]
fn writes_nothing_while_idle_and_degrades_log_to_plain_lines_when_non_interactive() {
	let theme = ascii_theme();
	let (idle_output, idle_writes) = recording(true);
	let mut idle = LiveBoard::new(|_spinner, _width| Vec::new(), Box::new(idle_output), &theme);
	idle.repaint();
	idle.close();
	assert!(idle_writes.borrow().is_empty());

	let (plain_output, plain_writes) = recording(false);
	let mut board = LiveBoard::new(|_spinner, _width| vec!["row".to_string()], Box::new(plain_output), &theme);
	board.repaint();
	board.log("plain");
	board.close();
	assert_eq!(*plain_writes.borrow(), vec!["plain\n".to_string()]);
}

#[test]
#[ignore = "missing facility: CleanseBoardModel has no agent_finished preview builder in the crate"]
fn bounds_cleanse_error_previews_by_terminal_cells_rather_than_utf16_units() {
	let _ = ascii_theme();
}
