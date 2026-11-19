//! Interactive bash/PTY console types and input normalization.
//!
//! Reference: `l/src/tools/bash-interactive.ts`.
//!
//! Deviations:
//! - TS implements a full TUI overlay component backed by `@oh-my-pi/pi-utils/vterm`
//!   (xterm), TUI rendering, key parsing, native describe nodes, and PTY resize
//!   plumbing. The terminal emulator and component rendering are not ported;
//!   Rust provides backend/state types and PTY input normalization helpers.
//! - Kitty/key parsing is approximated for common keys; full `parseKittySequence`
//!   / `matchesKey` parity depends on host key infrastructure.

/// Resize-only backend exposed by an interactive PTY controller.
pub trait BashInteractiveTerminalBackend: Send + Sync {
	/// Resize PTY.
	fn resize(&self, columns: u16, rows: u16);
}

/// Interactive console lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BashInteractiveState {
	Running,
	Complete,
	TimedOut,
	Killed,
}

/// Completion result for an interactive session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BashInteractiveCompletion {
	/// Exit code.
	pub exit_code: Option<i32>,
	/// Cancelled.
	pub cancelled: bool,
	/// Timed out.
	pub timed_out: bool,
}

/// Translate common key inputs into PTY bytes (simplified).
///
/// This is a best-effort port of `normalizeInputForPty` for standard keys;
/// application-cursor mode affects arrow/home/end sequences.
pub fn normalize_input_for_pty(data: &str, application_cursor_keys_mode: bool) -> String {
	// Printable single chars / text passthrough (approximation of extractPrintableText).
	if data.chars().count() == 1 {
		let ch = data.chars().next().unwrap();
		if !ch.is_control() {
			return data.to_string();
		}
	}
	match data {
		"\r" | "\n" => "\r".to_string(),
		"\t" => "\t".to_string(),
		" " => " ".to_string(),
		"\x7f" | "\x08" => "\x7f".to_string(),
		"\x1b" => "\x1b".to_string(),
		// Common escape sequences (TUI may pass canonical names or raw sequences).
		"\x1b[A" | "up" => if application_cursor_keys_mode { "\x1bOA" } else { "\x1b[A" }.to_string(),
		"\x1b[B" | "down" => if application_cursor_keys_mode { "\x1bOB" } else { "\x1b[B" }.to_string(),
		"\x1b[C" | "right" => if application_cursor_keys_mode { "\x1bOC" } else { "\x1b[C" }.to_string(),
		"\x1b[D" | "left" => if application_cursor_keys_mode { "\x1bOD" } else { "\x1b[D" }.to_string(),
		"\x1b[H" | "home" => if application_cursor_keys_mode { "\x1bOH" } else { "\x1b[H" }.to_string(),
		"\x1b[F" | "end" => if application_cursor_keys_mode { "\x1bOF" } else { "\x1b[F" }.to_string(),
		"\x1b[5~" | "pageup" => "\x1b[5~".to_string(),
		"\x1b[6~" | "pagedown" => "\x1b[6~".to_string(),
		"\x1b[2~" | "insert" => "\x1b[2~".to_string(),
		"\x1b[3~" | "delete" => "\x1b[3~".to_string(),
		"\x1b[Z" | "shift+tab" => "\x1b[Z".to_string(),
		_ => {
			// ctrl+<a-z> / alt+<a-z> textual forms
			if let Some(rest) = data.strip_prefix("ctrl+").or_else(|| data.strip_prefix("Ctrl+")) {
				if rest.len() == 1 {
					let b = rest.as_bytes()[0];
					if b.is_ascii_alphabetic() {
						let code = (b.to_ascii_lowercase() - b'a' + 1) as char;
						return code.to_string();
					}
				}
			}
			if let Some(rest) = data.strip_prefix("alt+").or_else(|| data.strip_prefix("Alt+")) {
				if rest.len() == 1 {
					return format!("\x1b{rest}");
				}
			}
			data.to_string()
		}
	}
}

/// Max live write queue chunks before dropping (mirrors TS cap).
pub const MAX_LIVE_WRITE_QUEUE_CHUNKS: usize = 512;
/// Native sheet inset columns.
pub const NATIVE_SHEET_INSET_COLS: usize = 8;
/// Native kill action id.
pub const KILL_ACTION: &str = "kill";

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalize_arrows_and_controls() {
		assert_eq!(normalize_input_for_pty("up", false), "\x1b[A");
		assert_eq!(normalize_input_for_pty("up", true), "\x1bOA");
		assert_eq!(normalize_input_for_pty("home", true), "\x1bOH");
		assert_eq!(normalize_input_for_pty("ctrl+c", false), "\x03");
		assert_eq!(normalize_input_for_pty("alt+x", false), "\x1bx");
		assert_eq!(normalize_input_for_pty("a", false), "a");
	}

	#[test]
	fn constants() {
		assert_eq!(MAX_LIVE_WRITE_QUEUE_CHUNKS, 512);
		assert_eq!(KILL_ACTION, "kill");
	}
}
