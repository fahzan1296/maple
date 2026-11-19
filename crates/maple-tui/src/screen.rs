//! Differential screen painter.
//!
//! Mirrors the `l/src/tui.ts` paint contract: every frame is bracketed by
//! `PAINT_BEGIN`/`PAINT_END` (hide cursor, DEC 2026 synchronized output when
//! enabled, autowrap guard), content rows are addressed with `CSI row;1H`,
//! stale rows are reset and cleared before rewriting, and ordinary frames
//! never clear scrollback.

use crate::terminal::Terminal;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

const PAINT_BEGIN_SYNC: &str = "\x1b[?25l\x1b[?2026h\x1b[?7l";
const PAINT_END_SYNC: &str = "\x1b[?7h\x1b[?2026l";
const PAINT_BEGIN_NO_SYNC: &str = "\x1b[?25l\x1b[?7l";
const PAINT_END_NO_SYNC: &str = "\x1b[?7h";
/// Escape bytes a single row may carry into a paint. Zero-width escape floods
/// (a hostile or buggy upstream) otherwise pass through unbounded: the visible
/// text is preserved either way, so past the budget escapes are dropped while
/// text keeps flowing.
const MAX_ROW_ESCAPE_BYTES: usize = 4096;

/// Drop escape sequences past [`MAX_ROW_ESCAPE_BYTES`] in `line`, keeping every
/// printable character.
fn bound_row_escapes(line: &str) -> String {
	let mut out = String::with_capacity(line.len());
	let mut escape_bytes = 0usize;
	let mut bounded = false;
	for token in crate::ansi_scan::scan(line) {
		match token {
			crate::ansi_scan::Token::Escape(seq) => {
				if escape_bytes + seq.len() <= MAX_ROW_ESCAPE_BYTES {
					escape_bytes += seq.len();
					out.push_str(seq);
				} else {
					bounded = true;
				}
			}
			crate::ansi_scan::Token::Text(text) => out.push_str(text),
		}
	}
	if bounded {
		out
	} else {
		line.to_string()
	}
}


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
        !crate::env_lookup("MAPLE_NO_SYNC_OUTPUT")
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
            .map(|line| truncate_to_width(&bound_row_escapes(line), width as usize, Ellipsis::Omit))
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

/// OSC 66 scaled-span prefix (`ESC ] 66 ;`).
const OSC66_PREFIX: &str = "\x1b]66;";
/// Maximum number of blank rows a scaled heading may reserve below it.
const OSC66_MAX_SPACER_ROWS: usize = 6;

/// Whether `line` carries an OSC 66 scaled span.
///
/// Reference: `l/src/utils.ts::isOsc66Line`.
pub fn is_osc66_line(line: &str) -> bool {
	line.contains(OSC66_PREFIX)
}

/// Largest `s=` scale among the OSC 66 spans in `line` (1 when none is scaled).
///
/// Reference: `l/src/utils.ts::osc66MaxScale`.
pub fn osc66_max_scale(line: &str) -> u32 {
	if !line.contains(OSC66_PREFIX) {
		return 1;
	}
	let mut max = 1;
	for span in line.split(OSC66_PREFIX).skip(1) {
		let body = span
			.split('\x07')
			.next()
			.unwrap_or(span);
		let body = body.split("\x1b\\").next().unwrap_or(body);
		let header = body.split(';').next().unwrap_or(body);
		for part in header.split(':') {
			let bytes = part.as_bytes();
			if bytes.first() != Some(&b's') || bytes.get(1) != Some(&b'=') {
				continue;
			}
			if let Ok(value) = part[2..].parse::<u32>() {
				if value > max && value <= 7 {
					max = value;
				}
			}
		}
	}
	max
}

/// Columns to preserve when `lines[index]` is a blank row that a scaled OSC 66
/// heading flows into, or `-1` when it is not such a row.
///
/// A scale-`s` heading occupies `s` rows and `visibleWidth` columns; the `s - 1`
/// blank rows beneath hold the multicell glyph's lower half and must never be
/// erased. Scans upward across the contiguous blank run.
///
/// Reference: `l/src/tui.ts::TUI#osc66SpacerGlyphWidth`.
pub fn osc66_spacer_glyph_width(lines: &[String], index: usize) -> i64 {
	if index == 0 || lines.get(index).is_none_or(|line| !line.is_empty()) {
		return -1;
	}
	let mut gap = 1usize;
	while gap < OSC66_MAX_SPACER_ROWS
		&& index - gap > 0
		&& lines.get(index - gap).is_some_and(|line| line.is_empty())
	{
		gap += 1;
	}
	let Some(above) = lines.get(index - gap) else {
		return -1;
	};
	if !is_osc66_line(above) {
		return -1;
	}
	if gap as i64 > osc66_max_scale(above) as i64 - 1 {
		return -1;
	}
	visible_width(above) as i64
}

/// Terminal bytes rewriting one reserved OSC 66 spacer row: preserve the glyph's
/// columns `[0, spacer_glyph_width)` and clear stale cells to their right.
///
/// Reference: `l/src/tui.ts::TUI#lineRewriteSequence` spacer branch.
pub fn spacer_row_rewrite(spacer_glyph_width: i64, width: usize) -> String {
	if spacer_glyph_width < 0 || spacer_glyph_width as usize >= width {
		return String::new();
	}
	format!("\x1b[0m\x1b[{spacer_glyph_width}C\x1b[K")
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
