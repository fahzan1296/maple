//! Edit tool render-model types and helpers.
//!
//! Reference: `l/src/tools/edit.ts`.
//!
//! - [`render_call`]/[`render_result`] emit plain text rows (the reference builds
//!   framed diff cards with colored hunks); [`describe_call`]/[`describe_result`]
//!   port the native-view contract. [`render_per_file_diff_preview`] ports the
//!   streaming multi-file preview body only, without viewport tail-slicing.
//! - Hashline/apply_patch parsing and inspection, LSP diagnostics formatting and
//!   streaming preview caches are host concerns; [`EditRenderContext`] carries
//!   the previews the renderer consumes.
//! - `unknown`/dynamic args become typed fields or `serde_json::Value`-agnostic
//!   strings/vecs; inspect/preview symbols and caches are omitted.

use crate::chrome::diff::{DiffTheme, render_diff};
use crate::native::describe::{code, node, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props, Tone};
use crate::render::render_utils::replace_tabs;
use crate::status_line::segments::shorten_path;
use crate::theme::theme_class::Theme;
use crate::tools::native_view::{NativeToolHead, NativeToolView};

/// Edit payload syntax selected by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditMode {
	Replace,
	Patch,
	Hashline,
	ApplyPatch,
	Sloppy,
}

impl EditMode {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Replace => "replace",
			Self::Patch => "patch",
			Self::Hashline => "hashline",
			Self::ApplyPatch => "apply_patch",
			Self::Sloppy => "sloppy",
		}
	}
}

/// Filesystem operation represented by an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
	Create,
	Delete,
	Update,
}

impl Operation {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Create => "create",
			Self::Delete => "delete",
			Self::Update => "update",
		}
	}
}

/// Computed streaming diff or preview error for one file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PerFileDiffPreview {
	pub path: String,
	pub diff: Option<String>,
	pub first_changed_line: Option<usize>,
	pub error: Option<String>,
}

/// Result metadata for one edited file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditToolPerFileResult {
	pub path: String,
	pub diff: String,
	pub first_changed_line: Option<usize>,
	pub op: Option<Operation>,
	pub r#move: Option<String>,
	pub is_error: bool,
	pub error_text: Option<String>,
	pub display_error_text: Option<String>,
	pub snapshots_pruned: bool,
	pub source_path: Option<String>,
}

/// Diff/diagnostics/snapshots returned by an edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditToolDetails {
	pub diff: String,
	pub error_kind: Option<String>,
	pub first_changed_line: Option<usize>,
	pub op: Option<Operation>,
	pub r#move: Option<String>,
	pub per_file_results: Vec<EditToolPerFileResult>,
	pub path: Option<String>,
	pub snapshots_pruned: bool,
	pub source_path: Option<String>,
}

/// Edit render args (streaming/pending).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditRenderArgs {
	pub path: Option<String>,
	pub file_path: Option<String>,
	pub old_text: Option<String>,
	pub new_text: Option<String>,
	pub patch: Option<String>,
	pub input: Option<String>,
	pub replace_all: Option<bool>,
	pub op: Option<Operation>,
	pub rename: Option<String>,
	pub diff: Option<String>,
	pub preview_diff: Option<String>,
}

/// Resolve display path from args.
pub fn display_edit_path(args: Option<&EditRenderArgs>) -> Option<String> {
	args.and_then(|a| a.file_path.clone().or_else(|| a.path.clone()))
		.filter(|s| !s.is_empty())
}

/// Count lines in text.
pub fn count_lines(text: &str) -> usize {
	if text.is_empty() { 0 } else { text.lines().count().max(1) }
}

/// Summary for edit call/result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSummary {
	pub path: Option<String>,
	pub mode: Option<EditMode>,
	pub op: Option<Operation>,
	pub is_error: bool,
	pub error_kind: Option<String>,
	pub first_changed_line: Option<usize>,
	pub file_count: usize,
}

/// Build a plain edit summary.
pub fn summarize_edit(
	args: Option<&EditRenderArgs>,
	details: Option<&EditToolDetails>,
	mode: Option<EditMode>,
	is_error: bool,
) -> EditSummary {
	let path = details
		.and_then(|d| d.path.clone())
		.or_else(|| display_edit_path(args));
	let file_count = details.map(|d| d.per_file_results.len()).unwrap_or(0);
	let op = details.and_then(|d| d.op).or_else(|| args.and_then(|a| a.op));
	EditSummary {
		path,
		mode,
		op,
		is_error,
		error_kind: details.and_then(|d| d.error_kind.clone()),
		first_changed_line: details.and_then(|d| d.first_changed_line),
		file_count: if file_count == 0 && details.is_some() { 1 } else { file_count },
	}
}

/// Render context the transcript threads into the edit renderer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditRenderContext {
	/// Payload syntax the caller selected.
	pub edit_mode: Option<EditMode>,
	/// Per-file streaming diff previews.
	pub per_file_diff_preview: Vec<PerFileDiffPreview>,
	/// Single-file streaming diff preview.
	pub edit_diff_preview: Option<PerFileDiffPreview>,
}

/// Facts behind a native edit head: one file (path, first changed line,
/// rename) or a file count.
#[derive(Clone, Debug, Default)]
pub struct EditHeadFacts {
	pub op: Option<Operation>,
	pub path: Option<String>,
	pub files: usize,
	pub line: Option<usize>,
	pub href: Option<String>,
	pub rename: Option<String>,
	pub added: usize,
	pub removed: usize,
}

/// Native edit head: `Edit · path:line · +8 −1 · → new/path` for one file,
/// `Edit · 3 files · +21 −4` for several.
pub fn edit_tool_head(facts: &EditHeadFacts) -> NativeToolHead {
	let multi = facts.files > 1;
	let mut meta = Vec::new();
	if let Some(stats) = crate::tools::native_view::diff_stats_meta(facts.added, facts.removed) {
		meta.push(stats);
	}
	if !multi {
		if let Some(rename) = facts.rename.as_deref() {
			meta.push(format!("→ {}", crate::tools::native_view::display_path(rename)));
		}
	}
	let target = if multi {
		Some(format!("{} files", facts.files))
	} else {
		facts.path.as_deref().map(|path| {
			format!(
				"{}{}",
				crate::tools::native_view::display_path(path),
				facts.line.map(|line| format!(":{line}")).unwrap_or_default()
			)
		})
	};
	NativeToolHead {
		title: Some(NodeText::Plain(if multi {
			"Edit".to_string()
		} else {
			match facts.op {
				Some(Operation::Create) => "Create",
				Some(Operation::Delete) => "Delete",
				_ => "Edit",
			}
			.to_string()
		})),
		target: target.map(NodeText::Plain),
		target_kind: Some(if multi { "text" } else { "path" }.to_string()),
		href: if multi { None } else { facts.href.clone() },
		meta: if meta.is_empty() {
			None
		} else {
			Some(meta.into_iter().map(NodeText::Plain).collect())
		},
		..NativeToolHead::default()
	}
}

/// A file's unified diff node (no path header: the head names the file).
fn edit_diff_node(diff_text: &str, file_path: Option<&str>) -> Option<NativeNode> {
	if diff_text.trim().is_empty() {
		return None;
	}
	let mut props = Props::new();
	if let Some(path) = file_path.filter(|path| !path.is_empty()) {
		props.set_str("path", path);
	}
	Some(crate::native::describe::diff(diff_text.to_string(), props))
}

/// Diff text a call previews, from args or the streaming context.
fn edit_preview_diff(args: Option<&EditRenderArgs>, context: Option<&EditRenderContext>) -> Option<String> {
	if let Some(diff) = args.and_then(|args| args.preview_diff.clone().or_else(|| args.diff.clone())) {
		return Some(diff);
	}
	let preview = context?.edit_diff_preview.as_ref()?;
	preview.diff.clone()
}

/// Native describe of an edit call.
pub fn describe_call(args: Option<&EditRenderArgs>, context: Option<&EditRenderContext>) -> NativeToolView {
	let raw_path = display_edit_path(args).unwrap_or_default();
	let op = args.and_then(|args| args.op);
	let rename = args.and_then(|args| args.rename.clone());
	let previews = context.map(|context| context.per_file_diff_preview.as_slice()).unwrap_or(&[]);
	let mut body = Vec::new();
	let head;
	if previews.len() > 1 {
		let mut added = 0;
		let mut removed = 0;
		for preview in previews {
			let diff = preview.diff.clone().unwrap_or_default();
			let stats = crate::render::render_utils::get_diff_stats(&diff);
			added += stats.added;
			removed += stats.removed;
			body.push(node(
				NodeKind::Section,
				Props::new(),
				vec![
					text(NodeText::Plain(preview.path.clone()), Props::new()),
					match &preview.error {
						Some(error) => crate::tools::native_view::error_text(error),
						None => edit_diff_node(&diff, Some(&preview.path))
							.unwrap_or_else(|| crate::tools::native_view::note_text("", "muted", None)),
					},
				],
				None,
			));
		}
		head = edit_tool_head(&EditHeadFacts {
			op,
			files: previews.len(),
			added,
			removed,
			..EditHeadFacts::default()
		});
	} else {
		let diff = edit_preview_diff(args, context);
		let stats = diff
			.as_deref()
			.map(crate::render::render_utils::get_diff_stats)
			.unwrap_or_default();
		body.extend(edit_diff_node(diff.as_deref().unwrap_or(""), Some(&raw_path)));
		head = edit_tool_head(&EditHeadFacts {
			op,
			path: Some(raw_path.clone()).filter(|path| !path.is_empty()),
			files: 1,
			line: context.and_then(|context| context.edit_diff_preview.as_ref()).and_then(|preview| preview.first_changed_line),
			href: crate::tools::native_view::file_href(Some(&raw_path)),
			rename,
			added: stats.added,
			removed: stats.removed,
		});
	}
	if body.is_empty() {
		if let Some(patch) = args.and_then(|args| args.patch.clone().or_else(|| args.new_text.clone())) {
			let mut props = Props::new();
			if !raw_path.is_empty() {
				props.set_str("path", raw_path);
			}
			body.push(code(patch, props));
		}
	}
	NativeToolView {
		tool: Some(head),
		body: if body.is_empty() { None } else { Some(body) },
		..NativeToolView::default()
	}
}

/// One file's edit result: head facts plus its body (diff or error).
struct EditFileParts {
	op: Option<Operation>,
	path: String,
	line: Option<usize>,
	rename: Option<String>,
	added: usize,
	removed: usize,
	is_error: bool,
	body: Vec<NativeNode>,
}

fn edit_file_parts(
	details: Option<&EditToolDetails>,
	args: Option<&EditRenderArgs>,
	progress_text: &str,
	is_error: bool,
) -> EditFileParts {
	let raw_path = details
		.and_then(|details| details.source_path.clone())
		.or_else(|| details.and_then(|details| details.path.clone()))
		.or_else(|| display_edit_path(args))
		.unwrap_or_default();
	let op = details.and_then(|details| details.op).or_else(|| args.and_then(|args| args.op));
	let rename = details
		.and_then(|details| details.r#move.clone())
		.or_else(|| args.and_then(|args| args.rename.clone()));
	let diff = details.map(|details| details.diff.clone()).unwrap_or_default();
	let mut body = Vec::new();
	if is_error {
		let message = if progress_text.trim().is_empty() { "Edit failed" } else { progress_text };
		body.push(crate::tools::native_view::error_text(message));
	} else if let Some(diff_node) = edit_diff_node(&diff, Some(&raw_path)) {
		body.push(diff_node);
	} else if details.is_some() && op != Some(Operation::Delete) && op != Some(Operation::Create) && rename.is_none() {
		let shown = crate::tools::native_view::display_path(&raw_path);
		body.push(crate::tools::native_view::note_text(
			&format!(
				"No changes were made{}.",
				if shown.is_empty() { String::new() } else { format!(" to {shown}") }
			),
			"dim",
			None,
		));
	}
	let stats = crate::render::render_utils::get_diff_stats(&diff);
	EditFileParts {
		op,
		path: raw_path,
		line: details.and_then(|details| details.first_changed_line),
		rename,
		added: stats.added,
		removed: stats.removed,
		is_error,
		body,
	}
}

/// Native describe of an edit result: one section per edited file.
pub fn describe_result(
	progress_text: &str,
	details: Option<&EditToolDetails>,
	args: Option<&EditRenderArgs>,
	is_error: bool,
) -> NativeToolView {
	let per_file = details.map(|details| details.per_file_results.as_slice()).unwrap_or(&[]);
	if per_file.len() > 1 {
		let mut added = 0;
		let mut removed = 0;
		let mut failed = false;
		let body: Vec<NativeNode> = per_file
			.iter()
			.map(|file| {
				let stats = crate::render::render_utils::get_diff_stats(&file.diff);
				added += stats.added;
				removed += stats.removed;
				let mut node_body = Vec::new();
				if file.is_error {
					node_body.push(crate::tools::native_view::error_text(
						file.display_error_text
							.as_deref()
							.or(file.error_text.as_deref())
							.unwrap_or("Edit failed"),
					));
				} else if let Some(diff_node) = edit_diff_node(&file.diff, Some(&file.path)) {
					node_body.push(diff_node);
				}
				failed |= file.is_error;
				crate::tools::native_view::file_diff_section(
					&crate::tools::native_view::FileDiffSectionFile {
						path: file.path.clone(),
						rename: file.r#move.clone(),
						added: Some(stats.added),
						removed: Some(stats.removed),
					},
					node_body,
					&crate::tools::native_view::FileDiffSectionProps {
						role: "file".to_string(),
						tone: file.is_error.then_some(Tone::Error),
					},
				)
			})
			.collect();
		return NativeToolView {
			tool: Some(edit_tool_head(&EditHeadFacts {
				files: per_file.len(),
				added,
				removed,
				..EditHeadFacts::default()
			})),
			tone: failed.then_some(Tone::Error),
			body: Some(body),
			..NativeToolView::default()
		};
	}
	let parts = edit_file_parts(details, args, progress_text, is_error);
	NativeToolView {
		tool: Some(edit_tool_head(&EditHeadFacts {
			op: parts.op,
			path: Some(parts.path.clone()).filter(|path| !path.is_empty()),
			files: 1,
			line: parts.line,
			href: crate::tools::native_view::file_href(Some(&parts.path)),
			rename: parts.rename,
			added: parts.added,
			removed: parts.removed,
		})),
		tone: parts.is_error.then_some(Tone::Error),
		body: if parts.body.is_empty() { None } else { Some(parts.body) },
		..NativeToolView::default()
	}
}

/// Plain render-call line for edit.
pub fn render_call(args: Option<&EditRenderArgs>, context: Option<&EditRenderContext>) -> String {
	let path = display_edit_path(args).unwrap_or_default();
	let op = args.and_then(|args| args.op);
	let title = match op {
		Some(Operation::Create) => "Create",
		Some(Operation::Delete) => "Delete",
		_ => "Edit",
	};
	let mut line = if path.is_empty() {
		title.to_string()
	} else {
		format!("{title}: {}", crate::tools::native_view::display_path(&path))
	};
	if let Some(rename) = args.and_then(|args| args.rename.as_deref()) {
		line.push_str(&format!(" → {}", crate::tools::native_view::display_path(rename)));
	}
	if let Some(diff) = edit_preview_diff(args, context) {
		line.push_str(&diff_stats_suffix(&diff));
	}
	line
}

/// Streaming multi-file diff preview rows.
///
/// Port of `formatMultiFileStreamingDiff` (`l/src/tools/edit.ts`): one dim
/// `── path ──` header per file carrying a payload, an error row for a failed
/// preview, otherwise the colored diff rows.
///
/// Deviations: the crate has no live viewport window, so the reference's
/// visual-row tail slicing is left to the caller; `width`/`expanded` are
/// accepted for API parity. Paths are shortened against `$HOME`.
pub fn render_per_file_diff_preview(
    previews: &[PerFileDiffPreview],
    width: u16,
    expanded: bool,
    theme: &Theme,
) -> Vec<String> {
    let _ = (width, expanded);
    let home = std::env::var("HOME").unwrap_or_default();
    let diff_theme = DiffTheme::default();
    let mut rows = Vec::new();
    for preview in previews {
        if preview.diff.is_none() && preview.error.is_none() {
            continue;
        }
        let path = shorten_path(&preview.path, &home);
        rows.push(String::new());
        rows.push(String::new());
        let header = format!("── {path} ──");
        rows.push(theme.fg("dim", &header).unwrap_or(header));
        if let Some(error) = preview.error.as_deref() {
            let error = replace_tabs(error);
            rows.push(theme.fg("error", &error).unwrap_or(error));
            continue;
        }
        if let Some(diff) = preview.diff.as_deref() {
            rows.extend(render_diff(&replace_tabs(diff), &diff_theme));
        }
    }
    rows
}

/// ` · +8 −1` suffix for a diff.
fn diff_stats_suffix(diff_text: &str) -> String {
	let stats = crate::render::render_utils::get_diff_stats(diff_text);
	crate::tools::native_view::diff_stats_meta(stats.added, stats.removed)
		.map(|meta| format!(" · {meta}"))
		.unwrap_or_default()
}

/// Plain render of an edit result.
pub fn render_result(
	progress_text: &str,
	details: Option<&EditToolDetails>,
	args: Option<&EditRenderArgs>,
	is_error: bool,
	expanded: bool,
) -> Vec<String> {
	if is_error {
		return vec![format!(
			"Edit failed: {}",
			if progress_text.trim().is_empty() {
				"Unknown error"
			} else {
				progress_text.trim()
			}
		)];
	}
	let per_file = details.map(|details| details.per_file_results.as_slice()).unwrap_or(&[]);
	if per_file.len() > 1 {
		let mut lines = vec![format!("Edit: {} files", per_file.len())];
		for file in per_file {
			lines.push(format!(
				"{} {}",
				file.path,
				diff_stats_suffix(&file.diff).trim_start_matches(" · ")
			));
			if file.is_error {
				if let Some(error) = file.display_error_text.clone().or_else(|| file.error_text.clone()) {
					lines.push(format!("  {error}"));
				}
			}
		}
		return lines;
	}
	let path = display_edit_path(args)
		.or_else(|| details.and_then(|details| details.path.clone()))
		.unwrap_or_default();
	let diff = details.map(|details| details.diff.as_str()).unwrap_or("");
	let mut lines = vec![format!(
		"Edit: {}{}",
		crate::tools::native_view::display_path(&path),
		diff_stats_suffix(diff)
	)];
	if diff.trim().is_empty() {
		lines.push("No changes were made".to_string());
		return lines;
	}
	let limit = if expanded {
		usize::MAX
	} else {
		crate::render::render_utils::PREVIEW_LIMITS.DIFF_COLLAPSED_LINES
	};
	let total = diff.lines().count();
	lines.extend(diff.lines().take(limit).map(str::to_string));
	if total > limit {
		lines.push(crate::render::render_utils::format_more_items(total - limit, "line"));
	}
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mode_and_op_strings() {
		assert_eq!(EditMode::ApplyPatch.as_str(), "apply_patch");
		assert_eq!(Operation::Create.as_str(), "create");
	}

	#[test]
	fn count_lines_and_path() {
		assert_eq!(count_lines(""), 0);
		assert_eq!(count_lines("a\nb"), 2);
		let args = EditRenderArgs { file_path: Some("x.rs".into()), ..Default::default() };
		assert_eq!(display_edit_path(Some(&args)).as_deref(), Some("x.rs"));
	}

	#[test]
	fn summarize_prefers_details_path() {
		let d = EditToolDetails { path: Some("d.rs".into()), ..Default::default() };
		let s = summarize_edit(None, Some(&d), Some(EditMode::Replace), false);
		assert_eq!(s.path.as_deref(), Some("d.rs"));
		assert_eq!(s.file_count, 1);
	}

	fn diff() -> String {
		"@@ -1,2 +1,2 @@\n-old\n+new\n context".to_string()
	}

	#[test]
	fn head_reports_path_line_and_stats() {
		let head = edit_tool_head(&EditHeadFacts {
			op: Some(Operation::Update),
			path: Some("src/a.rs".to_string()),
			files: 1,
			line: Some(4),
			added: 1,
			removed: 1,
			..EditHeadFacts::default()
		});
		assert_eq!(head.target_kind.as_deref(), Some("path"));
		match head.target.expect("target") {
			NodeText::Plain(text) => assert!(text.contains("src/a.rs:4")),
			_ => panic!("expected plain target"),
		}
		let meta = head.meta.expect("meta");
		assert_eq!(meta.len(), 1);
	}

	#[test]
	fn describe_call_previews_streaming_diff() {
		let args = EditRenderArgs {
			file_path: Some("src/a.rs".to_string()),
			..EditRenderArgs::default()
		};
		let context = EditRenderContext {
			edit_diff_preview: Some(PerFileDiffPreview {
				path: "src/a.rs".to_string(),
				diff: Some(diff()),
				first_changed_line: Some(1),
				error: None,
			}),
			..EditRenderContext::default()
		};
		let view = describe_call(Some(&args), Some(&context));
		assert_eq!(view.body.expect("body").len(), 1);
	}

	#[test]
	fn describe_result_sections_per_file() {
		let details = EditToolDetails {
			per_file_results: vec![
				EditToolPerFileResult {
					path: "a.rs".to_string(),
					diff: diff(),
					..EditToolPerFileResult::default()
				},
				EditToolPerFileResult {
					path: "b.rs".to_string(),
					is_error: true,
					display_error_text: Some("boom".to_string()),
					..EditToolPerFileResult::default()
				},
			],
			..EditToolDetails::default()
		};
		let view = describe_result("", Some(&details), None, false);
		assert_eq!(view.tone, Some(Tone::Error));
		assert_eq!(view.body.expect("body").len(), 2);
	}

	#[test]
	fn render_result_plain_rows() {
		let args = EditRenderArgs {
			file_path: Some("src/a.rs".to_string()),
			diff: Some(diff()),
			..EditRenderArgs::default()
		};
		let rows = render_call(Some(&args), None);
		assert!(rows.contains("src/a.rs"));
		assert!(rows.contains("+1"));
		let details = EditToolDetails {
			path: Some("src/a.rs".to_string()),
			diff: diff(),
			..EditToolDetails::default()
		};
		let rows = render_result("", Some(&details), Some(&args), false, false);
		assert!(rows[0].contains("src/a.rs"));
		assert!(rows.iter().any(|row| row.starts_with("+new")));
		assert!(render_result("", None, Some(&args), true, false)[0].starts_with("Edit failed"));
	}
}
