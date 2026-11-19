//! Port of `l/test/find-render.test.ts`.
//!
//! The crate ports the find render model plus the native describe view; themed
//! terminal rows are not ported. URL-scope (`omp://`) hit linking and per-range
//! terminal links are host concerns, so those stay ignored.

use maple_tui::native::node::{NativeNode, Text as NodeText};
use maple_tui::tools::find::{
	describe_result, render_result, FindCoverage, FindHit, FindRange, FindRenderArgs, FindStats,
	FindTimeout, FindToolDetails,
};

fn args() -> Option<FindRenderArgs> {
	Some(FindRenderArgs {
		query: Some("retry budget".to_string()),
		grep_keywords: vec![],
		path: None,
	})
}

fn details() -> FindToolDetails {
	FindToolDetails {
		query: "retry budget".to_string(),
		keywords: vec!["retry".to_string(), "budget".to_string()],
		threshold: 0.2,
		cwd: "/repo".to_string(),
		elapsed_ms: 1500,
		coverage: Some(FindCoverage::Partial),
		hits: vec![
			FindHit {
				rel: "src/retry.ts".to_string(),
				name_score: None,
				content_score: 0.9,
				lines_seen: 40,
				truncated: false,
				ranges: vec![
					FindRange { start: 10, end: 20, p: 0.9, snippet: "function retryBudget() {".to_string() },
					FindRange { start: 30, end: 40, p: 0.5, snippet: "\tconst attempts = 3;".to_string() },
				],
			},
			FindHit {
				rel: "src/other.ts".to_string(),
				name_score: None,
				content_score: 0.3,
				lines_seen: 5,
				truncated: true,
				ranges: vec![FindRange { start: 1, end: 5, p: 0.3, snippet: "x".to_string() }],
			},
		],
		stats: Some(FindStats {
			listed: 2,
			requests: 3,
			failed: 1,
			unanswered: 0,
			judged: 2,
			files_read: 2,
			file_bytes: 100,
			input_tokens: 1200,
			output_tokens: 0,
			cost: 0.0,
			api_ms: 900,
			windows_judged: 3,
			windows_pruned: 0,
			map_cards: 3,
			failures: vec!["verification: boom".to_string()],
		}),
		timeout: None,
		scope_path: None,
	}
}

fn head_texts(view: &maple_tui::tools::native_view::NativeToolView) -> Vec<String> {
	let mut out = Vec::new();
	let Some(head) = &view.tool else { return out };
	for part in head.meta.iter().flatten() {
		match part {
			NodeText::Plain(text) => out.push(text.clone()),
			NodeText::Spans(spans) => out.push(spans.iter().map(|span| span.t.clone()).collect()),
		}
	}
	out
}

fn walk(node: &NativeNode, hrefs: &mut Vec<String>, codes: &mut Vec<(i64, String)>, texts: &mut Vec<String>) {
	if let Some(spans) = node.props.get("spans").and_then(|value| value.as_array()) {
		for span in spans {
			if let Some(href) = span.get("href").and_then(|value| value.as_str()) {
				hrefs.push(href.to_string());
			}
			if let Some(text) = span.get("t").and_then(|value| value.as_str()) {
				texts.push(text.to_string());
			}
		}
	}
	if let Some(text) = node.props.get_str("text") {
		texts.push(text.to_string());
	}
	if let Some(start) = node.props.get("start").and_then(|value| value.as_i64()) {
		codes.push((start, node.props.get_str("text").unwrap_or_default().to_string()));
	}
	for child in &node.children {
		walk(child, hrefs, codes, texts);
	}
}

fn body_facts(view: &maple_tui::tools::native_view::NativeToolView) -> (Vec<String>, Vec<(i64, String)>, Vec<String>) {
	let mut hrefs = Vec::new();
	let mut codes = Vec::new();
	let mut texts = Vec::new();
	for node in view.body.iter().flatten() {
		walk(node, &mut hrefs, &mut codes, &mut texts);
	}
	(hrefs, codes, texts)
}

#[test]
fn native_links_each_hit_to_its_absolute_file_location() {
	let view = describe_result("", args().as_ref(), Some(&details()), false, false, true);
	assert_eq!(view.tone, None);
	let (hrefs, _, texts) = body_facts(&view);
	assert_eq!(hrefs, ["file:///repo/src/retry.ts", "file:///repo/src/other.ts"]);
	assert!(texts.contains(&"src/retry.ts".to_string()));
	assert!(texts.contains(&"src/other.ts".to_string()));
}

#[test]
fn native_keeps_control_bytes_only_in_the_link_target() {
	let mut raw = details();
	raw.cwd = "/repo/\x1b[1m".to_string();
	raw.hits[0].rel = "src/\x1b[31mretry.ts".to_string();
	let view = describe_result("", args().as_ref(), Some(&raw), false, false, true);
	let (hrefs, _, _) = body_facts(&view);
	assert_eq!(hrefs, ["file:///repo/%1B%5B1m/src/%1B%5B31mretry.ts"]);
}

#[test]
fn native_collapsed_and_expanded_range_limits() {
	let collapsed = describe_result("", args().as_ref(), Some(&details()), false, false, false);
	let (_, collapsed_codes, _) = body_facts(&collapsed);
	assert_eq!(collapsed_codes.iter().map(|(start, _)| *start).collect::<Vec<_>>(), [10, 1]);
	assert!(head_texts(&collapsed).iter().any(|meta| meta == "1/3 requests failed"));

	let expanded = describe_result("", args().as_ref(), Some(&details()), false, false, true);
	let (_, expanded_codes, texts) = body_facts(&expanded);
	assert_eq!(expanded_codes.iter().map(|(start, _)| *start).collect::<Vec<_>>(), [10, 30, 1]);
	assert_eq!(expanded_codes[1].1, "\tconst attempts = 3;");
	assert!(texts.iter().any(|text| text == "verification: boom"));
	assert!(head_texts(&expanded).iter().any(|meta| meta == "keywords: retry, budget"));
}

#[test]
fn native_warns_when_the_deadline_cut_verification_short() {
	let mut cut = details();
	cut.timeout = Some(FindTimeout { verified: 3, passages: 8 });
	let view = describe_result("", args().as_ref(), Some(&cut), false, false, false);
	assert!(head_texts(&view).iter().any(|meta| meta == "timed out, 3/8 passages verified"));

	let plain = describe_result("", args().as_ref(), Some(&details()), false, false, false);
	assert!(!head_texts(&plain).iter().any(|meta| meta.contains("timed out")));
}

#[test]
fn native_renders_the_streamed_phase_while_running() {
	let view = describe_result("verifying 8 passages in 5 files", args().as_ref(), None, false, true, false);
	assert_eq!(head_texts(&view), ["verifying 8 passages in 5 files"]);
	let target = match view.tool.as_ref().and_then(|head| head.target.as_ref()) {
		Some(NodeText::Plain(text)) => text.clone(),
		_ => String::new(),
	};
	assert_eq!(target, "retry budget");
	assert_eq!(view.tone, None);
}

#[test]
fn plain_rows_collapse_one_range_and_expand_the_rest() {
	let collapsed = render_result("", Some(&details()), false, false, false);
	let collapsed_text = collapsed.join("\n");
	assert!(collapsed_text.contains(":10-20"), "{collapsed_text}");
	assert!(!collapsed_text.contains(":30-40"), "{collapsed_text}");

	let expanded = render_result("", Some(&details()), false, false, true);
	let expanded_text = expanded.join("\n");
	assert!(expanded_text.contains(":30-40"), "{expanded_text}");
	assert!(expanded_text.contains("const attempts = 3;"), "{expanded_text}");
}

#[test]
#[ignore = "missing API: URL-scope (`omp://`) hit linking is a host concern; the crate joins every hit onto cwd"]
fn links_omp_hits_to_their_doc_url() {}

#[test]
#[ignore = "missing API: no themed terminal renderer; per-range OSC-8 links are not emitted"]
fn links_every_range_row_in_the_terminal() {}
