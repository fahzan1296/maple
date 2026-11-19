//! Reference: `l/src/prompt/skill-tokens.ts`.
//!
//! `/skill:<name>` token recognition, and the drafts in which such tokens are
//! invocations rather than content belonging to another slash command or a
//! local-execution sigil.
//!
//! Deviation: the TypeScript global regex ends in a `(?=\s|$)` lookahead, which
//! the `regex` crate lacks; the pattern stops before the delimiter and the
//! iterator rejects a match whose following character is not whitespace.

use std::sync::LazyLock;

use regex::Regex;

/// One `/skill:<name>` token delimited by whitespace or line edges.
///
/// Group 1 is the leading delimiter (empty at line start), group 2 the skill
/// name: a bare name or a collision-namespaced `<namespace>/<name>` (exactly one
/// `/`; deeper paths never match). Callers must honour the trailing-delimiter
/// lookahead themselves; [`skill_tokens`] does.
pub static SKILL_TOKEN_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(^|\s)/skill:([^\s/]+(?:/[^\s/]+)?)").expect("skill token regex"));

/// A matched `/skill:<name>` token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillToken {
	/// Leading delimiter: the whitespace character, or empty at line start.
	pub delimiter: String,
	/// Bare or namespaced skill name.
	pub name: String,
	/// Byte offset of the delimiter (or of `/skill:` at line start).
	pub start: usize,
	/// Byte offset one past the name.
	pub end: usize,
}

impl SkillToken {
	/// Byte offset of the `/skill:` sigil.
	pub fn sigil_start(&self) -> usize {
		self.start + self.delimiter.len()
	}
}

/// Every `/skill:<name>` token in `text`, left to right.
pub fn skill_tokens(text: &str) -> Vec<SkillToken> {
	let mut tokens = Vec::new();
	for caps in SKILL_TOKEN_RE.captures_iter(text) {
		let whole = caps.get(0).expect("whole match");
		let after = text[whole.end()..].chars().next();
		if after.is_some_and(|ch| !ch.is_whitespace()) {
			continue;
		}
		let delimiter = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
		let name = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
		tokens.push(SkillToken {
			start: whole.start(),
			end: whole.end(),
			delimiter,
			name,
		});
	}
	tokens
}

/// Whether the (already left-trimmed) draft begins with a TUI local-execution
/// sigil that downstream branches consume verbatim.
fn starts_with_local_execution_prefix(trimmed_start: &str) -> bool {
	if trimmed_start.starts_with('!') {
		return true;
	}
	let chars: Vec<char> = trimmed_start.chars().collect();
	if chars.first() != Some(&'$') {
		return false;
	}
	if chars.get(1) == Some(&'{') {
		return false;
	}
	let sigil_length = if chars.get(1) == Some(&'$') { 2 } else { 1 };
	match chars.get(sigil_length) {
		None => true,
		Some(' ') | Some('\t') | Some('\n') | Some('\r') => true,
		Some(_) => false,
	}
}

/// Whether `/skill:<name>` tokens in `text` are invocations rather than content
/// belonging to another slash command or local-execution sigil.
pub fn allows_skill_tokens(text: &str) -> bool {
	let trimmed_start = text.trim_start();
	if trimmed_start.starts_with("/skill:") {
		return true;
	}
	if trimmed_start.starts_with('/') {
		return false;
	}
	!starts_with_local_execution_prefix(trimmed_start)
}

/// Whether model mentions may collapse in this draft; local execution consumes
/// its body verbatim.
pub fn allows_model_mentions(text: &str) -> bool {
	!starts_with_local_execution_prefix(text.trim_start())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_skill_tokens_with_delimiters() {
		let cases: [(&str, Vec<(&str, &str)>); 6] = [
			("/skill:review", vec![("", "review")]),
			("run /skill:ns/review now", vec![(" ", "ns/review")]),
			("/skill:a/b/c", vec![]),
			("/skill:x\n/skill:y", vec![("", "x"), ("\n", "y")]),
			("text /skill:trailing", vec![(" ", "trailing")]),
			("/skill:", vec![]),
		];
		for (text, expected) in cases {
			let got: Vec<(String, String)> = skill_tokens(text)
				.into_iter()
				.map(|t| (t.delimiter, t.name))
				.collect();
			let expected: Vec<(String, String)> = expected
				.into_iter()
				.map(|(d, n)| (d.to_string(), n.to_string()))
				.collect();
			assert_eq!(got, expected, "text {text:?}");
		}
	}

	#[test]
	fn token_spans_cover_sigil_and_name() {
		let tokens = skill_tokens("hi /skill:ns/a");
		assert_eq!(tokens.len(), 1);
		let token = &tokens[0];
		assert_eq!(token.sigil_start(), 3);
		assert_eq!(&"hi /skill:ns/a"[token.start..token.end], " /skill:ns/a");
	}

	#[test]
	fn gates_skill_tokens_by_draft_shape() {
		let cases: [(&str, bool); 8] = [
			("/skill:review", true),
			("  /skill:review", true),
			("/help /skill:x", false),
			("!echo /skill:x", false),
			("$ echo /skill:x", false),
			("$echo /skill:x", false),
			("$$ x /skill:y", false),
			("plain text /skill:y", true),
		];
		for (text, expected) in cases {
			assert_eq!(allows_skill_tokens(text), expected, "text {text:?}");
		}
	}

	#[test]
	fn gates_model_mentions_by_draft_shape() {
		let cases: [(&str, bool); 5] = [
			("^model", true),
			("  ^model", true),
			("!echo ^model", false),
			("$ ^model", false),
			("$x ^model", true),
		];
		for (text, expected) in cases {
			assert_eq!(allows_model_mentions(text), expected, "text {text:?}");
		}
	}
}
