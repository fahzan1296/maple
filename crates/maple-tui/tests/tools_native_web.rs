//! Port of `l/test/tools-native-web.test.ts` (web_search, ask, generic, MCP).
//!
//! Deviations: the crate splits the reference's single `tool` head into a
//! `NativeToolHead` (`view.tool`) and fallback-card spans (`view.head`), so the
//! generic/MCP head assertions read `view.head`; the default renderer draws
//! JSON output as a `code` node and MCP non-json output as `text` rather than
//! `tree`.

use maple_tui::native::node::{NativeNode, NodeKind, NodeText};
use maple_tui::tools::ask::{
	AskAnswer, AskOptionNote, AskRenderArgs, AskToolDetails, describe_ask_call, describe_ask_result,
};
use maple_tui::tools::default_renderer::{
	DefaultRenderOptions, DefaultToolRenderInput, DefaultToolRenderResult,
	describe_default_tool_execution,
};
use maple_tui::tools::mcp::{describe_call, describe_result};
use maple_tui::tools::native_view::NativeToolView;
use maple_tui::tools::web_search::{SearchRenderDetails, describe_search_result_native};
use maple_tui::tools::web_search_types::{SearchProviderId, SearchResponse, SearchSource, SearchUsage};
use serde_json::{Value, json};

fn plain(text: &NodeText) -> String {
	match text {
		NodeText::Plain(value) => value.clone(),
		NodeText::Spans(spans) => spans.iter().map(|span| span.t.clone()).collect(),
	}
}

fn node_text(node: &NativeNode) -> String {
	let mut out = String::new();
	if let Some(spans) = node.props.spans() {
		for span in spans {
			out.push_str(&span.t);
		}
	}
	if let Some(text) = node.text() {
		out.push_str(text);
	}
	for child in &node.children {
		out.push_str(&node_text(child));
	}
	out
}

fn head_text(view: &NativeToolView) -> String {
	match view.head.as_ref() {
		Some(NodeText::Spans(spans)) => spans.iter().map(|span| span.t.clone()).collect(),
		Some(NodeText::Plain(text)) => text.clone(),
		None => String::new(),
	}
}

fn child_with_role<'a>(nodes: &'a [NativeNode], role: &str) -> Option<&'a NativeNode> {
	nodes.iter().find(|node| node.props.get_str("role") == Some(role))
}

#[test]
fn web_search_heads_with_the_query_and_lists_sources_as_linked_rows() {
	let details = SearchRenderDetails {
		response: Some(SearchResponse {
			provider: Some(SearchProviderId::Parallel),
			answer: Some("The answer".to_string()),
			sources: vec![SearchSource {
				title: "Example".to_string(),
				url: "https://www.example.com/a".to_string(),
				age_seconds: Some(3 * 86400),
				..SearchSource::default()
			}],
			usage: Some(SearchUsage {
				input_tokens: Some(5),
				..SearchUsage::default()
			}),
			model: Some("m1".to_string()),
			..SearchResponse::default()
		}),
		error: None,
	};

	let view = describe_search_result_native(Some("what is x"), Some(&details), "");
	let tool = view.tool.expect("tool head");
	assert_eq!(tool.target.as_ref().map(plain).as_deref(), Some("what is x"));
	assert_eq!(tool.target_kind.as_deref(), Some("query"));
	let meta = tool.meta.expect("meta");
	assert_eq!(plain(&meta[0]), "Parallel · 1 source");
	let badge = &tool.badges.expect("badges")[0];
	assert_eq!(badge.text, "m1");
	assert!(badge.title.expect("badge title").contains("in 5"));

	let body = view.body.expect("body");
	assert_eq!(body[0].kind, NodeKind::Md);
	let sources = &body[1];
	assert_eq!(sources.props.get_str("role"), Some("omp.tool.files"));
	let row = &sources.children[0];
	assert_eq!(row.props.get_str("role"), Some("omp.tool.source"));
	assert_eq!(row.props.get_str("href"), Some("https://www.example.com/a"));
	assert_eq!(row.children[0].props.get_str("text"), Some("E"));
	let title = row.children[1]
		.props
		.spans()
		.expect("title spans")
		.into_iter()
		.next()
		.expect("title span");
	assert_eq!(title.href.as_deref(), Some("https://www.example.com/a"));
	let meta_span = row.children[2]
		.props
		.spans()
		.expect("meta spans")
		.into_iter()
		.next()
		.expect("meta span");
	assert_eq!(meta_span.t, "example.com · 3d");
}

#[test]
fn ask_pending_call_names_the_first_question_and_waits() {
	let args = AskRenderArgs {
		html: Some(
			"<form><p>Pick one?</p><label><input type=\"radio\" name=\"pick\" value=\"a\"> A</label><label>Why? <input name=\"why\"></label></form>".to_string(),
		),
	};

	let view = describe_ask_call(Some(&args));
	assert_eq!(view.inline, Some(true));
	let tool = view.tool.expect("tool head");
	assert_eq!(tool.target.as_ref().map(plain).as_deref(), Some("Pick one?"));
	let meta = tool.meta.expect("meta");
	assert_eq!(plain(&meta[0]), "2 fields");
	assert_eq!(view.body.as_ref().map(Vec::len), Some(1));
}

#[test]
fn ask_answered_result_pairs_each_question_and_marks_empty_answers() {
	let details = AskToolDetails {
		answers: vec![
			AskAnswer {
				name: "db".to_string(),
				question: "Database?".to_string(),
				answer: "Postgres".to_string(),
			},
			AskAnswer {
				name: "notes".to_string(),
				question: "Notes".to_string(),
				answer: String::new(),
			},
		],
		timed_out: Some(true),
		..AskToolDetails::default()
	};

	let view = describe_ask_result(Some(&details), "", None);
	let body = view.body.expect("body");
	let sections: Vec<&NativeNode> = body
		.iter()
		.filter(|node| node.kind == NodeKind::Section)
		.collect();
	assert_eq!(sections.len(), 2);
	let answer_roles: Vec<Option<&str>> = sections
		.iter()
		.map(|section| section.children.last().and_then(|node| node.props.get_str("role")))
		.collect();
	assert_eq!(
		answer_roles,
		vec![Some("omp.tool.answer"), Some("omp.tool.answer.off")]
	);
	assert!(node_text(sections[0]).contains("Postgres"));
	assert_eq!(body.last().and_then(|node| node.props.get_str("role")), Some("omp.tool.notice"));
}

#[test]
fn ask_renders_persisted_option_notes_under_the_matching_answer() {
	let details = AskToolDetails {
		answers: vec![
			AskAnswer {
				name: "unrelated".to_string(),
				question: "Unrelated?".to_string(),
				answer: "First".to_string(),
			},
			AskAnswer {
				name: "db".to_string(),
				question: "Database?".to_string(),
				answer: "First".to_string(),
			},
		],
		notes: Some(vec![AskOptionNote {
			name: "db".to_string(),
			question: "Database?".to_string(),
			option: "Second".to_string(),
			value: "second".to_string(),
			text: "Team standard".to_string(),
		}]),
		..AskToolDetails::default()
	};

	let view = describe_ask_result(Some(&details), "", None);
	let body = view.body.expect("body");
	let sections: Vec<&NativeNode> = body
		.iter()
		.filter(|node| node.kind == NodeKind::Section)
		.collect();
	assert_eq!(sections.len(), 2);
	assert!(child_with_role(&sections[0].children, "omp.tool.answer.note").is_none());
	let note = sections[1].children.last().expect("note node");
	assert_eq!(note.props.get_str("role"), Some("omp.tool.answer.note"));
	assert!(node_text(note).contains("Second: Team standard"));
}

#[test]
fn ask_falls_back_to_the_result_text_when_details_carry_no_answers() {
	let fallback = "The form needs at least one named control";

	let view = describe_ask_result(None, fallback, None);
	assert_eq!(view.tone, Some(maple_tui::native::node::Tone::Warning));
	let body = view.body.expect("body");
	assert_eq!(body.len(), 1);
	assert!(node_text(&body[0]).contains(fallback));
}

#[test]
fn default_renderer_summarizes_args_and_renders_json_output() {
	let input = DefaultToolRenderInput {
		label: "custom".to_string(),
		args: Some(json!({ "a": 1 })),
		result: Some(DefaultToolRenderResult {
			output: "{\"x\":1}".to_string(),
			is_error: false,
			skipped: false,
		}),
		options: DefaultRenderOptions::default(),
	};

	let view = describe_default_tool_execution(&input);
	let head = head_text(&view);
	assert!(head.contains("custom"), "{head}");
	assert!(head.contains('a'), "{head}");
	// Deviation: the crate draws JSON output as a `code` node, not a `tree`.
	assert_eq!(view.body.expect("body")[0].kind, NodeKind::Code);
}

#[test]
fn mcp_call_and_result_keep_full_args_in_a_folded_args_section() {
	let query = format!("find {}", "long ".repeat(40));
	let mut args = serde_json::Map::new();
	args.insert("query".to_string(), Value::String(query.clone()));
	args.insert("i".to_string(), Value::String("intent".to_string()));

	let call = describe_call(Some(&args), "srv/tool");
	let res = describe_result("[1,2]", false, Some("srv/tool"), None, Some(&args), false);
	for view in [&call, &res] {
		assert!(!head_text(view).contains(&query));
		let body = view.body.as_ref().expect("body");
		let section = body.last().expect("args section");
		assert_eq!(section.kind, NodeKind::Section);
		assert_eq!(section.key.as_deref(), Some("args"));
		assert_eq!(section.props.get_bool("collapsible"), Some(true));
		assert_eq!(section.props.get_bool("collapsed"), Some(true));
		let value = section.children[0]
			.props
			.get("value")
			.expect("args json value")
			.to_string();
		assert!(value.contains(&query), "{value}");
		assert!(!value.contains("intent"), "{value}");
	}
	// Deviation: non-json output renders as a text node, not a `tree`.
	assert_eq!(
		res.body.as_ref().expect("body").iter().map(|node| node.kind).collect::<Vec<_>>(),
		vec![NodeKind::Text, NodeKind::Section]
	);
	assert!(describe_call(Some(&serde_json::Map::new()), "srv/tool").body.is_none());
}

#[test]
fn mcp_json_mode_sends_values_and_drops_hidden_root_keys() {
	let mut args = serde_json::Map::new();
	args.insert("query".to_string(), Value::String("tokyo".to_string()));
	args.insert("limit".to_string(), json!(2));
	args.insert("i".to_string(), Value::String("intent".to_string()));
	let text = serde_json::to_string(&json!({ "hits": [{ "id": 1 }], "__partialJson": "{" }))
		.expect("json text");

	let res = describe_result(&text, false, Some("srv/tool"), None, Some(&args), true);
	let body = res.body.expect("body");
	assert_eq!(body[0].kind, NodeKind::Json);
	assert_eq!(body[0].props.get("value"), Some(&json!({ "hits": [{ "id": 1 }] })));
	let section = &body[1];
	assert_eq!(section.kind, NodeKind::Section);
	let args_node = &section.children[0];
	assert_eq!(args_node.kind, NodeKind::Json);
	assert_eq!(args_node.props.get("value"), Some(&json!({ "query": "tokyo", "limit": 2 })));

	let call = describe_call(Some(&args), "srv/tool");
	let call_body = call.body.expect("call body");
	assert_eq!(call_body[0].kind, NodeKind::Section);
	assert_eq!(call_body[0].children[0].kind, NodeKind::Json);
}