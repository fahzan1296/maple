//! Reference: `l/src/prompt/prose-gate.ts`.
//!
//! Client-side gates shared by the prose assistants (macOS typo/autocorrect and
//! word completion): only plain prose words in modest buffers are ever sent to
//! a spelling or prediction backend.
//!
//! Deviation: offsets are character indices (see [`crate::prompt::markdown_prose`]),
//! and the buffer caps count characters rather than UTF-16 code units.

use crate::prompt::markdown_prose::mask_non_prose;
use crate::prompt::typing_word::TypingWord;

const CODEISH_CHARACTERS: [char; 10] = ['\\', '/', '@', '_', '=', ':', '{', '}', '[', ']'];
/// Buffers above this many characters get no prose assistance at all.
const MAX_PROSE_BUFFER_LENGTH: usize = 20_000;
/// Lines above this many characters get no prose assistance at all.
const MAX_PROSE_LINE_LENGTH: usize = 1_000;

/// Logical source location for one rendered editor segment.
#[derive(Clone, Debug)]
pub struct SpellingDecorationContext {
	pub editor_text: String,
	pub lines: Vec<String>,
	pub line: usize,
	pub start_col: usize,
	/// The word being typed: typo marks touching it wait until the caret leaves it.
	pub typing: Option<TypingWord>,
}

/// The whitespace-delimited token containing `text[start..end]`.
fn token_at(chars: &[char], start: usize, end: usize) -> String {
	let mut token_start = start;
	while token_start > 0 && !chars[token_start - 1].is_whitespace() {
		token_start -= 1;
	}
	let mut token_end = end;
	while token_end < chars.len() && !chars[token_end].is_whitespace() {
		token_end += 1;
	}
	chars[token_start..token_end].iter().collect()
}

/// Whether `text[start..end]` is a prose word: unmasked by
/// [`mask_non_prose`], not inside a path/identifier-like token, and not on a
/// slash-command or queue-shorthand line.
pub fn is_prose_word(text: &str, masked: &str, start: usize, end: usize) -> bool {
	let chars: Vec<char> = text.chars().collect();
	let masked_chars: Vec<char> = masked.chars().collect();
	if end <= start || end > chars.len() || end > masked_chars.len() {
		return false;
	}
	if masked_chars[start..end].iter().collect::<String>().trim().is_empty() {
		return false;
	}
	let token = token_at(&chars, start, end);
	if token.chars().any(|ch| CODEISH_CHARACTERS.contains(&ch)) {
		return false;
	}
	if token.chars().any(|ch| ch.is_ascii_digit()) {
		return false;
	}
	// Camel case: a lowercase letter immediately followed by an uppercase one.
	let token_chars: Vec<char> = token.chars().collect();
	if token_chars
		.windows(2)
		.any(|pair| pair[0].is_lowercase() && pair[1].is_uppercase())
	{
		return false;
	}
	let trimmed = text.trim_start();
	!trimmed.starts_with('/') && !text.starts_with("->") && !text.starts_with("=>")
}

/// Whole-buffer context for `line`, starting at column 0.
pub fn line_context(lines: &[String], line: usize) -> SpellingDecorationContext {
	SpellingDecorationContext {
		editor_text: lines.join("\n"),
		lines: lines.to_vec(),
		line,
		start_col: 0,
		typing: None,
	}
}

/// Memoized whole-buffer prose mask: answers whether a character range is prose
/// in the context of the full editor text (fenced code spans lines).
#[derive(Clone, Debug, Default)]
pub struct ProseSource {
	text: String,
	mask: String,
	line_offsets: Vec<usize>,
}

impl ProseSource {
	/// True when `[start_col, end_col)` of `context.line` is prose and the buffer
	/// is within the caps.
	pub fn is_prose(&mut self, context: &SpellingDecorationContext, start_col: usize, end_col: usize) -> bool {
		if context.editor_text.chars().count() > MAX_PROSE_BUFFER_LENGTH || end_col <= start_col {
			return false;
		}
		let Some(line) = context.lines.get(context.line) else {
			return false;
		};
		if line.chars().count() > MAX_PROSE_LINE_LENGTH || end_col > line.chars().count() {
			return false;
		}
		self.prepare(context);
		let Some(&line_offset) = self.line_offsets.get(context.line) else {
			return false;
		};
		let mask_chars: Vec<char> = self.mask.chars().collect();
		let from = line_offset + start_col;
		let to = line_offset + end_col;
		if to > mask_chars.len() {
			return false;
		}
		!mask_chars[from..to].iter().collect::<String>().trim().is_empty()
	}

	fn prepare(&mut self, context: &SpellingDecorationContext) {
		if self.text == context.editor_text {
			return;
		}
		self.text = context.editor_text.clone();
		self.mask = mask_non_prose(&context.editor_text);
		self.line_offsets = Vec::with_capacity(context.lines.len());
		let mut offset = 0usize;
		for line in &context.lines {
			self.line_offsets.push(offset);
			offset += line.chars().count() + 1;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn plain_prose_word_passes() {
		let text = "please review this";
		let masked = mask_non_prose(text);
		assert!(is_prose_word(text, &masked, 7, 13));
	}

	#[test]
	fn rejects_codeish_identifier_and_command_lines() {
		let cases: [(&str, usize, usize); 5] = [
			("src/main.rs", 4, 8),
			("some_thing", 0, 10),
			("camelCase", 0, 9),
			("var1able", 0, 8),
			("/help me", 6, 8),
		];
		for (text, start, end) in cases {
			let masked = mask_non_prose(text);
			assert!(!is_prose_word(text, &masked, start, end), "text {text:?}");
		}
	}

	#[test]
	fn rejects_arrow_prefixed_lines() {
		for prefix in ["-> note", "=> note"] {
			let masked = mask_non_prose(prefix);
			assert!(!is_prose_word(prefix, &masked, 3, 7), "prefix {prefix:?}");
		}
	}

	#[test]
	fn prose_source_memoizes_mask() {
		let lines = vec!["prose words".to_string(), "```".to_string(), "code".to_string()];
		let mut source = ProseSource::default();
		let context = line_context(&lines, 0);
		assert!(source.is_prose(&context, 0, 5));
		assert!(!source.is_prose(&context, 0, 0));
		let code_context = line_context(&lines, 2);
		assert!(!source.is_prose(&code_context, 0, 4));

		// Buffer caps.
		let long = vec!["x".repeat(MAX_PROSE_LINE_LENGTH + 1)];
		let long_context = line_context(&long, 0);
		assert!(!source.is_prose(&long_context, 0, 1));
	}
}
