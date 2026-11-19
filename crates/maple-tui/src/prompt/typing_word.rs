//! Reference: `l/src/prompt/typing-word.ts`.
//!
//! The word being typed in the composer. Misspelling marks hold off on it until
//! the caret leaves it (a space or punctuation, a caret move, losing focus), the
//! way Slack and Chromium mark a typo only once its word is done; marks on other
//! words stay while typing.
//!
//! Deviation: the TypeScript class keeps private `#` fields; Rust uses private
//! fields with the same getter (`word`).

/// A word of one logical line, as UTF-16 columns `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypingWord {
	pub line: usize,
	pub start: usize,
	pub end: usize,
}

/// Letters, marks and digits, with apostrophes joining contractions.
fn is_word_char(ch: char) -> bool {
	ch.is_alphanumeric()
		|| matches!(ch, '\'' | '\u{2019}')
		|| is_mark(ch)
}

/// Unicode mark category; `char::is_alphanumeric` covers letters and digits but
/// not combining marks (`\p{M}` in the reference pattern).
fn is_mark(ch: char) -> bool {
	// Combining diacritical marks and the general mark blocks the reference
	// pattern admits through `\p{M}`.
	matches!(ch as u32,
		0x0300..=0x036F
		| 0x0483..=0x0489
		| 0x0591..=0x05BD
		| 0x0610..=0x061A
		| 0x064B..=0x065F
		| 0x0900..=0x0903
		| 0x093A..=0x094F
		| 0x0E31..=0x0E3A
		| 0x1AB0..=0x1AFF
		| 0x1DC0..=0x1DFF
		| 0x20D0..=0x20F0
		| 0xFE20..=0xFE2F)
}

/// The run of word characters around column `col` of `line` (`None` between
/// non-word characters).
fn word_around(line: &str, col: usize) -> Option<(usize, usize)> {
	let chars: Vec<char> = line.chars().collect();
	if col > chars.len() {
		return None;
	}
	let mut start = col;
	while start > 0 && is_word_char(chars[start - 1]) {
		start -= 1;
	}
	let mut end = col;
	while end < chars.len() && is_word_char(chars[end]) {
		end += 1;
	}
	(start < end).then_some((start, end))
}

/// Follows an editor's text revision and caret to know which word is being
/// typed.
#[derive(Clone, Debug, Default)]
pub struct TypingWordTracker {
	revision: Option<u64>,
	word: Option<TypingWord>,
}

impl TypingWordTracker {
	/// The word being typed, if any.
	pub fn word(&self) -> Option<TypingWord> {
		self.word
	}

	/// Catch up with the editor: an edit that leaves the caret in (or right
	/// after) a word starts typing it; a caret move out of that word, or losing
	/// focus, ends it. The first call only records the revision, so a restored
	/// draft is not held. Returns whether the held word changed.
	pub fn update(
		&mut self,
		revision: u64,
		lines: &[String],
		line: usize,
		col: usize,
		focused: bool,
	) -> bool {
		let before = self.word;
		let edited = self.revision.is_some_and(|previous| previous != revision);
		self.revision = Some(revision);
		if !focused {
			self.word = None;
		} else if edited {
			let text = lines.get(line).map(String::as_str).unwrap_or("");
			self.word = word_around(text, col).map(|(start, end)| TypingWord { line, start, end });
		} else if let Some(held) = before {
			if held.line != line || col < held.start || col > held.end {
				self.word = None;
			}
		}
		before.map(|w| (w.line, w.start, w.end)) != self.word.map(|w| (w.line, w.start, w.end))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn lines(value: &str) -> Vec<String> {
		value.split('\n').map(str::to_string).collect()
	}

	#[test]
	fn tracks_word_being_typed() {
		let buffer = lines("hello world");
		let mut tracker = TypingWordTracker::default();
		// First call only records the revision.
		assert!(!tracker.update(1, &buffer, 0, 5, true));
		assert_eq!(tracker.word(), None);

		assert!(tracker.update(2, &buffer, 0, 5, true));
		assert_eq!(
			tracker.word(),
			Some(TypingWord {
				line: 0,
				start: 0,
				end: 5
			})
		);

		// Caret still inside the word: unchanged.
		assert!(!tracker.update(2, &buffer, 0, 2, true));
		assert!(tracker.word().is_some());

		// Caret left the word: cleared.
		assert!(tracker.update(2, &buffer, 0, 8, true));
		assert_eq!(tracker.word(), None);

		// Blur clears.
		assert!(tracker.update(3, &buffer, 0, 8, false));
		assert_eq!(tracker.word(), None);
	}

	#[test]
	fn contraction_and_non_word_cases() {
		let cases: [(&str, usize, Option<(usize, usize)>); 5] = [
			("don't", 4, Some((0, 5))),
			("a b", 1, Some((0, 1))),
			("a b", 2, None),
			("  ", 1, None),
			("", 0, None),
		];
		for (line, col, expected) in cases {
			assert_eq!(word_around(line, col), expected, "line {line:?} col {col}");
		}
	}
}
