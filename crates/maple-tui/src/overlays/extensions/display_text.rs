//! Untrusted display strings for `/extensions`.
//!
//! Ported from `l/src/overlays/extensions/display-text.ts` (verified to exist).
//!
//! MCP `serverInfo`, tool/resource/prompt catalogs, custom-tool schemas, and
//! discovered file content can carry ANSI, OSC/BEL, C0/C1 controls, or tabs.
//! Theme helpers wrap whatever they are given, so those sequences must be
//! stripped *before* SGR is applied.
//!
//! Deviations:
//! - `replaceTabs` is [`crate::render::render_utils::replace_tabs`].
//! - `sanitizeText` (`@oh-my-pi/pi-utils`) has no crate equivalent, so
//!   [`sanitize_text`] is a private port of
//!   `oh-my-pi/packages/utils/src/sanitize-text.ts`. `Bun.stripANSI` maps to
//!   [`crate::strip_ansi`] (same CSI/OSC/DCS/PM/APC/two-character coverage);
//!   the `toWellFormed()` + replacement-character step is a no-op because Rust
//!   `&str` is always well-formed UTF-8.
//! - `sanitizeDisplayField`/`sanitizeDisplayLineField` take `Option<&str>`
//!   instead of `string | undefined | null`.

use crate::render::render_utils::replace_tabs;

/// Strip ANSI/C0/C1/malformed unicode, then expand tabs. Call before theming.
pub fn sanitize_display_text(text: &str) -> String {
	replace_tabs(&sanitize_text(text))
}

/// Collapse newlines to spaces so a hostile title cannot inject extra TUI rows
/// (no trimming).
pub fn sanitize_display_single_line(text: &str) -> String {
	let cleaned = sanitize_display_text(text);
	let mut out = String::with_capacity(cleaned.len());
	let mut in_newline_run = false;
	for ch in cleaned.chars() {
		if ch == '\r' || ch == '\n' {
			if !in_newline_run {
				out.push(' ');
				in_newline_run = true;
			}
		} else {
			out.push(ch);
			in_newline_run = false;
		}
	}
	out
}

/// Collapse newlines so a hostile title cannot inject extra TUI rows.
pub fn sanitize_display_line(text: &str) -> String {
	sanitize_display_single_line(text).trim().to_string()
}

/// Like [`sanitize_display_text`], dropping empty results.
pub fn sanitize_display_field(text: Option<&str>) -> Option<String> {
	let text = text?;
	if text.is_empty() {
		return None;
	}
	let cleaned = sanitize_display_text(text);
	if cleaned.is_empty() {
		None
	} else {
		Some(cleaned)
	}
}

/// Like [`sanitize_display_line`], dropping empty results.
pub fn sanitize_display_line_field(text: Option<&str>) -> Option<String> {
	let text = text?;
	if text.is_empty() {
		return None;
	}
	let cleaned = sanitize_display_line(text);
	if cleaned.is_empty() {
		None
	} else {
		Some(cleaned)
	}
}

/// `sanitizeText` from `@oh-my-pi/pi-utils`, ported privately.
///
/// Reference: `oh-my-pi/packages/utils/src/sanitize-text.ts`. Drops ANSI
/// escapes, then removes C0 (except `\t`/`\n`), CR, DEL, and C1 controls.
fn sanitize_text(text: &str) -> String {
	let stripped = if text.contains('\x1b') {
		crate::strip_ansi(text)
	} else {
		text.to_string()
	};
	stripped.chars().filter(|ch| !is_control_char(*ch)).collect()
}

/// `CONTROL_RE = /[\x00-\x08\x0B-\x1F\x7F-\x9F]/g` from the reference.
fn is_control_char(ch: char) -> bool {
	matches!(ch, '\x00'..='\x08' | '\x0b'..='\x1f' | '\x7f'..='\u{9f}')
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sanitize_display_text_strips_ansi_controls_and_expands_tabs() {
		let cases = [
			("plain", "plain"),
			("\x1b[31mred\x1b[0m", "red"),
			("\x1b]8;;http://example.invalid\x07link\x1b]8;;\x07", "link"),
			("\x1b[1;32mgreen\x1b[0m!", "green!"),
			("a\tb", "a   b"),
			("\x00a\x07b\x7fc\u{9f}d", "abcd"),
			("keep\ttab\tand\nnewline", "keep   tab   and\nnewline"),
		];
		for (input, expected) in cases {
			assert_eq!(sanitize_display_text(input), expected, "input {input:?}");
		}
	}

	#[test]
	fn sanitize_display_single_line_collapses_newline_runs_without_trimming() {
		let cases = [
			("a\r\n\nb", "a b"),
			("a\nb", "a b"),
			("  a\nb  ", "  a b  "),
			("no-newlines", "no-newlines"),
			("\n\n", " "),
		];
		for (input, expected) in cases {
			assert_eq!(sanitize_display_single_line(input), expected, "input {input:?}");
		}
	}

	#[test]
	fn sanitize_display_line_trims_after_collapsing() {
		let cases = [("  a\nb  ", "a b"), ("\n\nx\r\n", "x"), ("   ", "")];
		for (input, expected) in cases {
			assert_eq!(sanitize_display_line(input), expected, "input {input:?}");
		}
	}

	#[test]
	fn fields_drop_missing_empty_and_blank_results() {
		assert_eq!(sanitize_display_field(None), None);
		assert_eq!(sanitize_display_field(Some("")), None);
		assert_eq!(sanitize_display_field(Some("\x1b[0m")), None);
		assert_eq!(sanitize_display_field(Some("  kept  ")), Some("  kept  ".to_string()));
		assert_eq!(sanitize_display_field(Some("x")), Some("x".to_string()));

		assert_eq!(sanitize_display_line_field(None), None);
		assert_eq!(sanitize_display_line_field(Some("")), None);
		assert_eq!(sanitize_display_line_field(Some("\x1b[31m\x1b[0m")), None);
		assert_eq!(sanitize_display_line_field(Some("  a\nb  ")), Some("a b".to_string()));
		assert_eq!(sanitize_display_line_field(Some("\t")), None);
	}
}
