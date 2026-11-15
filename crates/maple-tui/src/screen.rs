//! Differential screen painter.
//!
//! Mirrors the pi-tui paint contract: every frame is bracketed by
//! `PAINT_BEGIN`/`PAINT_END` (hide cursor, DEC 2026 synchronized output when
//! enabled, autowrap guard), content rows are addressed with `CSI row;1H`,
//! stale rows are reset and cleared before rewriting, and ordinary frames
//! never clear scrollback.

use crate::terminal::Terminal;
use crate::text_shape::{truncate_to_width, Ellipsis};

const PAINT_BEGIN_SYNC: &str = "\x1b[?25l\x1b[?2026h\x1b[?7l";
const PAINT_END_SYNC: &str = "\x1b[?7h\x1b[?2026l";
const PAINT_BEGIN_NO_SYNC: &str = "\x1b[?25l\x1b[?7l";
const PAINT_END_NO_SYNC: &str = "\x1b[?7h";

pub struct Screen {
    width: u16,
    rows: Vec<String>,
    cursor_row: u16,
    first_frame: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        Screen {
            width: 0,
            rows: Vec::new(),
            cursor_row: 0,
            first_frame: true,
        }
    }

    /// True when DEC 2026 synchronized output is emitted (default on).
    pub fn synchronized_output_enabled() -> bool {
        !std::env::var_os("PI_NO_SYNC_OUTPUT")
            .map(|value| value == "1")
            .unwrap_or(false)
    }

    /// Paint `target` rows at `width`, diffing against the previous frame.
    pub fn flush(&mut self, terminal: &mut dyn Terminal, target: &[String], width: u16) {
        if width == 0 {
            return;
        }
        let payload: Vec<String> = target
            .iter()
            .map(|line| truncate_to_width(line, width as usize, Ellipsis::Omit))
            .collect();

        let synchronized = Self::synchronized_output_enabled();
        let (begin, end) = if synchronized {
            (PAINT_BEGIN_SYNC, PAINT_END_SYNC)
        } else {
            (PAINT_BEGIN_NO_SYNC, PAINT_END_NO_SYNC)
        };
        terminal.write(begin);

        let needs_full =
            self.first_frame || width != self.width || payload.len() != self.rows.len();

        if needs_full {
            let erased_rows = self.erase_screen_body(terminal, &payload);
            self.cursor_row = payload.len().saturating_sub(1) as u16;
            if erased_rows {
                self.cursor_row = self.cursor_row.saturating_add(1);
            }
        } else if let Some(first) = first_difference(&self.rows, &payload) {
            self.rewrite_rows(terminal, &payload, first);
            self.cursor_row = payload.len().saturating_sub(1) as u16;
        }

        terminal.write(end);
        self.width = width;
        self.rows = payload;
        self.first_frame = false;
    }

    /// Home the cursor and paint all rows; returns true when leftover rows
    /// below the new content had to be erased.
    fn erase_screen_body(&mut self, terminal: &mut dyn Terminal, payload: &[String]) -> bool {
        terminal.write("\x1b[H");
        for (index, row) in payload.iter().enumerate() {
            if index > 0 {
                terminal.write("\n");
            }
            terminal.write(row);
            terminal.write("\r");
        }
        let leftover = self.rows.len().saturating_sub(payload.len());
        if leftover > 0 {
            let start_row = payload.len() as u16 + 1;
            terminal.write(&format!("\x1b[{start_row};1H"));
            terminal.write("\x1b[J");
            return true;
        }
        false
    }

    fn rewrite_rows(&mut self, terminal: &mut dyn Terminal, payload: &[String], first: usize) {
        let _ = self.cursor_row;
        for (offset, row) in payload.iter().enumerate().skip(first) {
            terminal.write(&format!("\x1b[{};1H", offset + 1));
            terminal.write("\x1b[0m\x1b[K");
            terminal.write(row);
            terminal.write("\r");
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn rows(&self) -> &[String] {
        &self.rows
    }
}

fn first_difference(previous: &[String], next: &[String]) -> Option<usize> {
    previous
        .iter()
        .zip(next.iter())
        .position(|(old, new)| old != new)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::terminal::virt::VirtualTerminal;

    #[test]
    fn wraps_frames_in_synchronized_paint() {
        let mut terminal = VirtualTerminal::new(20, 5);
        let mut screen = Screen::new();
        screen.flush(&mut terminal, &["hello".into()], 20);
        assert!(terminal.output().starts_with(PAINT_BEGIN_SYNC));
        assert!(terminal.output().ends_with(PAINT_END_SYNC));
        assert!(terminal.output().contains("\x1b[Hhello\r"));
    }

    #[test]
    fn rewrites_only_changed_rows() {
        let mut terminal = VirtualTerminal::new(20, 5);
        let mut screen = Screen::new();
        screen.flush(&mut terminal, &["a".into(), "b".into()], 20);
        terminal.clear_output();
        screen.flush(&mut terminal, &["a".into(), "c".into()], 20);
        assert!(!terminal.output().contains("\x1b[1;1H"));
        assert!(terminal.output().contains("\x1b[2;1H\x1b[0m\x1b[Kc\r"));
    }

    #[test]
    fn erases_rows_dropped_by_shorter_content() {
        let mut terminal = VirtualTerminal::new(20, 5);
        let mut screen = Screen::new();
        screen.flush(&mut terminal, &["a".into(), "b".into(), "c".into()], 20);
        terminal.clear_output();
        screen.flush(&mut terminal, &["a".into(), "b".into()], 20);
        assert!(terminal.output().contains("\x1b[3;1H\x1b[J"));
    }
}
