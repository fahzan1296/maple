//! Port of `l/test/web-search-render.test.ts`.
//!
//! The crate ports the search summary (`web_search::describe_search_result`) but
//! not the terminal `renderSearchResult` component that renders the answer as
//! markdown and caps it in compact mode, so the four rendering cases stay
//! ignored; each body exercises the summary seam such a port would reuse.

use maple_tui::tools::web_search::{describe_search_result, SearchRenderDetails};
use maple_tui::tools::web_search_types::{SearchResponse, SearchSource};

const ANSWER: &str = "\n## Overview Heading\nThis is the **first** paragraph with bold text.\n\nPara two line here.\nPara three line here.\nPara four line here.\nPara five line here.\nPara six line here.\nPara seven line here.\nPara eight line here.\nThe FINAL_UNIQUE_MARKER paragraph at the very end.";

fn details(answer: &str) -> SearchRenderDetails {
	SearchRenderDetails {
		response: Some(SearchResponse {
			answer: Some(answer.to_string()),
			sources: vec![
				SearchSource {
					title: "Src One".to_string(),
					url: "https://example.com/a".to_string(),
					snippet: Some("snip a".to_string()),
					..SearchSource::default()
				},
				SearchSource {
					title: "Src Two".to_string(),
					url: "https://example.com/b".to_string(),
					snippet: Some("snip b".to_string()),
					..SearchSource::default()
				},
			],
			..SearchResponse::default()
		}),
		error: None,
	}
}

#[test]
#[ignore = "missing facility: no renderSearchResult component rendering the answer as markdown"]
fn renders_the_answer_as_markdown() {
	let summary = describe_search_result(Some("test query"), Some(&details(ANSWER)), "");
	assert_eq!(summary.answer.as_deref(), Some(ANSWER.trim()));
}

#[test]
#[ignore = "missing facility: no expanded answer view without a truncation summary"]
fn shows_the_full_answer_when_expanded() {
	let summary = describe_search_result(Some("test query"), Some(&details(ANSWER)), "");
	assert!(summary.answer.expect("answer").contains("FINAL_UNIQUE_MARKER"));
}

#[test]
#[ignore = "missing facility: no collapsed answer view"]
fn shows_the_full_answer_when_collapsed_by_default() {
	let summary = describe_search_result(Some("test query"), Some(&details(ANSWER)), "");
	assert!(summary.answer.expect("answer").contains("FINAL_UNIQUE_MARKER"));
}

#[test]
#[ignore = "missing facility: no compact maxAnswerLines capping in the answer view"]
fn truncates_the_answer_only_when_compact_mode_provides_max_answer_lines() {
	let summary = describe_search_result(Some("test query"), Some(&details(ANSWER)), "");
	assert!(summary.answer.expect("answer").contains("FINAL_UNIQUE_MARKER"));
}
