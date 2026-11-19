//! Reference: `l/src/overlays/plan-toc.ts`.
//!
//! Pure heading/section parser for the plan-review overlay: splits a plan's
//! markdown into a flat list of sections — a leading preamble (text before the
//! first heading) followed by one entry per ATX heading — preserving the exact
//! source bytes of each section so the overlay can render, delete without
//! re-rendering markdown, and round-trip the document.
//!
//! No TUI dependencies: unit-tested in isolation.
//!
//! Deviations:
//! - The reference uses two regexes; this port scans by hand (no `regex`
//!   dependency in the crate).

/// One section of a plan document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanSection {
	/// `0` = preamble (no heading, no ToC entry); `1..=6` = heading depth.
	pub level: usize,
	/// Plain-text heading label with inline markdown lightly stripped.
	pub title: String,
	/// Exact source slice for this section, including its trailing newline(s).
	pub raw: String,
}

/// Strip inline markdown emphasis/link/code syntax down to readable text.
///
/// Deliberately light (not a markdown render) so ToC entries read cleanly:
/// `**Goal** & [docs](x)` becomes `Goal & docs`.
#[must_use]
pub fn strip_inline_markdown(text: &str) -> String {
	// Images first (so the link pass below does not eat the `(url)`), then
	// links, reference links, and autolinks.
	let text = strip_inline_links(text, true);
	let text = strip_inline_links(&text, false);
	let text = strip_reference_links(&text);
	let text = strip_autolinks(&text);
	// Inline code, then bold/italic/strikethrough emphasis runs.
	let text = strip_delimited(&text, "`");
	let text = strip_emphasis(&text);
	let text = strip_delimited(&text, "~~");
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Replace `![alt](url)` (when `image`) or `[alt](url)` with the alt text.
fn strip_inline_links(text: &str, image: bool) -> String {
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::with_capacity(text.len());
	let mut index = 0usize;
	while index < chars.len() {
		let open = if image {
			chars[index] == '!'
				&& index + 1 < chars.len()
				&& chars[index + 1] == '['
				&& index + 2 <= chars.len()
		} else {
			chars[index] == '['
		};
		if open {
			let bracket = if image { index + 1 } else { index };
			if let Some(close) = find_char(&chars, bracket + 1, ']') {
				if chars.get(close + 1) == Some(&'(') {
					if let Some(paren) = find_char(&chars, close + 2, ')') {
						out.extend(chars[bracket + 1..close].iter());
						index = paren + 1;
						continue;
					}
				}
			}
		}
		out.push(chars[index]);
		index += 1;
	}
	out
}

/// Replace `[text][label]` with the text.
fn strip_reference_links(text: &str) -> String {
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::with_capacity(text.len());
	let mut index = 0usize;
	while index < chars.len() {
		if chars[index] == '[' {
			if let Some(close) = find_char(&chars, index + 1, ']') {
				if chars.get(close + 1) == Some(&'[') {
					if let Some(label) = find_char(&chars, close + 2, ']') {
						out.extend(chars[index + 1..close].iter());
						index = label + 1;
						continue;
					}
				}
			}
		}
		out.push(chars[index]);
		index += 1;
	}
	out
}

/// Replace `<https://…>` with the URL, keeping its text.
fn strip_autolinks(text: &str) -> String {
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::with_capacity(text.len());
	let mut index = 0usize;
	while index < chars.len() {
		if chars[index] == '<' {
			if let Some(close) = find_char(&chars, index + 1, '>') {
				let inner = &chars[index + 1..close];
				if !inner.is_empty() && !inner.iter().any(|c| c.is_whitespace() || *c == '<') {
					out.extend(inner.iter());
					index = close + 1;
					continue;
				}
			}
		}
		out.push(chars[index]);
		index += 1;
	}
	out
}

/// Replace `delim content delim` runs (at least one content char) with the content.
fn strip_delimited(text: &str, delimiter: &str) -> String {
	if !text.contains(delimiter) {
		return text.to_string();
	}
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find(delimiter) {
		out.push_str(&rest[..start]);
		let after = &rest[start + delimiter.len()..];
		match after.find(delimiter) {
			Some(end) if end > 0 => {
				out.push_str(&after[..end]);
				rest = &after[end + delimiter.len()..];
			}
			_ => {
				out.push_str(delimiter);
				rest = after;
			}
		}
	}
	out.push_str(rest);
	out
}

/// Replace `**bold**`, `__bold__`, `*italic*` and `_italic_` runs with their text.
fn strip_emphasis(text: &str) -> String {
	let bold = ["**", "__"];
	let italic = ["*", "_"];
	let text = strip_paired(&text, &bold);
	strip_paired(&text, &italic)
}

fn strip_paired(text: &str, delimiters: &[&str]) -> String {
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	'outer: while !rest.is_empty() {
		for delimiter in delimiters {
			if let Some(stripped) = rest.strip_prefix(*delimiter) {
				if let Some(end) = rest[delimiter.len()..].find(delimiter) {
					if end > 0 {
						out.push_str(&rest[delimiter.len()..delimiter.len() + end]);
						rest = &rest[delimiter.len() + end + delimiter.len()..];
						continue 'outer;
					}
				}
				out.push_str(delimiter);
				rest = &rest[delimiter.len()..];
				continue 'outer;
			}
		}
		let next = rest.chars().next().unwrap_or('\0');
		out.push(next);
		rest = &rest[next.len_utf8()..];
	}
	out
}

fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
	chars.iter().skip(from).position(|c| *c == needle).map(|offset| offset + from)
}

/// Parse one ATX heading line: 1–6 `#`, required whitespace, a title, and an
/// optional closing `#` run.
fn parse_heading(line: &str) -> Option<(usize, String)> {
	let level = line.chars().take_while(|c| *c == '#').count();
	if level == 0 || level > 6 {
		return None;
	}
	let rest = &line[level..];
	if !rest.starts_with(' ') && !rest.starts_with('\t') {
		return None;
	}
	let body = rest.trim_end();
	let stripped = body.trim_end_matches('#').trim_end();
	let title = if stripped.is_empty() {
		// The title matched a single character before the closing run.
		body.chars().next().map(String::from).unwrap_or_default()
	} else {
		stripped.to_string()
	};
	if title.is_empty() {
		return None;
	}
	Some((level, strip_inline_markdown(&title)))
}

/// Parse a code fence run: up to three lead spaces, then ``` or ~~~ (3+).
fn parse_fence(line: &str) -> Option<(char, usize, &str)> {
	let lead = line.chars().take_while(|c| *c == ' ').count();
	if lead > 3 {
		return None;
	}
	let rest = &line[lead..];
	let character = rest.chars().next()?;
	if character != '`' && character != '~' {
		return None;
	}
	let run = rest.chars().take_while(|c| *c == character).count();
	if run < 3 {
		return None;
	}
	Some((character, run, &rest[run..]))
}

/// Split `text` into preamble + heading sections.
///
/// `#` characters inside fenced code blocks are never treated as headings.
/// Concatenating every section's `raw` reproduces the original text exactly.
#[must_use]
pub fn parse_plan_sections(text: &str) -> Vec<PlanSection> {
	let lines: Vec<&str> = text.split('\n').collect();
	// Byte offset of each line start so section `raw` can slice the source.
	let mut offsets = Vec::with_capacity(lines.len());
	let mut cursor = 0usize;
	for line in &lines {
		offsets.push(cursor);
		cursor += line.len() + 1; // +1 for the "\n" join separator
	}

	// Heading line indices with metadata.
	let mut heads: Vec<(usize, usize, String)> = Vec::new();
	let mut fence: Option<(char, usize)> = None;
	for (index, line) in lines.iter().enumerate() {
		match parse_fence(line) {
			Some((character, run, _rest)) if fence.is_none() => {
				fence = Some((character, run));
				continue;
			}
			Some((character, run, rest)) => {
				if let Some((open, open_len)) = fence {
					if character == open && run >= open_len && rest.trim().is_empty() {
						fence = None;
					}
				}
				continue;
			}
			None => {}
		}
		if fence.is_some() {
			continue;
		}
		if let Some((level, title)) = parse_heading(line) {
			heads.push((index, level, title));
		}
	}
	let slice_raw = |start_line: usize, end_line: usize| -> String {
		let start_offset = offsets[start_line];
		let end_offset = if end_line < lines.len() {
			offsets[end_line]
		} else {
			text.len()
		};
		text[start_offset..end_offset].to_string()
	};

	let mut sections = Vec::with_capacity(heads.len() + 1);
	// Preamble: everything before the first heading (only when non-empty).
	let first_head_line = heads.first().map(|head| head.0).unwrap_or(lines.len());
	if first_head_line > 0 {
		let raw = slice_raw(0, first_head_line);
		if !raw.is_empty() {
			sections.push(PlanSection {
				level: 0,
				title: String::new(),
				raw,
			});
		}
	}

	for (position, (line, level, title)) in heads.iter().enumerate() {
		let end_line = heads
			.get(position + 1)
			.map(|head| head.0)
			.unwrap_or(lines.len());
		sections.push(PlanSection {
			level: *level,
			title: title.clone(),
			raw: slice_raw(*line, end_line),
		});
	}

	sections
}

/// Concatenate every section's `raw` back into one document with a single
/// trailing newline. Inverse of [`parse_plan_sections`].
#[must_use]
pub fn join_plan_sections(sections: &[PlanSection]) -> String {
	let mut joined = String::new();
	for section in sections {
		joined.push_str(&section.raw);
	}
	if joined.is_empty() {
		return String::new();
	}
	if !joined.ends_with('\n') {
		joined.push('\n');
	}
	joined
}

/// Indices to remove when deleting `sections[index]`: the heading itself plus
/// every following section nested deeper than it. The preamble (level 0) is
/// never a deletion target and yields an empty span.
#[must_use]
pub fn section_deletion_span(sections: &[PlanSection], index: usize) -> Vec<usize> {
	let Some(target) = sections.get(index) else {
		return Vec::new();
	};
	if target.level == 0 {
		return Vec::new();
	}
	let mut span = vec![index];
	for (offset, section) in sections.iter().enumerate().skip(index + 1) {
		if section.level > target.level {
			span.push(offset);
		} else {
			break;
		}
	}
	span
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strips_images_links_code_and_emphasis() {
		assert_eq!(strip_inline_markdown("**Goal** & [docs](x)"), "Goal & docs");
		assert_eq!(strip_inline_markdown("![shot](a.png) here"), "shot here");
		assert_eq!(strip_inline_markdown("see [docs][ref] now"), "see docs now");
		assert_eq!(strip_inline_markdown("<https://x.dev/a>"), "https://x.dev/a");
		assert_eq!(strip_inline_markdown("run `cargo --x`  twice"), "run cargo --x twice");
		assert_eq!(strip_inline_markdown("__bold__ and _soft_ and ~~gone~~"), "bold and soft and gone");
	}

	#[test]
	fn keeps_unbalanced_delimiters_verbatim() {
		assert_eq!(strip_inline_markdown("2 * 3 * 4"), "2 * 3 * 4");
		assert_eq!(strip_inline_markdown("unclosed `code"), "unclosed `code");
	}

	#[test]
	fn parses_preamble_and_heading_sections() {
		let text = "intro line\n\n# One\na\n## Two\nb\n# Three\nc\n";
		let sections = parse_plan_sections(text);
		assert_eq!(sections.len(), 4);
		assert_eq!(sections[0].level, 0);
		assert_eq!(sections[0].raw, "intro line\n\n");
		assert_eq!(sections[1].title, "One");
		assert_eq!(sections[1].level, 1);
		assert_eq!(sections[1].raw, "# One\na\n## Two\n");
		assert_eq!(sections[2].level, 2);
		assert_eq!(sections[2].raw, "## Two\nb\n");
		assert_eq!(sections[3].title, "Three");
	}

	#[test]
	fn no_preamble_when_the_document_starts_with_a_heading() {
		let sections = parse_plan_sections("# Title\nbody\n");
		assert_eq!(sections.len(), 1);
		assert_eq!(sections[0].level, 1);
	}

	#[test]
	fn headings_inside_fenced_code_are_body() {
		let text = "# Real\n```sh\n# not a heading\n```\n## After\n";
		let sections = parse_plan_sections(text);
		assert_eq!(sections.len(), 2);
		assert_eq!(sections[0].raw, "# Real\n```sh\n# not a heading\n```\n");
		assert_eq!(sections[1].title, "After");
	}

	#[test]
	fn closing_fence_must_match_char_and_length() {
		let text = "~~~\n````\n# inner\n~~~\n# heading\n";
		let sections = parse_plan_sections(text);
		assert_eq!(sections.len(), 2);
		assert_eq!(sections[1].title, "heading");
	}

	#[test]
	fn strips_a_trailing_closing_hash_run() {
		assert_eq!(parse_heading("## Title ##"), Some((2, "Title".to_string())));
		assert_eq!(parse_heading("#nospace"), None);
		assert_eq!(parse_heading("####### seven"), None);
		assert_eq!(parse_heading("   indented"), None);
	}

	#[test]
	fn round_trips_the_document() {
		let text = "# One\nbody\n\n## Two\nmore\n";
		assert_eq!(join_plan_sections(&parse_plan_sections(text)), text);
		let unterminated = "# One\nbody";
		assert_eq!(join_plan_sections(&parse_plan_sections(unterminated)), "# One\nbody\n");
	}

	#[test]
	fn deletion_span_covers_nested_subsections() {
		let text = "# A\na\n## A1\nx\n## A2\ny\n# B\nb\n";
		let sections = parse_plan_sections(text);
		assert_eq!(section_deletion_span(&sections, 0), vec![0, 1, 2]);
		assert_eq!(section_deletion_span(&sections, 1), vec![1]);
		assert_eq!(section_deletion_span(&sections, 3), vec![3]);
		assert!(section_deletion_span(&sections, 99).is_empty());
	}

	#[test]
	fn preamble_is_not_a_deletion_target() {
		let sections = parse_plan_sections("intro\n# A\na\n");
		assert_eq!(sections[0].level, 0);
		assert!(section_deletion_span(&sections, 0).is_empty());
	}
}
