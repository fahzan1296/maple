//! AST-edit tool render-model types and helpers.
//!
 //! Reference: `l/src/tools/ast-edit.ts`.
 //!
 //! Deviations:
//! - Interactive TUI component/cached state and injected syntax-highlighted
//!   grouped frame rendering not fully wired here. Diffs/grouped text can be
//!   rendered as code/ANSI by host; native describe emits structured nodes.
//! - Themed string helpers take `&Theme`; native describe uses token-styled
//!   spans/nodes for host theme-aware rendering.
use crate::tools::ast_grep::{fit_groups_to_budget, group_line_indices_by_blank, groups_by_file, header_depth, is_dir_header};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstEditToolDetails {
	pub total_replacements: usize,
	pub files_touched: usize,
	pub files_searched: usize,
	pub applied: bool,
	pub limit_reached: bool,
	pub parse_errors: Vec<String>,
	/// Total parse error count before capping.
	pub parse_errors_total: Option<usize>,
	pub scope_path: Option<String>,
	pub files: Vec<String>,
	pub file_replacements: Vec<AstEditFileReplacement>,
	/// Pre-formatted TUI display content (guttered).
	pub display_content: Option<String>,
	pub search_path: Option<String>,
	pub cwd: Option<String>,
}

/// Per-file replacement count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstEditFileReplacement {
	pub path: String,
	pub count: usize,
}

/// AST edit render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstEditRenderArgs {
	/// Rewrite ops (pattern/out previews).
	pub ops: Vec<AstEditOp>,
	/// Requested paths.
	pub paths: Vec<String>,
}

/// One rewrite op preview.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstEditOp {
	pub pat: Option<String>,
	pub out: Option<String>,
}

/// One-line header preview of an AST pattern.
pub fn pattern_preview(pat: Option<&str>) -> Option<String> {
	let collapsed = pat?
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	if collapsed.is_empty() {
		None
	} else {
		Some(collapsed)
	}
}

/// Single rewrite pattern preview or `N rewrites`.
pub fn rewrite_label(args: Option<&AstEditRenderArgs>) -> Option<String> {
	let count = args.map(|a| a.ops.len()).unwrap_or(0);
	if count == 1 {
		pattern_preview(args.and_then(|a| a.ops.first()).and_then(|o| o.pat.as_deref()))
	} else if count > 1 {
		Some(format!("{count} rewrites"))
	} else {
		None
	}
}

/// Subject `pattern in scope`.
pub fn ast_edit_subject_plain(
	label: Option<&str>,
	args: Option<&AstEditRenderArgs>,
	scope_path: Option<&str>,
) -> Option<String> {
	let scope = scope_path
		.filter(|s| !s.is_empty())
		.map(|s| s.to_string())
		.or_else(|| args.map(|a| a.paths.join(", ")).filter(|s| !s.is_empty()));
	match (label, scope) {
		(Some(l), Some(s)) => Some(format!("{l} in {s}")),
		(Some(l), None) => Some(l.to_string()),
		(None, Some(s)) => Some(format!("in {s}")),
		(None, None) => None,
	}
}

/// One file's proposed rewrite parsed from display content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstEditFileDiff {
	pub path: String,
	pub lines: Vec<String>,
	pub added: usize,
	pub removed: usize,
}

/// Parse simple ast-edit change rows from display content lines.
///
/// Recognizes grouped headers (`# dir/`, `## file (N replacements)`) in a
/// simplified form and change rows like `-12│old`, `+12│new`, ` 12│ctx`.
pub fn parse_ast_edit_changes(lines: &[String]) -> Vec<AstEditFileDiff> {
	let mut files: Vec<AstEditFileDiff> = Vec::new();
	let mut current: Option<AstEditFileDiff> = None;
	let mut last_old: Option<i64> = None;

	for line in lines {
		if let Some(header) = parse_file_header(line) {
			if let Some(f) = current.take() {
				if !f.lines.is_empty() {
					files.push(f);
				}
			}
			current = Some(AstEditFileDiff {
				path: header,
				..Default::default()
			});
			last_old = None;
			continue;
		}
		if line.starts_with('#') {
			// dir/other header ends current file context until file header seen
			if let Some(f) = current.take() {
				if !f.lines.is_empty() {
					files.push(f);
				}
			}
			last_old = None;
			continue;
		}
		let Some(file) = current.as_mut() else {
			continue;
		};
		if let Some((sign, num, content)) = parse_change_line(line) {
			let n = num as i64;
			let need_hunk = match last_old {
				None => true,
				Some(prev) => n > prev + 1 || n < prev,
			};
			if need_hunk {
				file.lines.push(format!("@@ -{n} +{n} @@"));
			}
			last_old = Some(n);
			match sign {
				'+' => file.added += 1,
				'-' => file.removed += 1,
				_ => {}
			}
			file.lines.push(format!("{sign}{content}"));
		}
	}
	if let Some(f) = current.take() {
		if !f.lines.is_empty() {
			files.push(f);
		}
	}
	files
}

fn parse_file_header(line: &str) -> Option<String> {
	// `## file (N replacements)` or `# file ...` (depth >=2 treated as file in simplified parse)
	let hashes = line.chars().take_while(|&c| c == '#').count();
	if hashes < 2 {
		return None;
	}
	if line.chars().nth(hashes) != Some(' ') {
		return None;
	}
	let rest = line[hashes + 1..].trim();
	if rest.ends_with('/') {
		return None;
	}
	// strip trailing parenthesized note
	let name = if let Some(idx) = rest.rfind(" (") {
		if rest.ends_with(')') { &rest[..idx] } else { rest }
	} else {
		rest
	};
	if name.is_empty() {
		None
	} else {
		Some(name.to_string())
	}
}

fn parse_change_line(line: &str) -> Option<(char, u32, String)> {
	let mut chars = line.chars();
	let sign = chars.next()?;
	if !matches!(sign, '+' | '-' | ' ') {
		return None;
	}
	let rest: String = chars.collect();
	let trimmed = rest.trim_start();
	let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
	if digits.is_empty() {
		return None;
	}
	let after = &trimmed[digits.len()..];
	let sep = after.chars().next()?;
	if !matches!(sep, '│' | ':' | '|') {
		return None;
	}
	let content = after[sep.len_utf8()..].to_string();
	let num = digits.parse().ok()?;
	Some((sign, num, content))
}

/// Scope stats parts for final line.
pub fn ast_edit_scope_stats(details: Option<&AstEditToolDetails>) -> Vec<String> {
	let mut parts = Vec::new();
	if let Some(scope) = details.and_then(|d| d.scope_path.as_ref()).filter(|s| !s.is_empty()) {
		parts.push(format!("in {scope}"));
	}
	if let Some(n) = details.map(|d| d.files_searched).filter(|&n| n > 0) {
		parts.push(format!("searched {n} file{}", if n == 1 { "" } else { "s" }));
	}
	if details.map(|d| d.limit_reached).unwrap_or(false) {
		parts.push("limit reached; narrow path".to_string());
	}
	parts
}

/// Plain render of an AST edit call: subject or "ast_edit".
pub fn render_call(args: Option<&AstEditRenderArgs>) -> String {
	let label = rewrite_label(args);
	let subject = ast_edit_subject_plain(label.as_deref(), args, None);
	subject.unwrap_or_else(|| "ast_edit".to_string())
}

/// Plain render of an AST edit result: status-style summary lines.
pub fn render_result(details: Option<&AstEditToolDetails>, output: &str, is_error: bool) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		return vec![if msg.is_empty() { "ast_edit failed".into() } else { format!("ast_edit failed: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	let mut lines = Vec::new();
	if let Some(d) = details {
		let total = d.total_replacements;
		lines.push(format!(
			"{} in {}",
			if total == 1 { "1 replacement".into() } else { format!("{total} replacements") },
			if d.files_touched == 1 { "1 file".into() } else { format!("{} files", d.files_touched) }
		));
		for st in ast_edit_scope_stats(details) {
			lines.push(st);
		}
		for p in &d.parse_errors {
			lines.push(format!("parse error: {p}"));
		}
	}
	if lines.is_empty() {
		let t = output.trim();
		if !t.is_empty() {
			lines.push(t.lines().next().unwrap_or(t).to_string());
		} else {
			lines.push("No matches".into());
		}
	}
	lines
}

/// Collapsed preview budget: two change rows per collapsed preview line.
const COLLAPSED_CHANGE_LIMIT: usize = crate::render::render_utils::PREVIEW_LIMITS.COLLAPSED_LINES * 2;

/// Body rows of the collapsed (or expanded) per-file change preview: file
/// groups fitted to [`COLLAPSED_CHANGE_LIMIT`] with blank lines between files.
pub fn render_result_collapsed(
	details: Option<&AstEditToolDetails>,
	output: &str,
	expanded: bool,
) -> Vec<String> {
	let total = details.map(|d| d.total_replacements).unwrap_or(0);
	if total == 0 {
		return Vec::new();
	}
	let lines: Vec<&str> = output.split('\n').collect();
	let file_groups = groups_by_file(&group_line_indices_by_blank(&lines), &lines);
	let kept: Vec<Vec<usize>> = file_groups
		.into_iter()
		.filter(|group| {
			let first = lines[group[0]];
			!first.starts_with("Safety cap reached") && !first.starts_with("Parse issues:")
		})
		.collect();
	let budget = if expanded { usize::MAX } else { COLLAPSED_CHANGE_LIMIT };
	let (shown, summary) = fit_groups_to_budget(
		&kept,
		budget,
		1,
		|index| header_depth(lines[index]).is_some() && !is_dir_header(lines[index]),
		"file",
		None,
	);
	let mut body = Vec::new();
	for (position, group) in shown.iter().enumerate() {
		if position > 0 {
			body.push(String::new());
		}
		for &index in group {
			body.push(lines[index].to_string());
		}
	}
	if !summary.is_empty() {
		body.push(summary);
	}
	body
}

/// Framed header: `AST Edit: <pattern> in <scope>`.
#[must_use]
pub fn ast_edit_header(args: Option<&AstEditRenderArgs>, scope_path: Option<&str>) -> String {
	let label = rewrite_label(args);
	let subject = ast_edit_subject_plain(label.as_deref(), args, scope_path);
	format!("AST Edit: {}", subject.unwrap_or_else(|| label.unwrap_or_else(|| "?".into())))
}

/// Framed footer: proposed state plus replacement/file/search counts.
#[must_use]
pub fn ast_edit_footer(details: Option<&AstEditToolDetails>) -> String {
	let Some(details) = details else {
		return String::new();
	};
	let mut parts = Vec::new();
	if details.total_replacements == 0 {
		parts.push("0 replacements".to_string());
	} else {
		if !details.applied {
			parts.push("proposed".to_string());
		}
		parts.push(format!(
			"{} replacement{}",
			details.total_replacements,
			if details.total_replacements == 1 { "" } else { "s" }
		));
		parts.push(format!(
			"{} file{}",
			details.files_touched,
			if details.files_touched == 1 { "" } else { "s" }
		));
	}
	if details.files_searched > 0 {
		parts.push(format!("searched {}", details.files_searched));
	}
	parts.join(" · ")
}

/// Native describe of an AST edit call.
pub fn describe_call(args: Option<&AstEditRenderArgs>) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::span as nspan;
	use crate::native::node::{NodeText, Span, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let label = rewrite_label(args);
	let subject = ast_edit_subject_plain(label.as_deref(), args, None).unwrap_or_else(|| "ast_edit".to_string());
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(
			"AST Edit",
			&[ToolHeadDetail::Span(Span::styled(subject, "muted"))],
		))),
		tone: Some(Tone::Accent),
		..NativeToolView::default()
	}
}

/// Native describe of an AST edit result.
pub fn describe_result(
	details: Option<&AstEditToolDetails>,
	output: &str,
	is_error: bool,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::{code, compact, span as nspan, text as ntext};
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, footnote_text, note_text, result_text, tool_head, NativeToolView, ToolHeadDetail};
	if is_error {
		let msg = output.trim();
		return error_view("AST Edit", if msg.is_empty() { "failed" } else { msg }, &[]);
	}
	let Some(d) = details else {
		let body = output.trim_end();
		if body.is_empty() {
			return NativeToolView {
				head: Some(NodeText::Spans(tool_head("AST Edit", &[]))),
				body: Some(vec![note_text("No matches", "dim", None)]),
				..NativeToolView::default()
			};
		}
		let mut p = Props::new();
		p.set_str("wrap", "word");
		return NativeToolView {
			head: Some(NodeText::Spans(tool_head("AST Edit", &[]))),
			body: Some(vec![ntext(NodeText::Plain(body.to_string()), p)]),
			..NativeToolView::default()
		};
	};
	let total = d.total_replacements;
	let head = tool_head(
		"AST Edit",
		&[
			ToolHeadDetail::Span(Span::styled(
				if total == 1 { "1 replacement".into() } else { format!("{total} replacements") },
				"accent",
			)),
			ToolHeadDetail::Span(Span::styled(
				if d.files_touched == 1 { "1 file".into() } else { format!("{} files", d.files_touched) },
				"muted",
			)),
		],
	);
	let mut children = Vec::new();
	if !output.trim().is_empty() {
		let mut p = Props::new();
		p.set_str("path", d.search_path.clone().unwrap_or_else(|| "changes.diff".into()));
		children.push(code(output.trim_end().to_string(), p));
	} else {
		children.push(note_text("No matches", "dim", None));
	}
	if let Some(foot) = footnote_text(&ast_edit_scope_stats(details), None) {
		children.push(foot);
	}
	NativeToolView {
		head: Some(NodeText::Spans(head)),
		body: Some(children),
		tone: Some(if total > 0 { Tone::Success } else { Tone::Muted }),
		..NativeToolView::default()
	}
}

/// Themed plain render of AST edit result summary.
pub fn render_result_themed(details: Option<&AstEditToolDetails>, output: &str, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		let head = theme.fg("error", "ast_edit failed").unwrap_or_else(|_| "ast_edit failed".into());
		return vec![if msg.is_empty() { head } else { format!("{head}: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	render_result(details, output, is_error).into_iter().enumerate().map(|(i, line)| {
		if i == 0 { theme.fg(if line.contains("No matches") { "dim" } else { "success" }, &line).unwrap_or(line) }
		else if line.starts_with("parse error") { theme.fg("warning", &line).unwrap_or(line) }
		else { theme.fg("muted", &line).unwrap_or(line) }
	}).collect()
}
#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let args = AstEditRenderArgs { ops: vec![AstEditOp { pat: Some("foo".into()), ..Default::default() }], ..Default::default() };
		let s = render_call(Some(&args));
		assert!(s.contains("foo"));
		let v = describe_call(Some(&args));
		assert!(v.head.is_some());
		let d = AstEditToolDetails { files_touched: 1, total_replacements: 2, ..Default::default() };
 		let lines = render_result(Some(&d), "", false);
 		assert!(lines.iter().any(|l| l.contains("2 replacements")));
 		let r = describe_result(Some(&d), "-a\n+b\n", false);
 		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn pattern_and_label() {
		assert_eq!(pattern_preview(Some("  foo\n\tbar ")), Some("foo bar".into()));
		let args = AstEditRenderArgs {
			ops: vec![AstEditOp { pat: Some("x".into()), out: None }, AstEditOp { pat: None, out: None }],
			..Default::default()
		};
		assert_eq!(rewrite_label(Some(&args)).as_deref(), Some("2 rewrites"));
	}

	#[test]
	fn parse_changes_basic() {
		let lines = vec![
			"# src/".to_string(),
			"## a.rs (1 replacements)".to_string(),
			"-1│old".to_string(),
			"+1│new".to_string(),
		];
		let diffs = parse_ast_edit_changes(&lines);
		assert_eq!(diffs.len(), 1);
		assert_eq!(diffs[0].path, "a.rs");
		assert_eq!(diffs[0].added, 1);
		assert_eq!(diffs[0].removed, 1);
		assert!(diffs[0].lines.iter().any(|l| l.starts_with("@@")));
	}

	#[test]
	fn scope_stats_parts() {
		let d = AstEditToolDetails {
			scope_path: Some("src".into()),
			files_searched: 3,
			limit_reached: true,
			..Default::default()
		};
		let p = ast_edit_scope_stats(Some(&d));
		assert!(p.iter().any(|s| s == "in src"));
		assert!(p.iter().any(|s| s.contains("searched 3 files")));
		assert!(p.iter().any(|s| s.contains("limit reached")));
	}
}
