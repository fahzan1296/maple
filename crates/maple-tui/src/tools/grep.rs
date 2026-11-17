//! Grep tool render-model types and helpers.
//!
//! Reference: `l/src/tools/grep.ts`.
//!
//! Deviations:
//! - [`render_call`]/[`render_result`] emit plain text rows (the reference builds
//!   themed status lines and tree lists); [`describe_call`]/[`describe_result`]
//!   port the native-view contract.
//! - Syntax highlighting (`highlightCode`), per-line hyperlinks and the
//!   grouped-output tree fold are host concerns; grouped rows fall back to a
//!   path row per `#` header plus one `code` node per body.

/// Display metadata for grep tool results.
use crate::native::describe::{code, text as ntext};
use crate::native::node::{NativeNode, NodeText, Props, Span, Tone};
use crate::tools::grouped_file_output::is_grouped_header;
use crate::tools::native_view::{
	footnote_text, inline_error_view, tool_head, NativePreview, NativeToolHead, NativeToolView,
	ToolHeadDetail,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrepToolDetails {
	/// File limit reached count.
	pub file_limit_reached: Option<usize>,
	/// Per-file limit reached count.
	pub per_file_limit_reached: Option<usize>,
	/// Lines truncated flag.
	pub lines_truncated: bool,
	/// Scope path.
	pub scope_path: Option<String>,
	/// Match count.
	pub match_count: Option<usize>,
	/// File count.
	pub file_count: Option<usize>,
	/// Files.
	pub files: Vec<String>,
	/// Per-file match counts.
	pub file_matches: Vec<GrepFileMatch>,
	/// Truncated flag.
	pub truncated: bool,
	/// Error text.
	pub error: Option<String>,
	/// Pre-formatted display content (TUI-friendly).
	pub display_content: Option<String>,
	/// Search path (absolute base/original).
	pub search_path: Option<String>,
	/// Cwd at search time.
	pub cwd: Option<String>,
	/// Missing user-supplied paths.
	pub missing_paths: Vec<String>,
	/// Files skipped as binary with matches.
	pub binary_matches: Vec<String>,
}

/// Per-file match count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrepFileMatch {
	/// Path.
	pub path: String,
	/// Match count.
	pub count: usize,
}

/// Grep render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrepRenderArgs {
	/// Pattern.
	pub pattern: Option<String>,
	/// Path/path list.
	pub path: Vec<String>,
	/// Legacy `paths`.
	pub paths: Vec<String>,
	/// Case-sensitive flag (`Some(false)` => insensitive in TS when explicitly false).
	pub case_sensitive: Option<bool>,
	/// Gitignore flag.
	pub gitignore: Option<bool>,
	/// Skip count.
	pub skip: Option<usize>,
}

/// Format count as `"N match"`/`"N matches"` style.
pub fn format_count(kind: &str, n: usize) -> String {
	if n == 1 {
		format!("1 {kind}")
	} else {
		format!("{n} {kind}s")
	}
}

/// Replace tabs with spaces.
pub fn replace_tabs(s: &str) -> String {
	s.replace('\t', " ")
}

/// Path list from args (path or legacy paths).
pub fn to_path_list(args: &GrepRenderArgs) -> Vec<String> {
	if !args.path.is_empty() {
		args.path.clone()
	} else {
		args.paths.clone()
	}
}

/// Flag meta differing from defaults: `case:insensitive`, `gitignore:false`, `skip:N`.
pub fn grep_flag_meta(args: &GrepRenderArgs) -> Vec<String> {
	let mut meta = Vec::new();
	if args.case_sensitive == Some(false) {
		meta.push("case:insensitive".to_string());
	}
	if args.gitignore == Some(false) {
		meta.push("gitignore:false".to_string());
	}
	if let Some(skip) = args.skip.filter(|&s| s > 0) {
		meta.push(format!("skip:{skip}"));
	}
	meta
}

/// Header subject helper: `pattern in scope` pieces (plain, unhighlighted).
pub fn grep_subject_plain(
	pattern: Option<&str>,
	args: Option<&GrepRenderArgs>,
	details: Option<&GrepToolDetails>,
) -> Option<String> {
	let pat = pattern.unwrap_or("?").to_string();
	let scope = details
		.and_then(|d| d.scope_path.clone().filter(|s| !s.is_empty()))
		.or_else(|| args.map(to_path_list).filter(|p| !p.is_empty()).map(|p| p.join(", ")));
	Some(match scope {
		Some(scope) => format!("{pat} in {scope}"),
		None => pat,
	})
}

/// Native grep head meta: result meta or scope/flag args before result.
pub fn grep_native_head_meta(args: Option<&GrepRenderArgs>, result_meta: Option<&[String]>) -> Vec<String> {
	if let Some(meta) = result_meta {
		return meta.to_vec();
	}
	let mut parts = Vec::new();
	if let Some(args) = args {
		let paths = to_path_list(args);
		if !paths.is_empty() {
			parts.push(format!("in {}", paths.join(", ")));
		}
		parts.extend(grep_flag_meta(args));
	}
	parts
}

/// Native grep head: pattern target plus result `meta` or scope/flag args.
pub fn grep_native_head(args: Option<&GrepRenderArgs>, meta: Option<&[String]>) -> NativeToolHead {
	let target = args
		.and_then(|a| a.pattern.clone())
		.filter(|p| !p.is_empty())
		.unwrap_or_else(|| "?".to_string());
	let items = grep_native_head_meta(args, meta);
	NativeToolHead {
		title: Some(NodeText::Plain("Grep".to_string())),
		target: Some(NodeText::Plain(target)),
		target_kind: Some("pattern".to_string()),
		meta: if items.is_empty() {
			None
		} else {
			Some(items.into_iter().map(NodeText::Plain).collect())
		},
		..NativeToolHead::default()
	}
}

/// Files a collapsed native grep shows.
pub const NATIVE_COLLAPSED_FILES: usize = 2;

/// Collapsed/expanded text limits (line budgets).
pub const COLLAPSED_TEXT_LIMIT: usize = 40;
pub const EXPANDED_TEXT_LIMIT: usize = 400;

/// Native grep head: the pattern, then the result `meta` or, before a result,
/// the scope/flag arguments.
pub fn grep_native_head_spans(args: Option<&GrepRenderArgs>, meta: Option<&[String]>) -> Vec<Span> {
	let target = args
		.and_then(|a| a.pattern.clone())
		.filter(|p| !p.is_empty())
		.unwrap_or_else(|| "?".to_string());
	let meta_items = grep_native_head_meta(args, meta);
	let mut details = vec![ToolHeadDetail::Span(Span::styled(target, "accent"))];
	for m in meta_items {
		details.push(ToolHeadDetail::Span(Span::styled(m, "muted")));
	}
	tool_head("Grep", &details)
}

/// Whether a grep result counts as truncated.
pub fn grep_result_truncated(details: Option<&GrepToolDetails>) -> bool {
	let Some(details) = details else {
		return false;
	};
	details.truncated
		|| details.lines_truncated
		|| details.file_limit_reached.is_some()
		|| details.per_file_limit_reached.is_some()
}

/// Grouped search rows: a path row per `#` header, then its block as code.
fn search_body_nodes(lines: &[String]) -> Vec<NativeNode> {
	let mut nodes = Vec::new();
	let mut block: Vec<String> = Vec::new();
	let flush = |block: &mut Vec<String>, nodes: &mut Vec<NativeNode>| {
		if block.is_empty() {
			return;
		}
		nodes.push(code(block.join("\n"), Props::new()));
		block.clear();
	};
	for line in lines {
		if is_grouped_header(line) {
			flush(&mut block, &mut nodes);
			let label = line.trim_start_matches('#').trim().to_string();
			nodes.push(ntext(NodeText::Spans(vec![Span::styled(label, "accent")]), Props::new()));
			continue;
		}
		block.push(line.clone());
	}
	flush(&mut block, &mut nodes);
	nodes
}

/// Native describe of a grep call.
pub fn describe_call(args: Option<&GrepRenderArgs>) -> NativeToolView {
	NativeToolView {
		head: Some(NodeText::Spans(grep_native_head_spans(args, None))),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native describe of a grep result.
pub fn describe_result(
	content_text: &str,
	details: Option<&GrepToolDetails>,
	is_error: bool,
) -> NativeToolView {
	if is_error || details.and_then(|d| d.error.as_deref()).is_some() {
		let message = details
			.and_then(|d| d.error.clone())
			.filter(|e| !e.trim().is_empty())
			.unwrap_or_else(|| {
				let t = content_text.trim();
				if t.is_empty() { "Unknown error".to_string() } else { t.to_string() }
			});
		return inline_error_view(
			NativeToolHead { title: Some(NodeText::Plain("Grep".into())), ..Default::default() },
			&message,
		);
	}
	let text_content = details
		.and_then(|d| d.display_content.clone())
		.unwrap_or_else(|| content_text.to_string());
	let missing_note = details
		.filter(|d| !d.missing_paths.is_empty())
		.map(|d| format!("skipped missing: {}", d.missing_paths.join(", ")));
	let binary_note = details
		.filter(|d| !d.binary_matches.is_empty())
		.map(|d| format!("binary matches: {}", format_count("file", d.binary_matches.len())));
	let scope = details
		.and_then(|d| d.scope_path.clone())
		.filter(|p| !p.is_empty())
		.map(|p| format!("in {p}"));
	let has_detailed = details.map_or(false, |d| d.match_count.is_some() || d.file_count.is_some());
	let match_count = details.and_then(|d| d.match_count).unwrap_or(0);
	let file_count = details.and_then(|d| d.file_count).unwrap_or(0);
	if (!has_detailed && (text_content.trim().is_empty() || text_content == "No matches found"))
		|| (has_detailed && match_count == 0)
	{
		let mut head_meta: Vec<String> = scope.into_iter().collect();
		head_meta.push("0 matches".to_string());
		if grep_result_truncated(details) { head_meta.push("truncated".to_string()); }
		let head = NodeText::Spans(grep_native_head_spans(None, Some(&head_meta)));
		let foot = footnote_parts(&[missing_note, binary_note]);
		return NativeToolView {
			head: Some(head),
			tone: Some(Tone::Warning),
			body: foot.map(|n| vec![n]),
			inline: Some(true),
			..NativeToolView::default()
		};
	}
	let truncated = grep_result_truncated(details);
	let lines: Vec<String> = text_content.split('\n').map(str::to_string).collect();
	let single_file_scope = scope.is_some() && file_count == 1 && !lines.iter().any(|line| is_grouped_header(line));
	let counts = if has_detailed {
		if single_file_scope { Some(format_count("match", match_count)) }
		else { Some(format!("{} · {}", format_count("match", match_count), format_count("file", file_count))) }
	} else { None };
	let mut head_meta: Vec<String> = scope.into_iter().collect();
	head_meta.extend(counts);
	if truncated { head_meta.push("truncated".to_string()); }
	let head = NodeText::Spans(grep_native_head_spans(None, Some(&head_meta)));
	let mut body = search_body_nodes(&lines);
	body.extend(footnote_parts(&[missing_note, binary_note]));
	NativeToolView {
		head: Some(head),
		inline: Some(true),
		body: if body.is_empty() { None } else { Some(body) },
		preview: Some(NativePreview::Lines(EXPANDED_TEXT_LIMIT)),
		..NativeToolView::default()
	}
}

/// `a · b` footnote node from optional facts.
fn footnote_parts(parts: &[Option<String>]) -> Option<NativeNode> {
	let all: Vec<String> = parts.iter().flatten().cloned().collect();
	footnote_text(&all, None)
}

/// Plain render-call line for grep.
pub fn render_call(args: Option<&GrepRenderArgs>) -> String {
	let subject = grep_subject_plain(args.and_then(|a| a.pattern.as_deref()), args, None)
		.unwrap_or_else(|| "grep".to_string());
	format!("Grep: {subject}")
}

/// Plain render of a grep result: a summary line then the matched output.
pub fn render_result(
	content_text: &str,
	details: Option<&GrepToolDetails>,
	is_error: bool,
	expanded: bool,
) -> Vec<String> {
	if is_error {
		let message = details
			.and_then(|d| d.error.clone())
			.unwrap_or_else(|| content_text.trim().to_string());
		return vec![format!("Grep failed: {}", message.lines().next().unwrap_or("Unknown error"))];
	}
	let text_content = details
		.and_then(|d| d.display_content.clone())
		.unwrap_or_else(|| content_text.to_string());
	let has_detailed = details.map_or(false, |d| d.match_count.is_some() || d.file_count.is_some());
	let match_count = details.and_then(|d| d.match_count).unwrap_or(0);
	let file_count = details.and_then(|d| d.file_count).unwrap_or(0);
	if (!has_detailed && text_content.trim().is_empty()) || (has_detailed && match_count == 0) {
		return vec!["No matches found".to_string()];
	}
	let mut lines = vec![format!(
		"Grep: {} in {}",
		format_count("match", match_count),
		format_count("file", file_count)
	)];
	if grep_result_truncated(details) {
		lines.push("truncated".to_string());
	}
	let limit = if expanded { EXPANDED_TEXT_LIMIT } else { COLLAPSED_TEXT_LIMIT };
	let body: Vec<String> = text_content.lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect();
	let shown = body.len().min(limit);
	lines.extend(body.into_iter().take(shown));
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn flag_meta_defaults_and_overrides() {
		let args = GrepRenderArgs {
			case_sensitive: Some(false),
			gitignore: Some(false),
			skip: Some(3),
			..Default::default()
		};
		let m = grep_flag_meta(&args);
		assert_eq!(m, vec!["case:insensitive", "gitignore:false", "skip:3"]);
		assert!(grep_flag_meta(&GrepRenderArgs::default()).is_empty());
	}

	#[test]
	fn subject_and_head_meta() {
		let args = GrepRenderArgs {
			pattern: Some("foo".into()),
			path: vec!["src".into()],
			..Default::default()
		};
		let s = grep_subject_plain(Some("foo"), Some(&args), None).unwrap();
		assert_eq!(s, "foo in src");
		let meta = grep_native_head_meta(Some(&args), None);
		assert_eq!(meta, vec!["in src"]);
	}

	#[test]
	fn counts() {
		assert_eq!(format_count("match", 1), "1 match");
		assert_eq!(format_count("file", 2), "2 files");
	}

	fn details(count: usize) -> GrepToolDetails {
		GrepToolDetails {
			match_count: Some(count),
			file_count: Some(1),
			display_content: Some("# src/a.rs\n*1│foo\n 2│bar".to_string()),
			scope_path: Some("src".to_string()),
			..GrepToolDetails::default()
		}
	}

	#[test]
	fn native_head_carries_pattern_and_counts() {
		let head = grep_native_head(None, Some(&["3 matches".to_string()]));
		assert_eq!(head.target_kind.as_deref(), Some("pattern"));
		assert_eq!(head.meta.expect("meta").len(), 1);
	}

	#[test]
	fn describe_result_groups_rows() {
		let view = describe_result("", Some(&details(1)), false);
		let body = view.body.expect("body");
		assert_eq!(body.len(), 2);
		assert!(view.tool.is_some());
	}

	#[test]
	fn describe_result_warns_on_no_matches() {
		let view = describe_result("No matches found", Some(&details(0)), false);
		assert_eq!(view.tone, Some(Tone::Warning));
	}

	#[test]
	fn render_result_plain_rows() {
		let rows = render_result("", Some(&details(1)), false, false);
		assert!(rows[0].contains("1 match"));
		assert!(rows.iter().any(|row| row.contains("foo")));
		assert_eq!(render_result("", None, true, false)[0], "Grep failed: Unknown error");
	}
}
