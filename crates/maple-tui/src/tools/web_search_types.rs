//! Web search response types shared by providers and renderers.
//!
//! Reference: `l/src/tools/web-search-types.ts`.

use serde::{Deserialize, Serialize};

/// Id of a search implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchProviderId {
	Parallel,
	Perplexity,
	Gemini,
	Anthropic,
	Codex,
	Openai,
	Xai,
	Openrouter,
	Zai,
	Exa,
	Tinyfish,
	Keenable,
	Sofya,
	Jina,
	Kagi,
	Tavily,
	Firecrawl,
	Brave,
	Deepseek,
	Kimi,
	Synthetic,
	Ollama,
	Searxng,
	Startpage,
	Duckduckgo,
	Ecosia,
	Google,
	Mojeek,
	Public,
	Hackernews,
	/// Fallback/unknown id preserved as raw string.
	#[serde(other)]
	Unknown,
}

/// Label for a provider id; unknown ids are not representable as enum variants
/// with payload here, so callers should preserve raw ids separately when needed.
/// This helper covers known ids.
pub fn get_search_provider_label(id: &SearchProviderId) -> &'static str {
	match id {
		SearchProviderId::Parallel => "Parallel",
		SearchProviderId::Perplexity => "Perplexity",
		SearchProviderId::Gemini => "Gemini",
		SearchProviderId::Anthropic => "Anthropic",
		SearchProviderId::Codex => "OpenAI Codex",
		SearchProviderId::Openai => "OpenAI API",
		SearchProviderId::Xai => "xAI",
		SearchProviderId::Openrouter => "OpenRouter",
		SearchProviderId::Zai => "Z.AI",
		SearchProviderId::Exa => "Exa",
		SearchProviderId::Tinyfish => "TinyFish",
		SearchProviderId::Keenable => "Keenable",
		SearchProviderId::Sofya => "Sofya",
		SearchProviderId::Jina => "Jina",
		SearchProviderId::Kagi => "Kagi",
		SearchProviderId::Tavily => "Tavily",
		SearchProviderId::Firecrawl => "Firecrawl",
		SearchProviderId::Brave => "Brave",
		SearchProviderId::Deepseek => "DeepSeek",
		SearchProviderId::Kimi => "Kimi",
		SearchProviderId::Synthetic => "Synthetic",
		SearchProviderId::Ollama => "Ollama",
		SearchProviderId::Searxng => "SearXNG",
		SearchProviderId::Startpage => "Startpage",
		SearchProviderId::Duckduckgo => "DuckDuckGo",
		SearchProviderId::Ecosia => "Ecosia",
		SearchProviderId::Google => "Google",
		SearchProviderId::Mojeek => "Mojeek",
		SearchProviderId::Public => "Public Web",
		SearchProviderId::Hackernews => "Hacker News",
		SearchProviderId::Unknown => "Unknown",
	}
}

/// Source returned by search.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSource {
	/// Title.
	pub title: String,
	/// URL.
	pub url: String,
	/// Snippet.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub snippet: Option<String>,
	/// ISO date string or relative.
	#[serde(rename = "publishedDate", skip_serializing_if = "Option::is_none")]
	pub published_date: Option<String>,
	/// Age in seconds.
	#[serde(rename = "ageSeconds", skip_serializing_if = "Option::is_none")]
	pub age_seconds: Option<u64>,
	/// Author.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub author: Option<String>,
}

/// Citation with text reference.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCitation {
	/// URL.
	pub url: String,
	/// Title.
	pub title: String,
	/// Cited text.
	#[serde(rename = "citedText", skip_serializing_if = "Option::is_none")]
	pub cited_text: Option<String>,
}

/// Usage metrics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchUsage {
	/// Input tokens.
	#[serde(rename = "inputTokens", skip_serializing_if = "Option::is_none")]
	pub input_tokens: Option<u64>,
	/// Output tokens.
	#[serde(rename = "outputTokens", skip_serializing_if = "Option::is_none")]
	pub output_tokens: Option<u64>,
	/// Number of web search requests made.
	#[serde(rename = "searchRequests", skip_serializing_if = "Option::is_none")]
	pub search_requests: Option<u64>,
	/// Combined token count.
	#[serde(rename = "totalTokens", skip_serializing_if = "Option::is_none")]
	pub total_tokens: Option<u64>,
}

/// Unified response across providers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
	/// Provider id or "none" in TS; here `None` variant represented separately.
	#[serde(default)]
	pub provider: Option<SearchProviderId>,
	/// Synthesized answer text.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub answer: Option<String>,
	/// Search result sources.
	#[serde(default)]
	pub sources: Vec<SearchSource>,
	/// Text citations with context.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub citations: Option<Vec<SearchCitation>>,
	/// Intermediate search queries.
	#[serde(rename = "searchQueries", skip_serializing_if = "Option::is_none")]
	pub search_queries: Option<Vec<String>>,
	/// Follow-up question suggestions.
	#[serde(rename = "relatedQuestions", skip_serializing_if = "Option::is_none")]
	pub related_questions: Option<Vec<String>>,
	/// Token usage metrics.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub usage: Option<SearchUsage>,
	/// Model used.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub model: Option<String>,
	/// Request ID for debugging.
	#[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
	pub request_id: Option<String>,
	/// Authentication mode used.
	#[serde(rename = "authMode", skip_serializing_if = "Option::is_none")]
	pub auth_mode: Option<String>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn labels_known() {
		assert_eq!(
			get_search_provider_label(&SearchProviderId::Hackernews),
			"Hacker News"
		);
		assert_eq!(
			get_search_provider_label(&SearchProviderId::Openai),
			"OpenAI API"
		);
	}

	#[test]
	fn response_roundtrip_minimal() {
		let r = SearchResponse {
			provider: None,
			sources: vec![SearchSource {
				title: "t".into(),
				url: "u".into(),
				..Default::default()
			}],
			..Default::default()
		};
		let j = serde_json::to_string(&r).unwrap();
		let back: SearchResponse = serde_json::from_str(&j).unwrap();
		assert_eq!(back.sources.len(), 1);
	}
}
