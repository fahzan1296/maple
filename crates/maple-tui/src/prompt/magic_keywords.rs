//! Reference: `l/src/prompt/magic-keywords.ts`.
//!
//! Magic-keyword engine: standalone prose words the host registers via
//! [`set_magic_keywords`] glow with a per-word gradient in the editor and in
//! sent bubbles, and are exempt from spelling autocorrect. The host owns the
//! word list and the notices each word injects; this module only knows how to
//! find and paint them.
//!
//! Deviations:
//! - The boundary expressions use variable-length lookbehind/lookahead, absent
//!   from the `regex` crate; matches of the bare word are filtered through
//!   [`boundaries_ok`], which applies the same rules character by character.
//! - Ranges are character indices (see [`crate::prompt::markdown_prose`]).
//! - The gradient's [`ColorMode`] is passed to [`highlight_magic_keywords`]
//!   instead of being read from the global theme.
//! - The registry and the last-probe memo live in process-wide `Mutex`es.

use std::sync::{LazyLock, Mutex};

use regex::Regex;

use crate::prompt::gradient_highlight::{
	create_gradient_highlighter, GradientHighlightSpec,
};
use crate::prompt::markdown_prose::{keyword_in_prose, mask_non_prose};
use crate::theme::color::ColorMode;

/// One registered magic keyword: the prose trigger and its editor gradient.
#[derive(Clone, Debug, PartialEq)]
pub struct MagicKeywordSpec {
	/// Exact lowercase trigger, matched only as standalone prose.
	pub word: String,
	/// HSL hue sweep `[from, to]` in degrees painted across the word; `to` may
	/// exceed 360 to wrap through red.
	pub hue: (f64, f64),
	/// HSL lightness sweep `[from, to]` in percent; `None` keeps the default.
	pub lightness: Option<(f64, f64)>,
}

struct RegisteredKeyword {
	word: String,
	pattern: Regex,
	/// Non-global copy for prose detection.
	matcher: Regex,
	from: f64,
	to: f64,
	lightness: Option<(f64, f64)>,
}

impl RegisteredKeyword {
	fn spec(&self) -> GradientHighlightSpec {
		let from = self.from;
		let to = self.to;
		let mut spec = GradientHighlightSpec::new(
			self.word.clone(),
			// The pattern is reused for painting; a clone keeps the spec owning it.
			self.pattern.clone(),
			14,
			move |t| (from + t * (to - from)) % 360.0,
		);
		if let Some((light_from, light_to)) = self.lightness {
			spec.lightness = Some(std::sync::Arc::new(move |t| {
				light_from + t * (light_to - light_from)
			}));
		}
		spec
	}
}

static REGISTRY: LazyLock<Mutex<Vec<RegisteredKeyword>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static REGISTRY_GENERATION: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(0));

/// Escape a literal for the `regex` crate (`[\\^$.*+?()[\]{}|]`).
fn escape_regex(word: &str) -> String {
	let mut out = String::with_capacity(word.len());
	for ch in word.chars() {
		if matches!(
			ch,
			'\\' | '^' | '$' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
		) {
			out.push('\\');
		}
		out.push(ch);
	}
	out
}

fn alnum(ch: char) -> bool {
	ch.is_alphanumeric()
}

/// Apply the reference's left/right boundary rules around a candidate match.
fn boundaries_ok(chars: &[char], start: usize, end: usize) -> bool {
	// Left: no letter/number/underscore/dot/slash/backslash/hyphen, no `::`.
	if start > 0 {
		let previous = chars[start - 1];
		if alnum(previous) || matches!(previous, '_' | '.' | '/' | '\\' | '-') {
			return false;
		}
		if start > 1 && chars[start - 2] == ':' && previous == ':' {
			return false;
		}
	}
	// Right: no letter/number/underscore/slash/backslash/hyphen, no
	// `.ext`, no immediate call parenthesis.
	if end < chars.len() {
		let next = chars[end];
		if alnum(next) || matches!(next, '_' | '/' | '\\' | '-') {
			return false;
		}
		if next == '.' {
			if let Some(after) = chars.get(end + 1) {
				if alnum(*after) || matches!(*after, '_' | '-') {
					return false;
				}
			}
		}
		if next == '(' {
			return false;
		}
	}
	true
}

fn keyword_regex(word: &str) -> Regex {
	Regex::new(&escape_regex(word)).expect("magic keyword regex")
}

/// Standalone-prose matches of `keyword.pattern` in `text`, filtered by the
/// boundary rules.
fn keyword_matches(text: &str, keyword: &RegisteredKeyword) -> Vec<(usize, usize)> {
	let chars: Vec<char> = text.chars().collect();
	let mut matches = Vec::new();
	for found in keyword.pattern.find_iter(text) {
		// Byte offsets to character indices.
		let start = text[..found.start()].chars().count();
		let end = start + found.as_str().chars().count();
		if boundaries_ok(&chars, start, end) {
			matches.push((start, end));
		}
	}
	matches
}

/// Replace the registered keyword set. The host calls this once at startup;
/// until then nothing glows and no word is exempt from autocorrect.
pub fn set_magic_keywords(specs: &[MagicKeywordSpec]) {
	let registry: Vec<RegisteredKeyword> = specs
		.iter()
		.map(|spec| RegisteredKeyword {
			word: spec.word.clone(),
			pattern: keyword_regex(&spec.word),
			matcher: keyword_regex(&spec.word),
			from: spec.hue.0,
			to: spec.hue.1,
			lightness: spec.lightness,
		})
		.collect();
	*REGISTRY.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = registry;
	*REGISTRY_GENERATION
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
	let mut probe = LAST_PROBE
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	*probe = None;
}

/// Whether `word` is exactly a registered magic keyword (used to shield it from
/// spelling autocorrect).
pub fn is_magic_keyword(word: &str) -> bool {
	let registry = REGISTRY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	registry.iter().any(|keyword| keyword.word == word)
}

/// Whether `text` contains `word` as standalone lowercase prose — never inside a
/// code block, inline code span, or XML/HTML section. Pure: does not require
/// `word` to be registered.
pub fn contains_magic_keyword(text: &str, word: &str) -> bool {
	let matcher = keyword_regex(word);
	let chars: Vec<char> = text.chars().collect();
	let standalone = matcher.find_iter(text).any(|found| {
		let start = text[..found.start()].chars().count();
		boundaries_ok(&chars, start, start + found.as_str().chars().count())
	});
	if !standalone {
		return false;
	}
	keyword_in_prose(text, &matcher)
}

/// Gradient-highlight every registered keyword that appears as standalone prose,
/// skipping any occurrence inside a code block, inline code span, or XML/HTML
/// section.
///
/// `reset_to` is the SGR foreground sequence restored after each painted
/// keyword; `phase` in `[0, 1)` cyclically rotates each gradient.
pub fn highlight_magic_keywords(
	text: &str,
	reset_to: Option<&str>,
	phase: f64,
	mode: ColorMode,
) -> String {
	let mut out = text.to_string();
	let registry = REGISTRY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	for keyword in registry.iter() {
		let highlighter = create_gradient_highlighter(keyword.spec(), mode);
		out = highlighter.highlight_with(&out, reset_to.unwrap_or(crate::theme::color::FG_RESET), phase);
	}
	out
}

/// Character ranges of every registered keyword standing as prose in `text`
/// (never inside code or XML/HTML sections), for decorating the keywords
/// without painting them: a TSP terminal draws the gradient sweep itself.
pub fn magic_keyword_ranges(text: &str) -> Vec<(usize, usize)> {
	let registry = REGISTRY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	let mut ranges: Vec<(usize, usize)> = Vec::new();
	let mut masked: Option<String> = None;
	for keyword in registry.iter() {
		if !text.contains(&keyword.word) {
			continue;
		}
		let mask = masked.get_or_insert_with(|| mask_non_prose(text));
		ranges.extend(keyword_matches(mask, keyword));
	}
	ranges.sort_by_key(|(from, _)| *from);
	ranges
}

/// Last [`has_magic_keyword`] answer: the live editor asks once per layout
/// segment with the same buffer.
static LAST_PROBE: LazyLock<Mutex<Option<(String, u64, bool)>>> = LazyLock::new(|| Mutex::new(None));

/// Cheap test for "does this text contain any registered keyword as standalone
/// prose?". Short-circuits on a substring probe before paying for the
/// markdown-aware prose check, so the common "no keyword in buffer" path is one
/// `contains` per word.
pub fn has_magic_keyword(text: &str) -> bool {
	let generation = *REGISTRY_GENERATION
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	if let Some((probed_text, probed_generation, found)) =
		LAST_PROBE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref()
	{
		if probed_text == text && *probed_generation == generation {
			return *found;
		}
	}
	let mut found = false;
	{
		let registry = REGISTRY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		for keyword in registry.iter() {
			if text.contains(&keyword.word) && contains_magic_keyword(text, &keyword.word) {
				found = true;
				break;
			}
		}
	}
	*LAST_PROBE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
		Some((text.to_string(), generation, found));
	found
}

#[cfg(test)]
mod tests {
	use super::*;

	fn spec(word: &str, hue: (f64, f64)) -> MagicKeywordSpec {
		MagicKeywordSpec {
			word: word.to_string(),
			hue,
			lightness: None,
		}
	}

	fn install() {
		set_magic_keywords(&[
			MagicKeywordSpec {
				lightness: Some((40.0, 70.0)),
				..spec("think", (260.0, 300.0))
			},
			spec("boost", (0.0, 120.0)),
		]);
	}

	#[test]
	fn detects_standalone_prose_keywords() {
		install();
		let cases: [(&str, bool); 9] = [
			("please think now", true),
			("think.", true),
			("think()", false),
			("path/think", false),
			"think_more",
			("foo::think", false),
			("think.txt", false),
			("```\nthink\n```", false),
			("no keywords", false),
		]
		.map(|(text, expected)| if text.contains('_') { (text, false) } else { (text, expected) });
		for (text, expected) in cases {
			assert_eq!(contains_magic_keyword(text, "think"), expected, "text {text:?}");
		}
		assert!(!contains_magic_keyword("THINK", "think"));
	}

	#[test]
	fn registration_queries_words() {
		install();
		assert!(is_magic_keyword("think"));
		assert!(is_magic_keyword("boost"));
		assert!(!is_magic_keyword("Think"));
		set_magic_keywords(&[]);
		assert!(!is_magic_keyword("think"));
		assert!(!has_magic_keyword("please think now"));
	}

	#[test]
	fn collects_sorted_prose_ranges() {
		install();
		let ranges = magic_keyword_ranges("boost now, then think later");
		assert_eq!(ranges, vec![(0, 5), (17, 22)]);
		let masked = magic_keyword_ranges("`think` and ```\nboost\n```");
		assert!(masked.is_empty(), "ranges {masked:?}");
	}

	#[test]
	fn highlights_with_zero_width_escapes() {
		install();
		let painted = highlight_magic_keywords("think", None, 0.0, ColorMode::TrueColor);
		assert!(painted.starts_with("\x1b[38;2;"));
		assert_eq!(crate::ansi_scan::strip_ansi(&painted), "think");
		let plain = highlight_magic_keywords("nothing here", None, 0.25, ColorMode::Color256);
		assert_eq!(plain, "nothing here");
	}

	#[test]
	fn probe_memo_tracks_registry_changes() {
		install();
		assert!(has_magic_keyword("think about it"));
		assert!(has_magic_keyword("think about it"));
		assert!(!has_magic_keyword("only words"));
	}
}
