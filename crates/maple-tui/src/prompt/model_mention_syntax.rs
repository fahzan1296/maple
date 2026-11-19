//! Reference: `l/src/prompt/model-mention-syntax.ts`.
//!
//! Dependency-free `^model` mention syntax shared by the composer, message
//! rendering, and the session registry: token and submitted-tag grammar plus
//! the display-name helpers.
//!
//! Deviations:
//! - The TypeScript regexes use `(?=\s|$)` lookaheads, absent from the `regex`
//!   crate; [`MODEL_MENTION_RE`] stops at the delimiter and
//!   [`model_mention_tokens`] rejects matches not followed by whitespace.
//! - `Model` and `Effort` come from the catalog package in TypeScript. The Rust
//!   port takes the two fields the helpers read (`name`, `id`) and the thinking
//!   level as strings, matching how `theme::theme_class::thinking_level_token`
//!   already treats levels crate-wide.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::prompt::skill_tokens::allows_model_mentions;

/// Whitespace-delimited token; groups are leading delimiter and selector.
pub static MODEL_MENTION_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(^|\s)\^([^\s^]+)").expect("model mention regex"));

/// Submitted mention; groups are agent pseudonym and display name.
pub static MODEL_MENTION_TAG_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r#"<model agent="(m\d+)" name="([^"]*)"/>"#).expect("model mention tag regex"));

/// A matched `^selector` mention token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMentionToken {
	/// Leading delimiter: the whitespace character, or empty at line start.
	pub delimiter: String,
	/// Selector: `provider/id`, plus `:<level>` when typed.
	pub selector: String,
	/// Byte offset of the delimiter (or of `^` at line start).
	pub start: usize,
	/// Byte offset one past the selector.
	pub end: usize,
}

impl ModelMentionToken {
	/// Byte offset of the `^` sigil.
	pub fn sigil_start(&self) -> usize {
		self.start + self.delimiter.len()
	}
}

/// A user-tagged model usable as a subagent pseudonym on the current session
/// branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMention {
	pub agent: String,
	/// Canonical selector: `provider/id`, plus `:<effort>` when the tag names one.
	pub selector: String,
	pub name: String,
	/// Initial effort from the tag's `:<level>` suffix; `None` keeps the agent's
	/// default policy.
	pub thinking_level: Option<String>,
}

/// Every `^selector` token in `text`, left to right.
pub fn model_mention_tokens(text: &str) -> Vec<ModelMentionToken> {
	let mut tokens = Vec::new();
	for caps in MODEL_MENTION_RE.captures_iter(text) {
		let whole = caps.get(0).expect("whole match");
		if text[whole.end()..].chars().next().is_some_and(|ch| !ch.is_whitespace()) {
			continue;
		}
		tokens.push(ModelMentionToken {
			delimiter: caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(),
			selector: caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
			start: whole.start(),
			end: whole.end(),
		});
	}
	tokens
}

/// Expand a composer atom back into its canonical model selector.
pub fn model_mention_token(selector: &str) -> String {
	format!("^{selector}")
}

/// Encode a registered mention for the user message sent to the model.
pub fn model_mention_tag(mention: &ModelMention) -> String {
	format!(r#"<model agent="{}" name="{}"/>"#, mention.agent, mention.name)
}

/// Use the catalog display name, without quotes, or the id when the name is
/// empty.
pub fn model_mention_display_name(name: &str, id: &str) -> String {
	let trimmed = name.trim();
	let base = if trimmed.is_empty() { id } else { trimmed };
	base.replace('"', "")
}

/// Display name of a mention, naming the tagged effort so tags of one model
/// stay distinct.
pub fn model_mention_name(display_name: &str, thinking_level: &str) -> String {
	format!("{display_name} · {thinking_level}")
}

/// Restore known submitted tags to editable selectors; leave unknown agents
/// literal.
pub fn expand_model_mention_tags<F>(text: &str, selector_for: F) -> String
where
	F: Fn(&str) -> Option<String>,
{
	MODEL_MENTION_TAG_RE
		.replace_all(text, |caps: &Captures<'_>| {
			let whole = caps.get(0).expect("whole match").as_str();
			let agent = caps.get(1).expect("agent").as_str();
			match selector_for(agent) {
				Some(selector) => model_mention_token(&selector),
				None => whole.to_string(),
			}
		})
		.into_owned()
}

/// Whether mentions may collapse in this draft (re-exported grammar gate).
pub fn mentions_allowed(text: &str) -> bool {
	allows_model_mentions(text)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn mention(agent: &str, selector: &str, name: &str) -> ModelMention {
		ModelMention {
			agent: agent.to_string(),
			selector: selector.to_string(),
			name: name.to_string(),
			thinking_level: None,
		}
	}

	#[test]
	fn finds_mention_tokens() {
		let cases: [(&str, Vec<(&str, &str)>); 6] = [
			("^gpt-4o", vec![("", "gpt-4o")]),
			("ask ^a/b:high now", vec![(" ", "a/b:high")]),
			("^^x", vec![]),
			("^a^b", vec![]),
			("^x\n^y", vec![("", "x"), ("\n", "y")]),
			("^trailing", vec![("", "trailing")]),
		];
		for (text, expected) in cases {
			let got: Vec<(String, String)> = model_mention_tokens(text)
				.into_iter()
				.map(|t| (t.delimiter, t.selector))
				.collect();
			let expected: Vec<(String, String)> = expected
				.into_iter()
				.map(|(d, s)| (d.to_string(), s.to_string()))
				.collect();
			assert_eq!(got, expected, "text {text:?}");
		}
	}

	#[test]
	fn token_span_marks_sigil() {
		let tokens = model_mention_tokens("hi ^a/b");
		assert_eq!(tokens[0].sigil_start(), 3);
		assert_eq!(tokens[0].selector, "a/b");
	}

	#[test]
	fn builds_tokens_and_tags() {
		assert_eq!(model_mention_token("a/b:high"), "^a/b:high");
		let tag = model_mention_tag(&mention("m1", "a/b", "Claude · high"));
		assert_eq!(tag, r#"<model agent="m1" name="Claude · high"/>"#);
	}

	#[test]
	fn display_names_drop_quotes_and_fall_back_to_id() {
		let cases: [(&str, &str, &str); 3] = [
			("  Claude ", "claude-x", "Claude"),
			("", "gpt-4o", "gpt-4o"),
			("a\"b", "id", "ab"),
		];
		for (name, id, expected) in cases {
			assert_eq!(model_mention_display_name(name, id), expected);
		}
		assert_eq!(model_mention_name("Claude", "high"), "Claude · high");
	}

	#[test]
	fn expands_known_tags_only() {
		let text = r#"a <model agent="m1" name="x"/> b <model agent="m2" name="y"/>"#;
		let expanded = expand_model_mention_tags(text, |agent| {
			(agent == "m1").then(|| "p/m1".to_string())
		});
		assert_eq!(expanded, r#"a ^p/m1 b <model agent="m2" name="y"/>"#);
	}
}
