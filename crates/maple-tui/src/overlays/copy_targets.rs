//! Extract copyable targets (code blocks, quotes, links, commands) from text.
//!
//! Reference: `l/src/overlays/copy-targets.ts`.
//!
//! Deviations:
//! - `extractMarkdownLinks` / `lexDocument` from the TS render/index layer have
//!   no crate equivalent. This port implements lightweight regex/string scanners
//!   for fenced code blocks, blockquotes, inline/autolinks/bare URLs, and simple
//!   tool-call command extraction over `serde_json::Value` message shapes.
//! - `AgentMessage` / `ToolCall` types are not ported; transcript walkers accept
//!   `&[serde_json::Value]` and inspect common fields.

use std::collections::HashSet;

use regex::Regex;
use serde_json::Value;

/// A fenced code block extracted from assistant markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
	pub language: Option<String>,
	pub code: String,
}

/// A blockquote block, as the renderer draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteBlock {
	pub text: String,
}

/// A drillable block within an assistant message, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageBlock {
	Code(CodeBlock),
	Quote(QuoteBlock),
}

/// A runnable command found in the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastCommand {
	pub tool_name: String,
	pub command: String,
	pub language: Option<String>,
}

/// A hyperlink found in assistant markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkTarget {
	pub href: String,
	pub label: String,
}

/// Split assistant markdown into drillable blocks (fenced code and blockquotes).
#[must_use]
pub fn extract_blocks(text: &str) -> Vec<MessageBlock> {
	let mut blocks = Vec::new();
	let fence_re = Regex::new(r"(?ms)^```([^\n`]*)\n(.*?)^```[ \t]*$").ok();
	if let Some(re) = fence_re {
		let mut last = 0usize;
		for cap in re.captures_iter(text) {
			let m = cap.get(0).expect("match");
			if m.start() > last {
				blocks.extend(extract_quote_blocks_as_messages(&text[last..m.start()]));
			}
			let lang = cap
				.get(1)
				.map(|m| m.as_str().trim().to_string())
				.filter(|s| !s.is_empty());
			let code = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
			blocks.push(MessageBlock::Code(CodeBlock { language: lang, code }));
			last = m.end();
		}
		if last < text.len() {
			blocks.extend(extract_quote_blocks_as_messages(&text[last..]));
		}
	} else {
		blocks.extend(extract_quote_blocks_as_messages(text));
	}
	blocks
}

fn extract_quote_blocks_as_messages(text: &str) -> Vec<MessageBlock> {
	extract_quote_blocks(text)
		.into_iter()
		.map(MessageBlock::Quote)
		.collect()
}

/// Extract fenced code blocks from assistant markdown, in document order.
#[must_use]
pub fn extract_code_blocks(text: &str) -> Vec<CodeBlock> {
	extract_blocks(text)
		.into_iter()
		.filter_map(|b| match b {
			MessageBlock::Code(c) => Some(c),
			MessageBlock::Quote(_) => None,
		})
		.collect()
}

/// Extract `>`-quoted blocks from assistant markdown, in document order.
#[must_use]
pub fn extract_quote_blocks(text: &str) -> Vec<QuoteBlock> {
	let mut out = Vec::new();
	let mut current: Vec<String> = Vec::new();
	for line in text.lines() {
		let trimmed_start = line.trim_start();
		if trimmed_start.starts_with('>') {
			let rest = trimmed_start.strip_prefix('>').unwrap_or("");
			let rest = rest.strip_prefix(' ').unwrap_or(rest);
			current.push(rest.to_string());
		} else if !current.is_empty() {
			let t = current.join("\n").trim().to_string();
			if !t.is_empty() {
				out.push(QuoteBlock { text: t });
			}
			current.clear();
		}
	}
	if !current.is_empty() {
		let t = current.join("\n").trim().to_string();
		if !t.is_empty() {
			out.push(QuoteBlock { text: t });
		}
	}
	out
}

/// Extract hyperlinks from markdown text (inline, autolinks, bare URLs), in order.
#[must_use]
pub fn extract_links(text: &str) -> Vec<LinkTarget> {
	let mut links = Vec::new();
	let mut seen: HashSet<(String, String)> = HashSet::new();
	if let Ok(re) = Regex::new(r"\[([^\]]+)\]\(([^)\s]+)\)") {
		for cap in re.captures_iter(text) {
			let label = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
			let href = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
			if seen.insert((href.clone(), label.clone())) {
				links.push(LinkTarget { href, label });
			}
		}
	}
	if let Ok(re) = Regex::new(r"<(https?://[^>\s]+)>") {
		for cap in re.captures_iter(text) {
			let href = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
			let label = href.clone();
			if seen.insert((href.clone(), label.clone())) {
				links.push(LinkTarget { href, label });
			}
		}
	}
	if let Ok(re) = Regex::new(r#"\bhttps?://[^\s<>\][)\"]+"#) {
		for m in re.find_iter(text) {
			let href = m.as_str().to_string();
			let label = href.clone();
			if seen.insert((href.clone(), label.clone())) {
				links.push(LinkTarget { href, label });
			}
		}
	}
	links
}

/// Concatenated visible text of an assistant message, verbatim; `None` when blank.
#[must_use]
pub fn assistant_text(msg: &Value) -> Option<String> {
	let role = msg.get("role").and_then(Value::as_str)?;
	if role != "assistant" {
		return None;
	}
	let content = msg.get("content")?;
	let text = match content {
		Value::String(s) => s.clone(),
		Value::Array(parts) => parts
			.iter()
			.filter_map(|p| {
				if p.get("type").and_then(Value::as_str) == Some("text") {
					p.get("text").and_then(Value::as_str).map(str::to_string)
				} else {
					None
				}
			})
			.collect::<Vec<_>>()
			.join("\n"),
		_ => return None,
	};
	if text.trim().is_empty() {
		None
	} else {
		Some(text)
	}
}

/// Walk transcript backwards for the most recent fenced assistant code block.
#[must_use]
pub fn extract_last_code_block(messages: &[Value]) -> Option<CodeBlock> {
	for msg in messages.iter().rev() {
		if let Some(text) = assistant_text(msg) {
			if let Some(last) = extract_code_blocks(&text).into_iter().last() {
				return Some(last);
			}
		}
	}
	None
}

/// Walk transcript backwards for the most recent link in an assistant message.
#[must_use]
pub fn extract_last_link(messages: &[Value]) -> Option<LinkTarget> {
	for msg in messages.iter().rev() {
		if let Some(text) = assistant_text(msg) {
			if let Some(last) = extract_links(&text).into_iter().last() {
				return Some(last);
			}
		}
	}
	None
}

/// Runnable command carried by a tool call value, if any.
#[must_use]
pub fn command_from_tool_call(tc: &Value) -> Option<LastCommand> {
	let name = tc
		.get("toolName")
		.or_else(|| tc.get("name"))
		.and_then(Value::as_str)?;
	let args = tc.get("arguments").or_else(|| tc.get("args"));
	match name {
		"bash" | "shell" => {
			let command = args?.get("command").and_then(Value::as_str)?.to_string();
			Some(LastCommand {
				tool_name: name.to_string(),
				command,
				language: Some("bash".to_string()),
			})
		}
		"eval" | "python" => extract_eval_code(args).map(|(code, language)| LastCommand {
			tool_name: name.to_string(),
			command: code,
			language: Some(language),
		}),
		_ => None,
	}
}

fn extract_eval_code(args: Option<&Value>) -> Option<(String, String)> {
	let args = args?;
	let code = args
		.get("code")
		.or_else(|| args.get("script"))
		.and_then(Value::as_str)?
		.to_string();
	let language = args
		.get("language")
		.and_then(Value::as_str)
		.unwrap_or("python")
		.to_string();
	Some((code, language))
}

/// Walk transcript backwards for the most recent bash command or eval code.
#[must_use]
pub fn extract_last_command(messages: &[Value]) -> Option<LastCommand> {
	for msg in messages.iter().rev() {
		if let Some(calls) = msg.get("toolCalls").and_then(Value::as_array) {
			for tc in calls.iter().rev() {
				if let Some(cmd) = command_from_tool_call(tc) {
					return Some(cmd);
				}
			}
		}
		if let Some(content) = msg.get("content").and_then(Value::as_array) {
			for part in content.iter().rev() {
				if part.get("type").and_then(Value::as_str) == Some("toolCall") {
					if let Some(cmd) = command_from_tool_call(part) {
						return Some(cmd);
					}
				}
			}
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn extracts_fenced_code_blocks() {
		let text = "hi\n```rust\nfn main() {}\n```\nbye";
		let blocks = extract_code_blocks(text);
		assert_eq!(blocks.len(), 1);
		assert_eq!(blocks[0].language.as_deref(), Some("rust"));
		assert!(blocks[0].code.contains("fn main"));
	}

	#[test]
	fn extracts_quote_blocks() {
		let text = "a\n> one\n> two\n\nb";
		let quotes = extract_quote_blocks(text);
		assert_eq!(quotes.len(), 1);
		assert_eq!(quotes[0].text, "one\ntwo");
	}

	#[test]
	fn extracts_inline_and_bare_links() {
		let text = "see [x](https://a.test) and https://b.test";
		let links = extract_links(text);
		assert!(links.iter().any(|l| l.href == "https://a.test" && l.label == "x"));
		assert!(links.iter().any(|l| l.href == "https://b.test"));
	}

	#[test]
	fn assistant_text_concatenates_text_parts() {
		let msg = json!({"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"text","text":"there"}]});
		assert_eq!(assistant_text(&msg).as_deref(), Some("hi\nthere"));
		assert!(assistant_text(&json!({"role":"user","content":"x"})).is_none());
	}

	#[test]
	fn finds_last_command_from_tool_call_array() {
		let msgs = vec![json!({
			"role":"assistant",
			"toolCalls":[{"name":"bash","arguments":{"command":"echo hi"}}]
		})];
		let cmd = extract_last_command(&msgs).unwrap();
		assert_eq!(cmd.tool_name, "bash");
		assert_eq!(cmd.command, "echo hi");
	}

	#[test]
	fn extract_blocks_preserves_order_of_code_and_quotes() {
		let text = "> q\n\n```sh\necho\n```\n";
		let blocks = extract_blocks(text);
		assert_eq!(blocks.len(), 2);
		assert!(matches!(blocks[0], MessageBlock::Quote(_)));
		assert!(matches!(blocks[1], MessageBlock::Code(_)));
	}
}
