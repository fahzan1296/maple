//! Read tool render-model types and path/selector helpers.
//!
//! Reference: `l/src/tools/read.ts`.
//!
//! Deviations:
//! - [`render_call`]/[`render_result`] emit plain text rows (the reference builds
//!   framed code/markdown cells and streaming previews); [`describe_call`]/
//!   [`describe_result`] port the native-view contract.
//! - `proc://`/`cfg://` reads delegate to [`crate::tools::proc_render`] and
//!   [`crate::tools::cfg_render`]; image/media blocks and OSC 8 links are host
//!   concerns.
//! - Many helpers depend on `line-ranges`, `hashline-format`, `output-meta`,
//!   and URL-scheme host modules with partial parity; implementations here are
//!   faithful where local and documented as simplified where they approximate
//!   TS regex/behavior details.

use crate::native::describe::{code, md};
use crate::native::node::{NativeNode, NodeText, Props};
use crate::tools::native_view::{
	footnote_text, inline_error_view, NativeToolHead, NativeToolView, ToolBadge,
};

use crate::tools::line_ranges::{parse_line_ranges, BareLineMode, LineRange};
use crate::tools::output_meta::format_full_output_reference;

/// Read result truncation stats (omits duplicated body content).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadTruncationStats {
	/// Truncated flag.
	pub truncated: bool,
	/// Lines or size.
	pub truncated_by: Option<String>,
	/// Artifact id.
	pub artifact_id: Option<String>,
	/// Bytes omitted from the saved artifact, when known.
	pub artifact_elided_bytes: Option<usize>,
}

/// One display content block for text-like reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadDisplayContent {
	/// Raw text without hashline/line-number prefixes.
	pub text: String,
	/// Start line.
	pub start_line: usize,
	/// Optional line numbers per row.
	pub line_numbers: Vec<Option<usize>>,
}

/// Summary info for minimized reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadSummary {
	pub lines: usize,
	pub elided_spans: usize,
	pub elided_lines: usize,
}

/// Suffix resolution info.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuffixResolution {
	pub from: String,
	pub to: String,
	pub raw: bool,
}

/// Display metadata for file and URL reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadToolDetails {
	pub kind: Option<String>, // "file" | "url"
	pub display_target: Option<String>,
	pub truncation: Option<ReadTruncationStats>,
	pub is_directory: bool,
	pub resolved_path: Option<String>,
	pub media_path: Option<String>,
	pub suffix_resolution: Option<SuffixResolution>,
	pub raw: bool,
	pub url: Option<String>,
	pub final_url: Option<String>,
	pub content_type: Option<String>,
	pub method: Option<String>,
	pub notes: Vec<String>,
	pub file_size: Option<u64>,
	pub total_lines: Option<usize>,
	pub row_prefixes: Option<bool>,
	pub display_content: Option<ReadDisplayContent>,
	pub summary: Option<ReadSummary>,
	pub conflict_count: Option<usize>,
	pub display_read_targets: Vec<String>,
	pub display_read_target_links: Vec<Option<String>>,
}

/// A bare line number selector.
pub fn is_bare_line_selector(s: &str) -> bool {
	!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Strict file selector token: line-range/list, `raw`, `conflicts`, `img`.
pub fn is_file_selector_token(token: &str) -> bool {
	let t = token.to_ascii_lowercase();
	if matches!(t.as_str(), "raw" | "conflicts" | "img") {
		return true;
	}
	// range list: chunks of range or tail separated by commas
	if token.is_empty() {
		return false;
	}
	token.split(',').all(|chunk| {
		is_bare_line_selector(chunk)
			|| is_tail_selector(chunk)
			|| is_range_like_selector(chunk)
	})
}

fn is_tail_selector(s: &str) -> bool {
	s.starts_with('-')
		&& s.len() > 1
		&& s[1..].chars().all(|c| c.is_ascii_digit())
}

fn is_range_like_selector(s: &str) -> bool {
	// simplified: digits/dashes/plus in selector-y forms (e.g. 1-2, 1-, 1+2)
	s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '+')
		&& s.chars().any(|c| c.is_ascii_digit())
		&& (s.contains('-') || s.contains('+'))
}

/// Split filesystem path from trailing read selector (simplified).
///
/// Returns `(path, selector)`; does not perform full `foldListColons`
/// normalization of all TS edge cases, but handles common `path:range`,
/// `path:raw`, `path:range:raw`, `path:raw:range` forms.
pub fn split_path_and_sel(raw_path: &str) -> (String, Option<String>) {
	let Some(colon) = raw_path.rfind(':') else {
		return (raw_path.to_string(), None);
	};
	if colon == 0 {
		return (raw_path.to_string(), None);
	}
	let candidate = &raw_path[colon + 1..];
	if !is_file_selector_token(candidate) {
		return (raw_path.to_string(), None);
	}
	let base = &raw_path[..colon];
	// compound: raw + range in either order
	let candidate_is_range = candidate
		.split(',')
		.all(|c| is_tail_selector(c) || is_range_like_selector(c) || is_bare_line_selector(c));
	let candidate_is_raw = candidate.eq_ignore_ascii_case("raw");
	if candidate_is_range || candidate_is_raw {
		if let Some(inner_colon) = base.rfind(':') {
			if inner_colon > 0 {
				let inner = &base[inner_colon + 1..];
				let inner_is_range = inner
					.split(',')
					.all(|c| is_tail_selector(c) || is_range_like_selector(c) || is_bare_line_selector(c));
				let inner_is_raw = inner.eq_ignore_ascii_case("raw");
				if (candidate_is_raw && inner_is_range) || (candidate_is_range && inner_is_raw) {
					return (
						base[..inner_colon].to_string(),
						Some(format!("{inner}:{candidate}")),
					);
				}
			}
		}
	}
	(base.to_string(), Some(candidate.to_string()))
}

/// Parse selector ranges if selector is a line-range/list.
pub fn parse_selector_ranges(sel: &str) -> Option<Vec<LineRange>> {
	// `parse_line_ranges` takes comma-separated ranges; tail `-N` is not a
	// range in that parser's form, so skip if any chunk is a pure tail token.
	if sel.split(',').any(is_tail_selector) {
		return None;
	}
	parse_line_ranges(sel, BareLineMode::default()).ok().flatten()
}

/// Format truncation/artifact reference notice text.
pub fn truncation_notice(stats: Option<&ReadTruncationStats>) -> Option<String> {
	let s = stats?;
	if !s.truncated {
		return None;
	}
	if let Some(id) = &s.artifact_id {
		Some(format!("Output truncated · {}", format_full_output_reference(id, s.artifact_elided_bytes)))
	} else {
		Some("Output truncated".to_string())
	}
}

/// Read render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadRenderArgs {
	pub path: Option<String>,
	pub file_path: Option<String>,
	/// Legacy offset.
	pub offset: Option<usize>,
	/// Legacy limit.
	pub limit: Option<usize>,
	/// Ask for raw source.
	pub raw: bool,
}

/// Raw read path: `file_path` first, then `path`.
pub fn read_raw_path(args: Option<&ReadRenderArgs>) -> String {
	args.and_then(|args| args.file_path.clone().or_else(|| args.path.clone()))
		.unwrap_or_default()
}

/// Whether a read result renders as a Markdown preview rather than numbered
/// source: tagged `text/markdown` and the call did not ask for `:raw`.
pub fn read_result_is_markdown(details: Option<&ReadToolDetails>, args: Option<&ReadRenderArgs>) -> bool {
	if details.and_then(|details| details.content_type.as_deref()) != Some("text/markdown")
		|| args.map(|args| args.raw).unwrap_or(false)
	{
		return false;
	}
	let sel = split_path_and_sel(&read_raw_path(args)).1;
	!sel.map(|sel| sel.split(':').any(|chunk| chunk.eq_ignore_ascii_case("raw")))
		.unwrap_or(false)
}

/// First line a read shows: the selector's (or legacy `offset`'s) range start.
pub fn read_range_start(raw_path: &str, args: Option<&ReadRenderArgs>) -> usize {
	let sel = split_path_and_sel(raw_path).1;
	let from_selector = sel
		.as_deref()
		.and_then(|sel| sel.split(':').find(|chunk| !chunk.eq_ignore_ascii_case("raw")))
		.and_then(|chunk| chunk.split(['-', ',', '+']).next())
		.and_then(|start| start.parse::<usize>().ok());
	from_selector
		.or_else(|| args.and_then(|args| args.offset))
		.unwrap_or(1)
}

/// Read content as one numbered, wrapped `code` block (no path header: the head
/// names the file).
pub fn read_content_code(
	content: &str,
	start: usize,
	numbers: Option<&[Option<usize>]>,
	path: Option<&str>,
) -> Vec<NativeNode> {
	if content.is_empty() {
		return Vec::new();
	}
	let line_count = content.split('\n').count();
	let mut props = Props::new();
	props.set_bool("numbers", true);
	props.set_str("wrap", "word");
	if let Some(path) = path {
		props.set_str("path", path);
	}
	match numbers.filter(|numbers| numbers.len() == line_count) {
		Some(numbers) => {
			let all_numbered = numbers.iter().all(Option::is_some);
			if all_numbered {
				props.set_i64("start", numbers[0].unwrap_or(start) as i64);
			} else {
				props.insert(
					"nums",
					serde_json::Value::Array(
						numbers
							.iter()
							.map(|n| n.map_or(serde_json::Value::Null, |n| serde_json::Value::from(n as i64)))
							.collect(),
					),
				);
			}
		}
		None => props.set_i64("start", start as i64),
	}
	vec![code(content.to_string(), props)]
}

/// Native read head: `Read path:13-36`, a `file://` link when the fs path is
/// known, and a suffix correction as meta.
pub fn read_native_head(
	args: Option<&ReadRenderArgs>,
	details: Option<&ReadToolDetails>,
) -> NativeToolHead {
	let raw_path = read_raw_path(args);
	let (path, sel) = split_path_and_sel(&raw_path);
	let suffix = details.and_then(|details| details.suffix_resolution.clone());
	let source = suffix
		.as_ref()
		.map(|suffix| suffix.to.clone())
		.unwrap_or_else(|| if path.is_empty() { raw_path.clone() } else { path.clone() });
	let mut target = crate::tools::native_view::display_path(&source);
	if let Some(sel) = &sel {
		target.push_str(&format!(":{sel}"));
	}
	let offset = args.and_then(|args| args.offset);
	let limit = args.and_then(|args| args.limit);
	if offset.is_some() || limit.is_some() {
		let start_line = offset.unwrap_or(1);
		let end_line = limit.map(|limit| start_line + limit.saturating_sub(1));
		target.push_str(&format!(
			":{start_line}{}",
			end_line.map(|end| format!("-{end}")).unwrap_or_default()
		));
	}
	let mut meta = Vec::new();
	if let Some(suffix) = &suffix {
		meta.push(format!("corrected from {}", suffix.from));
	}
	if let Some(summary) = details.and_then(|details| details.summary.as_ref()) {
		let count = summary.elided_spans;
		meta.push(format!(
			"summary: {count} elided {}",
			if count == 1 { "span" } else { "spans" }
		));
	}
	let conflicts = details.and_then(|details| details.conflict_count).unwrap_or(0);
	NativeToolHead {
		title: Some(NodeText::Plain("Read".to_string())),
		target: Some(NodeText::Plain(if target.is_empty() { "…".to_string() } else { target })),
		target_kind: Some("path".to_string()),
		href: crate::tools::native_view::file_href(
			details
				.and_then(|details| details.resolved_path.as_deref())
				.or_else(|| details.and_then(|details| details.display_target.as_deref())),
		),
		meta: if meta.is_empty() {
			None
		} else {
			Some(meta.into_iter().map(NodeText::Plain).collect())
		},
		badges: (conflicts > 0).then(|| {
			vec![ToolBadge {
				text: format!(
					"{conflicts} {}",
					if conflicts == 1 { "conflict" } else { "conflicts" }
				),
				tone: "warning".to_string(),
				title: None,
			}]
		}),
		..NativeToolHead::default()
	}
}

/// Native describe of a read call.
pub fn describe_call(args: Option<&ReadRenderArgs>) -> NativeToolView {
	NativeToolView {
		tool: Some(read_native_head(args, None)),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native describe of a read result: the content as one numbered code block (or
/// Markdown preview), over a quiet truncation footnote.
pub fn describe_result(
	content_text: &str,
	details: Option<&ReadToolDetails>,
	args: Option<&ReadRenderArgs>,
	is_error: bool,
) -> NativeToolView {
	let head = read_native_head(args, details);
	if is_error {
		let message = content_text.trim();
		let message = message.strip_prefix("Error:").unwrap_or(message).trim();
		return crate::tools::native_view::inline_error_view(
			head,
			if message.is_empty() { "Unknown error" } else { message },
		);
	}
	let display = details.and_then(|details| details.display_content.as_ref());
	let text = display.map(|display| display.text.clone()).unwrap_or_else(|| content_text.to_string());
	let fact = details
		.and_then(|details| details.resolved_path.clone())
		.map(|path| format!("resolved {path}"));
	let mut body = Vec::new();
	if read_result_is_markdown(details, args) {
		body.push(md(text, Props::new()));
	} else {
		let raw_path = read_raw_path(args);
		let start = display.map(|display| display.start_line).unwrap_or_else(|| read_range_start(&raw_path, args));
		let numbers = display.map(|display| display.line_numbers.as_slice());
		let path = split_path_and_sel(&raw_path).0;
		body.extend(read_content_code(
			&text,
			start,
			numbers,
			if path.is_empty() { None } else { Some(&path) },
		));
	}
	let notice = truncation_notice(details.and_then(|details| details.truncation.as_ref()));
	body.extend(crate::tools::native_view::footnote_text(
		&fact.iter().cloned().chain(notice).collect::<Vec<String>>(),
		None,
	));
	NativeToolView {
		tool: Some(head),
		inline: Some(true),
		body: if body.is_empty() { None } else { Some(body) },
		..NativeToolView::default()
	}
}

/// Plain render-call line for read.
pub fn render_call(args: Option<&ReadRenderArgs>) -> String {
	let raw_path = read_raw_path(args);
	let (path, sel) = split_path_and_sel(&raw_path);
	let mut line = format!("Read: {}", if path.is_empty() { raw_path.as_str() } else { path.as_str() });
	if let Some(sel) = sel {
		line.push_str(&format!(":{sel}"));
	}
	if let Some(offset) = args.and_then(|args| args.offset) {
		line.push_str(&format!(":{offset}"));
		if let Some(limit) = args.and_then(|args| args.limit) {
			line.push_str(&format!("-{}", offset + limit.saturating_sub(1)));
		}
	}
	line
}

/// Plain render of a read result.
pub fn render_result(
	content_text: &str,
	details: Option<&ReadToolDetails>,
	args: Option<&ReadRenderArgs>,
	is_error: bool,
	expanded: bool,
) -> Vec<String> {
	if is_error {
		let message = content_text.trim();
		return vec![format!(
			"Read failed: {}",
			if message.is_empty() { "Unknown error" } else { message.lines().next().unwrap_or(message) }
		)];
	}
	let display = details.and_then(|details| details.display_content.as_ref());
	let text = display.map(|display| display.text.clone()).unwrap_or_else(|| content_text.to_string());
	let limit = if expanded {
		usize::MAX
	} else {
		crate::render::render_utils::PREVIEW_LIMITS.EXPANDED_LINES
	};
	let total = text.lines().count();
	let mut lines: Vec<String> = text.lines().take(limit).map(str::to_string).collect();
	if total > limit {
		lines.push(crate::render::render_utils::format_more_items(total - limit, "line"));
	}
	lines.extend(truncation_notice(details.and_then(|details| details.truncation.as_ref())));
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn selector_detection() {
		assert!(is_file_selector_token("1-20"));
		assert!(is_file_selector_token("raw"));
		assert!(is_file_selector_token("1-2,4-5"));
		assert!(is_file_selector_token("-10"));
		assert!(!is_file_selector_token("foo"));
	}

	#[test]
	fn split_simple_and_compound() {
		let (p, s) = split_path_and_sel("src/lib.rs:10-20");
		assert_eq!(p, "src/lib.rs");
		assert_eq!(s.as_deref(), Some("10-20"));
		let (p, s) = split_path_and_sel("src/lib.rs:10-20:raw");
		assert_eq!(p, "src/lib.rs");
		assert_eq!(s.as_deref(), Some("10-20:raw"));
		let (p, s) = split_path_and_sel("src/lib.rs");
		assert_eq!(p, "src/lib.rs");
		assert!(s.is_none());
	}

	#[test]
	fn truncation_notice_text() {
		let s = ReadTruncationStats {
			truncated: true,
			artifact_id: Some("art-1".into()),
			..Default::default()
		};
		assert!(truncation_notice(Some(&s)).unwrap().contains("art-1"));
	}

	fn args() -> ReadRenderArgs {
		ReadRenderArgs {
			path: Some("src/lib.rs:10-20".to_string()),
			file_path: None,
			offset: None,
			limit: None,
			raw: false,
		}
	}

	#[test]
	fn head_carries_selector_and_target() {
		let head = read_native_head(Some(&args()), None);
		assert_eq!(head.target_kind.as_deref(), Some("path"));
		match head.target.expect("target") {
			NodeText::Plain(text) => assert!(text.contains("10-20")),
			_ => panic!("expected plain target"),
		}
	}

	#[test]
	fn describe_result_numbers_content() {
		let details = ReadToolDetails {
			display_content: Some(ReadDisplayContent {
				text: "a\nb".to_string(),
				start_line: 10,
				line_numbers: vec![Some(10), Some(11)],
			}),
			..ReadToolDetails::default()
		};
		let view = describe_result("", Some(&details), Some(&args()), false);
		let body = view.body.expect("body");
		assert_eq!(body.len(), 1);
		let failed = describe_result("Error: boom", None, Some(&args()), true);
		assert_eq!(failed.tone, Some(crate::native::node::Tone::Error));
	}

	#[test]
	fn markdown_detection_and_render() {
		let details = ReadToolDetails {
			content_type: Some("text/markdown".to_string()),
			..ReadToolDetails::default()
		};
		assert!(read_result_is_markdown(Some(&details), None));
		let args = ReadRenderArgs { raw: true, ..args() };
		assert!(!read_result_is_markdown(Some(&details), Some(&args)));
		let rows = render_result("line\n", None, None, false, false);
		assert_eq!(rows, vec!["line".to_string()]);
		assert!(render_result("", None, None, true, false)[0].starts_with("Read failed"));
	}

	#[test]
	fn content_code_single_block() {
		let nodes = read_content_code("x\ny", 3, None, Some("a.rs"));
		assert_eq!(nodes.len(), 1);
		assert!(read_content_code("", 1, None, None).is_empty());
	}
}
