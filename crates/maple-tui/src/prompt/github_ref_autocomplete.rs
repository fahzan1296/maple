//! Reference: `l/src/prompt/github-ref-autocomplete.ts`.
//!
//! Autocomplete for GitHub issue/PR references typed as `#<number>` (e.g.
//! `#3164`). The token is rewritten to an internal URL (`pr://3164` or
//! `issue://3164`) plus a trailing space, and the existing tool-mediated
//! pipeline resolves it from the session cwd's git remote.
//!
//! No network at suggestion time: GitHub shares the issue/PR number space and
//! there is no cheap way to tell which a given number is while typing, so both a
//! PR and an Issue candidate are offered by default. Naming the type first
//! (`pr #3164` / `issue #3164`) constrains the candidates to that kind.

use std::sync::LazyLock;

use regex::Regex;

use crate::autocomplete::AutocompleteItem;

/// A GitHub reference kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GithubRefKind {
	/// Qualifier word the user types (`pr`, `issue`).
	pub qualifier: &'static str,
	/// Scheme of the internal URL the token becomes.
	pub scheme: &'static str,
	/// Display label.
	pub label: &'static str,
	pub description: &'static str,
}

/// Candidate kinds, in default display order.
pub const GITHUB_REF_KINDS: [GithubRefKind; 2] = [
	GithubRefKind {
		qualifier: "pr",
		scheme: "pr",
		label: "PR",
		description: "GitHub pull request",
	},
	GithubRefKind {
		qualifier: "issue",
		scheme: "issue",
		label: "Issue",
		description: "GitHub issue",
	},
];

/// Which kind an optional qualifier word constrains to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GithubRefQualifier {
	Pr,
	Issue,
}

impl GithubRefQualifier {
	/// The kind this qualifier selects.
	pub fn kind(self) -> GithubRefKind {
		match self {
			GithubRefQualifier::Pr => GITHUB_REF_KINDS[0],
			GithubRefQualifier::Issue => GITHUB_REF_KINDS[1],
		}
	}
}

/// A `#<number>` reference being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubRefContext {
	/// Text to replace on accept: `#3164`, or `pr #3164` when a qualifier precedes it.
	pub prefix: String,
	/// Type the user named (`pr`/`pull` → `pr`, `issue` → `issue`), or `None` to
	/// offer both.
	pub qualifier: Option<GithubRefQualifier>,
	/// The numeric reference, e.g. `3164`.
	pub number: String,
}

/// A standalone `#<positive-number>` token ending at the cursor. The `#` must be
/// preceded by a token boundary (start, whitespace, or an opening quote/paren/`<`/`=`,
/// matching the internal-URL boundary set) so embedded hashes like `owner/repo#N`,
/// `foo#N`, `C#12`, or a URL fragment do not match. An optional
/// `pr`/`pull`/`issue` qualifier word (case-insensitive) immediately before the
/// `#` constrains the kind.
pub static GITHUB_REF_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r#"(?i)(?:^|[\s"'`(<=])(?:(pr|pull|issue)(\s+))?#([1-9]\d*)$"#)
		.expect("github ref token regex")
});

/// The reference being typed before the cursor, if any.
pub fn get_github_ref_context(text_before_cursor: &str) -> Option<GithubRefContext> {
	let caps = GITHUB_REF_TOKEN_RE.captures(text_before_cursor)?;
	let qualifier_word = caps.get(1).map(|m| m.as_str());
	let whitespace = caps.get(2).map(|m| m.as_str()).unwrap_or("");
	let number = caps.get(3).map(|m| m.as_str()).unwrap_or("");
	Some(GithubRefContext {
		prefix: match qualifier_word {
			Some(word) => format!("{word}{whitespace}#{number}"),
			None => format!("#{number}"),
		},
		qualifier: qualifier_word.map(|word| {
			if word.eq_ignore_ascii_case("issue") {
				GithubRefQualifier::Issue
			} else {
				GithubRefQualifier::Pr
			}
		}),
		number: number.to_string(),
	})
}

/// Suggestions for a `#<number>` token. Both kinds are offered unless the user
/// named a type, in which case only that kind is offered. `None` when the text
/// before the cursor is not a standalone `#<number>` token.
pub fn get_github_ref_suggestions(text_before_cursor: &str) -> Option<(Vec<AutocompleteItem>, String)> {
	let context = get_github_ref_context(text_before_cursor)?;
	let kinds: Vec<GithubRefKind> = match context.qualifier {
		Some(qualifier) => vec![qualifier.kind()],
		None => GITHUB_REF_KINDS.to_vec(),
	};
	let items: Vec<AutocompleteItem> = kinds
		.into_iter()
		.map(|kind| {
			AutocompleteItem::new(
				format!("{}://{}", kind.scheme, context.number),
				format!("{} #{}", kind.label, context.number),
			)
			.with_description(kind.description)
		})
		.collect();
	Some((items, context.prefix.clone()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn detects_standalone_references() {
		let cases: [(&str, Option<(&str, Option<GithubRefQualifier>, &str)>); 10] = [
			("#3164", Some(("#3164", None, "3164"))),
			("see #12", Some(("#12", None, "12"))),
			("pr #7", Some(("pr #7", Some(GithubRefQualifier::Pr), "7"))),
			("pull  #7", Some(("pull  #7", Some(GithubRefQualifier::Pr), "7"))),
			("Issue #8", Some(("Issue #8", Some(GithubRefQualifier::Issue), "8"))),
			("(#5", Some(("#5", None, "5"))),
			("owner/repo#1", None),
			("foo#1", None),
			("C#12", None),
			("#0", None),
			("#", None),
		];
		for (text, expected) in cases {
			let context = get_github_ref_context(text);
			let got = context.as_ref().map(|c| {
				(c.prefix.as_str(), c.qualifier, c.number.as_str())
			});
			assert_eq!(got, expected, "text {text:?}");
		}
	}

	#[test]
	fn offers_both_kinds_by_default() {
		let (items, prefix) = get_github_ref_suggestions("fixes #3164").expect("suggestions");
		assert_eq!(prefix, "#3164");
		let values: Vec<&str> = items.iter().map(|item| item.value.as_str()).collect();
		assert_eq!(values, vec!["pr://3164", "issue://3164"]);
		let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
		assert_eq!(labels, vec!["PR #3164", "Issue #3164"]);
		assert_eq!(items[0].description.as_deref(), Some("GitHub pull request"));
	}

	#[test]
	fn qualifier_constrains_kind() {
		let (items, prefix) = get_github_ref_suggestions("pr #42").expect("suggestions");
		assert_eq!(prefix, "pr #42");
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].value, "pr://42");

		let (items, _) = get_github_ref_suggestions("Issue #9").expect("suggestions");
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].value, "issue://9");
	}

	#[test]
	fn ignores_non_reference_text() {
		assert!(get_github_ref_suggestions("hello").is_none());
		assert!(get_github_ref_suggestions("see https://x.test/#12").is_none());
	}
}
