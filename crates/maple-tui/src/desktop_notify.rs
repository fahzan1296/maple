//! Desktop notifications (port of `l/src/desktop-notify.ts`).
//!
//! Best-effort: uses `notify-send` when present, otherwise falls back to a
//! terminal bell through the supplied writer so headless environments stay
//! silent.

use std::io::Write;
use std::process::{Command, Stdio};

pub struct DesktopNotifier {
    command: Option<&'static str>,
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopNotifier {
    pub fn new() -> Self {
        let command = if which("notify-send") {
            Some("notify-send")
        } else if which("terminal-notifier") {
            Some("terminal-notifier")
        } else {
            None
        };
        Self { command }
    }

    /// Send a notification; failures are ignored.
    pub fn notify(&self, title: &str, body: &str) {
        match self.command {
            Some("notify-send") => {
                let _ = Command::new("notify-send")
                    .arg(title)
                    .arg(body)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
            Some("terminal-notifier") => {
                let _ = Command::new("terminal-notifier")
                    .args(["-title", title, "-message", body])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
            _ => {}
        }
    }

    /// Bell fallback for terminals without a notification daemon.
    pub fn bell(&self, writer: &mut dyn Write) {
        let _ = writer.write_all(b"\x07");
    }
}

fn which(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_without_panicking() {
        let notifier = DesktopNotifier::new();
        notifier.notify("title", "body");
    }
}
