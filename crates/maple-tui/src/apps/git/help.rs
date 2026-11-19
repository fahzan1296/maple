//! Keyboard shortcuts sheet of the git TUI (`?`).
//!
//! Reference: `l/src/apps/git/help.ts`.
//!
//! The keys are the git TUI's own; this sheet only lists them. ANSI draws
//! grouped `key  description` rows; the native render is the hotkeys sheet
//! (NATIVE_REDESIGN §9.7): a glass sheet titled "Keyboard shortcuts" with one
//! section per group and a two-column table whose key cells carry the `key`
//! span token, so Tern draws keycaps.
//!
//! Deviations:
//! - `formatKeyHint` becomes a caller-supplied formatter closure (the crate has
//!   no global keybinding formatter); `render` takes `(width, format_hint)`.
//! - `matchesKey`/string input becomes crate [`Key`] handling in
//!   [`GitHelpSheet::handle_key`]; the TS `Component` contract differs.
//! - Native description builders (`col`/`node`/`span`/`actionBar`/`actionButton`)
//!   are represented as data in [`HelpNode`] because crate native APIs are not
//!   1:1 with the TS helpers; a native host can lower this tree.
//! - No global `theme`; ANSI colors use direct SGR escapes matching the TS
//!   `muted`/`accent`/`dim`/`bold` intent.

use crate::keys::Key;

/// A keyboard shortcut entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
	pub keys: Vec<&'static str>,
	pub label: &'static str,
}

/// One titled group of shortcuts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
	pub title: &'static str,
	pub rows: Vec<Shortcut>,
}

/// Default shortcut groups shown by the git help sheet.
pub fn groups() -> Vec<ShortcutGroup> {
	vec![
		ShortcutGroup {
			title: "Move",
			rows: vec![
				Shortcut {
					keys: vec!["up", "down"],
					label: "Move the selection",
				},
				Shortcut {
					keys: vec!["left", "right"],
					label: "Fold or unfold a folder or section",
				},
				Shortcut {
					keys: vec!["enter"],
					label: "Open the file in the diff",
				},
				Shortcut {
					keys: vec!["tab"],
					label: "Switch between the diff and the changes",
				},
				Shortcut {
					keys: vec!["]", "["],
					label: "Next or previous file",
				},
				Shortcut {
					keys: vec!["alt+down", "alt+up"],
					label: "Next or previous change",
				},
				Shortcut {
					keys: vec!["g", "shift+g"],
					label: "First or last row",
				},
			],
		},
		ShortcutGroup {
			title: "Stage",
			rows: vec![
				Shortcut {
					keys: vec!["space"],
					label: "Stage or unstage the row",
				},
				Shortcut {
					keys: vec!["s", "u"],
					label: "Stage or unstage (the hunk or selected lines in the diff)",
				},
				Shortcut {
					keys: vec!["shift+up", "shift+down"],
					label: "Select lines in the diff",
				},
				Shortcut {
					keys: vec!["x"],
					label: "Discard the hunk or selected lines (press twice)",
				},
				Shortcut {
					keys: vec!["delete"],
					label: "Discard the file or folder (press twice)",
				},
			],
		},
		ShortcutGroup {
			title: "View",
			rows: vec![
				Shortcut {
					keys: vec!["v"],
					label: "Cycle the diff view",
				},
				Shortcut {
					keys: vec!["1", "2", "3", "4"],
					label: "File, split, inline or hunk view",
				},
				Shortcut {
					keys: vec!["w"],
					label: "Wrap long lines",
				},
				Shortcut {
					keys: vec!["b"],
					label: "Ignore whitespace, then formatting",
				},
				Shortcut {
					keys: vec!["t"],
					label: "Paths or folder tree",
				},
				Shortcut {
					keys: vec!["r"],
					label: "Refresh",
				},
			],
		},
		ShortcutGroup {
			title: "Commit",
			rows: vec![
				Shortcut {
					keys: vec!["c"],
					label: "Write the commit message (empty: generate one)",
				},
				Shortcut {
					keys: vec!["escape"],
					label: "Leave the field",
				},
				Shortcut {
					keys: vec!["q"],
					label: "Quit",
				},
			],
		},
	]
}

/// Native description node (data-only stand-in for TS native helpers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelpNode {
	Col {
		role: &'static str,
		children: Vec<HelpNode>,
	},
	Section {
		title: &'static str,
		role: &'static str,
		rows: Vec<HelpTableRow>,
	},
	ActionBar {
		label: &'static str,
		act: &'static str,
		keys: &'static str,
	},
}

/// One hotkeys table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpTableRow {
	pub keys: Vec<String>,
	pub label: &'static str,
}

/// The shortcuts overlay; `?`, escape, `q` or ctrl+c closes it.
#[derive(Debug)]
pub struct GitHelpSheet<F>
where
	F: FnMut(),
{
	close: F,
	native: Option<HelpNode>,
}

impl<F> GitHelpSheet<F>
where
	F: FnMut(),
{
	/// Create a sheet that invokes `close` when dismissed.
	pub fn new(close: F) -> Self {
		Self { close, native: None }
	}

	/// Native overlay metadata (TS `nativeOverlay`).
	pub fn native_overlay_role(&self) -> &'static str {
		"omp.overlay.hotkeys"
	}
	pub fn native_overlay_head(&self) -> &'static str {
		"Keyboard shortcuts"
	}
	pub fn native_overlay_size(&self) -> &'static str {
		"lg"
	}

	/// Handle a key press; returns `true` when the sheet closed.
	pub fn handle_key(&mut self, key: &Key) -> bool {
		let close = match key {
			Key::Esc | Key::Ctrl('c') => true,
			Key::Char('?') | Key::Char('q') => true,
			_ => false,
		};
		if close {
			(self.close)();
		}
		close
	}

	/// Handle a native action event (`act == "close"` closes).
	pub fn handle_native_action(&mut self, act: &str) -> bool {
		if act == "close" {
			(self.close)();
			return true;
		}
		false
	}

	/// Build the native description tree.
	pub fn describe(&mut self, format_hint: &mut dyn FnMut(&str) -> String) -> HelpNode {
		if let Some(n) = &self.native {
			return n.clone();
		}
		let mut children = Vec::new();
		for group in groups() {
			let rows = group
				.rows
				.iter()
				.map(|r| HelpTableRow {
					keys: r.keys.iter().map(|k| format_hint(k)).collect(),
					label: r.label,
				})
				.collect();
			children.push(HelpNode::Section {
				title: group.title,
				role: "omp.hotkeys.group",
				rows,
			});
		}
		children.push(HelpNode::ActionBar {
			label: "Close",
			act: "close",
			keys: "escape",
		});
		let node = HelpNode::Col {
			role: "omp.app.git.help",
			children,
		};
		self.native = Some(node.clone());
		node
	}

	/// Render ANSI lines for `width` columns.
	pub fn render(
		&self,
		width: usize,
		format_hint: &mut dyn FnMut(&str) -> String,
	) -> Vec<String> {
		let gs = groups();
		let key_width = gs
			.iter()
			.flat_map(|g| g.rows.iter())
			.map(|r| {
				r.keys
					.iter()
					.map(|k| format_hint(k))
					.collect::<Vec<_>>()
					.join(" ")
					.chars()
					.count()
			})
			.max()
			.unwrap_or(0);
		let mut lines = Vec::new();
		lines.push(truncate_plain(&format!(" \x1b[1mKeyboard shortcuts\x1b[0m"), width));
		lines.push(String::new());
		for group in &gs {
			lines.push(truncate_plain(
				&format!(" \x1b[2m{}\x1b[0m", group.title),
				width,
			));
			for row in &group.rows {
				let keys = row
					.keys
					.iter()
					.map(|k| format_hint(k))
					.collect::<Vec<_>>()
					.join(" ");
				let pad = " ".repeat(key_width.saturating_sub(keys.chars().count()) + 2);
				lines.push(truncate_plain(
					&format!("   \x1b[36m{}\x1b[0m{}{}", keys, pad, row.label),
					width,
				));
			}
			lines.push(String::new());
		}
		let esc = format_hint("escape");
		lines.push(truncate_plain(
			&format!(" \x1b[2m{} close\x1b[0m", esc),
			width,
		));
		lines
	}

	/// Drop cached native description.
	pub fn invalidate(&mut self) {
		self.native = None;
	}
}

fn truncate_plain(s: &str, width: usize) -> String {
	if width == 0 {
		return String::new();
	}
	// Best-effort visible truncation by chars (ANSI-aware truncation lives in
	// crate text utils; this keeps the port dependency-light).
	let mut out = String::new();
	let mut vis = 0usize;
	let mut chars = s.chars().peekable();
	while let Some(c) = chars.next() {
		if c == '\x1b' {
			out.push(c);
			// copy escape sequence through final byte
			while let Some(&n) = chars.peek() {
				out.push(n);
				chars.next();
				if n.is_ascii_alphabetic() {
					break;
				}
			}
			continue;
		}
		if vis >= width {
			break;
		}
		out.push(c);
		vis += 1;
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fmt(k: &str) -> String {
		k.to_uppercase()
	}

	#[test]
	fn groups_match_reference_shape() {
		let gs = groups();
		assert_eq!(gs.len(), 4);
		assert_eq!(gs[0].title, "Move");
		assert_eq!(gs[1].title, "Stage");
		assert_eq!(gs[2].title, "View");
		assert_eq!(gs[3].title, "Commit");
		assert_eq!(gs[0].rows[0].keys, vec!["up", "down"]);
		assert_eq!(gs[3].rows[2].keys, vec!["q"]);
	}

	#[test]
	fn render_contains_title_groups_and_close_hint() {
		let sheet = GitHelpSheet::new(|| {});
		let mut f = fmt;
		let lines = sheet.render(120, &mut f);
		let text = lines.join("\n");
		assert!(text.contains("Keyboard shortcuts"));
		assert!(text.contains("Move"));
		assert!(text.contains("Stage"));
		assert!(text.contains("View"));
		assert!(text.contains("Commit"));
		assert!(text.contains("UP DOWN"));
		assert!(text.contains("ESCAPE close"));
	}

	#[test]
	fn handle_key_closes_on_expected_keys() {
		let mut closed = 0;
		{
			let mut sheet = GitHelpSheet::new(|| closed += 1);
			assert!(sheet.handle_key(&Key::Esc));
			assert!(sheet.handle_key(&Key::Char('q')));
			assert!(sheet.handle_key(&Key::Char('?')));
			assert!(sheet.handle_key(&Key::Ctrl('c')));
			assert!(!sheet.handle_key(&Key::Char('x')));
		}
		assert_eq!(closed, 4);
	}

	#[test]
	fn describe_builds_and_caches_native_tree() {
		let mut sheet = GitHelpSheet::new(|| {});
		let mut f = fmt;
		let a = sheet.describe(&mut f);
		let b = sheet.describe(&mut f);
		assert_eq!(a, b);
		sheet.invalidate();
		let c = sheet.describe(&mut f);
		assert_eq!(a, c);
	}

	#[test]
	fn native_overlay_metadata() {
		let sheet = GitHelpSheet::new(|| {});
		assert_eq!(sheet.native_overlay_role(), "omp.overlay.hotkeys");
		assert_eq!(sheet.native_overlay_head(), "Keyboard shortcuts");
		assert_eq!(sheet.native_overlay_size(), "lg");
	}
}
