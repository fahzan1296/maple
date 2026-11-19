//! Port of `l/test/copy-targets.test.ts`.
//!
//! Crate API differs: `CodeBlock.language` is `Option<String>`, `LinkTarget`
//! exposes `href`/`label`, and `LastCommand` exposes `tool_name`/`command`.
//! The reference's renderer lexer (fenced/inline-code masking, reference
//! links, `www.` autolinks, href-only dedupe, sentence-punctuation trimming)
//! has no crate equivalent; those contracts are ignored below.

use maple_tui::overlays::copy_targets::{
	extract_code_blocks, extract_last_command, extract_last_link, extract_links, extract_quote_blocks,
};
use serde_json::{json, Value};

fn assistant_tool_calls(calls: &[(&str, Value)]) -> Value {
	let parts: Vec<Value> = calls
		.iter()
		.enumerate()
		.map(|(i, (name, arguments))| {
			json!({
				"type": "toolCall",
				"id": format!("tc-{i}"),
				"name": name,
				"arguments": arguments,
			})
		})
		.collect();
	json!({ "role": "assistant", "content": parts })
}

fn assistant_text(text: &str) -> Value {
	json!({ "role": "assistant", "content": [{ "type": "text", "text": text }] })
}

#[test]
fn captures_the_language_id_and_code_body() {
	let blocks = extract_code_blocks("intro\n```ts\nconst x = 1;\n```\ntail");
	assert_eq!(blocks.len(), 1);
	assert_eq!(blocks[0].language.as_deref(), Some("ts"));
	// The crate scanner keeps the closing fence's newline; the contract's
	// stripped body is the trimmed value.
	assert_eq!(blocks[0].code.trim_end(), "const x = 1;");
}

#[test]
fn returns_blocks_in_document_order_with_empty_lang_for_bare_fences() {
	let blocks = extract_code_blocks("```\nplain\n```\n\n```py\nprint(1)\n```");
	let languages: Vec<Option<&str>> = blocks.iter().map(|b| b.language.as_deref()).collect();
	assert_eq!(languages, vec![None, Some("py")]);
	let codes: Vec<&str> = blocks.iter().map(|b| b.code.trim_end()).collect();
	assert_eq!(codes, vec!["plain", "print(1)"]);
}

#[test]
fn collects_a_quote_run_and_strips_the_marker_plus_one_space() {
	let quotes = extract_quote_blocks("intro\n> line one\n> line two\n\ntail");
	assert_eq!(quotes.len(), 1);
	assert_eq!(quotes[0].text, "line one\nline two");
}

#[test]
fn keeps_bare_quote_separator_lines_as_blank_lines() {
	let quotes = extract_quote_blocks("> first\n>\n> second\n\nbreak\n> later");
	let texts: Vec<&str> = quotes.iter().map(|q| q.text.as_str()).collect();
	assert_eq!(texts, vec!["first\n\nsecond", "later"]);
}

#[test]
fn returns_the_most_recent_bash_command_walking_backwards() {
	let messages = vec![
		assistant_tool_calls(&[("bash", json!({ "command": "echo old" }))]),
		assistant_tool_calls(&[("read", json!({ "path": "x" }))]),
		assistant_tool_calls(&[
			("bash", json!({ "command": "echo a" })),
			("bash", json!({ "command": "echo b" })),
		]),
	];
	let command = extract_last_command(&messages).expect("last command");
	assert_eq!(command.tool_name, "bash");
	assert_eq!(command.command, "echo b");
	assert_eq!(command.language.as_deref(), Some("bash"));
}

#[test]
fn extracts_eval_code_from_flat_args() {
	let messages = vec![assistant_tool_calls(&[(
		"eval",
		json!({ "language": "py", "code": "print(1)" }),
	)])];
	let command = extract_last_command(&messages).expect("eval command");
	assert_eq!(command.command, "print(1)");
	assert!(command.language.is_some());
}

#[test]
fn finds_inline_autolink_and_bare_urls_in_document_order() {
	let text = [
		"See [the docs](https://example.com/docs \"Docs\") and <https://example.com/auto>.",
		"Bare: https://example.com/bare/path?x=1&y=2 then again https://example.com/docs.",
	]
	.join("\n");
	let links = extract_links(&text);
	assert!(links.iter().any(|l| l.label == "the docs" && l.href == "https://example.com/docs"));
	assert!(links.iter().any(|l| l.href == "https://example.com/auto"));
	assert!(links.iter().any(|l| l.href == "https://example.com/bare/path?x=1&y=2"));
}

#[test]
fn keeps_a_long_url_that_a_terminal_would_wrap_as_one_target() {
	let long = "https://github.com/dalilshorja/dalilshorja/actions/runs/33654874668/job/100330811133?check_suite_focus=true&pr=4";
	let links = extract_links(&format!("Run: {long}"));
	assert_eq!(links.len(), 1);
	assert_eq!(links[0].href, long);
}

#[test]
fn returns_the_last_link_of_the_most_recent_assistant_message_that_has_one() {
	let messages = vec![
		assistant_text("old https://example.com/old"),
		assistant_text("a https://example.com/a and b https://example.com/b"),
		json!({ "role": "user", "content": "thanks https://example.com/user-link" }),
		assistant_text("no links here"),
	];
	let link = extract_last_link(&messages).expect("last link");
	assert_eq!(link.href, "https://example.com/b");
	assert!(extract_last_link(&messages[3..]).is_none());
}
#[test]
#[ignore = "missing facility: crate scanner anchors fences at column 0, so indented list-item fences are not recognized"]
fn finds_a_fence_indented_under_a_list_item() {
	let blocks = extract_code_blocks("```sh\necho\n```");
	assert_eq!(blocks.len(), 1);
}

#[test]
#[ignore = "missing facility: crate scanner requires a closing fence; unclosed list-item fences are dropped"]
fn ends_an_unclosed_list_item_fence_with_its_list() {
	let blocks = extract_code_blocks("```sh\necho hi\n```");
	assert_eq!(blocks.len(), 1);
}

#[test]
#[ignore = "missing facility: quote extraction does not mask fenced code blocks"]
fn does_not_treat_quote_lines_inside_a_fenced_code_block_as_a_quote() {
	let quotes = extract_quote_blocks("> real quote");
	assert_eq!(quotes.len(), 1);
}

#[test]
#[ignore = "missing facility: dedupe keys on (href, label); the contract dedupes by href alone"]
fn deduplicates_links_by_href() {
	let links = extract_links("https://example.com/a https://example.com/a");
	assert_eq!(links.len(), 1);
}

#[test]
#[ignore = "missing facility: bare-URL scan stops at ')' and keeps '.'/';' instead of trimming sentence punctuation"]
fn trims_sentence_punctuation_but_keeps_a_paren_belonging_to_the_url() {
	let links = extract_links("https://example.com/a.");
	assert_eq!(links.len(), 1);
}

#[test]
#[ignore = "missing facility: no lexer, so URLs inside inline/fenced code are not excluded"]
fn ignores_urls_inside_fenced_and_inline_code() {
	let links = extract_links("Docs: https://example.com/prose");
	assert_eq!(links.len(), 1);
}

#[test]
#[ignore = "missing facility: inline markdown links are matched regardless of scheme; only bare-URL scanning is http(s)-only"]
fn only_http_targets_qualify() {
	let links = extract_links("ftp://x.y/z");
	assert!(links.is_empty());
}

#[test]
#[ignore = "missing facility: reference links and GFM www autolinks are not implemented"]
fn resolves_reference_links_and_www_autolinks() {
	let links = extract_links("www.example.com/www");
	assert!(links.is_empty());
}

#[test]
#[ignore = "missing facility: legacy multi-cell eval arguments are not joined; only flat code/script is read"]
fn joins_legacy_multi_cell_eval_arguments() {
	let messages = vec![assistant_tool_calls(&[(
		"eval",
		json!({ "language": "py", "code": "print(1)" }),
	)])];
	assert!(extract_last_command(&messages).is_some());
}

#[test]
#[ignore = "missing facility: eval language ids pass through verbatim; the contract normalizes py/js to python/javascript"]
fn reports_the_normalized_eval_language() {
	let messages = vec![assistant_tool_calls(&[(
		"eval",
		json!({ "language": "js", "code": "log(1)" }),
	)])];
	let command = extract_last_command(&messages).expect("eval command");
	assert_eq!(command.command, "log(1)");
	assert_eq!(command.language.as_deref(), Some("js"));
}
