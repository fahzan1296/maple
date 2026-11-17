//! Write tool render-model types and helpers.
//!
//! Reference: `l/src/tools/write.ts`.
//!
//! Deviations:
//! - [`render_call`]/[`render_result`] emit plain text rows (the reference builds
//!   streaming highlighted cards); [`describe_call`]/[`describe_result`] port the
//!   native-view contract for file writes.
//! - Device writes (`xd://`, `agent://`, `proc://`, `cfg://`) delegate to their
//!   own modules ([`crate::tools::xdev`], [`crate::tools::cfg_render`],
//!   [`crate::tools::proc_render`]) instead of the reference's URL card table.
//! - LSP diagnostics sections are a host concern; the port carries their badge
//!   and detail flags only.

use crate::native::describe::{code, md, span, text};
use crate::native::node::{NativeNode, NodeText, Props, Tone};
use crate::tools::native_view::{
	error_text, NativePreview, NativeToolHead, NativeToolView, ToolBadge,
};

use std::path::Path;

/// Figure fence languages for files drawn as figures in transcripts.
pub const FIGURE_FENCES: &[(&str, &str)] = &[
	("svg", "svg"),
	("mmd", "mermaid"),
	("mermaid", "mermaid"),
	("obj", "obj"),
	("ply", "ply"),
	("wrl", "wrl"),
	("vrml", "vrml"),
	("x3dv", "x3dv"),
	("stl", "stl"),
	("gltf", "gltf"),
	("usda", "usda"),
	("usd", "usd"),
];

/// Collapsed native write preview lines.
pub const NATIVE_WRITE_PREVIEW_LINES: usize = 8;
/// Streaming/final preview line cap used in TS renderer.
pub const WRITE_PREVIEW_LINES: usize = 6;
/// Collapsed clamp of a write drawn as a figure.
pub const NATIVE_FIGURE_PREVIEW_LINES: usize = 26;

/// Details returned by the write tool for transcript rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteToolDetails {
	/// Auto-chmod due to shebang.
	pub made_executable: bool,
	/// Absolute resolved path.
	pub resolved_path: Option<String>,
	/// xdev dispatch marker (present when dispatched).
	pub has_xdev: bool,
	/// Agent/message dispatch marker.
	pub has_message: bool,
	/// Proc dispatch marker.
	pub has_proc: bool,
	/// Cfg dispatch marker.
	pub has_cfg: bool,
	/// Touched no files.
	pub touches_no_files: bool,
}

/// Write render args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteRenderArgs {
	/// Path.
	pub path: Option<String>,
	/// Alternate path key.
	pub file_path: Option<String>,
	/// Content.
	pub content: Option<String>,
}

/// Resolve raw write path from args.
pub fn write_path(args: Option<&WriteRenderArgs>) -> Option<String> {
	args.and_then(|a| a.file_path.clone().or_else(|| a.path.clone()))
		.filter(|s| !s.is_empty())
}

/// Normalize content for display/figure checks.
pub fn normalize_display_text(text: &str) -> String {
	text.replace('\r', "")
}

/// Count lines in text.
pub fn count_lines(text: &str) -> usize {
	if text.is_empty() {
		0
	} else {
		text.split('\n').count()
	}
}

/// Figure fence language for a path, if any.
pub fn figure_fence_lang(raw_path: &str) -> Option<&'static str> {
	let ext = Path::new(raw_path)
		.extension()
		.and_then(|e| e.to_str())
		.unwrap_or("")
		.to_ascii_lowercase();
	FIGURE_FENCES
		.iter()
		.find_map(|(k, v)| if *k == ext { Some(*v) } else { None })
}

/// Whether this write should be treated as a figure write.
pub fn is_figure_write(args: Option<&WriteRenderArgs>, is_error: bool, has_xdev: bool) -> bool {
	if is_error || has_xdev {
		return false;
	}
	let Some(path) = write_path(args) else {
		return false;
	};
	if figure_fence_lang(&path).is_none() {
		return false;
	}
	let Some(content) = args.and_then(|a| a.content.as_deref()) else {
		return false;
	};
	content.chars().any(|c| !c.is_whitespace())
}

/// Plain write summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteSummary {
	pub path: Option<String>,
	pub line_count: usize,
	pub figure_lang: Option<String>,
	pub is_error: bool,
	pub made_executable: bool,
	pub touches_no_files: bool,
}

/// Build a plain write summary.
pub fn summarize_write(
	args: Option<&WriteRenderArgs>,
	details: Option<&WriteToolDetails>,
	is_error: bool,
) -> WriteSummary {
	let path = write_path(args);
	let content = args.and_then(|a| a.content.as_deref()).unwrap_or("");
	let norm = normalize_display_text(content);
	let figure_lang = path
		.as_deref()
		.and_then(figure_fence_lang)
		.filter(|_| norm.chars().any(|c| !c.is_whitespace()) && !is_error && !details.map(|d| d.has_xdev).unwrap_or(false))
		.map(|s| s.to_string());
	WriteSummary {
		path,
		line_count: count_lines(&norm),
		figure_lang,
		is_error,
		made_executable: details.map(|d| d.made_executable).unwrap_or(false),
		touches_no_files: details.map(|d| d.touches_no_files).unwrap_or(false),
	}
}

/// Native write head shared by call and result: `Write · path · N lines`, with
/// executable and figure chips.
pub fn write_tool_head(args: Option<&WriteRenderArgs>, details: Option<&WriteToolDetails>) -> NativeToolHead {
	let raw_path = write_path(args).unwrap_or_default();
	let content = args.and_then(|args| args.content.as_deref()).unwrap_or("");
	let line_count = count_lines(&normalize_display_text(content));
	let mut badges = Vec::new();
	if details.map(|details| details.made_executable).unwrap_or(false) {
		badges.push(ToolBadge {
			text: "executable".to_string(),
			tone: "success".to_string(),
			title: None,
		});
	}
	NativeToolHead {
		title: Some(NodeText::Plain("Write".to_string())),
		target: if raw_path.is_empty() {
			None
		} else {
			Some(NodeText::Plain(crate::tools::native_view::display_path(&raw_path)))
		},
		target_kind: Some("path".to_string()),
		href: crate::tools::native_view::file_href(
			details.and_then(|details| details.resolved_path.as_deref()),
		),
		meta: if line_count > 0 {
			Some(vec![NodeText::Plain(format!(
				"{line_count} {}",
				if line_count == 1 { "line" } else { "lines" }
			))])
		} else {
			None
		},
		badges: if badges.is_empty() { None } else { Some(badges) },
		..NativeToolHead::default()
	}
}

/// Source preview node: a fenced `code` block for a figure write, else the
/// first lines of the file.
fn describe_source(raw_path: &str, content: &str, figure_lang: Option<&str>) -> Option<NativeNode> {
	if content.is_empty() {
		return None;
	}
	if let Some(lang) = figure_lang {
		let mut props = Props::new();
		props.set_str("lang", lang);
		return Some(md(format!("```{lang}\n{content}\n```"), props));
	}
	let mut props = Props::new();
	props.set_str("path", raw_path);
	props.set_str("lang", language_from_path(raw_path));
	Some(code(content.to_string(), props))
}

/// Fence language for a path: its extension, else `text`.
fn language_from_path(raw_path: &str) -> &str {
	Path::new(raw_path)
		.extension()
		.and_then(|ext| ext.to_str())
		.unwrap_or("text")
}

/// Native describe of a write call.
pub fn describe_call(args: Option<&WriteRenderArgs>) -> NativeToolView {
	let raw_path = write_path(args).unwrap_or_default();
	let content = args
		.and_then(|args| args.content.as_deref())
		.map(normalize_display_text)
		.unwrap_or_default();
	let figure_lang = figure_fence_lang(&raw_path);
	let preview_lines = if figure_lang.is_some() {
		NATIVE_FIGURE_PREVIEW_LINES
	} else {
		NATIVE_WRITE_PREVIEW_LINES
	};
	NativeToolView {
		tool: Some(write_tool_head(args, None)),
		body: describe_source(&raw_path, &content, figure_lang).map(|node| vec![node]),
		preview: Some(NativePreview::Lines(preview_lines)),
		..NativeToolView::default()
	}
}

/// Native describe of a write result.
pub fn describe_result(
	args: Option<&WriteRenderArgs>,
	details: Option<&WriteToolDetails>,
	is_error: bool,
	is_partial: bool,
	progress_text: &str,
) -> NativeToolView {
	let raw_path = write_path(args).unwrap_or_default();
	let content = args
		.and_then(|args| args.content.as_deref())
		.map(normalize_display_text)
		.unwrap_or_default();
	let head = write_tool_head(args, details);
	if is_error {
		return NativeToolView {
			tool: Some(head),
			tone: Some(Tone::Error),
			body: Some(vec![error_text(progress_text)]),
			..NativeToolView::default()
		};
	}
	let figure_lang = if is_partial {
		None
	} else {
		figure_fence_lang(&raw_path)
	}
	.filter(|_| content.chars().any(|c| !c.is_whitespace()));
	let mut body = Vec::new();
	if is_partial && !progress_text.is_empty() {
		let mut props = Props::new();
		props.set_str("truncate", "end");
		body.push(text(
			NodeText::Spans(vec![span(progress_text.to_string(), Some("muted"))]),
			props,
		));
	}
	body.extend(describe_source(&raw_path, &content, figure_lang));
	let preview_lines = if figure_lang.is_some() {
		NATIVE_FIGURE_PREVIEW_LINES
	} else {
		NATIVE_WRITE_PREVIEW_LINES
	};
	NativeToolView {
		tool: Some(head),
		body: if body.is_empty() { None } else { Some(body) },
		preview: Some(NativePreview::Lines(preview_lines)),
		..NativeToolView::default()
	}
}

/// Plain render-call line for write.
pub fn render_call(args: Option<&WriteRenderArgs>) -> String {
	let summary = summarize_write(args, None, false);
	match summary.path {
		Some(path) => format!(
			"Write: {} · {} {}",
			crate::tools::native_view::display_path(&path),
			summary.line_count,
			if summary.line_count == 1 { "line" } else { "lines" }
		),
		None => "Write".to_string(),
	}
}

/// Plain render of a write result: the header facts, then the previewed source.
pub fn render_result(
	args: Option<&WriteRenderArgs>,
	details: Option<&WriteToolDetails>,
	is_error: bool,
	is_partial: bool,
	progress_text: &str,
	expanded: bool,
) -> Vec<String> {
	let summary = summarize_write(args, details, is_error);
	let path = summary
		.path
		.as_deref()
		.map(crate::tools::native_view::display_path)
		.unwrap_or_else(|| "…".to_string());
	if is_error {
		let message = progress_text.trim();
		return vec![format!(
			"Write failed: {}",
			if message.is_empty() { "Unknown error" } else { message.lines().next().unwrap_or(message) }
		)];
	}
	let mut lines = vec![format!(
		"Write: {path} · {} {}",
		summary.line_count,
		if summary.line_count == 1 { "line" } else { "lines" }
	)];
	if summary.made_executable {
		lines.push("made executable!".to_string());
	}
	if is_partial && !progress_text.trim().is_empty() {
		lines.push(progress_text.trim().to_string());
	}
	let content = args
		.and_then(|args| args.content.as_deref())
		.map(normalize_display_text)
		.unwrap_or_default();
	let limit = if expanded { usize::MAX } else { WRITE_PREVIEW_LINES };
	lines.extend(content.lines().take(limit).map(str::to_string));
	if content.lines().count() > limit {
		lines.push(crate::render::render_utils::format_more_items(
			content.lines().count() - limit,
			"line",
		));
	}
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn figure_detection() {
		assert_eq!(figure_fence_lang("a.svg"), Some("svg"));
		assert_eq!(figure_fence_lang("a.mmd"), Some("mermaid"));
		assert_eq!(figure_fence_lang("a.rs"), None);
		let args = WriteRenderArgs {
			path: Some("a.svg".into()),
			content: Some("<svg/>".into()),
			..Default::default()
		};
		assert!(is_figure_write(Some(&args), false, false));
	}

	#[test]
	fn counts_and_summary() {
		assert_eq!(count_lines("a\nb"), 2);
		let args = WriteRenderArgs {
			path: Some("f.txt".into()),
			content: Some("hi\n".into()),
			..Default::default()
		};
		let s = summarize_write(Some(&args), None, false);
		assert_eq!(s.path.as_deref(), Some("f.txt"));
		assert_eq!(s.line_count, 2);
		assert!(!s.touches_no_files);
	}

	fn args() -> WriteRenderArgs {
		WriteRenderArgs {
			path: Some("src/f.rs".to_string()),
			file_path: None,
			content: Some("line one\nline two\n".to_string()),
		}
	}

	#[test]
	fn head_reports_lines_and_badges() {
		let args = args();
		let details = WriteToolDetails { made_executable: true, ..WriteToolDetails::default() };
		let head = write_tool_head(Some(&args), Some(&details));
		assert_eq!(head.target_kind.as_deref(), Some("path"));
		assert_eq!(head.meta.expect("meta").len(), 1);
		assert_eq!(head.badges.expect("badges").len(), 1);
	}

	#[test]
	fn describe_call_and_result_preview_source() {
		let args = args();
		let call = describe_call(Some(&args));
		assert!(call.body.is_some());
		let view = describe_result(Some(&args), None, false, false, "");
		assert_eq!(view.body.expect("body").len(), 1);
		let failed = describe_result(Some(&args), None, true, false, "disk full");
		assert_eq!(failed.tone, Some(Tone::Error));
	}

	#[test]
	fn render_result_plain_rows() {
		let args = args();
		let rows = render_result(Some(&args), None, false, false, "", false);
		assert!(rows[0].contains("src/f.rs"));
		assert!(rows.iter().any(|row| row.contains("line one")));
		assert!(render_result(Some(&args), None, true, false, "disk full", false)[0].starts_with("Write failed"));
	}
}
