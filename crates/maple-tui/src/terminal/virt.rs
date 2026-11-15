//! In-memory terminal used by tests and headless harnesses.

use super::{TtyEvent, Terminal};
use std::collections::VecDeque;
use std::time::Duration;

pub struct VirtualTerminal {
    width: u16,
    height: u16,
    output: String,
    events: VecDeque<TtyEvent>,
    cursor_row: u16,
    cursor_visible: bool,
    started: bool,
}

impl VirtualTerminal {
    pub fn new(width: u16, height: u16) -> Self {
        VirtualTerminal {
            width,
            height,
            output: String::new(),
            events: VecDeque::new(),
            cursor_row: 0,
            cursor_visible: true,
            started: false,
        }
    }

    pub fn push_event(&mut self, event: TtyEvent) {
        self.events.push_back(event);
    }

    /// Everything written so far, including control sequences.
    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn into_output(self) -> String {
        self.output
    }

    pub fn cursor_row(&self) -> u16 {
        self.cursor_row
    }
    /// Discard captured output (test helper).
    pub fn clear_output(&mut self) {
        self.output.clear();
    }


    pub fn is_started(&self) -> bool {
        self.started
    }
}

impl Terminal for VirtualTerminal {
    fn start(&mut self) -> std::io::Result<()> {
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> std::io::Result<()> {
        self.started = false;
        Ok(())
    }

    fn write(&mut self, data: &str) {
        self.output.push_str(data);
        for byte in data.bytes() {
            if byte == b'\n' {
                self.cursor_row = self.cursor_row.saturating_add(1);
            }
        }
    }

    fn event(&mut self, _timeout: Duration) -> Option<TtyEvent> {
        self.events.pop_front()
    }

    fn columns(&self) -> u16 {
        self.width
    }

    fn rows(&self) -> u16 {
        self.height
    }

    fn move_by(&mut self, lines: i32) {
        self.cursor_row = if lines > 0 {
            self.cursor_row.saturating_add(lines as u16)
        } else {
            self.cursor_row.saturating_sub((-lines) as u16)
        };
        self.output.push_str(&format!("\x1b[{}B", lines.abs()));
    }

    fn hide_cursor(&mut self, _force: bool) {
        self.cursor_visible = false;
        self.output.push_str("\x1b[?25l");
    }

    fn show_cursor(&mut self, _force: bool) {
        self.cursor_visible = true;
        self.output.push_str("\x1b[?25h");
    }

    fn clear_line(&mut self) {
        self.output.push_str("\x1b[2K");
    }

    fn clear_from_cursor(&mut self) {
        self.output.push_str("\x1b[J");
    }

    fn clear_screen(&mut self) {
        self.cursor_row = 0;
        self.output.push_str("\x1b[2J\x1b[H");
    }
}
