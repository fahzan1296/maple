//! Web-search tool render-model types and plain/render helpers.
//!
//! Reference: `l/src/tools/web-search.ts` and `l/src/tools/web-search-types.ts`.
//!
//! Deviations:
//! - TS renders TUI components (Markdown/theme/tree/framed cards, hyperlinks,
//!   native describe nodes/memos, cached components). Rust ports data/helper
//!   logic, source row/head/result derivation, plain render-line helpers, and
//!   native-view describe builders for renderer-completion integration.
//! - Uses `crate::tools::web_search_types::{SearchResponse, get_search_provider_label}`.
//! - `format_age`/`get_domain`/`format_count` are local helpers.

use crate::native::describe::{compact, md, node};
use crate::native::node::{NodeKind, Props, Span, Text as NodeText};
use crate::native::spans::plain_text;
use crate::tools::native_view::{
	error_text, note_text, NativePreview, NativeToolHead, NativeToolView, ToolBadge,
};
use crate::tools::web_search_types::{get_search_provider_label, SearchProviderId, SearchResponse, SearchSource};

/// Title shown for the web search tool.
pub const SEARCH_TITLE: &str = "Web search";
/// Collapsed source items shown in TUI.
pub const MAX_COLLAPSED_ITEMS: usize = 8;

/// Search response and optional failure shown in the transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchRenderDetails {
	/// Search response.
	pub response: Option<SearchResponse>,
	/// Error message.
	pub error: Option<String>,
}

/// Call args preview fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchRenderArgs {
	/// Search query.
	pub query: Option<String>,
	/// Optional max answer lines (CLI compact mode in TS).
	pub max_answer_lines: Option<usize>,
}

/// Extract hostname/domain from a URL string, best-effort.
pub fn get_domain(url: &str) -> String {
	let s = url.trim();
	let without_scheme = s
		.strip_prefix("https://")
		.or_else(|| s.strip_prefix("http://"))
		.unwrap_or(s);
	let host = without_scheme.split(['/', '?', '#']).next().unwrap_or("");
	host.split(':').next().unwrap_or(host).to_string()
}

/// Format age seconds into compact text (`5m`, `2h`, `3d`, `1w`).
pub fn format_age(age_seconds: Option<f64>) -> String {
	let Some(secs) = age_seconds else {
		return String::new();
	};
	if secs < 0.0 || !secs.is_finite() {
		return String::new();
	}
	let s = secs as u64;
	if s < 60 {
		format!("{s}s ago")
	} else if s < 3600 {
		format!("{}m ago", s / 60)
	} else if s < 86_400 {
		format!("{}h ago", s / 3600)
	} else if s < 604_800 {
		format!("{}d ago", s / 86_400)
	} else {
		format!("{}w ago", s / 604_800)
	}
}

/// Format age for native/meta (no " ago" suffix, TS native strips it).
pub fn format_age_compact(age_seconds: Option<f64>) -> String {
	format_age(age_seconds).trim_end_matches(" ago").to_string()
}

/// Format count as `"N source"`/`"N sources"`.
pub fn format_count(kind: &str, n: usize) -> String {
	if n == 1 {
		format!("1 {kind}")
	} else {
		format!("{n} {kind}s")
	}
}

/// Truncate text to `max` chars with ellipsis when needed.
pub fn truncate_preview(text: &str, max: usize) -> String {
	let count = text.chars().count();
	if count <= max {
		text.to_string()
	} else if max <= 1 {
		"…".to_string()
	} else {
		let mut out: String = text.chars().take(max - 1).collect();
		out.push('…');
		out
	}
}

/// Head fields for search call/result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHead {
	pub title: String,
	pub target: Option<String>,
	pub target_kind: String,
	pub meta: Vec<String>,
	pub badge: Option<(String, Option<String>)>,
}

/// Build search head.
pub fn search_head(query: Option<&str>, meta: Option<&str>, badge: Option<(String, Option<String>)>) -> SearchHead {
	SearchHead {
		title: SEARCH_TITLE.to_string(),
		target: query.filter(|s| !s.is_empty()).map(|s| s.to_string()),
		target_kind: "query".to_string(),
		meta: meta.map(|m| vec![m.to_string()]).unwrap_or_default(),
		badge,
	}
}

/// One cited source row summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSourceRow {
	pub initial: String,
	pub title: String,
	pub url: String,
	pub domain: String,
	pub age: String,
	pub meta: String,
}

/// Derive a source row summary.
pub fn source_row(
	src_title: Option<&str>,
	src_url: Option<&str>,
	age_seconds: Option<f64>,
	published_date: Option<&str>,
) -> SearchSourceRow {
	let url = src_url.unwrap_or("").to_string();
	let title = src_title
		.filter(|s| !s.trim().is_empty())
		.map(|s| s.to_string())
		.or_else(|| {
			if url.trim().is_empty() { None } else { Some(url.clone()) }
		})
		.unwrap_or_else(|| "Untitled".to_string());
	let domain = get_domain(&url);
	let age = {
		let a = format_age_compact(age_seconds);
		if a.is_empty() {
			published_date.unwrap_or("").to_string()
		} else {
			a
		}
	};
	let meta_parts: Vec<&str> = [domain.as_str(), age.as_str()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect();
	let meta = meta_parts.join(" · ");
	let initial_domain = domain.strip_prefix("www.").unwrap_or(&domain);
	let initial = initial_domain
		.chars()
		.next()
		.unwrap_or('?')
		.to_ascii_uppercase()
		.to_string();
	SearchSourceRow { initial, title, url, domain, age, meta }
}

/// Plain description of a search result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResultSummary {
	pub head: SearchHead,
	pub tone: Option<String>,
	pub error: Option<String>,
	pub answer: Option<String>,
	pub sources: Vec<SearchSourceRow>,
	pub provider_info: Option<String>,
}

/// Build a search result summary from details/args/raw text.
pub fn describe_search_result(
	arg_query: Option<&str>,
	details: Option<&SearchRenderDetails>,
	raw_text: &str,
) -> SearchResultSummary {
	if let Some(err) = details.and_then(|d| d.error.as_deref()) {
		let provider = details
			.and_then(|d| d.response.as_ref())
			.and_then(|r| r.provider.clone());
		let label = match provider.as_ref() {
			None | Some(SearchProviderId::Unknown) => None,
			Some(p) => Some(get_search_provider_label(p).to_string()),
		};
		return SearchResultSummary {
			head: search_head(arg_query, label.as_deref(), None),
			tone: Some("error".to_string()),
			error: Some(if err.trim().is_empty() { "Web search failed".to_string() } else { err.trim().to_string() }),
			answer: None,
			sources: Vec::new(),
			provider_info: None,
		};
	}

	let Some(response) = details.and_then(|d| d.response.as_ref()) else {
		return SearchResultSummary {
			head: search_head(arg_query, None, None),
			tone: Some("warning".to_string()),
			error: None,
			answer: Some(if raw_text.trim().is_empty() { "No response data".to_string() } else { raw_text.trim().to_string() }),
			sources: Vec::new(),
			provider_info: None,
		};
	};

	let source_rows: Vec<SearchSourceRow> = response
		.sources
		.iter()
		.map(|s| {
			source_row(
				Some(s.title.as_str()),
				Some(s.url.as_str()),
				s.age_seconds.map(|age| age as f64),
				s.published_date.as_deref(),
			)
		})
		.collect();
	let provider_label = match response.provider.as_ref() {
		None | Some(SearchProviderId::Unknown) => "None".to_string(),
		Some(p) => get_search_provider_label(p).to_string(),
	};
	let query = arg_query.map(|s| s.to_string()).or_else(|| {
		response
			.search_queries
			.as_ref()
			.and_then(|queries| queries.first())
			.cloned()
	});
	let answer = response
		.answer
		.as_deref()
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.or_else(|| {
			let t = raw_text.trim();
			if t.is_empty() { None } else { Some(t.to_string()) }
		});

	let auth_short = match response.auth_mode.as_deref() {
		Some("oauth") => Some("OAuth"),
		Some("api_key") => Some("API"),
		Some(other) => Some(other),
		None => None,
	};
	let mut provider_info = if let Some(model) = &response.model {
		format!("{model} @ {provider_label}")
	} else {
		provider_label.clone()
	};
	if let Some(auth) = auth_short {
		provider_info.push_str(&format!(" ({auth})"));
	}
	let usage_parts = response.usage.as_ref().map(|u| {
		let mut parts = Vec::new();
		if let Some(v) = u.input_tokens { parts.push(format!("in {v}")); }
		if let Some(v) = u.output_tokens { parts.push(format!("out {v}")); }
		if let Some(v) = u.total_tokens { parts.push(format!("total {v}")); }
		if let Some(v) = u.search_requests { parts.push(format!("search {v}")); }
		parts
	}).unwrap_or_default();
	let tooltip = [provider_info.clone(), usage_parts.join(" · ")]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join("\n");
	let badge = response
		.model
		.as_ref()
		.map(|m| (m.clone(), if tooltip.is_empty() { None } else { Some(tooltip.clone()) }));

	SearchResultSummary {
		head: search_head(
			query.as_deref(),
			Some(&format!("{provider_label} · {}", format_count("source", source_rows.len()))),
			badge,
		),
		tone: if source_rows.is_empty() { Some("warning".to_string()) } else { None },
		error: None,
		answer,
		sources: source_rows,
		provider_info: Some(provider_info),
	}
}

fn search_native_head(head: &SearchHead) -> NativeToolHead {
	let mut meta_spans = Vec::new();
	for m in &head.meta {
		meta_spans.push(NodeText::Plain(m.clone()));
	}
	let badges = head.badge.as_ref().map(|(text, title)| {
		vec![ToolBadge { text: text.clone(), tone: "neutral".to_string(), title: title.clone() }]
	});
	NativeToolHead {
		title: Some(NodeText::Plain(head.title.clone())),
		target: head.target.as_ref().map(|t| NodeText::Plain(t.clone())),
		target_kind: Some(head.target_kind.clone()),
		meta: if meta_spans.is_empty() { None } else { Some(meta_spans) },
		badges,
		..NativeToolHead::default()
	}
}

/// Native describe for a web search call.
pub fn describe_search_call(query: Option<&str>) -> NativeToolView {
	let head = search_head(query, None, None);
	NativeToolView {
		tool: Some(search_native_head(&head)),
		inline: Some(true),
		..NativeToolView::default()
	}
}

fn source_node(row: &SearchSourceRow, index: usize) -> crate::native::node::NativeNode {
	let mut text_props = Props::new();
	text_props.set_i64("lines", 1);
	text_props.set_str("truncate", "end");
	let mut title_span = Span::styled(row.title.clone(), "link");
	if !row.url.is_empty() {
		title_span.href = Some(row.url.clone());
	}
	let mut children = Vec::new();
	let mut badge_props = Props::new();
	badge_props.set_str("text", &row.initial);
	if !row.domain.is_empty() {
		badge_props.set_str("title", &row.domain);
	}
	children.push(node(NodeKind::Badge, badge_props, Vec::new(), None));
	children.push(crate::native::describe::text(NodeText::Spans(vec![title_span]), text_props));
	if !row.meta.is_empty() {
		let mut meta_props = Props::new();
		meta_props.set_i64("lines", 1);
		children.push(crate::native::describe::text(
			NodeText::Spans(vec![Span::styled(row.meta.clone(), "muted")]),
			meta_props,
		));
	}
	let mut row_props = Props::new();
	row_props.set_str("gap", "sm");
	row_props.set_str("align", "baseline");
	row_props.set_str("role", "omp.tool.source");
	if !row.url.is_empty() {
		row_props.set_str("href", &row.url);
	}
	let key = format!("{}:{}", index, row.url);
	crate::native::describe::keyed(
		node(NodeKind::Row, row_props, children, None),
		key,
	)
}

/// Native describe for a web search result.
pub fn describe_search_result_native(
	arg_query: Option<&str>,
	details: Option<&SearchRenderDetails>,
	raw_text: &str,
) -> NativeToolView {
	let summary = describe_search_result(arg_query, details, raw_text);
	if summary.tone.as_deref() == Some("error") {
		return NativeToolView {
			tool: Some(search_native_head(&summary.head)),
			tone: Some(crate::native::node::Tone::Error),
			body: Some(vec![error_text(summary.error.as_deref().unwrap_or("Web search failed"))]),
			..NativeToolView::default()
		};
	}
	let mut body: Vec<crate::native::node::NativeNode> = Vec::new();
	if let Some(answer) = summary.answer.as_deref().filter(|s| !s.trim().is_empty()) {
		let mut p = Props::new();
		p.set_str("title", summary.provider_info.as_deref().unwrap_or(""));
		let mut n = md(answer, p);
		n.key = Some("answer".to_string());
		body.push(n);
	} else {
		body.push(note_text("No answer text returned", "muted", None));
	}
	if summary.sources.is_empty() {
		body.push(note_text("No sources returned", "muted", None));
	} else {
		let mut p = Props::new();
		p.set_str("role", "omp.tool.files");
		let src_nodes = summary.sources.iter().enumerate().map(|(i, s)| source_node(s, i)).collect();
		body.push(node(NodeKind::Col, p, src_nodes, Some("sources")));
	}
	NativeToolView {
		tool: Some(search_native_head(&summary.head)),
		tone: match summary.tone.as_deref() {
			Some("warning") => Some(crate::native::node::Tone::Warning),
			_ => None,
		},
		body: Some(body),
		..NativeToolView::default()
	}
}

/// Plain render-call line for search (no theme).
pub fn render_search_call_line(query: Option<&str>) -> String {
	let q = truncate_preview(query.unwrap_or(""), 80);
	if q.is_empty() { "Web Search".to_string() } else { format!("Web Search: {q}") }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tools::web_search_types::SearchSource;

	#[test]
	fn domain_and_age() {
		assert_eq!(get_domain("https://www.example.com/a?q=1"), "www.example.com");
		assert_eq!(format_age_compact(Some(90.0)), "1m");
		assert_eq!(format_age_compact(Some(7200.0)), "2h");
	}

	#[test]
	fn source_row_meta() {
		let r = source_row(Some("Title"), Some("https://example.com/x"), Some(3600.0), None);
		assert_eq!(r.initial, "E");
		assert_eq!(r.title, "Title");
		assert!(r.meta.contains("example.com"));
		assert!(r.meta.contains("1h"));
	}

	#[test]
	fn search_head_basic() {
		let h = search_head(Some("rust tui"), Some("Perplexity · 2 sources"), None);
		assert_eq!(h.title, SEARCH_TITLE);
		assert_eq!(h.target.as_deref(), Some("rust tui"));
		assert_eq!(h.target_kind, "query");
	}

	#[test]
	fn describe_result_no_response_warning() {
		let s = describe_search_result(Some("q"), None, "");
		assert_eq!(s.tone.as_deref(), Some("warning"));
		assert_eq!(s.answer.as_deref(), Some("No response data"));
	}

	#[test]
	fn native_call_and_result() {
		let call = describe_search_call(Some("rust"));
		assert_eq!(call.inline, Some(true));
		assert!(call.tool.is_some());
		let view = describe_search_result_native(Some("q"), None, "");
		assert_eq!(view.tone, Some(crate::native::node::Tone::Warning));
		assert!(view.body.is_some());
	}
}
