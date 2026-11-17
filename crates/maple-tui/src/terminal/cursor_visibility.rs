//! Cursor-visibility write dedupe.
//!
//! Reference: `l/src/terminal.ts`.
//!
//! Ports the tracked `#cursorVisible` state and its sniffing rules:
//! `hide_cursor`/`show_cursor` skip the escape when the terminal already holds
//! that state; every outgoing write re-sniffs the last `?25h`/`?25l`, so frame
//! buffers that embed their own sequences stay in sync; an alt-screen switch
//! (`?1049h`/`?1049l`) newer than the last cursor sequence resets tracking to
//! unknown because some hosts keep DECTCEM per buffer. Force writes always emit.


/// Tracked cursor visibility for a terminal backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CursorVisibilityTracker {
	visible: Option<bool>,
}

impl CursorVisibilityTracker {
	/// A tracker with unknown state.
	pub fn new() -> Self {
		Self::default()
	}

	/// The tracked state; `None` means unknown (alt-screen reset).
	pub fn is_visible(&self) -> Option<bool> {
		self.visible
	}

	/// Whether a hide write is needed.
	pub fn should_hide(&self, force: bool) -> bool {
		force || self.visible != Some(false)
	}

	/// Whether a show write is needed.
	pub fn should_show(&self, force: bool) -> bool {
		force || self.visible != Some(true)
	}

	/// Record an emitted hide escape.
	pub fn record_hide(&mut self) {
		self.visible = Some(false);
	}

	/// Record an emitted show escape.
	pub fn record_show(&mut self) {
		self.visible = Some(true);
	}

	/// Sniff outgoing data for the last cursor-visibility change.
	pub fn track_write(&mut self, data: &str) {
		let mut idx = data.rfind("\x1b[?25");
		while let Some(found) = idx {
			let terminator = data.as_bytes().get(found + 5).copied();
			if terminator == Some(b'h') || terminator == Some(b'l') {
				break;
			}
			idx = if found == 0 {
				None
			} else {
				data[..found].rfind("\x1b[?25")
			};
		}
		let cursor_idx = idx.map(|found| found + 1);
		// Only a switch after the last cursor sequence matters: search that tail.
		if let Some(tail) = cursor_idx {
			if data[tail..].contains("\x1b[?1049") {
				self.visible = None;
				return;
			}
		} else if data.contains("\x1b[?1049") {
			self.visible = None;
			return;
		}
		if let Some(found) = idx {
			self.visible = Some(data.as_bytes()[found + 5] == b'h');
		}
	}
}
