//! Port of `l/test/fuzzy.test.ts` (`fuzzyFilter` returns indices in the Rust port).

use maple_tui::fuzzy::{fuzzy_filter, fuzzy_match};

#[test]
fn does_not_satisfy_long_tokens_by_scattering_letters_across_unrelated_words() {
	let labels = [
		"Image Provider",
		"Block Images",
		"Include Model in Prompt",
		"Service Tier",
	];
	let texts = [
		"Image Provider providers.image openrouter Preferred provider for image generation",
		"Block Images images.blockImages false Prevent images from being sent to LLM providers",
		"Include Model in Prompt includeModelInPrompt true Surface the active model identifier in the system prompt so the agent knows which model it is",
		"Service Tier serviceTier openai-only Processing priority hint on supported providers",
	];

	let results: Vec<&str> = fuzzy_filter("image provider", &texts)
		.into_iter()
		.map(|index| labels[index])
		.collect();

	assert_eq!(results[0], "Image Provider");
	assert!(results.contains(&"Block Images"));
	assert!(!results.contains(&"Include Model in Prompt"));
	assert!(!results.contains(&"Service Tier"));
}

#[test]
fn still_supports_short_word_initial_abbreviations() {
	let items = ["Ollama", "Kagi", "OpenCode Go", "Tavily"];
	assert_eq!(fuzzy_filter("og", &items), vec![2]);
}

#[test]
fn filters_cjk_queries_instead_of_treating_them_as_match_all() {
	let items = ["文件搜索", "搜索历史", "Settings"];
	let results: Vec<&str> = fuzzy_filter("搜索", &items)
		.into_iter()
		.map(|index| items[index])
		.collect();
	assert_eq!(results, ["搜索历史", "文件搜索"]);
	assert!(!fuzzy_match("搜索", "Settings").matches);
}