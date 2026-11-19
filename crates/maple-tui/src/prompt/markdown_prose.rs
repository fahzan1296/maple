//! Reference: `l/src/prompt/markdown-prose.ts`.
//!
//! Markdown structure awareness for the magic-keyword affordances (see
//! `magic_keywords.rs`) and the spelling provider. Keyword detection and
//! editor/transcript highlighting must fire only on prose the user is actually
//! addressing to the model — never on a word inside a fenced code block, an
//! inline code span, or an HTML/XML section. [`mask_non_prose`] returns a
//! length-preserving copy of the text where every such region is blanked to
//! spaces, so a word-bounded match run against the mask never lands inside
//! code/markup while its indices still address the original text for painting.
//!
//! Deviation: offsets are character (Unicode scalar) indices, not UTF-16 code
//! units, because every Rust caller indexes `&str` by character. The mask keeps
//! a 1:1 mapping between character positions in the input and the output.

use std::sync::LazyLock;

use regex::Regex;

use crate::render::render_utils::FENCE_RE;

/// Tag/element name: HTML5/XML start char + name chars. Anchored at the probe
/// offset, so [`try_tag_name`] applies it to a suffix slice.
static TAG_NAME: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9-]*").expect("tag name regex"));

/// Match a tag name starting exactly at `chars[from..]`.
fn try_tag_name(chars: &[char], from: usize) -> Option<(String, usize)> {
	let rest: String = chars[from..].iter().collect();
	let matched = TAG_NAME.find(&rest)?;
	let name = matched.as_str().to_string();
	let len = name.chars().count();
	Some((name, from + len))
}

/// Index just past the run of backticks beginning at `i`.
fn backtick_run_end(chars: &[char], i: usize, n: usize) -> usize {
	let mut j = i;
	while j < n && chars[j] == '`' {
		j += 1;
	}
	j
}

/// Find the closing backtick run that matches an opening run of `run_len`
/// backticks, scanning from `from`. Returns the index just past the closing run,
/// or `None` when no run of the exact length exists (an unmatched run is literal
/// text, not a code span). Already-masked positions (fenced code) are skipped.
fn find_backtick_close(
	chars: &[char],
	from: usize,
	n: usize,
	run_len: usize,
	masked: &[bool],
) -> Option<usize> {
	let mut k = from;
	while k < n {
		if masked[k] {
			k += 1;
			continue;
		}
		if chars[k] == '`' {
			let e = backtick_run_end(chars, k, n);
			if e - k == run_len {
				return Some(e);
			}
			k = e;
			continue;
		}
		k += 1;
	}
	None
}

/// Index of the `>` that closes a tag whose attributes begin at `j`, honoring
/// quoted attribute values. `None` when the tag is malformed (a new `<` appears
/// first, or there is no `>`), so callers can treat the `<` as literal.
fn find_tag_end(chars: &[char], j: usize, n: usize) -> Option<usize> {
	let mut quote: Option<char> = None;
	for (k, ch) in chars.iter().enumerate().take(n).skip(j) {
		if let Some(open) = quote {
			if *ch == open {
				quote = None;
			}
			continue;
		}
		match *ch {
			'"' | '\'' => quote = Some(*ch),
			'>' => return Some(k),
			'<' => return None,
			_ => {}
		}
	}
	None
}

/// Locate the `</name>` that balances an opening `<name>` at `start`, counting
/// nested same-name tags. Returns the index just past the matching close tag's
/// `>`, or `None` when the section is never closed (so callers mask only the
/// opening tag rather than swallowing the rest of the document).
fn find_matching_close(
	chars: &[char],
	start: usize,
	n: usize,
	name: &str,
	masked: &[bool],
) -> Option<usize> {
	let lname = name.to_lowercase();
	let mut depth = 1usize;
	let mut k = start;
	while k < n {
		if masked[k] || chars[k] != '<' {
			k += 1;
			continue;
		}
		let mut m = k + 1;
		let mut is_close = false;
		if m < n && chars[m] == '/' {
			is_close = true;
			m += 1;
		}
		let Some((nm, name_end)) = try_tag_name(chars, m) else {
			k += 1;
			continue;
		};
		let Some(gt) = find_tag_end(chars, name_end, n) else {
			k += 1;
			continue;
		};
		if nm.to_lowercase() == lname {
			if is_close {
				depth -= 1;
				if depth == 0 {
					return Some(gt + 1);
				}
			} else if chars[gt - 1] != '/' {
				depth += 1;
			}
		}
		k = gt + 1;
	}
	None
}

/// Mask the HTML/XML construct beginning at `<` (index `i`): an HTML comment, a
/// self-closing/closing tag (the tag alone), or an opening tag together with the
/// content through its matching close tag. Returns the index just past the masked
/// region, or `i` when the `<` does not begin a tag (e.g. a stray less-than).
fn mask_tag_at(chars: &[char], i: usize, n: usize, masked: &mut [bool]) -> usize {
	if chars[i..].starts_with(&['<', '!', '-', '-']) {
		let mut stop = n;
		let mut p = i + 4;
		while p + 2 < n {
			if chars[p] == '-' && chars[p + 1] == '-' && chars[p + 2] == '>' {
				stop = p + 3;
				break;
			}
			p += 1;
		}
		for slot in masked.iter_mut().take(stop.min(n)).skip(i) {
			*slot = true;
		}
		return stop.min(n);
	}
	let mut j = i + 1;
	let mut closing = false;
	if j < n && chars[j] == '/' {
		closing = true;
		j += 1;
	}
	let Some((name, name_end)) = try_tag_name(chars, j) else {
		return i;
	};
	let Some(gt) = find_tag_end(chars, name_end, n) else {
		return i;
	};
	let tag_end = gt + 1;
	let self_closing = chars[gt - 1] == '/';
	for slot in masked.iter_mut().take(tag_end).skip(i) {
		*slot = true;
	}
	if closing || self_closing {
		return tag_end;
	}
	let Some(close) = find_matching_close(chars, tag_end, n, name.as_str(), masked) else {
		return tag_end;
	};
	for slot in masked.iter_mut().take(close).skip(tag_end) {
		*slot = true;
	}
	close
}

/// Return a copy of `text` with identical length (indices map 1:1) where every
/// character inside a non-prose region is replaced by a space. Non-prose regions
/// are markdown fenced code blocks, inline code spans, and HTML/XML tags together
/// with the content they enclose. Newlines are preserved. Text with no construct
/// that could open such a region is returned unchanged.
pub fn mask_non_prose(text: &str) -> String {
	if !text.contains('`') && !text.contains('<') && !text.contains("~~~") {
		return text.to_string();
	}
	let chars: Vec<char> = text.chars().collect();
	let n = chars.len();
	let mut masked = vec![false; n];

	// Phase 1: fenced code blocks, line by line.
	let mut fence_char: Option<char> = None;
	let mut fence_len = 0usize;
	// Line walk over character indices (newline positions).
	let mut newline_positions: Vec<usize> = Vec::new();
	for (index, ch) in chars.iter().enumerate() {
		if *ch == '\n' {
			newline_positions.push(index);
		}
	}
	newline_positions.push(n);
	let mut cursor = 0usize;
	for nl in newline_positions {
		let line: String = chars[cursor..nl].iter().collect();
		let caps = FENCE_RE.captures(&line);
		if let Some(open_char) = fence_char {
			for slot in masked.iter_mut().take(nl).skip(cursor) {
				*slot = true;
			}
			// A closing fence is the same char, at least as long, with nothing
			// else on the line.
			if let Some(caps) = &caps {
				let indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
				let marker = caps.get(2).map(|m| m.as_str()).unwrap_or("");
				let rest = &line[indent.len() + marker.len()..];
				if marker.starts_with(open_char) && marker.len() >= fence_len && rest.trim().is_empty() {
					fence_char = None;
					fence_len = 0;
				}
			}
		} else if let Some(caps) = &caps {
			let indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
			let marker = caps.get(2).map(|m| m.as_str()).unwrap_or("");
			if let Some(ch) = marker.chars().next() {
				let rest = &line[indent.len() + marker.len()..];
				// A backtick fence's info string may not contain a backtick.
				if !(ch == '`' && rest.contains('`')) {
					fence_char = Some(ch);
					fence_len = marker.len();
					for slot in masked.iter_mut().take(nl).skip(cursor) {
						*slot = true;
					}
				}
			}
		}
		if nl == n {
			break;
		}
		cursor = nl + 1;
	}

	// Phase 2: inline code spans and HTML/XML, over not-yet-masked regions.
	let mut i = 0usize;
	while i < n {
		if masked[i] {
			i += 1;
			continue;
		}
		match chars[i] {
			'`' => {
				let run_end = backtick_run_end(&chars, i, n);
				match find_backtick_close(&chars, run_end, n, run_end - i, &masked) {
					Some(close) => {
						for slot in masked.iter_mut().take(close).skip(i) {
							*slot = true;
						}
						i = close;
					}
					None => i = run_end,
				}
			}
			'<' => {
				let end = mask_tag_at(&chars, i, n, &mut masked);
				i = if end > i { end } else { i + 1 };
			}
			_ => i += 1,
		}
	}

	chars
		.iter()
		.enumerate()
		.map(|(p, ch)| {
			if masked[p] && *ch != '\n' {
				' '
			} else {
				*ch
			}
		})
		.collect()
}

/// Whether `text` contains a standalone keyword match (per the non-global,
/// word-bounded `word` regex) that lives in prose rather than inside a code
/// block, inline code span, or HTML/XML section. `word` MUST be non-global so
/// `is_match` stays stateless.
pub fn keyword_in_prose(text: &str, word: &Regex) -> bool {
	if !word.is_match(text) {
		return false;
	}
	word.is_match(&mask_non_prose(text))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn prose_at(masked: &str, needle: &str) -> bool {
		let chars: Vec<char> = masked.chars().collect();
		chars
			.windows(needle.chars().count())
			.any(|w| w.iter().collect::<String>() == needle)
	}

	#[test]
	fn keeps_plain_text_untouched() {
		let cases = ["hello world", "a < b", "", "plain\ntext"];
		for text in cases {
			assert_eq!(mask_non_prose(text), text, "text {text:?}");
		}
	}

	#[test]
	fn masks_fenced_code_blocks() {
		let text = "before\n```rust\nlet x = 1;\n```\nafter";
		let masked = mask_non_prose(text);
		assert_eq!(masked.chars().count(), text.chars().count());
		assert!(prose_at(&masked, "before"));
		assert!(prose_at(&masked, "after"));
		assert!(!prose_at(&masked, "let x"));
		// Newlines survive.
		assert_eq!(masked.matches('\n').count(), text.matches('\n').count());
	}

	#[test]
	fn masks_inline_code_spans() {
		let cases: [(&str, bool); 3] = [
			("use `code` here", false),
			("unmatched `code here", true),
			("``a ` b``", false),
		];
		for (text, expect_code_visible) in cases {
			let masked = mask_non_prose(text);
			assert_eq!(
				prose_at(&masked, "code") || prose_at(&masked, "a ` b"),
				expect_code_visible,
				"text {text:?}"
			);
		}
	}

	#[test]
	fn masks_markup_and_its_content() {
		let text = "<div class=\"x\">inner</div> tail";
		let masked = mask_non_prose(text);
		assert!(!prose_at(&masked, "inner"));
		assert!(prose_at(&masked, "tail"));

		let comment = mask_non_prose("a <!-- hidden --> b");
		assert!(!prose_at(&comment, "hidden"));
		assert!(prose_at(&comment, "b"));

		// Self-closing tag masks only the tag.
		let self_closing = mask_non_prose("a <br/> b");
		assert!(prose_at(&self_closing, "a"));
		assert!(prose_at(&self_closing, "b"));

		// Unclosed section masks only the opening tag.
		let unclosed = mask_non_prose("<div>text keeps");
		assert!(prose_at(&unclosed, "text keeps"));
	}

	#[test]
	fn keyword_must_be_in_prose() {
		let word = Regex::new(r"(?i)\bthink\b").expect("regex");
		assert!(keyword_in_prose("please think now", &word));
		assert!(!keyword_in_prose("```\nthink\n```", &word));
		assert!(!keyword_in_prose("`think`", &word));
		assert!(!keyword_in_prose("nothing here", &word));
	}
}
