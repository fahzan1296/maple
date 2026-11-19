//! Transient multi-line status board for standalone CLI commands.
//!
//! Repaints a caller-rendered block of lines in place at ~12.5fps while
//! permanent output logged through [`LiveBoard::log`] scrolls above it.
//!
//! Reference: `l/src/chrome/live-board.ts`.
//!
//! Deviation: the reference repaints from a `setInterval` timer, reads the
//! process-global `theme`, and defaults to `process.stdout`. Rust has no timers
//! or global theme here, so [`LiveBoard`] owns the render closure, an injected
//! [`LiveBoardOutput`] trait object, and a `&Theme`; the caller advances time
//! with [`LiveBoard::tick`] and chooses the output. The 80 ms cadence lives at
//! the call site ([`RENDER_INTERVAL_MS`]). The reference's `process.stdout`
//! default and its `output.write` boolean result have no Rust equivalent; the
//! trait returns `()`.

use crate::render::render_utils::replace_tabs;
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::theme::symbols::SpinnerType;
use crate::theme::Theme;

/// Repaint cadence of the reference board, in milliseconds.
pub const RENDER_INTERVAL_MS: u64 = 80;

/// Output contract for the live board (satisfied by `process.stdout`).
pub trait LiveBoardOutput {
	/// Whether the output is an interactive terminal.
	fn is_tty(&self) -> bool;
	/// Terminal width in columns; `None` when unknown.
	fn columns(&self) -> Option<u16>;
	/// Terminal height in rows; `None` when unknown.
	fn rows(&self) -> Option<u16>;
	/// Write one raw frame.
	fn write(&mut self, text: &str);
}

/// Live repaint handle returned by [`LiveBoard::new`].
pub struct LiveBoard<'a, F>
where
	F: Fn(&str, usize) -> Vec<String>,
{
	render: F,
	output: Box<dyn LiveBoardOutput>,
	theme: &'a Theme,
	interactive: bool,
	frame: usize,
	line_count: usize,
	cursor_hidden: bool,
	closed: bool,
}

impl<'a, F> LiveBoard<'a, F>
where
	F: Fn(&str, usize) -> Vec<String>,
{
	/// Create a board whose content comes from `render(spinner, width)` on every
	/// repaint. An empty render result paints nothing and releases the cursor.
	pub fn new(render: F, output: Box<dyn LiveBoardOutput>, theme: &'a Theme) -> Self {
		let interactive = output.is_tty();
		LiveBoard {
			render,
			output,
			theme,
			interactive,
			frame: 0,
			line_count: 0,
			cursor_hidden: false,
			closed: false,
		}
	}

	/// Whether the output is interactive; non-TTY boards degrade to plain writes.
	pub fn interactive(&self) -> bool {
		self.interactive
	}

	/// Advance the spinner frame and repaint, matching one reference timer tick.
	pub fn tick(&mut self) {
		self.frame += 1;
		self.repaint();
	}

	/// Print a permanent line above the board; a plain write when non-interactive.
	pub fn log(&mut self, text: &str) {
		if self.closed || !self.interactive {
			self.output.write(&format!("{text}\n"));
			return;
		}
		self.paint(&[]);
		self.output.write(&format!("{text}\n"));
		self.repaint();
	}

	/// Repaint immediately after a state change instead of waiting for the next tick.
	pub fn repaint(&mut self) {
		if !self.interactive || self.closed {
			return;
		}
		let (width, max_rows) = self.dimensions();
		let theme = self.theme;
		let frames = theme.get_spinner_frames(SpinnerType::Activity);
		let spinner = if frames.is_empty() {
			"*"
		} else {
			frames[self.frame % frames.len()].as_str()
		};
		let mut lines = (self.render)(spinner, width);
		if lines.len() > max_rows {
			let kept = max_rows - 1;
			let hidden = lines.len() - kept;
			let mut truncated: Vec<String> = lines.into_iter().take(kept).collect();
			truncated.push(format!("\u{2026} +{hidden} more"));
			lines = truncated;
		}
		self.paint(&lines);
	}

	/// Clear the board, stop the timer, and restore the cursor.
	pub fn close(&mut self) {
		if self.closed {
			return;
		}
		self.closed = true;
		if !self.interactive {
			return;
		}
		self.paint(&[]);
	}

	/// Terminal width and board row budget: `rows - 2`, floored at 4, with the
	/// reference's 80x24 fallback when the output reports no usable size.
	fn dimensions(&self) -> (usize, usize) {
		let width = match self.output.columns() {
			Some(columns) if columns > 0 => columns as usize,
			_ => 80,
		};
		let rows = match self.output.rows() {
			Some(rows) if rows > 0 => rows as usize,
			_ => 24,
		};
		(width, rows.saturating_sub(2).max(4))
	}

	/// Paint `lines` in place, erasing each logical row and restoring the cursor
	/// when the board becomes empty.
	fn paint(&mut self, lines: &[String]) {
		if lines.is_empty() && self.line_count == 0 {
			return;
		}
		let width = self.dimensions().0;
		let mut out = String::new();
		if self.line_count > 0 {
			out.push_str(&format!("\x1b[{}A", self.line_count));
		}
		out.push('\r');
		if !lines.is_empty() {
			let rendered: Vec<String> = lines
				.iter()
				.map(|line| {
					format!(
						"\x1b[2K{}",
						truncate_to_width(&replace_tabs(line), width, Ellipsis::Unicode)
					)
				})
				.collect();
			out.push_str(&rendered.join("\r\n"));
			out.push_str("\r\n");
		}
		out.push_str("\x1b[0J");
		if !lines.is_empty() && !self.cursor_hidden {
			out.push_str("\x1b[?25l");
			self.cursor_hidden = true;
		} else if lines.is_empty() && self.cursor_hidden {
			out.push_str("\x1b[?25h");
			self.cursor_hidden = false;
		}
		self.output.write(&out);
		self.line_count = lines.len();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::color::ColorMode;
	use crate::theme::{create_theme, CreateThemeOptions};
	use std::cell::RefCell;
	use std::rc::Rc;

	fn test_theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("embedded dark theme"),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.expect("dark theme resolves")
	}

	struct TestOutput {
		is_tty: bool,
		columns: Option<u16>,
		rows: Option<u16>,
		written: Rc<RefCell<String>>,
	}

	impl LiveBoardOutput for TestOutput {
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
			self.written.borrow_mut().push_str(text);
		}
	}

	fn output(is_tty: bool, columns: Option<u16>, rows: Option<u16>) -> (TestOutput, Rc<RefCell<String>>) {
		let written = Rc::new(RefCell::new(String::new()));
		(
			TestOutput {
				is_tty,
				columns,
				rows,
				written: written.clone(),
			},
			written,
		)
	}

	#[test]
	fn tick_rotates_the_activity_spinner_and_paints_the_exact_frame() {
		let theme = test_theme();
		let (output, written) = output(true, Some(80), Some(24));
		let mut board =
			LiveBoard::new(|spinner, _width| vec![format!("{spinner} working")], Box::new(output), &theme);
		assert!(board.interactive());
		board.tick();
		assert_eq!(
			written.borrow().as_str(),
			"\r\x1b[2K\u{2819} working\r\n\x1b[0J\x1b[?25l"
		);
		for _ in 0..9 {
			board.tick();
		}
		assert!(written.borrow().ends_with("\x1b[1A\r\x1b[2K\u{280b} working\r\n\x1b[0J"));
	}

	#[test]
	fn max_rows_is_rows_minus_two_and_caps_the_block() {
		let theme = test_theme();
		let (output, written) = output(true, Some(80), Some(6));
		let mut board = LiveBoard::new(
			|_, _| {
				vec![
					"a".to_string(),
					"b".to_string(),
					"c".to_string(),
					"d".to_string(),
					"e".to_string(),
					"f".to_string(),
				]
			},
			Box::new(output),
			&theme,
		);
		assert_eq!(board.dimensions(), (80, 4));
		board.tick();
		assert_eq!(
			written.borrow().as_str(),
			"\r\x1b[2Ka\r\n\x1b[2Kb\r\n\x1b[2Kc\r\n\x1b[2K\u{2026} +3 more\r\n\x1b[0J\x1b[?25l"
		);
	}

	#[test]
	fn tiny_terminals_clamp_to_four_rows_and_unknown_sizes_fall_back() {
		let theme = test_theme();
		let (tiny, _) = output(true, None, Some(1));
		let tiny = LiveBoard::new(|_, _| Vec::new(), Box::new(tiny), &theme);
		assert_eq!(tiny.dimensions(), (80, 4));

		let (unknown, _) = output(true, None, None);
		let unknown = LiveBoard::new(|_, _| Vec::new(), Box::new(unknown), &theme);
		assert_eq!(unknown.dimensions(), (80, 22));
	}

	#[test]
	fn lines_are_tab_expanded_and_truncated_to_the_terminal_width() {
		let theme = test_theme();
		let (output, written) = output(true, Some(5), Some(24));
		let mut board = LiveBoard::new(
			|_, _| vec!["abcdef".to_string(), "x\ty".to_string()],
			Box::new(output),
			&theme,
		);
		board.tick();
		assert_eq!(
			written.borrow().as_str(),
			"\r\x1b[2Kabcd\u{2026}\r\n\x1b[2Kx   y\r\n\x1b[0J\x1b[?25l"
		);
	}

	#[test]
	fn log_clears_the_board_writes_the_line_and_repaints() {
		let theme = test_theme();
		let (output, written) = output(true, Some(80), Some(24));
		let mut board =
			LiveBoard::new(|spinner, _width| vec![format!("{spinner} working")], Box::new(output), &theme);
		board.tick();
		board.log("done");
		assert_eq!(
			written.borrow().as_str(),
			"\r\x1b[2K\u{2819} working\r\n\x1b[0J\x1b[?25l\
			 \x1b[1A\r\x1b[0J\x1b[?25hdone\n\
			 \r\x1b[2K\u{2819} working\r\n\x1b[0J\x1b[?25l"
		);
	}

	#[test]
	fn close_erases_the_board_and_restores_the_cursor_once() {
		let theme = test_theme();
		let (output, written) = output(true, Some(80), Some(24));
		let mut board =
			LiveBoard::new(|spinner, _width| vec![spinner.to_string()], Box::new(output), &theme);
		board.tick();
		board.close();
		assert!(written.borrow().ends_with("\x1b[1A\r\x1b[0J\x1b[?25h"));
		let after = written.borrow().clone();
		board.close();
		assert_eq!(written.borrow().as_str(), after.as_str());
	}

	#[test]
	fn non_tty_output_degrades_to_plain_writes() {
		let theme = test_theme();
		let (output, written) = output(false, Some(80), Some(24));
		let mut board =
			LiveBoard::new(|spinner, _width| vec![spinner.to_string()], Box::new(output), &theme);
		assert!(!board.interactive());
		board.tick();
		board.log("plain");
		board.close();
		assert_eq!(written.borrow().as_str(), "plain\n");
	}
}
