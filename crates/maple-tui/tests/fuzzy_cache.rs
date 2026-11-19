//! Port of `l/test/fuzzy-cache.test.ts`.
//!
//! The crate's `fuzzy` module has no prepared-index cache: `fuzzy_rank` rebuilds
//! a search index per candidate on every call. Ranking determinism and the
//! empty-normalized-query contract are asserted directly; the prepared-text
//! APIs (`FuzzyQuery`, `FuzzyText`, `resetFuzzyIndexCache`) do not exist, so the
//! parity and cache-eviction tests stay ignored.

use maple_tui::fuzzy::{fuzzy_match, fuzzy_rank};

fn indices(items: &[String], query: &str) -> Vec<usize> {
	fuzzy_rank(items, query, |item| item.as_str())
		.into_iter()
		.map(|ranked| ranked.index)
		.collect()
}

#[test]
fn ranking_is_identical_across_repeated_passes() {
	let items = [
		"openai/gpt-4o".to_string(),
		"openai/gpt-4o-mini".to_string(),
		"openai/gpt-4-turbo".to_string(),
		"openai/o3".to_string(),
		"anthropic/claude-3.5-sonnet".to_string(),
		"anthropic/claude-4-opus".to_string(),
		"google/gemini-2.5-pro".to_string(),
	];
	let cold = indices(&items, "gpt 4o");
	let warm = indices(&items, "gpt 4o");
	assert_eq!(warm, cold);
	assert_eq!(cold, vec![0, 1, 2]);
}

#[test]
fn long_candidate_text_matches_deterministically() {
	let long = format!("openai/gpt-4o {}", "x".repeat(5000));
	let first = fuzzy_match("gpt4", &long);
	let second = fuzzy_match("gpt4", &long);
	assert_eq!(first, second);
	assert!(first.matches);
}

#[test]
fn empty_normalized_query_calls_get_text_for_every_item() {
	let items = ["alpha", "beta", "gamma"];
	let mut seen: Vec<&str> = Vec::new();
	let out = fuzzy_rank(&items, "!!!", |item| {
		seen.push(item);
		*item
	});
	assert_eq!(seen, vec!["alpha", "beta", "gamma"]);
	assert_eq!(out.iter().map(|ranked| ranked.index).collect::<Vec<_>>(), vec![0, 1, 2]);
	assert!(out.iter().all(|ranked| ranked.score == 0.0));
}

#[test]
#[ignore = "missing API: FuzzyQuery/FuzzyText prepared-text matchers"]
fn fuzzy_query_matches_fuzzy_match_for_strings_and_prepared_texts() {
}

#[test]
#[ignore = "missing API: resetFuzzyIndexCache / the per-text index cache itself"]
fn ranking_survives_cache_overflow_and_eviction() {
}
