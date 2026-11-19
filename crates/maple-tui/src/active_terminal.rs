//! Ownership of stdout by a started [`ProcessTerminal`].
//! Reference: `l/src/active-terminal.ts`.
//!
//! Port of `l/src/active-terminal.ts`. The terminal that owns stdout is kept in
//! a leaf module so writers that `terminal.rs` itself imports (capabilities,
//! notifications) can route through it without an import cycle.
//!
//! Divergence: the TypeScript original installs a `process.stdout` `error`
//! listener and consults `Bun.isMainThread`. Rust has neither: writes return
//! `io::Result`, so the best-effort path ignores errors, and every thread has
//! its own active terminal (normally only the main thread sets one).

use crate::terminal::process::ProcessTerminal;
use crate::terminal::Terminal;

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

thread_local! {
    static ACTIVE: RefCell<Option<Rc<RefCell<ProcessTerminal>>>> = const { RefCell::new(None) };
}

/// The started terminal that owns stdout on this thread, if any.
pub fn get_active_terminal() -> Option<Rc<RefCell<ProcessTerminal>>> {
    ACTIVE.with(|slot| slot.borrow().clone())
}

/// Record the terminal that owns stdout (`ProcessTerminal::start`), or release
/// it with `None` (`stop`).
pub fn set_active_terminal(terminal: Option<Rc<RefCell<ProcessTerminal>>>) {
    ACTIVE.with(|slot| *slot.borrow_mut() = terminal);
}

/// Hand `data` to the active terminal's ordered output path.
///
/// Returns `false` when no terminal on this thread has started. On the main
/// thread that means the caller owns stdout; another thread never sees the main
/// thread's terminal, so there `false` is not permission to write.
pub fn write_through_active_terminal(data: &str) -> bool {
    let Some(terminal) = get_active_terminal() else {
        return false;
    };
    terminal.borrow_mut().write(data);
    true
}

/// Write an out-of-band escape sequence (window title, OSC 52 clipboard,
/// notification).
///
/// While a TUI owns stdout its frames go through ordered writes; a direct
/// `stdout` write could land between two chunks and split a frame's escape
/// sequence, and the host terminal would print the tail as text. So the
/// sequence takes the active terminal's ordered path, and goes straight to
/// stdout, best-effort, only when no terminal owns it.
pub fn write_terminal_sequence(data: &str) {
    if write_through_active_terminal(data) {
        return;
    }
    let mut stdout = io::stdout();
    let _ = stdout.write_all(data.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> Rc<RefCell<ProcessTerminal>> {
        Rc::new(RefCell::new(ProcessTerminal::new()))
    }


    #[test]
    fn tracks_the_active_terminal_per_thread() {
        assert!(get_active_terminal().is_none());
        let terminal = terminal();
        set_active_terminal(Some(terminal.clone()));
        assert!(Rc::ptr_eq(&get_active_terminal().unwrap(), &terminal));
        set_active_terminal(None);
        assert!(get_active_terminal().is_none());
    }

    #[test]
    fn routes_through_the_active_terminal() {
        let terminal = terminal();
        set_active_terminal(Some(terminal));
        assert!(write_through_active_terminal("\x1b]0;title\x07"));
        write_terminal_sequence("\x1b]0;title\x07");
        set_active_terminal(None);
        assert!(!write_through_active_terminal("\x1b]0;title\x07"));
    }
}
