//! Reference: `l/src/prompt/macos-spelling.ts`.
//!
//! Bridges Apple's spelling service into the editor's typo and autocorrection
//! paths: red undercurls on misspelled prose, confident corrections after a
//! completed word, and replacement guesses for the word at the cursor.
//!
//! Deviations:
//! - The native backend is a synchronous [`SpellingBackend`] trait; the
//!   reference awaits four macOS host functions with a 2 s timeout. Rust callers
//!   own their timeouts, so no timeout wrapper or in-flight bookkeeping exists —
//!   a cache miss checks immediately instead of projecting ranges from a similar
//!   cached line while the check is pending.
//! - Offsets are character indices (see [`crate::prompt::prose_gate`]).
//! - `onUpdate` is a callback field; the reference's `TERMINAL.styledUnderlines`
//!   probe becomes a constructor argument.

use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;

use crate::prompt::magic_keywords::is_magic_keyword;
use crate::prompt::markdown_prose::mask_non_prose;
use crate::prompt::prose_gate::{
	is_prose_word, line_context, ProseSource, SpellingDecorationContext,
};

/// Styled underline: red curly undercurl via colon-subparameter SGR.
const STYLED_TYPO_MARKS: (&str, &str) = ("\x1b[4:3m\x1b[58:2::255:95:95m", "\x1b[4:0m\x1b[59m");
/// Flat underline: legacy CSI 4 m / CSI 24 m only, no SGR 58/59, used where the
/// terminal lacks styled underlines.
const FLAT_TYPO_MARKS: (&str, &str) = ("\x1b[4m", "\x1b[24m");

const CACHE_LIMIT: usize = 256;

/// Independently switchable macOS prose-assistance features.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacOSSpellingFeatures {
	pub typo_detection: bool,
	pub autocorrect: bool,
}

/// One misspelled range: `length` characters from `start`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpellingRange {
	pub start: usize,
	pub length: usize,
}

/// An inline replacement the editor applies at the caret.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorInlineReplacement {
	pub replace_len: usize,
	pub insert: String,
}

/// Replacement guesses for one word.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorWordReplacements {
	pub line: usize,
	pub start_col: usize,
	pub end_col: usize,
	pub items: Vec<String>,
}

/// Native spelling operations used by [`MacOSSpellingProvider`].
pub trait SpellingBackend: Send + Sync {
	fn is_available(&self) -> bool;
	fn check_spelling(&self, text: &str) -> Vec<SpellingRange>;
	fn autocorrect_word(&self, text: &str, start: usize, length: usize) -> Option<String>;
	fn spelling_guesses(&self, text: &str, start: usize, length: usize) -> Vec<String>;
}

/// `([\p{L}\p{M}']+)([\s.,;:!?"\])}])$` — a completed word plus its boundary.
static COMPLETED_WORD: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
	Regex::new(r#"([\p{L}\p{M}']+)([\s.,;:!?"\])}])$"#).expect("completed word regex")
});

fn is_word_boundary(ch: char) -> bool {
	ch.is_whitespace() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | '"' | ')' | ']' | '}')
}

/// Bridges the platform spelling service into the editor's typo and
/// autocorrection paths.
pub struct MacOSSpellingProvider {
	backend: Arc<dyn SpellingBackend>,
	features: MacOSSpellingFeatures,
	available: bool,
	availability_checked: bool,
	typo_cache: HashMap<String, Vec<SpellingRange>>,
	prose: ProseSource,
	marks: (&'static str, &'static str),
	/// Invoked when a spelling result can change rendered output.
	pub on_update: Option<Box<dyn Fn()>>,
}

impl MacOSSpellingProvider {
	/// `styled_underlines` mirrors the terminal's styled-underline capability.
	pub fn new(backend: Arc<dyn SpellingBackend>, styled_underlines: bool) -> Self {
		Self {
			backend,
			features: MacOSSpellingFeatures::default(),
			available: false,
			availability_checked: false,
			typo_cache: HashMap::new(),
			prose: ProseSource::default(),
			marks: if styled_underlines {
				STYLED_TYPO_MARKS
			} else {
				FLAT_TYPO_MARKS
			},
			on_update: None,
		}
	}

	/// Whether the backend reported availability.
	pub fn is_available(&self) -> bool {
		self.available
	}

	/// Apply both independent feature gates and invalidate rendered typo ranges.
	pub fn set_features(&mut self, features: MacOSSpellingFeatures) {
		if self.features == features {
			return;
		}
		self.features = features;
		if !self.availability_checked && (features.typo_detection || features.autocorrect) {
			self.availability_checked = true;
			self.available = self.backend.is_available();
		}
		self.clear_caches();
	}

	/// Add red undercurls to misspellings while preserving visible text width.
	pub fn decorate_typos(
		&mut self,
		text: &str,
		context: &SpellingDecorationContext,
		decorate: &dyn Fn(&str) -> String,
	) -> String {
		let ranges = self.typo_ranges(text, context);
		if ranges.is_empty() {
			return decorate(text);
		}
		let chars: Vec<char> = text.chars().collect();
		let mut rendered = String::new();
		let mut cursor = 0usize;
		for range in ranges {
			let end = range.start + range.length;
			rendered.push_str(&decorate(&chars[cursor..range.start].iter().collect::<String>()));
			rendered.push_str(self.marks.0);
			rendered.push_str(&decorate(&chars[range.start..end].iter().collect::<String>()));
			rendered.push_str(self.marks.1);
			cursor = end;
		}
		rendered.push_str(&decorate(&chars[cursor..].iter().collect::<String>()));
		rendered
	}

	/// Misspelled prose ranges in `text` (offsets into it), in order and
	/// non-overlapping, except ones touching `context.typing` (the word still
	/// being typed). Checks the full logical line so rendering cuts cannot change
	/// spelling or prose boundaries.
	pub fn typo_ranges(
		&mut self,
		text: &str,
		context: &SpellingDecorationContext,
	) -> Vec<SpellingRange> {
		if !self.available || !self.features.typo_detection || text.is_empty() {
			return Vec::new();
		}
		let Some(line) = context.lines.get(context.line) else {
			return Vec::new();
		};
		let line_chars: Vec<char> = line.chars().collect();
		let segment_end = context.start_col + text.chars().count();
		if segment_end > line_chars.len() {
			return Vec::new();
		}
		let segment: String = line_chars[context.start_col..segment_end].iter().collect();
		if segment != text {
			return Vec::new();
		}
		if !self.prose.is_prose(context, context.start_col, segment_end) {
			return Vec::new();
		}
		let ranges = self.load_typo_ranges(line);
		if ranges.is_empty() {
			return Vec::new();
		}

		let mut kept: Vec<SpellingRange> = Vec::new();
		let typing = context.typing.filter(|typing| typing.line == context.line);
		let mut cursor = 0usize;
		for range in ranges {
			let end = range.start + range.length;
			if let Some(typing) = typing {
				if range.start <= typing.end && typing.start <= end {
					continue;
				}
			}
			// Overlapping or out-of-bounds ranges (stale backend data) must never
			// re-emit already-rendered text: that doubles it on screen and desyncs
			// the rendered width from the measured width (cursor drift).
			if range.start < cursor || end > line_chars.len() {
				continue;
			}
			if !self.prose.is_prose(context, range.start, end) {
				continue;
			}
			let start = range.start.max(context.start_col);
			let clipped_end = end.min(segment_end);
			if start < clipped_end {
				kept.push(SpellingRange {
					start: start - context.start_col,
					length: clipped_end - start,
				});
			}
			cursor = end;
		}
		kept
	}

	/// Return the confident correction after a completed prose word.
	pub fn try_autocorrect(
		&mut self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
	) -> Option<EditorInlineReplacement> {
		if !self.available || !self.features.autocorrect {
			return None;
		}
		let line: String = lines
			.get(cursor_line)
			.map(|line| line.chars().take(cursor_col).collect())
			.unwrap_or_default();
		let caps = COMPLETED_WORD.captures(&line)?;
		let word = caps.get(1).map(|m| m.as_str()).unwrap_or("");
		// Magic keywords are deliberate non-dictionary words; never "fix" them.
		if is_magic_keyword(word) {
			return None;
		}
		let boundary = caps.get(2).map(|m| m.as_str()).unwrap_or("");
		let whole = caps.get(0).expect("whole match");
		let start = line[..whole.start()].chars().count();
		let word_len = word.chars().count();
		let masked = mask_non_prose(&line);
		if !is_prose_word(&line, &masked, start, start + word_len) {
			return None;
		}
		let context = line_context(lines, cursor_line);
		if !self.prose.is_prose(&context, start, start + word_len) {
			return None;
		}
		let correction = self.backend.autocorrect_word(&line, start, word_len)?;
		if correction.is_empty() || correction == word {
			return None;
		}
		Some(EditorInlineReplacement {
			replace_len: word_len + boundary.chars().count(),
			insert: format!("{correction}{boundary}"),
		})
	}

	/// Return replacement guesses for the misspelled word at the cursor.
	pub fn get_word_replacements(
		&mut self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
	) -> Option<EditorWordReplacements> {
		if !self.available || !self.features.typo_detection {
			return None;
		}
		let line = lines.get(cursor_line).cloned().unwrap_or_default();
		let line_chars: Vec<char> = line.chars().collect();
		let context = line_context(lines, cursor_line);
		if !self.prose.is_prose(&context, 0, line_chars.len()) {
			return None;
		}
		let ranges = self.load_typo_ranges(&line);
		let range = *ranges.iter().find(|candidate| {
			let end = candidate.start + candidate.length;
			let at_word = cursor_col >= candidate.start
				&& (cursor_col <= end
					|| (cursor_col == end + 1
						&& line_chars.get(end).copied().is_some_and(is_word_boundary)));
			at_word && self.prose.is_prose(&context, candidate.start, end)
		})?;
		if !self.available || !self.features.typo_detection {
			return None;
		}
		let mut seen: Vec<String> = Vec::new();
		for guess in self
			.backend
			.spelling_guesses(&line, range.start, range.length)
		{
			if guess.is_empty() || seen.contains(&guess) {
				continue;
			}
			seen.push(guess);
			if seen.len() == 10 {
				break;
			}
		}
		if seen.is_empty() {
			return None;
		}
		Some(EditorWordReplacements {
			line: cursor_line,
			start_col: range.start,
			end_col: range.start + range.length,
			items: seen,
		})
	}

	/// Cached spelling check for `text`, filtering to prose words that are not
	/// magic keywords.
	fn load_typo_ranges(&mut self, text: &str) -> Vec<SpellingRange> {
		if let Some(cached) = self.typo_cache.get(text) {
			return cached.clone();
		}
		if !self.available {
			return Vec::new();
		}
		let checked = self.backend.check_spelling(text);
		let masked = mask_non_prose(text);
		let chars: Vec<char> = text.chars().collect();
		let mut ranges: Vec<SpellingRange> = checked
			.into_iter()
			.filter(|range| {
				let end = range.start + range.length;
				if end > chars.len() {
					return false;
				}
				let word: String = chars[range.start..end].iter().collect();
				is_prose_word(text, &masked, range.start, end) && !is_magic_keyword(&word)
			})
			.collect();
		ranges.sort_by_key(|range| range.start);
		if self.typo_cache.len() >= CACHE_LIMIT {
			self.typo_cache.clear();
		}
		self.typo_cache.insert(text.to_string(), ranges.clone());
		if !ranges.is_empty() {
			if let Some(on_update) = self.on_update.as_ref() {
				on_update();
			}
		}
		ranges
	}

	fn clear_caches(&mut self) {
		self.typo_cache.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::prompt::prose_gate::SpellingDecorationContext;

	struct FakeBackend {
		ranges: Vec<SpellingRange>,
		correction: Option<String>,
		guesses: Vec<String>,
	}

	impl SpellingBackend for FakeBackend {
		fn is_available(&self) -> bool {
			true
		}

		fn check_spelling(&self, _text: &str) -> Vec<SpellingRange> {
			self.ranges.clone()
		}

		fn autocorrect_word(&self, _text: &str, _start: usize, _length: usize) -> Option<String> {
			self.correction.clone()
		}

		fn spelling_guesses(&self, _text: &str, _start: usize, _length: usize) -> Vec<String> {
			self.guesses.clone()
		}
	}

	fn provider(backend: FakeBackend) -> MacOSSpellingProvider {
		let mut provider = MacOSSpellingProvider::new(Arc::new(backend), true);
		provider.set_features(MacOSSpellingFeatures {
			typo_detection: true,
			autocorrect: true,
		});
		provider
	}

	fn context(line: &str) -> (Vec<String>, SpellingDecorationContext) {
		let lines = vec![line.to_string()];
		let context = SpellingDecorationContext {
			editor_text: line.to_string(),
			lines: lines.clone(),
			line: 0,
			start_col: 0,
			typing: None,
		};
		(lines, context)
	}

	#[test]
	fn unavailable_backend_disables_provider() {
		struct Unavailable;
		impl SpellingBackend for Unavailable {
			fn is_available(&self) -> bool {
				false
			}
			fn check_spelling(&self, _text: &str) -> Vec<SpellingRange> {
				vec![SpellingRange {
					start: 0,
					length: 3,
				}]
			}
			fn autocorrect_word(&self, _text: &str, _s: usize, _l: usize) -> Option<String> {
				Some("x".to_string())
			}
			fn spelling_guesses(&self, _text: &str, _s: usize, _l: usize) -> Vec<String> {
				vec!["y".to_string()]
			}
		}
		let mut provider = MacOSSpellingProvider::new(Arc::new(Unavailable), false);
		provider.set_features(MacOSSpellingFeatures {
			typo_detection: true,
			autocorrect: true,
		});
		assert!(!provider.is_available());
		let (_, ctx) = context("teh word");
		assert!(provider.typo_ranges("teh word", &ctx).is_empty());
		assert!(provider.try_autocorrect(&["teh ".to_string()], 0, 4).is_none());
	}

	#[test]
	fn decorates_typo_ranges_with_marks() {
		let backend = FakeBackend {
			ranges: vec![SpellingRange {
				start: 0,
				length: 3,
			}],
			correction: None,
			guesses: Vec::new(),
		};
		let mut provider = provider(backend);
		let (_, ctx) = context("teh word");
		let decorated = provider.decorate_typos("teh word", &ctx, &|text| text.to_string());
		assert!(decorated.starts_with(STYLED_TYPO_MARKS.0));
		assert_eq!(crate::ansi_scan::strip_ansi(&decorated), "teh word");
	}

	#[test]
	fn skips_ranges_touching_the_typing_word() {
		let backend = FakeBackend {
			ranges: vec![SpellingRange {
				start: 0,
				length: 3,
			}],
			correction: None,
			guesses: Vec::new(),
		};
		let mut provider = provider(backend);
		let lines = vec!["teh word".to_string()];
		let mut ctx = SpellingDecorationContext {
			editor_text: "teh word".to_string(),
			lines,
			line: 0,
			start_col: 0,
			typing: Some(crate::prompt::typing_word::TypingWord {
				line: 0,
				start: 0,
				end: 3,
			}),
		};
		assert!(provider.typo_ranges("teh word", &ctx).is_empty());
		ctx.typing = None;
		assert_eq!(provider.typo_ranges("teh word", &ctx).len(), 1);
	}

	#[test]
	fn autocorrects_completed_prose_words() {
		let backend = FakeBackend {
			ranges: Vec::new(),
			correction: Some("the".to_string()),
			guesses: Vec::new(),
		};
		let mut provider = provider(backend);
		let lines = vec!["teh ".to_string()];
		let replacement = provider.try_autocorrect(&lines, 0, 4).expect("replacement");
		assert_eq!(
			replacement,
			EditorInlineReplacement {
				replace_len: 4,
				insert: "the ".to_string()
			}
		);
		assert!(provider.try_autocorrect(&["teh".to_string()], 0, 3).is_none());
	}

	#[test]
	fn offers_capped_guesses_for_the_cursor_word() {
		let backend = FakeBackend {
			ranges: vec![SpellingRange {
				start: 0,
				length: 3,
			}],
			correction: None,
			guesses: (0..12).map(|index| format!("guess{index}")).collect(),
		};
		let mut provider = provider(backend);
		let lines = vec!["teh word".to_string()];
		let replacements = provider.get_word_replacements(&lines, 0, 2).expect("replacements");
		assert_eq!(replacements.items.len(), 10);
		assert_eq!(replacements.start_col, 0);
		assert_eq!(replacements.end_col, 3);
		assert!(provider.get_word_replacements(&lines, 0, 8).is_none());
	}
}
