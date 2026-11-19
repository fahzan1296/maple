//! (No direct single-file counterpart in `l/src/terminal.ts`; this is a test/virtual backend.)
use std::collections::VecDeque;
use std::time::Duration;

use super::{CursorVisibilityTracker, Terminal, TtyEvent};

pub struct VirtualTerminal {
    width: u16,
    height: u16,
    output: String,
    events: VecDeque<TtyEvent>,
    cursor_row: u16,
    cursor_tracker: CursorVisibilityTracker,
    started: bool,
    scrollback: Vec<String>,
    scrollback_limit: usize,
    viewport: Vec<String>,
    viewport_offset: usize,
    hardware_cursor: Option<(u16, u16)>,
}

impl VirtualTerminal {
    pub fn new(width: u16, height: u16) -> Self {
        VirtualTerminal {
            width,
            height,
            output: String::new(),
            events: VecDeque::new(),
            cursor_row: 0,
            cursor_tracker: CursorVisibilityTracker::new(),
            started: false,
            scrollback: Vec::new(),
            scrollback_limit: 4096,
            viewport: Vec::new(),
            viewport_offset: 0,
            hardware_cursor: None,
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

    /// Hardware cursor `(row, column)` placed by the last frame, if visible.
    pub fn hardware_cursor(&self) -> Option<(u16, u16)> {
        self.hardware_cursor
    }

    /// Rows evicted above the live viewport.
    pub fn scrollback(&self) -> &[String] {
        &self.scrollback
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.viewport_offset = 0;
    }

    /// Scroll up by `offset` rows from the live viewport; clamped to scrollback.
    pub fn scroll_to(&mut self, offset: usize) {
        self.viewport_offset = offset.min(self.scrollback.len());
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let next = (self.viewport_offset as i64 + i64::from(delta)).max(0) as usize;
        self.scroll_to(next);
    }

    pub fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    /// Visible rows: live viewport at offset zero, otherwise a scrollback window.
    pub fn viewport_rows(&self) -> Vec<String> {
        if self.viewport_offset == 0 {
            return self.viewport.clone();
        }
        let mut all = self.scrollback.clone();
        all.extend(self.viewport.iter().cloned());
        let end = all.len().saturating_sub(self.viewport_offset);
        let start = end.saturating_sub(self.height as usize);
        all[start..end].to_vec()
    }

    pub fn viewport_height(&self) -> usize {
        self.height as usize
    }

    pub fn viewport_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.viewport.truncate(height as usize);
        self.viewport.resize(height as usize, String::new());
        self.viewport_offset = self.viewport_offset.min(self.scrollback.len());
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
        self.cursor_tracker.track_write(data);
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

    fn hide_cursor(&mut self, force: bool) {
        if !self.cursor_tracker.should_hide(force) {
            return;
        }
        self.cursor_tracker.record_hide();
        self.output.push_str("\x1b[?25l");
    }

    fn show_cursor(&mut self, force: bool) {
        if !self.cursor_tracker.should_show(force) {
            return;
        }
        self.cursor_tracker.record_show();
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

    fn append_scrollback(&mut self, rows: &[String]) {
        self.scrollback.extend(rows.iter().cloned());
        let overflow = self.scrollback.len().saturating_sub(self.scrollback_limit);
        if overflow > 0 {
            self.scrollback.drain(0..overflow);
        }
    }

    fn set_viewport(&mut self, rows: &[String]) {
        self.viewport = rows.iter().map(|row| row.trim_end().to_string()).collect();
    }

    fn set_hardware_cursor(&mut self, position: Option<(u16, u16)>) {
        self.hardware_cursor = position;
    }

    fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.viewport_offset = 0;
    }
}
