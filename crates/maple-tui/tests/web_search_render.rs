//! Port of `l/test/web-search-render.test.ts`.
//!
//! The reference renders markdown/theme components. This crate renders the same
//! framed plain rows through `render_search_result`: answer section, optional
//! compact cap and cited sources.

use maple_tui::tools::web_search::{render_search_result, SearchRenderDetails};
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

fn render(expanded: bool, max_answer_lines: Option<usize>) -> Vec<String> {
	render_search_result(Some("test query"), Some(&details(ANSWER)), "", expanded, max_answer_lines)
}

fn answer_section(lines: &[String]) -> String {
	let start = lines.iter().position(|line| line == " Answer ").expect("answer section");
	let end = lines
		.iter()
		.position(|line| line == " Sources ")
		.expect("sources section");
	assert!(end > start);
	lines[start + 1..end].join("\n").trim().to_string()
}

#[test]
fn renders_the_answer_as_markdown() {
	let lines = render(true, None);
	let answer = answer_section(&lines);
	assert!(!answer.contains("##"), "{answer}");
	assert!(!answer.contains("**"), "{answer}");
	assert!(answer.to_lowercase().contains("overview heading"), "{answer}");
	assert!(answer.contains("first"), "{answer}");
	assert!(lines.iter().any(|line| line == " Sources "));
	assert!(lines.iter().any(|line| line.contains("Src One") && line.contains("example.com/a")));
}

#[test]
fn shows_the_full_answer_when_expanded() {
	let answer = answer_section(&render(true, None));
	assert!(answer.contains("FINAL_UNIQUE_MARKER"), "{answer}");
	assert!(!answer.contains("more line"), "{answer}");
}

#[test]
fn shows_the_full_answer_when_collapsed_by_default() {
	let answer = answer_section(&render(false, None));
	assert!(answer.contains("FINAL_UNIQUE_MARKER"), "{answer}");
	assert!(!answer.contains("more line"), "{answer}");
}

#[test]
fn truncates_the_answer_only_when_compact_mode_provides_max_answer_lines() {
	let answer = answer_section(&render(false, Some(3)));
	assert!(answer.contains("more line"), "{answer}");
	assert!(!answer.contains("FINAL_UNIQUE_MARKER"), "{answer}");
	assert!(answer.starts_with("Overview Heading"), "{answer}");
	assert!(answer.contains("This is the first paragraph with bold text."), "{answer}");
}
