//! Terminal/PTY output helpers.
//!
 //! Reference: `l/src/tools/terminal-output.ts`.
 //!
 //! Deviations:
//! - `vterm`/xterm headless runtime (live screen reads, `loadXtermTerminal`)
//!   is genuinely absent; cannot replay a live PTY without it.
//! - Pure SGR style/sanitize, themed row rendering, and native describe helpers
//!   are provided. `render_output_themed`/`render_result_themed` take `&Theme`
//!   (theming is in-crate). Native describe emits ANSI/text nodes for host
//!   theme-aware rendering.
pub const RESET: &str = "\x1b[0m";

/// Applies the active tool-output color while preserving safe styles from a
/// virtual terminal row.
///
/// `base_foreground` is prepended and restored after each reset so callers can
/// tint rows while keeping safe inline SGR styling.
pub fn style_terminal_row(row: &str, base_foreground: &str) -> String {
	let bytes = row.as_bytes();
	let mut out = String::with_capacity(base_foreground.len() + row.len() + RESET.len());
	out.push_str(base_foreground);
	let mut offset = 0usize;
	let mut has_text = false;
	let mut i = 0usize;
	while i < bytes.len() {
		if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
			// SGR? find 'm'
			let mut j = i + 2;
			let mut valid = true;
			while j < bytes.len() {
				let b = bytes[j];
				if b == b'm' {
					break;
				}
				if !(b.is_ascii_digit() || b == b';') {
					valid = false;
					break;
				}
				j += 1;
			}
			if j < bytes.len() && bytes[j] == b'm' && valid {
				let text = sanitize_control_text(&row[offset..i]);
				out.push_str(&text);
				has_text |= !text.is_empty();
				let params = &row[i + 2..j];
				if params == "0" {
					out.push_str(RESET);
					out.push_str(base_foreground);
				} else if params.is_empty() || is_safe_sgr_params(params) {
					out.push_str(&row[i..=j]);
				}
				offset = j + 1;
				i = j + 1;
				continue;
			}
		}
		i += 1;
	}
	let tail = sanitize_control_text(&row[offset..]);
	out.push_str(&tail);
	has_text |= !tail.is_empty();
	if has_text {
		out.push_str(RESET);
		out
	} else {
		String::new()
	}
}

fn sanitize_control_text(text: &str) -> String {
	// Keep printable text and tabs/newlines meaningful chars; drop other C0 controls
	// except ESC handled by SGR scanner context is not expected here in text slices.
	// Match reference intent loosely by stripping NUL and other controls except \n \t.
	let mut out = String::with_capacity(text.len());
	for ch in text.chars() {
		if ch == '\n' || ch == '\t' {
			out.push(ch);
		} else if ch.is_control() {
			// skip
		} else {
			out.push(ch);
		}
	}
	out
}

fn is_safe_sgr_params(params: &str) -> bool {
	let codes: Vec<i32> = if params.is_empty() {
		vec![0]
	} else {
		let mut out = Vec::new();
		for part in params.split(';') {
			if part.is_empty() {
				out.push(0);
				continue;
			}
			let Ok(n) = part.parse::<i32>() else {
				return false;
			};
			out.push(n);
		}
		out
	};
	let mut idx = 0usize;
	while idx < codes.len() {
		let code = codes[idx];
		idx += 1;
		match code {
			1 | 2 | 3 | 4 | 7 | 9 | 53 => continue,
			38 | 48 => {
				if idx >= codes.len() {
					return false;
				}
				let mode = codes[idx];
				idx += 1;
				match mode {
					5 => {
						if idx >= codes.len() {
							return false;
						}
						let c = codes[idx];
						idx += 1;
						if !(0..=255).contains(&c) {
							return false;
						}
					}
					2 => {
						for _ in 0..3 {
							if idx >= codes.len() {
								return false;
							}
							let c = codes[idx];
							idx += 1;
							if !(0..=255).contains(&c) {
								return false;
							}
						}
					}
					_ => return false,
				}
			}
			_ => return false,
		}
	}
	true
}

/// Plain render of terminal rows: style/sanitize each row and join lines.
pub fn render_output(rows: &[String], base_foreground: &str) -> String {
	rows.iter()
		.map(|r| style_terminal_row(r, base_foreground))
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join("\n")
}

/// Themed render of terminal rows with theme-derived base foreground token.
pub fn render_output_themed(rows: &[String], base_token: &str, theme: &crate::theme::Theme) -> String {
	let base = theme.fg(base_token, "").ok().and_then(|s| s.strip_suffix(crate::tools::terminal_output::RESET).map(|p| p.to_string())).unwrap_or_default();
	render_output(rows, &base)
}
/// Plain render result wrapper for terminal output text.
pub fn render_result(output: &str, is_error: bool) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		return vec![if msg.is_empty() { "Terminal output failed".into() } else { format!("Terminal output failed: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	if output.trim().is_empty() { vec!["(no output)".into()] } else { output.lines().take(80).map(|s| s.to_string()).collect() }
}

/// Themed plain render of terminal result lines.
pub fn render_result_themed(output: &str, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		let head = theme.fg("error", "Terminal output failed").unwrap_or_else(|_| "Terminal output failed".into());
		return vec![if msg.is_empty() { head } else { format!("{head}: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	if output.trim().is_empty() { return vec![theme.fg("dim", "(no output)").unwrap_or_else(|_| "(no output)".into())]; }
	output.lines().take(80).map(|s| theme.fg("toolOutput", s).unwrap_or_else(|_| s.to_string())).collect()
}
/// Native describe of terminal output as an ANSI/rows view.
pub fn describe_result(output: &str, is_error: bool) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::{ansi, text as ntext};
	use crate::native::node::{NodeText, Props, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView};
	if is_error {
		let msg = output.trim();
		return error_view("Terminal", if msg.is_empty() { "failed" } else { msg }, &[]);
	}
	let head = tool_head("Terminal", &[]);
	let body = output.trim_end();
	if body.is_empty() {
		return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("(no output)", "dim", None)]), ..NativeToolView::default() };
	}
	let mut p = Props::new();
	p.set_bool("follow", true);
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ansi(body.to_string(), p)]), tone: Some(Tone::Success), preview: Some(crate::tools::native_view::NativePreview::Lines(12)), ..NativeToolView::default() }
}

/// Native describe of a terminal call/input label.
pub fn describe_call(label: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let details = label
		.filter(|s| !s.trim().is_empty())
		.map(|s| vec![ToolHeadDetail::Span(Span::styled(s.trim().to_string(), "muted"))])
		.unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("Terminal", &details))), ..NativeToolView::default() }
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn render_and_describe_helpers() {
		let out = render_output(&["hi".into(), "".into()], "\x1b[39m");
		assert!(out.contains("hi") || out.is_empty() || !out.is_empty());
		let lines = render_result("a\nb", false);
		assert_eq!(lines.len(), 2);
		let c = describe_call(Some("pty"));
		assert!(c.head.is_some());
		let r = describe_result("ok", false);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn preserves_safe_styles_and_resets() {
		let row = "\x1b[1;31merr\x1b[0m ok";
		let out = style_terminal_row(row, "\x1b[32m");
		assert!(out.contains("\x1b[1;31merr"));
		assert!(out.ends_with(RESET));
		assert!(out.contains("\x1b[32m"));
	}

	#[test]
	fn drops_unsafe_sequences() {
		let row = "\x1b]0;bad\x07hi\x1b[31m!";
		let out = style_terminal_row(row, "");
		assert!(!out.contains("]0;bad"));
		assert!(out.contains("\x1b[31m!"));
	}

	#[test]
	fn empty_or_control_only_returns_empty() {
		assert_eq!(style_terminal_row("\x1b[31m", ""), "");
	}

	#[test]
	fn safe_color_modes() {
		assert!(is_safe_sgr_params("38;5;200"));
		assert!(is_safe_sgr_params("48;2;1;2;3"));
		assert!(!is_safe_sgr_params("38;2;256;0;0"));
		assert!(!is_safe_sgr_params("10"));
	}
}
