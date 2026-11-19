//! Find (semantic grep) tool render-model types and helpers.
//!
//! Reference: `l/src/tools/find.ts`.
//!
//! Deviations:
//! - [`render_call`]/[`render_result`] emit plain text rows (the reference draws
//!   score gauges, highlighted snippets and tree rows); [`describe_call`]/
//!   [`describe_result`] port the native-view contract.
//! - Snippet syntax highlighting and URL-scope versus file-scope link targets
//!   are host concerns; file links use [`crate::render::hyperlink::try_file_uri`].

use crate::native::describe::{code, node, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props, Span, Tone};
use crate::render::render_utils::PREVIEW_LIMITS;
use crate::tools::native_view::{
	inline_error_view, NativePreview, NativeToolHead, NativeToolView,
};

/// A verified line range with its yes-probability and a one-line preview.
#[derive(Clone, Debug, PartialEq)]
pub struct FindRange {
	pub start: u32,
	pub end: u32,
	pub p: f64,
	pub snippet: String,
}

/// A file whose verified passages cleared the threshold.
#[derive(Clone, Debug, PartialEq)]
pub struct FindHit {
	/// Display path relative to cwd, or internal URL under URL scopes.
	pub rel: String,
	/// Filename judgment.
	pub name_score: Option<f64>,
	/// Best verified passage probability.
	pub content_score: f64,
	/// Merged positive spans, strongest first.
	pub ranges: Vec<FindRange>,
	/// Lines of content actually judged.
	pub lines_seen: u32,
	/// File held more content.
	pub truncated: bool,
}

/// Search coverage classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindCoverage {
	Complete,
	Partial,
	Unavailable,
}

impl FindCoverage {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Complete => "complete",
			Self::Partial => "partial",
			Self::Unavailable => "unavailable",
		}
	}
}

/// Verification cut short by call deadline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindTimeout {
	pub verified: u32,
	pub passages: u32,
}

/// Search accounting reported alongside hits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindStats {
	pub listed: u32,
	pub requests: u32,
	pub failed: u32,
	pub unanswered: u32,
	pub judged: u32,
	pub files_read: u32,
	pub file_bytes: u64,
	pub input_tokens: u64,
	pub output_tokens: u64,
	pub cost: f64,
	pub api_ms: u64,
	pub windows_judged: u32,
	pub windows_pruned: u32,
	pub map_cards: u32,
	pub failures: Vec<String>,
}

/// Display metadata for find tool results.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindToolDetails {
	pub query: String,
	pub keywords: Vec<String>,
	pub threshold: f64,
	pub hits: Vec<FindHit>,
	pub coverage: Option<FindCoverage>,
	pub stats: Option<FindStats>,
	pub timeout: Option<FindTimeout>,
	pub elapsed_ms: u64,
	pub cwd: String,
	pub scope_path: Option<String>,
}

/// Find render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindRenderArgs {
	pub query: Option<String>,
	pub grep_keywords: Vec<String>,
	pub path: Option<String>,
}

/// Score thresholds.
pub const STRONG: f64 = 0.7;
pub const PLAUSIBLE: f64 = 0.4;
/// Cells in per-hit score gauge.
pub const GAUGE_WIDTH: usize = 6;
/// Hits shown before expansion.
pub const COLLAPSED_HITS: usize = 5;
/// Ranges shown per hit.
pub const RANGES_COLLAPSED: usize = 1;
pub const RANGES_EXPANDED: usize = 3;

/// Score tone token.
pub fn score_tone(p: f64) -> &'static str {
	if p >= STRONG {
		"success"
	} else if p >= PLAUSIBLE {
		"warning"
	} else {
		"muted"
	}
}

/// Format a range span label (`start` or `start-end`).
pub fn format_range_span(start: u32, end: u32) -> String {
	if start == end {
		start.to_string()
	} else {
		format!("{start}-{end}")
	}
}

/// Sort ranges strongest-first (desc p, asc start) and take limit.
pub fn top_ranges(ranges: &[FindRange], limit: usize) -> Vec<FindRange> {
	let mut v = ranges.to_vec();
	v.sort_by(|a, b| b.p.partial_cmp(&a.p).unwrap_or(std::cmp::Ordering::Equal).then(a.start.cmp(&b.start)));
	v.truncate(limit);
	v
}

/// Plain hit summary line parts.
pub fn summarize_hit(hit: &FindHit) -> String {
	let coverage = if hit.truncated {
		format!("{} lines judged, partial", hit.lines_seen)
	} else {
		format!("{} lines judged", hit.lines_seen)
	};
	format!("{:.2} {} {}", hit.content_score, hit.rel, coverage)
}

/// Native find head: the query, then result counts or the call's keywords and
/// scope.
pub fn find_native_head(query: Option<&str>, meta: &[String]) -> NativeToolHead {
	NativeToolHead {
		title: Some(NodeText::Plain("Find".to_string())),
		target: Some(NodeText::Plain(query.unwrap_or("").to_string())),
		target_kind: Some("query".to_string()),
		meta: if meta.is_empty() {
			None
		} else {
			Some(meta.iter().cloned().map(NodeText::Plain).collect())
		},
		..NativeToolHead::default()
	}
}

/// `"query"`, quoted for the head description.
pub fn quote_query(query: Option<&str>) -> Option<String> {
	query.map(|query| format!("\"{query}\""))
}

/// One hit: a row of the score bar, the path and `N lines judged`, then its
/// strongest ranges as numbered `code`.
fn describe_hit(hit: &FindHit, range_limit: usize, cwd: Option<&str>, raw_rel: &str) -> NativeNode {
	let ranges = top_ranges(&hit.ranges, range_limit);
	let tone = match score_tone(hit.content_score) {
		"success" => Tone::Success,
		"warning" => Tone::Warning,
		_ => Tone::Muted,
	};
	let score = format!("{:.2}", hit.content_score);
	let mut progress_props = Props::new();
	progress_props.set_f64("value", hit.content_score);
	progress_props.set_tone(tone);
	progress_props.set_str("title", format!("score {score}"));
	let mut head_children = vec![node(NodeKind::Progress, progress_props, Vec::new(), None)];
	let mut path_props = Props::new();
	path_props.set_i64("lines", 1);
	let mut path_span = Span::styled(hit.rel.clone(), "strong");
	if let Some(cwd) = cwd {
		path_span.href = crate::render::hyperlink::try_file_uri(
			&format!("{}/{}", cwd.trim_end_matches('/'), raw_rel),
			crate::render::hyperlink::FilePosition::default(),
			false,
		);
	}
	head_children.push(text(NodeText::Spans(vec![path_span]), path_props));
	let coverage = if hit.truncated {
		format!(" · {} lines judged, partial", hit.lines_seen)
	} else {
		format!(" · {} lines judged", hit.lines_seen)
	};
	let mut detail_props = Props::new();
	detail_props.set_str("wrap", "word");
	head_children.push(text(
		NodeText::Spans(vec![
			span(score, Some(score_tone(hit.content_score))),
			span(coverage, Some("muted")),
		]),
		detail_props,
	));
	let mut row_props = Props::new();
	row_props.set_str("gap", "sm");
	row_props.set_str("align", "baseline");
	let head = node(NodeKind::Row, row_props, head_children, Some("hit"));
	let mut children = vec![head];
	for range in &ranges {
		let mut snippet_props = Props::new();
		snippet_props.set_str("path", hit.rel.clone());
		snippet_props.set_i64("start", range.start as i64);
		snippet_props.set_bool("numbers", true);
		snippet_props.set_str("title", format!("{:.2}", range.p));
		let mut snippet = code(range.snippet.trim_end().to_string(), snippet_props);
		snippet.key = Some(format!("{}-{}", range.start, range.end));
		children.push(snippet);
	}
	let mut col_props = Props::new();
	col_props.set_str("gap", "xs");
	col_props.set_str("role", "omp.tool.find.hit");
	let mut group = node(NodeKind::Col, col_props, children, Some(&hit.rel));
	group.key = Some(hit.rel.clone());
	group
}

/// Search accounting line: files read, threshold, tokens, cost and elapsed.
pub fn find_accounting(details: &FindToolDetails) -> String {
	let stats = details.stats.clone().unwrap_or_default();
	format!(
		"{} files read · τ {:.2} · {} tokens · ${:.4} · {:.1}s",
		stats.files_read,
		details.threshold,
		stats.input_tokens,
		stats.cost,
		details.elapsed_ms as f64 / 1000.0
	)
}

/// Native describe of a find call.
pub fn describe_call(args: Option<&FindRenderArgs>) -> NativeToolView {
	let mut meta = Vec::new();
	if let Some(keywords) = args.map(|args| args.grep_keywords.as_slice()).filter(|k| !k.is_empty()) {
		meta.push(keywords.join(" "));
	}
	if let Some(path) = args.and_then(|args| args.path.as_deref()).filter(|path| !path.is_empty()) {
		meta.push(format!("in {path}"));
	}
	NativeToolView {
		tool: Some(find_native_head(args.and_then(|args| args.query.as_deref()), &meta)),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native describe of a find result: one group per hit, then the keywords
/// footnote.
pub fn describe_result(
	progress_text: &str,
	args: Option<&FindRenderArgs>,
	details: Option<&FindToolDetails>,
	is_error: bool,
	is_partial: bool,
	expanded: bool,
) -> NativeToolView {
	let query = args
		.and_then(|args| args.query.clone())
		.or_else(|| details.map(|details| details.query.clone()));
	if is_error {
		return inline_error_view(
			find_native_head(query.as_deref(), &[]),
			if progress_text.trim().is_empty() { "Unknown error" } else { progress_text },
		);
	}
	if is_partial || details.is_none() {
		let meta: Vec<String> = if progress_text.trim().is_empty() {
			Vec::new()
		} else {
			vec![progress_text.to_string()]
		};
		return NativeToolView {
			tool: Some(find_native_head(query.as_deref(), &meta)),
			inline: Some(true),
			..NativeToolView::default()
		};
	}
	let details = details.unwrap();
	let stats = details.stats.clone().unwrap_or_default();
	let scope = details.scope_path.clone().map(|scope| format!("in {scope}"));
	let failed = (stats.failed > 0).then(|| format!("{}/{} requests failed", stats.failed, stats.requests));
	let timed_out = details
		.timeout
		.as_ref()
		.map(|timeout| format!("timed out, {}/{} passages verified", timeout.verified, timeout.passages));
	let failures: Vec<NativeNode> = stats
		.failures
		.iter()
		.enumerate()
		.map(|(index, failure)| {
			let mut props = Props::new();
			props.set_str("wrap", "word");
			props.set_str("role", "omp.tool.notice");
			let mut node = text(NodeText::Spans(vec![span(failure.clone(), Some("warning"))]), props);
			node.key = Some(format!("fail{index}"));
			node
		})
		.collect();
	if details.hits.is_empty() {
		let mut meta: Vec<String> = vec!["0 hits".to_string()];
		meta.extend(scope);
		meta.extend(failed);
		return NativeToolView {
			tool: Some(find_native_head(query.as_deref(), &meta)),
			tone: Some(Tone::Warning),
			inline: Some(true),
			body: if failures.is_empty() { None } else { Some(failures) },
			..NativeToolView::default()
		};
	}
	let shown: Vec<&FindHit> = if expanded {
		details.hits.iter().collect()
	} else {
		details.hits.iter().take(COLLAPSED_HITS).collect()
	};
	let range_limit = if expanded { RANGES_EXPANDED } else { RANGES_COLLAPSED };
	let hidden = details.hits.len() - shown.len();
	let mut foot_parts = Vec::new();
	if hidden > 0 {
		foot_parts.push(format!("{} more hits", hidden));
	}
	if !details.keywords.is_empty() {
		foot_parts.push(format!("keywords: {}", details.keywords.join(", ")));
	}
	let mut footer_props = Props::new();
	footer_props.set_str("wrap", "word");
	footer_props.set_str("role", "omp.tool.stats");
	footer_props.set_str("title", find_accounting(details));
	let mut footer = text(
		NodeText::Spans(vec![span(foot_parts.join(" · "), Some("muted"))]),
		footer_props,
	);
	footer.key = Some("foot".to_string());
	let mut body: Vec<NativeNode> = shown
		.iter()
		.map(|hit| describe_hit(hit, range_limit, Some(details.cwd.as_str()), &hit.rel))
		.collect();
	body.extend(failures);
	if !foot_parts.is_empty() {
		body.push(footer);
	}
	let mut meta = vec![format!("{} · {} files", format_hits(details.hits.len()), stats.files_read)];
	meta.extend(scope);
	meta.extend(failed);
	meta.extend(timed_out);
	NativeToolView {
		tool: Some(find_native_head(query.as_deref(), &meta)),
		inline: Some(true),
		body: Some(body),
		preview: Some(NativePreview::Lines(PREVIEW_LIMITS.EXPANDED_LINES)),
		..NativeToolView::default()
	}
}

/// `N hit`/`N hits`.
fn format_hits(count: usize) -> String {
	if count == 1 { "1 hit".to_string() } else { format!("{count} hits") }
}

/// Plain render-call line for find.
pub fn render_call(args: Option<&FindRenderArgs>) -> String {
	let query = args.and_then(|args| args.query.clone()).unwrap_or_default();
	let mut line = format!("Find \"{query}\"");
	if let Some(keywords) = args.map(|args| args.grep_keywords.as_slice()).filter(|k| !k.is_empty()) {
		line.push_str(&format!(" {}", keywords.join(" ")));
	}
	if let Some(path) = args.and_then(|args| args.path.as_deref()).filter(|path| !path.is_empty()) {
		line.push_str(&format!(" in {path}"));
	}
	line
}

/// Plain render of a find result.
pub fn render_result(
	progress_text: &str,
	details: Option<&FindToolDetails>,
	is_error: bool,
	is_partial: bool,
	expanded: bool,
) -> Vec<String> {
	if is_error {
		return vec![format!(
			"Find failed: {}",
			if progress_text.trim().is_empty() { "Unknown error" } else { progress_text.trim() }
		)];
	}
	let Some(details) = details.filter(|_| !is_partial) else {
		return vec![progress_text.trim().to_string()];
	};
	let stats = details.stats.clone().unwrap_or_default();
	if details.hits.is_empty() {
		let mut lines = vec![format!("0 hits · ${:.4} · {:.1}s", stats.cost, details.elapsed_ms as f64 / 1000.0)];
		if stats.failed > 0 {
			lines.push(format!("{}/{} requests failed", stats.failed, stats.requests));
		}
		lines.extend(stats.failures.iter().cloned());
		return lines;
	}
	let mut lines = vec![
		format!("{} · {} files read", format_hits(details.hits.len()), stats.files_read),
		find_accounting(details),
	];
	let range_limit = if expanded { RANGES_EXPANDED } else { RANGES_COLLAPSED };
	let shown = if expanded { details.hits.len() } else { details.hits.len().min(COLLAPSED_HITS) };
	for hit in details.hits.iter().take(shown) {
		lines.push(summarize_hit(hit));
		for range in top_ranges(&hit.ranges, range_limit) {
			lines.push(format!(
				"  :{} {:.2} {}",
				format_range_span(range.start, range.end),
				range.p,
				range.snippet.trim()
			));
		}
	}
	if details.hits.len() > shown {
		lines.push(format!("{} more hits", details.hits.len() - shown));
	}
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tones_and_ranges() {
		assert_eq!(score_tone(0.8), "success");
		assert_eq!(score_tone(0.5), "warning");
		assert_eq!(score_tone(0.2), "muted");
		assert_eq!(format_range_span(3, 3), "3");
		assert_eq!(format_range_span(1, 4), "1-4");
	}

	#[test]
	fn top_ranges_order() {
		let rs = vec![
			FindRange { start: 10, end: 10, p: 0.4, snippet: "b".into() },
			FindRange { start: 2, end: 3, p: 0.9, snippet: "a".into() },
		];
		let top = top_ranges(&rs, 1);
		assert_eq!(top[0].start, 2);
	}

	#[test]
	fn coverage_str() {
		assert_eq!(FindCoverage::Partial.as_str(), "partial");
	}

	fn details() -> FindToolDetails {
		FindToolDetails {
			query: "auth flow".to_string(),
			keywords: vec!["auth".to_string()],
			threshold: 0.5,
			hits: vec![FindHit {
				rel: "src/auth.rs".to_string(),
				name_score: None,
				content_score: 0.82,
				ranges: vec![FindRange { start: 4, end: 6, p: 0.8, snippet: "fn auth()".to_string() }],
				lines_seen: 40,
				truncated: false,
			}],
			coverage: Some(FindCoverage::Complete),
			stats: Some(FindStats { files_read: 8, requests: 2, cost: 0.01, ..FindStats::default() }),
			timeout: None,
			elapsed_ms: 1200,
			cwd: "/repo".to_string(),
			scope_path: None,
		}
	}

	#[test]
	fn describe_call_quotes_query_meta() {
		let args = FindRenderArgs {
			query: Some("auth flow".to_string()),
			grep_keywords: vec!["auth".to_string()],
			path: Some("src".to_string()),
		};
		let view = describe_call(Some(&args));
		let head = view.tool.expect("head");
		assert_eq!(head.target_kind.as_deref(), Some("query"));
		assert_eq!(head.meta.expect("meta").len(), 2);
		assert_eq!(quote_query(Some("x")).as_deref(), Some("\"x\""));
	}

	#[test]
	fn describe_result_lists_hits_and_foot() {
		let details = details();
		let view = describe_result("", None, Some(&details), false, false, false);
		assert_eq!(view.body.expect("body").len(), 2);
	}

	#[test]
	fn render_result_plain_hit_rows() {
		let details = details();
		let rows = render_result("", Some(&details), false, false, false);
		assert!(rows[0].contains("1 hit"));
		assert!(rows.iter().any(|row| row.contains("src/auth.rs")));
		assert!(rows.iter().any(|row| row.contains("0.80")));
		assert!(render_result("", None, true, false, false)[0].starts_with("Find failed"));
	}

	#[test]
	fn accounting_mentions_tokens_and_cost() {
		let line = find_accounting(&details());
		assert!(line.contains("8 files read"));
		assert!(line.contains("$0.0100"));
	}
}
