//! Terminal backend contract plus the process and virtual implementations.

pub mod process;
pub mod virt;

use crate::keys::Key;
use crate::mouse::MouseEvent;
use std::time::Duration;

/// Events delivered by a terminal backend to the UI loop.
#[derive(Clone, Debug)]
pub enum TtyEvent {
    Key(Key),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
    Disconnect,
}

/// A terminal the renderer can paint to and read input from.
pub trait Terminal {
    /// Claim the terminal (raw mode, alternate screen, protocol flags).
    fn start(&mut self) -> std::io::Result<()>;

    /// Release everything claimed by `start`.
    fn stop(&mut self) -> std::io::Result<()>;

    /// Write raw bytes (already encoded by the caller).
    fn write(&mut self, data: &str);

    /// Return one pending event without blocking.
    fn poll_event(&mut self) -> Option<TtyEvent> {
        self.event(Duration::ZERO)
    }

    /// Wait up to `timeout` for the next event.
    fn event(&mut self, timeout: Duration) -> Option<TtyEvent>;

    fn columns(&self) -> u16;
    fn rows(&self) -> u16;

    /// Move the cursor up (negative) or down (positive) by whole lines.
    fn move_by(&mut self, lines: i32);

    fn hide_cursor(&mut self, force: bool);
    fn show_cursor(&mut self, force: bool);
    fn clear_line(&mut self);
    fn clear_from_cursor(&mut self);
    fn clear_screen(&mut self);
}
