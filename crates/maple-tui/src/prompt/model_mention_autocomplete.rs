//! Reference: `l/src/prompt/model-mention-autocomplete.ts`.
//!
//! `^` model-mention completion: picker-ranked model candidates after the sigil,
//! plus `:<effort>` rows for models that support levels.
//!
//! Deviations:
//! - The candidate source is a [`ModelMentionCandidateSource`] trait instead of a
//!   function object with optional properties; the overlay model-browser
//!   (scope cache, item ranker, discovery registry) has no port in this crate yet,
//!   so ranking is the source's responsibility and the candidate carries the
//!   efforts its model supports (the reference asks `getSupportedEfforts`).
//! - `retryDiscovery` returns whether a retry pass started rather than a promise.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::autocomplete::AutocompleteItem;
use crate::prompt::model_mention_syntax::{model_mention_display_name, model_mention_name};

/// `(?:^|\s)(\^[^\s]*)$` — the live mention token before the cursor.
pub static MODEL_MENTION_CONTEXT_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?:^|\s)(\^[^\s]*)$").expect("model mention context regex"));

const MAX_MODEL_MENTION_SUGGESTIONS: usize = 20;
const MAX_EFFORT_CANDIDATE_MODELS: usize = 3;

/// A model a mention can resolve to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMentionModel {
	pub id: String,
	pub name: String,
	/// `provider/id`, the canonical selector.
	pub selector: String,
	/// Thinking levels this model supports, e.g. `["low", "high"]`.
	pub efforts: Vec<String>,
}

/// One ranked candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMentionCandidate {
	pub selector: String,
	pub model: ModelMentionModel,
}

/// Supplies picker-ranked model candidates for the query after `^`.
pub trait ModelMentionCandidateSource {
	/// Candidates for `query`, best first.
	fn candidates(&self, query: &str) -> Vec<ModelMentionCandidate>;
	/// Every available model's selector, in or out of the picker scope.
	fn available_model_ids(&self) -> Option<HashSet<String>> {
		None
	}
	/// Start a due provider-discovery retry; `true` when a pass started.
	fn retry_discovery(&self) -> bool {
		false
	}
}

/// Mention choices for the token at the cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelMentionCompletion {
	/// The text before the cursor is not a mention token.
	Unhandled,
	/// A mention token, but no candidate matches.
	NoMatch,
	/// Candidates to show; `pending` reports a started discovery retry.
	Matched {
		suggestions: ModelMentionSuggestions,
		pending: bool,
	},
}

/// Items plus the buffer token they replace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMentionSuggestions {
	pub items: Vec<AutocompleteItem>,
	pub prefix: String,
}

/// Whether an autocomplete prefix belongs to a model mention.
pub fn is_model_mention_prefix(prefix: &str) -> bool {
	prefix.starts_with('^')
}

fn mention_row(value: &str, description: &str) -> AutocompleteItem {
	AutocompleteItem::new(value, value)
		.with_description(description)
		.with_icon(crate::theme::symbols::preset_symbols(crate::theme::symbols::SymbolPreset::Unicode)
			.get("icon.model")
			.unwrap_or(""))
		.with_icon_name("model")
}

/// Some provider model ids contain a colon or end in a level name (`qwen3:8b`,
/// `glm-4.7:max`), so literal matches are listed before level rows.
fn mention_rows(query: &str, candidates: &dyn ModelMentionCandidateSource) -> Vec<AutocompleteItem> {
	if let Some(colon) = query.rfind(':') {
		if colon > 0 {
			let typed = query[colon + 1..].to_lowercase();
			let mut level_rows: Vec<AutocompleteItem> = Vec::new();
			for candidate in candidates
				.candidates(&query[..colon])
				.into_iter()
				.take(MAX_EFFORT_CANDIDATE_MODELS)
			{
				for effort in &candidate.model.efforts {
					if !effort.starts_with(&typed) {
						continue;
					}
					level_rows.push(mention_row(
						&format!("{}:{effort}", candidate.selector),
						&model_mention_name(
							&model_mention_display_name(&candidate.model.name, &candidate.model.id),
							effort,
						),
					));
				}
			}
			if !level_rows.is_empty() {
				let lowered = query.to_lowercase();
				let mut literal_rows: Vec<AutocompleteItem> = Vec::new();
				for item in candidates.candidates(query) {
					if literal_rows.len() == MAX_MODEL_MENTION_SUGGESTIONS {
						break;
					}
					if item.selector.to_lowercase().contains(&lowered) {
						literal_rows.push(mention_row(
							&item.selector,
							&model_mention_display_name(&item.model.name, &item.model.id),
						));
					}
				}
				let literal_values: HashSet<String> =
					literal_rows.iter().map(|row| row.value.clone()).collect();
				let available = candidates.available_model_ids();
				let mut rows = literal_rows;
				rows.extend(level_rows.into_iter().filter(|row| {
					!literal_values.contains(&row.value)
						&& !available.as_ref().is_some_and(|ids| ids.contains(&row.value))
				}));
				return rows;
			}
		}
	}
	candidates
		.candidates(query)
		.into_iter()
		.take(MAX_MODEL_MENTION_SUGGESTIONS)
		.map(|item| {
			mention_row(
				&item.selector,
				&model_mention_display_name(&item.model.name, &item.model.id),
			)
		})
		.collect()
}

/// Build model mention choices for the whitespace-delimited token at the cursor.
pub fn get_model_mention_suggestions(
	text_before_cursor: &str,
	candidates: Option<&dyn ModelMentionCandidateSource>,
) -> ModelMentionCompletion {
	let Some(candidates) = candidates else {
		return ModelMentionCompletion::Unhandled;
	};
	let Some(caps) = MODEL_MENTION_CONTEXT_RE.captures(text_before_cursor) else {
		return ModelMentionCompletion::Unhandled;
	};
	let prefix = caps.get(1).expect("prefix").as_str().to_string();
	// Started even without a match: the next keystroke reads whatever the retry
	// recovers.
	let pending = candidates.retry_discovery();
	let items: Vec<AutocompleteItem> = mention_rows(&prefix[1..], candidates)
		.into_iter()
		.take(MAX_MODEL_MENTION_SUGGESTIONS)
		.collect();
	if items.is_empty() {
		ModelMentionCompletion::NoMatch
	} else {
		ModelMentionCompletion::Matched {
			suggestions: ModelMentionSuggestions { items, prefix },
			pending,
		}
	}
}

/// Replace the live mention token with the selected canonical selector and a
/// trailing space.
pub fn apply_model_mention_completion(
	lines: &[String],
	cursor_line: usize,
	cursor_col: usize,
	item: &AutocompleteItem,
	prefix: &str,
) -> (Vec<String>, usize, usize) {
	let current_line = lines.get(cursor_line).cloned().unwrap_or_default();
	let chars: Vec<char> = current_line.chars().collect();
	let cursor_col = cursor_col.min(chars.len());
	let text_before_cursor: String = chars[..cursor_col].iter().collect();
	let live_prefix = MODEL_MENTION_CONTEXT_RE
		.captures(&text_before_cursor)
		.and_then(|caps| caps.get(1))
		.map(|m| m.as_str().to_string());
	let prefix_start = match live_prefix.as_ref() {
		Some(live) => text_before_cursor.chars().count() - live.chars().count(),
		None => cursor_col.saturating_sub(prefix.chars().count()),
	};
	let replacement = format!("^{} ", item.value);
	let before: String = chars[..prefix_start].iter().collect();
	let after: String = chars[cursor_col..].iter().collect();
	let new_line = format!("{before}{replacement}{after}");
	let mut new_lines = lines.to_vec();
	if cursor_line < new_lines.len() {
		new_lines[cursor_line] = new_line;
	} else {
		new_lines.resize(cursor_line + 1, String::new());
		new_lines[cursor_line] = new_line;
	}
	(
		new_lines,
		cursor_line,
		prefix_start + replacement.chars().count(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Source {
		models: Vec<ModelMentionModel>,
		available: HashSet<String>,
		retried: bool,
	}

	impl Source {
		fn new() -> Self {
			Self {
				models: vec![
					ModelMentionModel {
						id: "claude".to_string(),
						name: "Claude".to_string(),
						selector: "anthropic/claude".to_string(),
						efforts: vec!["low".to_string(), "high".to_string()],
					},
					ModelMentionModel {
						id: "qwen3:8b".to_string(),
						name: String::new(),
						selector: "local/qwen3:8b".to_string(),
						efforts: Vec::new(),
					},
				],
				available: HashSet::new(),
				retried: false,
			}
		}
	}

	impl ModelMentionCandidateSource for Source {
		fn candidates(&self, query: &str) -> Vec<ModelMentionCandidate> {
			self.models
				.iter()
				.filter(|model| model.selector.to_lowercase().contains(&query.to_lowercase()))
				.map(|model| ModelMentionCandidate {
					selector: model.selector.clone(),
					model: model.clone(),
				})
				.collect()
		}

		fn available_model_ids(&self) -> Option<HashSet<String>> {
			Some(self.available.clone())
		}

		fn retry_discovery(&self) -> bool {
			self.retried
		}
	}

	#[test]
	fn unhandled_without_token_or_source() {
		let source = Source::new();
		assert_eq!(
			get_model_mention_suggestions("no mention", Some(&source)),
			ModelMentionCompletion::Unhandled
		);
		assert_eq!(get_model_mention_suggestions("^claude", None), ModelMentionCompletion::Unhandled);
		assert!(is_model_mention_prefix("^x"));
		assert!(!is_model_mention_prefix("x"));
	}

	#[test]
	fn lists_literal_matches() {
		match get_model_mention_suggestions("ask ^claude", Some(&Source::new())) {
			ModelMentionCompletion::Matched { suggestions, pending } => {
				assert!(!pending);
				assert_eq!(suggestions.prefix, "^claude");
				assert_eq!(suggestions.items.len(), 1);
				assert_eq!(suggestions.items[0].value, "anthropic/claude");
				assert_eq!(
					suggestions.items[0].description.as_deref(),
					Some("Claude")
				);
			}
			other => panic!("unexpected {other:?}"),
		}
	}

	#[test]
	fn lists_effort_rows_before_extra_literals() {
		match get_model_mention_suggestions("^anthropic/claude:h", Some(&Source::new())) {
			ModelMentionCompletion::Matched { suggestions, .. } => {
				let values: Vec<&str> = suggestions.items.iter().map(|item| item.value.as_str()).collect();
				assert_eq!(values, vec!["anthropic/claude:high"]);
				assert_eq!(
					suggestions.items[0].description.as_deref(),
					Some("Claude · high")
				);
			}
			other => panic!("unexpected {other:?}"),
		}
	}

	#[test]
	fn no_match_when_nothing_matches() {
		assert_eq!(
			get_model_mention_suggestions("^zzz", Some(&Source::new())),
			ModelMentionCompletion::NoMatch
		);
	}

	#[test]
	fn pending_reports_a_started_retry() {
		let mut source = Source::new();
		source.retried = true;
		match get_model_mention_suggestions("^claude", Some(&source)) {
			ModelMentionCompletion::Matched { pending, .. } => assert!(pending),
			other => panic!("unexpected {other:?}"),
		}
	}

	#[test]
	fn applies_the_live_token() {
		let lines = vec!["ask ^clau now".to_string()];
		let item = AutocompleteItem::new("anthropic/claude", "anthropic/claude");
		// Live token `^clau` ends at column 9.
		let (new_lines, line, col) = apply_model_mention_completion(&lines, 0, 9, &item, "^clau");
		assert_eq!(new_lines[0], "ask ^anthropic/claude  now");
		assert_eq!(line, 0);
		assert_eq!(col, 24);

		// A stale prefix falls back to the given prefix length.
		let (fallback, _, _) = apply_model_mention_completion(&["^x".to_string()], 0, 2, &item, "^x");
		assert_eq!(fallback[0], "^anthropic/claude ");
	}
}
