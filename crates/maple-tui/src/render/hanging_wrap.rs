//! Hanging/prefixed wrapping helpers.
//! Reference: `l/src/render/hanging-wrap.ts`.

use crate::text_shape::{visible_width, wrap_text_with_ansi};
use crate::theme::theme_class::Theme;

/// Wrap content under a distinct first-row prefix, preserving its column on continuation rows.
pub fn wrap_prefixed_line(prefix: &str, continuation_prefix: &str, content: &str, width: usize) -> Vec<String> {
	let prefix_width = visible_width(prefix);
	let continuation_width = visible_width(continuation_prefix);
	if width <= prefix_width.max(continuation_width) {
		return wrap_text_with_ansi(&format!("{prefix}{content}"), width.max(1));
	}
	let col = width.saturating_sub(prefix_width.max(continuation_width)).max(1);
	let rows = wrap_text_with_ansi(content, col);
	rows.into_iter()
		.enumerate()
		.map(|(i, row)| {
			if i == 0 {
				format!("{prefix}{row}")
			} else {
				format!("{continuation_prefix}{row}")
			}
		})
		.collect()
}

/// Preserve a tree connector's ancestor guides while hanging a long node beneath its label.
pub fn wrap_tree_line(line: &str, width: usize, theme: &Theme) -> Option<Vec<String>> {
	let branch = theme.symbol("tree.branch").to_string();
	let last = theme.symbol("tree.last").to_string();
	for connector in [&branch, &last] {
		let marker = theme.fg("dim", connector).unwrap_or_else(|_| connector.to_string());
		let Some(index) = line.find(&marker) else { continue };
		let before = &line[..index];
		let before_plain = strip_ansi(before).replace(&theme.symbol("tree.vertical").to_string(), "");
		if !before_plain.chars().all(|c| c == ' ') {
			continue;
		}
		let end = index + marker.len() + 1;
		if end == 0 || end > line.len() || !line.as_bytes().get(end - 1).is_some_and(|b| *b == b' ') {
			continue;
		}
		let connector_width = visible_width(connector);
		let vertical = theme.symbol("tree.vertical").to_string();
		let guide = if connector == &last {
			" ".repeat(connector_width)
		} else {
			let v = theme.fg("dim", &vertical).unwrap_or_else(|_| vertical.clone());
			format!(
				"{v}{}",
				" ".repeat(connector_width.saturating_sub(visible_width(&vertical)))
			)
		};
		let prefix = &line[..end];
		let cont = format!("{before}{guide} ");
		let rest = &line[end..];
		return Some(wrap_prefixed_line(prefix, &cont, rest, width));
	}
	None
}

fn guide_run_end(line: &str, vertical: &str) -> usize {
	let bytes = line.as_bytes();
	let mut i = 0usize;
	while i < bytes.len() {
		if bytes[i] == b' ' {
			i += 1;
			continue;
		}
		if !vertical.is_empty() && line[i..].starts_with(vertical) {
			i += vertical.len();
			continue;
		}
		if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
			let mut j = i + 2;
			while j < bytes.len() {
				let b = bytes[j];
				j += 1;
				if (b'@'..=b'~').contains(&b) {
					break;
				}
			}
			i = j;
			continue;
		}
		break;
	}
	i
}

/// Hang a tree row beneath its connector, or a connector-less detail row beneath its guide rails.
pub fn wrap_guided_line(line: &str, width: usize, theme: &Theme) -> Vec<String> {
	if let Some(tree) = wrap_tree_line(line, width, theme) {
		return tree;
	}
	let vertical = theme.symbol("tree.vertical").to_string();
	let end = guide_run_end(line, &vertical).min(line.len());
	let prefix = &line[..end];
	wrap_prefixed_line(prefix, prefix, &line[end..], width)
}

fn strip_ansi(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let bytes = s.as_bytes();
	let mut i = 0usize;
	while i < bytes.len() {
		if bytes[i] == 0x1b {
			i += 1;
			if i < bytes.len() && bytes[i] == b'[' {
				i += 1;
				while i < bytes.len() {
					let b = bytes[i];
					i += 1;
					if (b'@'..=b'~').contains(&b) {
						break;
					}
				}
				continue;
			}
			if i < bytes.len() && bytes[i] == b']' {
				i += 1;
				while i < bytes.len() {
					let b = bytes[i];
					if b == 0x07 {
						i += 1;
						break;
					}
					if b == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
						i += 2;
						break;
					}
					i += 1;
				}
				continue;
			}
			// skip single escaped char
			if i < bytes.len() {
				i += 1;
			}
			continue;
		}
		let ch = s[i..].chars().next().unwrap();
		out.push(ch);
		i += ch.len_utf8();
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::default_theme;

	#[test]
	fn prefixed_wrap_basic() {
		let rows = wrap_prefixed_line("> ", "  ", "hello world", 8);
		assert!(!rows.is_empty());
		assert!(rows[0].starts_with("> "));
	}

	#[test]
	fn guided_falls_back_to_prefix() {
		let theme = default_theme();
		let rows = wrap_guided_line("  detail text", 6, &theme);
		assert!(!rows.is_empty());
	}

	#[test]
	fn strip_ansi_removes_sgr() {
		assert_eq!(strip_ansi("\x1b[31mhi\x1b[0m"), "hi");
	}
}
