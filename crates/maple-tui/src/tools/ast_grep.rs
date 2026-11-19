//! AST-grep tool render-model types and helpers.
//!
 //! Reference: `l/src/tools/ast-grep.ts`.
 //!
 //! Deviations:
//! - Interactive TUI component/cached state and syntax-highlighted grouped
//!   match frames/tree-link expansion not fully wired here (needs injected
//!   highlighter + tree UI components). Structural summaries/native nodes provided.
//! - Themed string helpers take `&Theme`; native describe uses token-styled
//!   spans/nodes for host theme-aware rendering.
use crate::overlays::extensions::display_text::sanitize_display_text;

/// Display metadata returned by ast-grep.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstGrepToolDetails {
	pub match_count: usize,
	pub file_count: usize,
	pub files_searched: usize,
	pub limit_reached: bool,
	pub parse_errors: Vec<String>,
	/// Total parse error count before capping.
	pub parse_errors_total: Option<usize>,
	pub scope_path: Option<String>,
	pub files: Vec<String>,
	pub file_matches: Vec<AstGrepFileMatch>,
	/// Pre-formatted TUI display content.
	pub display_content: Option<String>,
	pub search_path: Option<String>,
	pub cwd: Option<String>,
}

/// Per-file match count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstGrepFileMatch {
	pub path: String,
	pub count: usize,
}

/// AST grep render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstGrepRenderArgs {
	pub pat: Option<String>,
	pub path: Vec<String>,
	/// Legacy paths.
	pub paths: Vec<String>,
	pub skip: Option<usize>,
	/// Parse language for highlighting.
	pub lang: Option<String>,
}

/// Format count as `N match(es)/file(s)`.
pub fn format_count(kind: &str, n: usize) -> String {
	if n == 1 {
		format!("1 {kind}")
	} else {
		format!("{n} {kind}s")
	}
}

/// Resolve scope paths from args.
pub fn scope_paths(args: Option<&AstGrepRenderArgs>) -> Vec<String> {
	let Some(a) = args else {
		return Vec::new();
	};
	if !a.path.is_empty() {
		a.path.clone()
	} else {
		a.paths.clone()
	}
}

/// Subject `pattern in scope`.
pub fn ast_grep_subject_plain(
	pattern: Option<&str>,
	args: Option<&AstGrepRenderArgs>,
	scope_path: Option<&str>,
) -> Option<String> {
	let pat = pattern.filter(|s| !s.is_empty());
	let scope = scope_path
		.filter(|s| !s.is_empty())
		.map(|s| s.to_string())
		.or_else(|| {
			let p = scope_paths(args);
			if p.is_empty() { None } else { Some(p.join(", ")) }
		});
	match (pat, scope) {
		(Some(p), Some(s)) => Some(format!("{p} in {s}")),
		(Some(p), None) => Some(p.to_string()),
		(None, Some(s)) => Some(format!("in {s}")),
		(None, None) => None,
	}
}

/// Call meta: scope and skip.
pub fn ast_grep_call_meta(args: Option<&AstGrepRenderArgs>) -> Vec<String> {
	let mut meta = Vec::new();
	let paths = scope_paths(args);
	if !paths.is_empty() {
		meta.push(format!("in {}", paths.join(", ")));
	}
	if let Some(skip) = args.and_then(|a| a.skip).filter(|&s| s > 0) {
		meta.push(format!("skip:{skip}"));
	}
	meta
}

/// Result meta/count summary.
pub fn ast_grep_result_meta(details: Option<&AstGrepToolDetails>) -> (usize, usize, usize, bool) {
	let Some(d) = details else {
		return (0, 0, 0, false);
	};
	(d.match_count, d.file_count, d.files_searched, d.limit_reached)
}

/// Parse error count label.
pub fn format_parse_errors_count_label(parse_errors: &[String], total: Option<usize>) -> String {
	let shown = parse_errors.len();
	let total = total.unwrap_or(shown);
	if total > shown {
		format!("{total} parse issues ({shown} shown)")
	} else if total == 1 {
		"1 parse issue".to_string()
	} else {
		format!("{total} parse issues")
	}
}

/// Plain render of an AST grep call.
pub fn render_call(args: Option<&AstGrepRenderArgs>) -> String {
	let subject = ast_grep_subject_plain(args.and_then(|a| a.pat.as_deref()), args, None);
	subject.unwrap_or_else(|| "ast_grep".to_string())
}

/// Plain render of an AST grep result.
pub fn render_result(details: Option<&AstGrepToolDetails>, output: &str, is_error: bool) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		return vec![if msg.is_empty() { "ast_grep failed".into() } else { format!("ast_grep failed: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	let (matches, file_count, parse_error_count, limit_reached) = ast_grep_result_meta(details);
	if matches == 0 && parse_error_count == 0 {
		return vec!["No matches".into()];
	}
	let mut lines = vec![format!(
		"{} in {}",
		format_count("match", matches),
		format_count("file", file_count)
	)];
	if parse_error_count > 0 {
		lines.push(format_parse_errors_count_label(
			details.map(|d| d.parse_errors.as_slice()).unwrap_or(&[]),
			details.and_then(|d| d.parse_errors_total),
		));
	}
	if limit_reached {
		lines.push("limit reached".into());
	}
    if !output.trim().is_empty() {
        lines.push(sanitize_display_text(output.trim_end()));
    }
	lines
}

/// Collapsed preview budget: two match rows per collapsed preview line.
const COLLAPSED_MATCH_LIMIT: usize = crate::render::render_utils::PREVIEW_LIMITS.COLLAPSED_LINES * 2;

/// Whether a grouped-output row is a match row (`*12│…`).
fn is_match_row(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix('*') else {
        return false;
    };
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let tail = &rest[digits..];
    tail.starts_with('│') || tail.starts_with(':') || tail.starts_with('|')
}

/// Header depth for a `#`/`##` grouped-output header, if the line is one.
pub(crate) fn header_depth(line: &str) -> Option<usize> {
    let depth = line.chars().take_while(|c| *c == '#').count();
    if depth == 0 {
        return None;
    }
    line[depth..].starts_with(char::is_whitespace).then_some(depth)
}

/// Whether a header names a directory (`name/`).
pub(crate) fn is_dir_header(line: &str) -> bool {
    let Some(depth) = header_depth(line) else {
        return false;
    };
    let rest = line[depth..].trim_end();
    !rest.contains("://") && rest.ends_with('/')
}

/// Split line indices into blank-line-separated groups.
pub(crate) fn group_line_indices_by_blank(lines: &[&str]) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    if lines.iter().any(|line| line.trim().is_empty()) {
        let mut current = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            } else {
                current.push(index);
            }
        }
        if !current.is_empty() {
            groups.push(current);
        }
    } else {
        let current: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, _)| index)
            .collect();
        if !current.is_empty() {
            groups.push(current);
        }
    }
    groups
}

/// One group per file header; directory headers are dropped.
pub(crate) fn groups_by_file(groups: &[Vec<usize>], lines: &[&str]) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    for group in groups {
        let mut current = Vec::new();
        for &index in group {
            let line = lines[index];
            if is_dir_header(line) {
                continue;
            }
            if header_depth(line).is_some() && !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
            current.push(index);
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    result
}

/// Fit line groups into `budget` rows; whole groups first, then the next group
/// cut short. The summary counts hidden units.
pub(crate) fn fit_groups_to_budget(
    groups: &[Vec<usize>],
    budget: usize,
    gap: usize,
    is_unit: impl Fn(usize) -> bool,
    unit: &str,
    total: Option<usize>,
) -> (Vec<Vec<usize>>, String) {
    let total_lines: usize = groups.iter().map(Vec::len).sum::<usize>() + groups.len().saturating_sub(1) * gap;
    if total_lines <= budget {
        return (groups.to_vec(), String::new());
    }
    if budget == 0 {
        return (Vec::new(), String::new());
    }
    let room = budget - 1;
    let mut shown: Vec<Vec<usize>> = Vec::new();
    let mut used = 0usize;
    let mut shown_units = 0usize;
    for group in groups {
        let lead = if shown.is_empty() { 0 } else { gap };
        let take = group.len().min(room.saturating_sub(used).saturating_sub(lead));
        if take == 0 || (take < group.len().min(2) && !shown.is_empty()) {
            break;
        }
        let part: Vec<usize> = if take == group.len() {
            group.clone()
        } else {
            group[..take].to_vec()
        };
        shown_units += part.iter().filter(|&&index| is_unit(index)).count();
        used += take;
        let partial = take != group.len();
        shown.push(part);
        if partial {
            break;
        }
    }
    let all_units = total.unwrap_or_else(|| {
        groups
            .iter()
            .flat_map(|group| group.iter())
            .filter(|&&index| is_unit(index))
            .count()
    });
    let hidden_units = all_units.saturating_sub(shown_units);
    if hidden_units > 0 {
        return (shown, crate::render::render_utils::format_more_items(hidden_units, unit));
    }
    let hidden_lines = total_lines.saturating_sub(used);
    if hidden_lines > 0 {
        (shown, crate::render::render_utils::format_more_items(hidden_lines, "line"))
    } else {
        (shown, String::new())
    }
}

/// Body rows of the collapsed (or expanded) match preview: match groups fitted
/// to [`COLLAPSED_MATCH_LIMIT`], capture (`meta:`) rows dropped when collapsed.
pub fn render_result_collapsed(
    details: Option<&AstGrepToolDetails>,
    output: &str,
    expanded: bool,
) -> Vec<String> {
    let (matches, _files, _searched, limit_reached) = ast_grep_result_meta(details);
    if matches == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = output.split('\n').collect();
    let file_groups = groups_by_file(&group_line_indices_by_blank(&lines), &lines);
    let mut kept: Vec<Vec<usize>> = file_groups
        .into_iter()
        .filter(|group| {
            let first = lines[group[0]];
            !first.starts_with("Result limit reached") && !first.starts_with("Parse issues:")
        })
        .collect();
    if !expanded {
        for group in &mut kept {
            group.retain(|&index| !lines[index].starts_with("  meta:"));
        }
    }
    let budget = if expanded { usize::MAX } else { COLLAPSED_MATCH_LIMIT };
    let (shown, summary) = fit_groups_to_budget(
        &kept,
        budget,
        0,
        |index| is_match_row(lines[index]),
        "match",
        Some(matches),
    );
    let mut body = Vec::new();
    for group in &shown {
        for &index in group {
            body.push(lines[index].to_string());
        }
    }
    if !summary.is_empty() {
        body.push(summary);
    }
    if limit_reached {
        body.push("limit reached; narrow path or use skip".into());
    }
    if let Some(details) = details {
        if !details.parse_errors.is_empty() {
            body.push(format_parse_errors_count_label(
                &details.parse_errors,
                details.parse_errors_total,
            ));
        }
    }
    body
}

/// Native describe of an AST grep call.
pub fn describe_call(args: Option<&AstGrepRenderArgs>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let subject = ast_grep_subject_plain(args.and_then(|a| a.pat.as_deref()), args, None)
		.unwrap_or_else(|| "ast_grep".to_string());
	let mut details = vec![ToolHeadDetail::Span(Span::styled(subject, "accent"))];
	for m in ast_grep_call_meta(args) {
		details.push(ToolHeadDetail::Span(Span::styled(m, "dim")));
	}
	NativeToolView {
		head: Some(NodeText::Spans(tool_head("AST Grep", &details))),
		tone: Some(Tone::Accent),
		..NativeToolView::default()
	}
}

/// Resolve a grouped-output header path to a file link, keeping the original
/// path (terminal controls included) out of the visible label.
fn ast_grep_file_href(details: Option<&AstGrepToolDetails>, raw: &str) -> Option<String> {
    let details = details?;
    let base = details.cwd.as_deref().or(details.search_path.as_deref())?;
    let path = if std::path::Path::new(raw).is_absolute() {
        raw.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), raw)
    };
    crate::render::hyperlink::try_file_uri(
        &path,
        crate::render::hyperlink::FilePosition::default(),
        false,
    )
}

/// Native describe of an AST grep result.

pub fn describe_result(
	details: Option<&AstGrepToolDetails>,
	output: &str,
	is_error: bool,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, footnote_text, note_text, tool_head, NativeToolView, ToolHeadDetail};
	if is_error {
		let msg = output.trim();
		return error_view("AST Grep", if msg.is_empty() { "failed" } else { msg }, &[]);
	}
	let (matches, file_count, parse_error_count, limit_reached) = ast_grep_result_meta(details);
	let head = tool_head(
		"AST Grep",
		&[
			ToolHeadDetail::Span(Span::styled(format_count("match", matches), "accent")),
			ToolHeadDetail::Span(Span::styled(format_count("file", file_count), "muted")),
		],
	);
	if matches == 0 && parse_error_count == 0 {
		return NativeToolView {
			head: Some(NodeText::Spans(head)),
			body: Some(vec![note_text("No matches", "dim", None)]),
			tone: Some(Tone::Muted),
			..NativeToolView::default()
		};
	}
    let mut children = Vec::new();
    if !output.trim().is_empty() {
        for line in output.trim_end().lines() {
            let mut p = Props::new();
            p.set_str("wrap", "word");
            if let Some(depth) = header_depth(line) {
                let raw = line[depth..].trim().to_string();
                let mut span = Span::styled(sanitize_display_text(&raw), "strong");
                span.href = ast_grep_file_href(details, &raw);
                children.push(ntext(NodeText::Spans(vec![span]), p));
            } else {
                children.push(ntext(NodeText::Plain(sanitize_display_text(line)), p));
            }
        }
    }
	let mut stats = Vec::new();
	if parse_error_count > 0 {
		stats.push(format_parse_errors_count_label(
			details.map(|d| d.parse_errors.as_slice()).unwrap_or(&[]),
			details.and_then(|d| d.parse_errors_total),
		));
	}
	if limit_reached {
		stats.push("limit reached".into());
	}
	if let Some(foot) = footnote_text(&stats, None) {
		children.push(foot);
	}
	NativeToolView {
		head: Some(NodeText::Spans(head)),
		body: if children.is_empty() { None } else { Some(children) },
		tone: Some(Tone::Success),
		..NativeToolView::default()
	}
}

/// Themed plain render of AST grep result summary.
pub fn render_result_themed(details: Option<&AstGrepToolDetails>, output: &str, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		let head = theme.fg("error", "ast_grep failed").unwrap_or_else(|_| "ast_grep failed".into());
		return vec![if msg.is_empty() { head } else { format!("{head}: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	render_result(details, output, is_error).into_iter().enumerate().map(|(i, line)| {
		if i == 0 { theme.fg(if line.starts_with("No matches") { "dim" } else { "success" }, &line).unwrap_or(line) }
		else if line.contains("parse") || line.contains("limit") { theme.fg("warning", &line).unwrap_or(line) }
		else { theme.fg("toolOutput", &line).unwrap_or(line) }
	}).collect()
}
#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let args = AstGrepRenderArgs { pat: Some("foo".into()), ..Default::default() };
		assert!(render_call(Some(&args)).contains("foo"));
		let c = describe_call(Some(&args));
		assert!(c.head.is_some());
		let d = AstGrepToolDetails { match_count: 2, file_count: 1, ..Default::default() };
		let lines = render_result(Some(&d), "x", false);
		assert!(lines[0].contains("2 matches"));
		let r = describe_result(Some(&d), "x", false);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn subject_and_meta() {
		let args = AstGrepRenderArgs {
			pat: Some("foo".into()),
			path: vec!["src".into()],
			skip: Some(2),
			..Default::default()
		};
		assert_eq!(
			ast_grep_subject_plain(Some("foo"), Some(&args), None).as_deref(),
			Some("foo in src")
		);
		assert_eq!(ast_grep_call_meta(Some(&args)), vec!["in src", "skip:2"]);
	}

	#[test]
	fn counts_and_parse_label() {
		assert_eq!(format_count("match", 1), "1 match");
		assert_eq!(format_count("file", 3), "3 files");
		assert_eq!(
			format_parse_errors_count_label(&["a".into()], Some(5)),
			"5 parse issues (1 shown)"
		);
	}
}
