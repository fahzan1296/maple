//! JSON tree rendering constants, option types, and inline-format helpers.
//!
 //! Reference: `l/src/tools/json-tree.ts`.
 //!
 //! Deviations:
//! - Interactive `TreeView` TUI component, cached component state, and exact
//!   token-per-type ANSI tree painting parity are not fully ported (no
//!   interactive tree widget here). Structural tree rendering + inline helpers
//!   are provided.
//! - Themed string helpers take `&Theme` (theming in-crate); native describe
//!   emits token-styled/text nodes for host theme-aware rendering.
use std::collections::BTreeSet;

/// Max depth for JSON tree rendering (collapsed).
pub const JSON_TREE_MAX_DEPTH_COLLAPSED: usize = 2;
/// Maximum expanded JSON tree depth.
pub const JSON_TREE_MAX_DEPTH_EXPANDED: usize = 6;
/// Maximum collapsed JSON tree rows.
pub const JSON_TREE_MAX_LINES_COLLAPSED: usize = 6;
/// Maximum expanded JSON tree rows.
pub const JSON_TREE_MAX_LINES_EXPANDED: usize = 200;
/// Maximum collapsed JSON scalar width.
pub const JSON_TREE_SCALAR_LEN_COLLAPSED: usize = 60;
/// Maximum expanded JSON scalar width.
pub const JSON_TREE_SCALAR_LEN_EXPANDED: usize = 2000;
/// Characters a container's one-line form may take before listing entries.
pub const JSON_TREE_INLINE_LEN: usize = 60;

/// Default hidden root argument keys (`i`/INTENT_FIELD and `__partialJson`).
pub const DEFAULT_HIDDEN_ROOT_KEYS: &[&str] = &["i", "__partialJson"];

/// Root row gutter connector mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonTreeRootConnectors {
	/// Hooked to block above (`└ •`, ` •`, nested branches under bullet).
	Hooked,
	/// Standard sibling connectors (`├─`/`└─`).
	Siblings,
}

/// JSON hierarchy policy for rendering.
#[derive(Clone, Debug)]
pub struct JsonTreeRenderOptions {
	/// Max depth.
	pub max_depth: usize,
	/// Max lines.
	pub max_lines: usize,
	/// Max scalar length.
	pub max_scalar_len: usize,
	/// Root object keys omitted from display.
	pub hidden_root_keys: Vec<String>,
	/// Preserve embedded newlines as continuation rows.
	pub multiline_strings: bool,
	/// Escape inline tabs/newlines as JSON-style sequences.
	pub escape_string_whitespace: bool,
	/// Root connector style.
	pub root_connectors: JsonTreeRootConnectors,
}

impl Default for JsonTreeRenderOptions {
	fn default() -> Self {
		Self {
			max_depth: JSON_TREE_MAX_DEPTH_EXPANDED,
			max_lines: JSON_TREE_MAX_LINES_EXPANDED,
			max_scalar_len: JSON_TREE_SCALAR_LEN_EXPANDED,
			hidden_root_keys: DEFAULT_HIDDEN_ROOT_KEYS.iter().map(|s| s.to_string()).collect(),
			multiline_strings: true,
			escape_string_whitespace: true,
			root_connectors: JsonTreeRootConnectors::Hooked,
		}
	}
}

/// Sanitization/string-summary/budget policy for inline JSON formatting.
#[derive(Clone, Debug, Default)]
pub struct InlineFormatOptions {
	/// Summarize multiline strings with line count.
	pub multiline_summary: bool,
	/// Character-count budget mode (legacy/ANSI-width approximation flag in TS).
	pub character_budget: bool,
}

/// Format a scalar for inline display (best-effort, unsanitized except basic escaping).
pub fn format_scalar(value: &serde_json::Value, max_len: usize, options: &InlineFormatOptions) -> String {
	match value {
		serde_json::Value::Null => "null".to_string(),
		serde_json::Value::Bool(b) => b.to_string(),
		serde_json::Value::Number(n) => n.to_string(),
		serde_json::Value::String(s) => {
			if options.multiline_summary {
				let lines: Vec<&str> = s.split('\n').collect();
				let first_line = lines.first().map(|l| l.trim()).unwrap_or("");
				if first_line.is_empty() {
					return format!("\"\" ({} lines)", lines.len());
				}
				let preview = truncate_chars(first_line, max_len);
				if lines.len() > 1 {
					format!("\"{preview}…\" ({} lines)", lines.len())
				} else {
					format!("\"{preview}\"")
				}
			} else {
				let escaped = s.replace('\n', "\\n").replace('\t', "\\t");
				format!("\"{}\"", truncate_chars(&escaped, max_len))
			}
		}
		serde_json::Value::Array(a) => format!("[{} items]", a.len()),
		serde_json::Value::Object(o) => format!("{{{} keys}}", o.len()),
	}
}

fn truncate_chars(s: &str, max_len: usize) -> String {
	let count = s.chars().count();
	if count <= max_len {
		s.to_string()
	} else if max_len == 0 {
		String::new()
	} else {
		s.chars().take(max_len).collect()
	}
}

/// Format object args inline as `k=v, ...`, hiding default metadata keys.
pub fn format_args_inline(
	args: &serde_json::Map<String, serde_json::Value>,
	max_width: usize,
	options: &InlineFormatOptions,
) -> String {
	let hidden: BTreeSet<&str> = DEFAULT_HIDDEN_ROOT_KEYS.iter().copied().collect();
	if options.character_budget {
		let mut pairs: Vec<String> = Vec::new();
		let mut length = 0usize;
		for (k, v) in args {
			if hidden.contains(k.as_str()) {
				continue;
			}
			let pair = format!("{k}={}", format_scalar(v, 24, options));
			let added = pair.chars().count() + if pairs.is_empty() { 0 } else { 2 };
			if length + added > max_width && !pairs.is_empty() {
				pairs.push("…".to_string());
				break;
			}
			length += added;
			pairs.push(pair);
		}
		return pairs.join(", ");
	}
	// Width-budget approximation by chars (TS uses display width).
	let mut keys: Vec<&String> = args.keys().filter(|k| !hidden.contains(k.as_str())).collect();
	keys.sort();
	let mut result = String::new();
	let mut width = 0usize;
	for (i, raw_key) in keys.iter().enumerate() {
		let value = &args[*raw_key];
		let sep = if width > 0 { ", " } else { "" };
		let sep_w = sep.chars().count();
		let current = width + sep_w;
		let cap = max_width.saturating_sub(current + 1); // reserve for ellipsis
		if cap == 0 {
			result.push('…');
			return result;
		}
		let mut tail_reserve = 0usize;
		for tail_key in keys.iter().skip(i + 1) {
			tail_reserve += 2 + tail_key.chars().count() + 1 + 4;
		}
		let piece_budget = cap.min(max_width.saturating_sub(current + tail_reserve));
		let value_max_len = piece_budget.saturating_sub(raw_key.chars().count() + 3).max(1);
		let value_str = format_scalar(value, value_max_len, options);
		let piece = format!("{raw_key}={value_str}");
		let piece_w = piece.chars().count();
		if piece_w > piece_budget {
			result.push_str(sep);
			result.push_str(&truncate_chars(&piece, cap));
			return result;
		}
		result.push_str(sep);
		result.push_str(&piece);
		width = current + piece_w;
	}
	result
}

/// Summarize task structured output without expanding its JSON tree.
pub fn format_output_inline(data: &serde_json::Value, max_width: usize) -> String {
	let options = InlineFormatOptions {
		multiline_summary: true,
		character_budget: true,
	};
	match data {
		serde_json::Value::Null => "Output: none".to_string(),
		serde_json::Value::Array(a) if a.is_empty() => "Output: []".to_string(),
		serde_json::Value::Array(a) => {
			let first = a.first().map(|v| format_scalar(v, 40, &options)).unwrap_or_default();
			if a.len() > 1 {
				format!("Output: [{} items] {first}…", a.len())
			} else {
				format!("Output: [1 item] {first}")
			}
		}
		serde_json::Value::Object(o) => {
			let body_budget = max_width.saturating_sub("Output: ".chars().count());
			let body = format_args_inline(o, body_budget, &options);
			if body.is_empty() {
				"Output: {}".to_string()
			} else {
				format!("Output: {body}")
			}
		}
		other => format!("Output: {}", format_scalar(other, 60, &options)),
	}
}

/// Render JSON as bounded plain tree-like lines (structural fallback).
pub fn render_json_tree_lines(value: &serde_json::Value, max_depth: usize, max_lines: usize, max_scalar_len: usize) -> Vec<String> {
	let mut lines = Vec::new();
	fn walk(v: &serde_json::Value, depth: usize, max_depth: usize, prefix: &str, lines: &mut Vec<String>, max_lines: usize, max_scalar_len: usize) {
		if lines.len() >= max_lines { return; }
		match v {
			serde_json::Value::Array(a) => {
				if a.is_empty() { lines.push(format!("{prefix}[]")); return; }
				if depth >= max_depth { lines.push(format!("{prefix}[… {} items]", a.len())); return; }
				lines.push(format!("{prefix}["));
				for (i, it) in a.iter().enumerate() {
					if lines.len() >= max_lines { lines.push(format!("{prefix}  …")); return; }
					walk(it, depth + 1, max_depth, &format!("{prefix}  {i}: "), lines, max_lines, max_scalar_len);
				}
				if lines.len() < max_lines { lines.push(format!("{prefix}]")); }
			}
			serde_json::Value::Object(o) => {
				if o.is_empty() { lines.push(format!("{prefix}{{}}")); return; }
				if depth >= max_depth { lines.push(format!("{prefix}{{… {} fields}}", o.len())); return; }
				lines.push(format!("{prefix}{{"));
				for (k, it) in o.iter() {
					if lines.len() >= max_lines { lines.push(format!("{prefix}  …")); return; }
					walk(it, depth + 1, max_depth, &format!("{prefix}  {k}: "), lines, max_lines, max_scalar_len);
				}
				if lines.len() < max_lines { lines.push(format!("{prefix}}}")); }
			}
			_ => {
				let s = format_scalar(v, max_scalar_len, &InlineFormatOptions::default());
				lines.push(format!("{prefix}{s}"));
			}
		}
	}
	walk(value, 0, max_depth, "", &mut lines, max_lines, max_scalar_len);
	if lines.is_empty() { lines.push("null".into()); }
	lines
}

/// Native describe a JSON value as a tree/text fallback (structural).
pub fn describe_json_tree(value: &serde_json::Value, max_depth: usize, max_lines: usize) -> crate::native::node::NativeNode {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props};
	let lines = render_json_tree_lines(value, max_depth, max_lines, JSON_TREE_SCALAR_LEN_EXPANDED);
	let mut p = Props::new();
	p.set_str("wrap", "none");
	ntext(NodeText::Plain(lines.join("\n")), p)
}

/// Native describe JSON value for host view (falls back to structural tree/text).
pub fn describe_json_value(value: &serde_json::Value) -> crate::native::node::NativeNode {
	describe_json_tree(value, JSON_TREE_MAX_DEPTH_EXPANDED, JSON_TREE_MAX_LINES_EXPANDED)
}

/// Plain render call helper for JSON-bearing tool args/results.
pub fn render_call(label: &str, args: Option<&serde_json::Map<String, serde_json::Value>>) -> String {
	match args {
		Some(a) if !a.is_empty() => format!("{label} {}", format_args_inline(a, 120, &InlineFormatOptions::default())),
		_ => label.to_string(),
	}
}

/// Plain render result helper for JSON-bearing output.
pub fn render_result(value: &serde_json::Value, max_lines: usize) -> Vec<String> {
	render_json_tree_lines(value, JSON_TREE_MAX_DEPTH_EXPANDED, max_lines.max(1), JSON_TREE_SCALAR_LEN_EXPANDED)
}

/// Themed render of JSON tree lines (keys/punctuation dimmed, scalars plain/toolOutput).
pub fn render_json_tree_lines_themed(value: &serde_json::Value, max_depth: usize, max_lines: usize, max_scalar_len: usize, theme: &crate::theme::Theme) -> Vec<String> {
	render_json_tree_lines(value, max_depth, max_lines, max_scalar_len)
		.into_iter()
		.map(|line| {
			if let Some((k, v)) = line.split_once(": ") {
				format!("{}: {}", theme.fg("accent", k).unwrap_or_else(|_| k.to_string()), theme.fg("toolOutput", v).unwrap_or_else(|_| v.to_string()))
			} else {
				theme.fg("muted", &line).unwrap_or(line)
			}
		})
		.collect()
}

/// Themed render result helper.
pub fn render_result_themed(value: &serde_json::Value, max_lines: usize, theme: &crate::theme::Theme) -> Vec<String> {
	render_json_tree_lines_themed(value, JSON_TREE_MAX_DEPTH_EXPANDED, max_lines.max(1), JSON_TREE_SCALAR_LEN_EXPANDED, theme)
}
#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn tree_helpers() {
		let v = serde_json::json!({"a":[1,2], "b":"x"});
		let lines = render_json_tree_lines(&v, 3, 50, 60);
		assert!(!lines.is_empty());
		let n = describe_json_tree(&v, 3, 50);
		let _ = n;
		let c = render_call("tool", v.as_object());
		assert!(c.contains("tool"));
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn constants_match_ts() {
		assert_eq!(JSON_TREE_MAX_DEPTH_COLLAPSED, 2);
		assert_eq!(JSON_TREE_MAX_LINES_EXPANDED, 200);
		assert_eq!(JSON_TREE_INLINE_LEN, 60);
		assert_eq!(DEFAULT_HIDDEN_ROOT_KEYS, &["i", "__partialJson"]);
	}

	#[test]
	fn format_scalar_variants() {
		let opts = InlineFormatOptions::default();
		assert_eq!(format_scalar(&json!(null), 10, &opts), "null");
		assert_eq!(format_scalar(&json!(true), 10, &opts), "true");
		assert_eq!(format_scalar(&json!(5), 10, &opts), "5");
		assert_eq!(format_scalar(&json!("ab\nc"), 10, &opts), "\"ab\\nc\"");
		assert_eq!(format_scalar(&json!([1, 2]), 10, &opts), "[2 items]");
	}

	#[test]
	fn inline_args_hide_intent() {
		let mut m = serde_json::Map::new();
		m.insert("i".into(), json!("x"));
		m.insert("path".into(), json!("a"));
		let s = format_args_inline(&m, 80, &InlineFormatOptions::default());
		assert!(!s.contains("i="));
		assert!(s.contains("path="));
	}

	#[test]
	fn output_inline_summary() {
		assert_eq!(format_output_inline(&json!(null), 80), "Output: none");
		assert_eq!(format_output_inline(&json!([]), 80), "Output: []");
		assert!(format_output_inline(&json!({"a": 1}), 80).starts_with("Output:"));
	}
}
