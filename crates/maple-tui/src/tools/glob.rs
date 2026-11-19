//! Glob tool render-model types and helpers.
//!
//! Reference: `l/src/tools/glob.ts`.
//!
//! Deviations:
//! - [`render_call`]/[`render_result`] emit plain text rows (the reference builds
//!   themed status lines, file lists and chips); [`describe_call`]/
//!   [`describe_result`] port the native-view contract.
//! - Link-path resolution reuses [`crate::render::hyperlink::try_file_uri`];
//!   the reference's tree/file-list layout is host-side.

use crate::native::describe::text;
use crate::native::node::{NativeNode, NodeText, Props, Span, Tone};
use crate::render::render_utils::format_more_items;
use crate::tools::native_view::{
	footnote_text, inline_error_view, note_text, NativePreview, NativeToolHead, NativeToolView, ToolBadge,
};

/// Display metadata for glob tool results.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobToolDetails {
	/// Scope path label.
	pub scope_path: Option<String>,
	/// File count.
	pub file_count: Option<usize>,
	/// Matched files.
	pub files: Vec<String>,
	/// Truncated flag.
	pub truncated: bool,
	/// Scan hit tool deadline / incomplete set.
	pub timed_out: bool,
	/// Error text.
	pub error: Option<String>,
	/// Working directory at search time.
	pub cwd: Option<String>,
	/// Missing user-supplied paths.
	pub missing_paths: Vec<String>,
	/// Result limit reached (count).
	pub result_limit_reached: Option<usize>,
}

/// Glob render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobRenderArgs {
	/// Path/pattern list.
	pub path: Vec<String>,
	/// Legacy pre-`path` argument name.
	pub paths: Vec<String>,
	/// Limit.
	pub limit: Option<usize>,
}

/// Format count as `N file(s)`.
pub fn format_count(kind: &str, n: usize) -> String {
	if n == 1 {
		format!("1 {kind}")
	} else {
		format!("{n} {kind}s")
	}
}

/// Render/search paths from args.
pub fn format_glob_render_paths(args: Option<&GlobRenderArgs>) -> String {
	let list = args
		.map(|a| if !a.path.is_empty() { a.path.as_slice() } else { a.paths.as_slice() })
		.unwrap_or(&[]);
	if list.is_empty() {
		"*".to_string()
	} else {
		list.join(", ")
	}
}

/// Native head meta: result counts/scope or call limit.
pub fn glob_head_meta(args: Option<&GlobRenderArgs>) -> Vec<String> {
	let mut meta = Vec::new();
	if let Some(limit) = args.and_then(|a| a.limit) {
		meta.push(format!("limit:{limit}"));
	}
	meta
}

/// Empty-result label given timeout/truncation state.
pub fn empty_label(timed_out: bool, truncated: bool) -> &'static str {
	if timed_out || truncated {
		"No matches before timeout (scan incomplete)"
	} else {
		"No files found"
	}
}

/// Most files a native glob lists as rows (chips threshold).
pub const NATIVE_LIST_MAX: usize = 6;
/// Collapsed preview line clamp.
pub const NATIVE_PREVIEW_LINES: usize = 8;

/// Plain summary for a glob result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobResultSummary {
	pub is_error: bool,
	pub error: Option<String>,
	pub pattern: String,
	pub file_count: Option<usize>,
	pub files: Vec<String>,
	pub scope: Option<String>,
	pub truncated: bool,
	pub timed_out: bool,
	pub missing_note: Option<String>,
	pub reasons: Vec<String>,
}

/// Build a plain glob result summary.
pub fn summarize_glob_result(
	args: Option<&GlobRenderArgs>,
	details: Option<&GlobToolDetails>,
	is_error: bool,
	fallback_text: &str,
) -> GlobResultSummary {
	let pattern = format_glob_render_paths(args);
	if is_error || details.and_then(|d| d.error.as_ref()).is_some() {
		let err = details
			.and_then(|d| d.error.clone())
			.filter(|s| !s.is_empty())
			.or_else(|| {
				let t = fallback_text.trim();
				if t.is_empty() { None } else { Some(t.to_string()) }
			})
			.unwrap_or_else(|| "Unknown error".to_string());
		return GlobResultSummary {
			is_error: true,
			error: Some(err),
			pattern,
			file_count: None,
			files: Vec::new(),
			scope: None,
			truncated: false,
			timed_out: false,
			missing_note: None,
			reasons: Vec::new(),
		};
	}
	let Some(d) = details else {
		return GlobResultSummary {
			is_error: false,
			error: None,
			pattern,
			file_count: None,
			files: Vec::new(),
			scope: None,
			truncated: false,
			timed_out: false,
			missing_note: None,
			reasons: Vec::new(),
		};
	};
	let missing_note = if d.missing_paths.is_empty() {
		None
	} else {
		Some(format!("skipped missing: {}", d.missing_paths.join(", ")))
	};
	let mut reasons = Vec::new();
	if let Some(n) = d.result_limit_reached {
		reasons.push(format!("limit {n} results"));
	}
	if d.truncated {
		reasons.push(if d.timed_out { "timed out" } else { "truncated" }.to_string());
	}
	GlobResultSummary {
		is_error: false,
		error: None,
		pattern,
		file_count: d.file_count,
		files: d.files.clone(),
		scope: d.scope_path.clone().filter(|s| !s.is_empty()),
		truncated: d.truncated || d.result_limit_reached.is_some(),
		timed_out: d.timed_out,
		missing_note,
		reasons,
	}
}

/// Native glob head: the pattern, then the call meta or result counts/scope.
pub fn glob_native_head(args: Option<&GlobRenderArgs>, meta: Option<&[String]>) -> NativeToolHead {
	let target = format_glob_render_paths(args);
	let meta = match meta {
		Some(meta) => meta.to_vec(),
		None => glob_head_meta(args),
	};
	NativeToolHead {
		title: Some(NodeText::Plain("Glob".to_string())),
		target: Some(NodeText::Plain(target)),
		target_kind: Some("pattern".to_string()),
		meta: if meta.is_empty() {
			None
		} else {
			Some(meta.into_iter().map(NodeText::Plain).collect())
		},
		..NativeToolHead::default()
	}
}

/// File rows for a native glob body: linked path rows capped at
/// [`NATIVE_LIST_MAX`], then a "more files" note.
fn glob_file_nodes(files: &[String], cwd: Option<&str>) -> Vec<NativeNode> {
	let mut nodes = Vec::new();
	let shown = files.len().min(NATIVE_LIST_MAX);
	for file in files.iter().take(shown) {
		let mut file_span = Span::styled(file.clone(), "path");
		if let Some(cwd) = cwd.filter(|_| !file.ends_with('/')) {
			file_span.href = crate::render::hyperlink::try_file_uri(
				&format!("{}/{}", cwd.trim_end_matches('/'), file),
				crate::render::hyperlink::FilePosition::default(),
				false,
			);
		}
		let mut props = Props::new();
		props.set_i64("lines", 1);
		nodes.push(crate::native::describe::text(NodeText::Spans(vec![file_span]), props));
	}
	if files.len() > shown {
		nodes.push(note_text(
			&format_more_items(files.len() - shown, "file"),
			"muted",
			None,
		));
	}
	nodes
}

/// Native describe of a glob call.
pub fn describe_call(args: Option<&GlobRenderArgs>) -> NativeToolView {
	NativeToolView {
		tool: Some(glob_native_head(args, None)),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native describe of a glob result: the file rows, then one quiet footnote for
/// limits and skipped paths.
pub fn describe_result(
	content_text: &str,
	args: Option<&GlobRenderArgs>,
	details: Option<&GlobToolDetails>,
	is_error: bool,
) -> NativeToolView {
	let summary = summarize_glob_result(args, details, is_error, content_text);
	if summary.is_error {
		let message = summary.error.unwrap_or_else(|| "Unknown error".to_string());
		return inline_error_view(glob_native_head(args, None), &message);
	}
	let fallback = content_text.trim();
	let Some(file_count) = summary.file_count else {
		if fallback.is_empty()
			|| content_text.contains("No files matching")
			|| content_text.contains("No files found")
		{
			return NativeToolView {
				tool: Some(glob_native_head(args, Some(&["0 files".to_string()]))),
				tone: Some(Tone::Warning),
				inline: Some(true),
				..NativeToolView::default()
			};
		}
		let files: Vec<String> = fallback.lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect();
		return NativeToolView {
			tool: Some(glob_native_head(args, Some(&[format_count("file", files.len())]))),
			inline: Some(true),
			body: Some(vec![crate::native::describe::col(
				glob_file_nodes(&files, None),
				Props::new(),
			)]),
			preview: Some(NativePreview::Lines(NATIVE_PREVIEW_LINES)),
			..NativeToolView::default()
		};
	};
	let scope = summary.scope.clone().map(|scope| format!("in {scope}"));
	if file_count == 0 {
		let mut meta: Vec<String> = vec!["0 files".to_string()];
		meta.extend(scope);
		let mut head = glob_native_head(args, Some(&meta));
		if summary.timed_out || summary.truncated {
			head.note = Some(NodeText::Plain("timed out".to_string()));
		}
		let foot = footnote_text(
			&summary.missing_note.iter().cloned().collect::<Vec<String>>(),
			None,
		);
		return NativeToolView {
			tool: Some(head),
			tone: Some(Tone::Warning),
			inline: Some(true),
			body: foot.map(|node| vec![node]),
			..NativeToolView::default()
		};
	}
	let mut meta: Vec<String> = vec![format_count("file", file_count)];
	meta.extend(scope);
	let mut head = glob_native_head(args, Some(&meta));
	if summary.truncated {
		head.badges = Some(vec![ToolBadge {
			text: if summary.timed_out { "timed out" } else { "truncated" }.to_string(),
			tone: "warning".to_string(),
			title: None,
		}]);
	}
	let mut body = vec![crate::native::describe::col(
		glob_file_nodes(&summary.files, details.and_then(|d| d.cwd.as_deref())),
		Props::new(),
	)];
	let mut foot_parts: Vec<String> = Vec::new();
	if !summary.reasons.is_empty() {
		foot_parts.push(format!("truncated: {}", summary.reasons.join(", ")));
	}
	foot_parts.extend(summary.missing_note);
	body.extend(footnote_text(&foot_parts, None));
	NativeToolView {
		tool: Some(head),
		inline: Some(true),
		body: Some(body),
		preview: Some(NativePreview::Lines(NATIVE_PREVIEW_LINES)),
		..NativeToolView::default()
	}
}

/// Plain render-call line for glob.
pub fn render_call(args: Option<&GlobRenderArgs>) -> String {
	format!("Glob: {}", format_glob_render_paths(args))
}

/// Plain render of a glob result.
pub fn render_result(
	content_text: &str,
	args: Option<&GlobRenderArgs>,
	details: Option<&GlobToolDetails>,
	is_error: bool,
	expanded: bool,
) -> Vec<String> {
	let summary = summarize_glob_result(args, details, is_error, content_text);
	if summary.is_error {
		return vec![format!(
			"Glob failed: {}",
			summary.error.unwrap_or_else(|| "Unknown error".to_string())
		)];
	}
	let Some(file_count) = summary.file_count else {
		let files: Vec<String> = content_text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
		let mut lines = vec![format_count("file", files.len())];
		lines.extend(files);
		return lines;
	};
	if file_count == 0 {
		let mut lines = vec![format!("{} · {}", format_count("file", 0), empty_label(summary.timed_out, summary.truncated))];
		lines.extend(summary.missing_note);
		return lines;
	}
	let mut lines = vec![format_count("file", file_count)];
	if let Some(scope) = &summary.scope {
		lines.push(format!("in {scope}"));
	}
	if summary.truncated {
		lines.push(format!("truncated: {}", summary.reasons.join(", ")));
	}
	let limit = if expanded { NATIVE_LIST_MAX * 4 } else { NATIVE_LIST_MAX };
	let shown = summary.files.len().min(limit);
	lines.extend(summary.files.iter().take(shown).cloned());
	if summary.files.len() > shown {
		lines.push(format_more_items(summary.files.len() - shown, "file"));
	}
	lines.extend(summary.missing_note);
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn paths_and_meta() {
		let args = GlobRenderArgs {
			path: vec!["src/**/*.rs".into()],
			limit: Some(10),
			..Default::default()
		};
		assert_eq!(format_glob_render_paths(Some(&args)), "src/**/*.rs");
		assert_eq!(glob_head_meta(Some(&args)), vec!["limit:10"]);
		assert_eq!(format_glob_render_paths(None), "*");
	}

	#[test]
	fn empty_label_states() {
		assert_eq!(empty_label(true, false), "No matches before timeout (scan incomplete)");
		assert_eq!(empty_label(false, false), "No files found");
	}

	#[test]
	fn summarize_empty_timed_out() {
		let d = GlobToolDetails {
			file_count: Some(0),
			truncated: true,
			timed_out: true,
			..Default::default()
		};
		let s = summarize_glob_result(None, Some(&d), false, "");
		assert!(!s.is_error);
		assert_eq!(s.file_count, Some(0));
		assert!(s.timed_out);
	}

	fn details() -> GlobToolDetails {
		GlobToolDetails {
			file_count: Some(2),
			files: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
			scope_path: Some("src".to_string()),
			..GlobToolDetails::default()
		}
	}

	#[test]
	fn describe_call_and_result_rows() {
		let view = describe_call(None);
		assert_eq!(view.tool.expect("head").target_kind.as_deref(), Some("pattern"));
		let details = details();
		let view = describe_result("", None, Some(&details), false);
		assert!(view.body.expect("body").len() >= 1);
	}

	#[test]
	fn describe_empty_glob_warns() {
		let empty = GlobToolDetails { file_count: Some(0), ..GlobToolDetails::default() };
		let view = describe_result("No files found", None, Some(&empty), false);
		assert_eq!(view.tone, Some(Tone::Warning));
	}

	#[test]
	fn render_result_lists_files() {
		let details = details();
		let rows = render_result("", None, Some(&details), false, false);
		assert_eq!(rows[0], "2 files");
		assert!(rows.iter().any(|row| row.contains("src/a.rs")));
		assert!(render_result("", None, None, true, false)[0].starts_with("Glob failed"));
	}
}
