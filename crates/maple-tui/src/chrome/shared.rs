//! Shared text sanitization and chrome theme helpers.
//!
//! Reference: `l/src/chrome/shared.ts`.
//!
//! Deviations:
//! - The reference reads the process-global `theme` singleton; maple-tui has no
//!   global theme, so [`tab_bar_theme`] takes an explicit `&Theme`.
//! - `sanitizeStatusText` is implemented against crate display sanitizers
//!   (`overlays::extensions::display_text`) and whitespace collapse, matching the
//!   reference behavior without depending on a process-global sanitizer object.

use crate::components::tab_bar::TabBarTheme;
use crate::overlays::extensions::display_text::sanitize_display_single_line;
use crate::style::Style;
use crate::symbols::SymbolTheme;
use crate::theme::Theme;
use crate::theme::tui_adapters::{bg_style, fg_style};

/// Compact single-line display text for statuses; collapse padding and trim.
pub fn sanitize_status_text(text: &str) -> String {
	sanitize_display_single_line(text)
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// 7-bit SGR reset emitted after styled hook status text.
const SGR_RESET: &str = "\x1b[0m";

fn is_sgr_sequence(bytes: &[u8], start: usize) -> Option<usize> {
	// `ESC [` + params (digits ; :) + `m`
	if bytes.get(start).copied()? != 0x1b || bytes.get(start + 1).copied()? != b'[' {
		return None;
	}
	let mut i = start + 2;
	while i < bytes.len() {
		match bytes[i] {
			b'0'..=b'9' | b';' | b':' => i += 1,
			b'm' => return Some(i + 1 - start),
			_ => return None,
		}
	}
	None
}

fn strip_non_sgr_escapes(text: &str) -> String {
	// Drop every escape except 7-bit SGR and drop C0/C1 control chars; keep
	// whitespace/control placeholders so visible whitespace can be normalized
	// consistently afterward.
	let bytes = text.as_bytes();
	let mut out = String::with_capacity(text.len());
	let mut i = 0;
	while i < bytes.len() {
		if let Some(len) = is_sgr_sequence(bytes, i) {
			out.push_str(&text[i..i + len]);
			i += len;
			continue;
		}
		let b = bytes[i];
		if b == 0x1b {
			// Drop ESC and its sequence payload.
			i += 1;
			if i >= bytes.len() {
				break;
			}
			match bytes[i] {
				b'[' => {
					i += 1;
					while i < bytes.len() {
						let c = bytes[i];
						i += 1;
						if (0x40..=0x7e).contains(&c) {
							break;
						}
					}
				}
				b']' => {
					// OSC: BEL or ST terminated.
					i += 1;
					while i < bytes.len() {
						if bytes[i] == 0x07 {
							i += 1;
							break;
						}
						if bytes[i] == 0x1b && bytes.get(i + 1).copied() == Some(b'\\') {
							i += 2;
							break;
						}
						i += 1;
					}
				}
				b'P' | b'_' | b'^' | b'X' => {
					// DCS/PM/APC/SOS: ST terminated.
					i += 1;
					while i < bytes.len() {
						if bytes[i] == 0x07 {
							i += 1;
							break;
						}
						if bytes[i] == 0x1b && bytes.get(i + 1).copied() == Some(b'\\') {
							i += 2;
							break;
						}
						i += 1;
					}
				}
				_ => {
					i += 1;
				}
			}
			continue;
		}
		let ch = text[i..].chars().next().unwrap();
		let ch_len = ch.len_utf8();
		if ch.is_control() {
			out.push(ch);
		} else {
			out.push(ch);
		}
		i += ch_len;
	}
	out
}

/// Like [`sanitize_status_text`], but keeps 7-bit SGR so styled status text can
/// be coloured. Every other escape, control, and line break is sanitized; space
/// runs collapse and edges trim by visible text. A styled result always ends
/// with a reset.
pub fn sanitize_hook_status_text(input: &str) -> String {
	let stripped = strip_non_sgr_escapes(input);
	if !stripped.contains("\x1b[") {
		return sanitize_status_text(&stripped);
	}

	let mut out = String::new();
	let mut pending = String::new();
	let mut last_was_space = false;
	let mut has_visible = false;
	let bytes = stripped.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if let Some(len) = is_sgr_sequence(bytes, i) {
			pending.push_str(&stripped[i..i + len]);
			i += len;
			continue;
		}
		let ch = stripped[i..].chars().next().unwrap();
		let ch_len = ch.len_utf8();
		i += ch_len;
		if ch.is_whitespace() {
			if has_visible && !(ch == ' ' && last_was_space) {
				pending.push(ch);
			}
			last_was_space = ch == ' ';
			continue;
		}
		if ch.is_control() {
			continue;
		}
		out.push_str(&pending);
		out.push(ch);
		pending.clear();
		last_was_space = false;
		has_visible = true;
	}
	if !has_visible {
		return String::new();
	}
	let trailing_sgr = {
		let mut only_sgr = String::new();
		let mut j = 0;
		let b = pending.as_bytes();
		let mut all_sgr = true;
		while j < b.len() {
			if let Some(len) = is_sgr_sequence(b, j) {
				only_sgr.push_str(&pending[j..j + len]);
				j += len;
			} else {
				let ch = pending[j..].chars().next().unwrap();
				if !ch.is_whitespace() {
					all_sgr = false;
					break;
				}
				j += ch.len_utf8();
			}
		}
		if all_sgr { only_sgr } else { String::new() }
	};
	format!("{out}{trailing_sgr}{SGR_RESET}")
}

/// Shared tab bar theme used by fullscreen overlays.
///
/// Deviation: takes an explicit `&Theme` because maple-tui has no process-global
/// theme singleton.
pub fn tab_bar_theme(theme: &Theme) -> TabBarTheme {
	let active = Style::merge(bg_style(theme, "selectedBg"), fg_style(theme, "text"));
	TabBarTheme {
		label: fg_style(theme, "accent").bold(),
		active_tab: active.bold(),
		inactive_tab: fg_style(theme, "muted"),
		hint: fg_style(theme, "dim").dim(),
		muted_tab: Some(fg_style(theme, "dim")),
		hover_tab: Some(active),
		symbols: SymbolTheme::unicode(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::loader::{create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		let json = crate::theme::builtin::builtin_theme_json("dark").expect("dark");
		create_theme(json, CreateThemeOptions::default()).expect("theme")
	}

	#[test]
	fn sanitize_status_text_collapses_and_trims() {
		assert_eq!(sanitize_status_text("  a \n\t b  "), "a b");
		assert_eq!(sanitize_status_text("\x1b[31mred\x1b[0m"), "red");
		assert_eq!(sanitize_status_text(""), "");
	}

	#[test]
	fn sanitize_hook_status_text_preserves_sgr_and_resets() {
		let styled = sanitize_hook_status_text(" \x1b[31mred\x1b[0m  text ");
		assert_eq!(styled, "\x1b[31mred\x1b[0m text\x1b[0m");
		assert!(sanitize_hook_status_text("  \t\n  ").is_empty());
	}

	#[test]
	fn sanitize_hook_status_text_strips_other_escapes() {
		let cleaned = sanitize_hook_status_text("\x1b]8;;x\x07hi\x1b]8;;\x07");
		assert_eq!(cleaned, "hi");
	}

	#[test]
	fn tab_bar_theme_populates_fields() {
		let theme = test_theme();
		let bar = tab_bar_theme(&theme);
		assert!(bar.muted_tab.is_some());
		assert!(bar.hover_tab.is_some());
	}
}
