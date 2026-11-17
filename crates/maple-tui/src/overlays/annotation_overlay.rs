//! Annotation overlay data model and pure helpers.
//!
//! Reference: `l/src/overlays/annotation-overlay.ts`.
//!
//! Deviations:
//! - Fullscreen interactive `AnnotationOverlay` component (rendering, focus,
//!   key/mouse handling, editor/scroll view, native description tree, diff
//!   rendering/wrapping, undo stack UI behavior details) is not ported because
//!   `Component`/`TUI`/`ScrollView`/`Editor`/theme/chrome/native helpers are
//!   unavailable. This module ports exported types/constants/functions and the
//!   core data/state logic (draft capture/restore, text-source replacement and
//!   line-note retarget/drop rules, annotation collections, action selection,
//!   scope/source helpers).
//! - `sanitizeText`, ANSI fitting, and diff styling are simplified to plain
//!   text transforms; host renders styled output.
//! - Callbacks/editor hooks are trait-based.

use serde::{Deserialize, Serialize};

use crate::overlays::annotation_types::{
	AnnotationOverlayDraft, CodeReviewAnnotation, CodeReviewOverlayResult, ReviewDiffFile, ReviewDiffRow,
	ReviewSourceRow, TextReviewAnnotation, TextReviewOverlayResult, TextReviewSource,
	TextReviewSourceProvenance,
};
use crate::text_shape::{Ellipsis, pad_to_width, truncate_to_width, visible_width, wrap_text_with_ansi};
use crate::theme::{Theme, default_theme};

pub const CONTINUE_CODE_REVIEW_ACTION: &str = "Continue with LLM review";
pub const PASTE_CODE_REVIEW_ACTION: &str = "Paste annotations into prompt";
pub const DISCARD_ANNOTATIONS_ACTION: &str = "Discard notes and close";

pub const CODE_REVIEW_ACTIONS: &[&str] = &[
	CONTINUE_CODE_REVIEW_ACTION,
	PASTE_CODE_REVIEW_ACTION,
	DISCARD_ANNOTATIONS_ACTION,
];
pub const TEXT_REVIEW_ACTIONS: &[&str] = &[PASTE_CODE_REVIEW_ACTION, DISCARD_ANNOTATIONS_ACTION];

pub const SOURCE_SELECTION_GUTTER_WIDTH: usize = 2;
pub const OVERLAY_TITLE: &str = "Code Review";
pub const TEXT_OVERLAY_TITLE: &str = "Annotate Text";
pub const MIN_BODY_ROWS: usize = 3;
pub const SIDEBAR_MIN_TOTAL_WIDTH: usize = 64;
pub const SIDEBAR_MIN_BODY_WIDTH: usize = 40;
pub const MAX_ANNOTATION_EDITOR_ROWS: usize = 6;
pub const NOTE_PLACEHOLDER: &str = "Write a note…";
pub const DIFF_SHEET_TITLE: &str = "Code review";
pub const TEXT_SHEET_TITLE: &str = "Annotate text";

/// Callbacks for overlay completion/warnings/editor hooks.
pub trait AnnotationOverlayCallbacks {
	fn on_complete_code(&self, result: Option<CodeReviewOverlayResult>);
	fn on_complete_text(&self, result: Option<TextReviewOverlayResult>);
	fn on_warning(&self, _message: &str) {}
	fn on_annotation_external_editor(&self, _draft: &str) -> Option<String> {
		None
	}
	fn on_external_editor(&self) {}
}

/// Focus region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FocusRegion {
	Files,
	Diff,
	Actions,
}

/// Annotation scope for chooser/editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnnotationScope {
	Line,
	File,
	Text,
}

/// Committed code annotation entry with indices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedAnnotation {
	pub file_index: usize,
	pub source_index: usize,
	pub annotation: CodeReviewAnnotation,
}

/// Committed text annotation entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedTextAnnotation {
	pub source_index: usize,
	pub annotation: TextReviewAnnotation,
}

/// Draft editor state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftEditorState {
	pub file_index: Option<usize>,
	pub source_index: usize,
	pub scope: AnnotationScopeKind,
	pub editing_annotation_index: Option<usize>,
	pub text: String,
}

/// Serializable scope kind for drafts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationScopeKind {
	Line,
	File,
	Text,
}

/// Overlay data state (logic core, no UI widgets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationOverlayState {
	pub file_index: usize,
	pub source_index: usize,
	pub action_index: usize,
	pub focus: FocusRegion,
	pub annotating: bool,
	pub annotation_scope: AnnotationScope,
	pub editing_annotation_index: Option<usize>,
	pub finished: bool,
	pub discard_armed: bool,
	pub text_source: Option<TextReviewSource>,
	pub text_lines: Vec<String>,
	pub text_source_edited: bool,
	pub annotations: Vec<CommittedAnnotation>,
	pub text_annotations: Vec<CommittedTextAnnotation>,
	pub actions: Vec<String>,
	pub editor_text: String,
}

impl Default for AnnotationOverlayState {
	fn default() -> Self {
		Self {
			file_index: 0,
			source_index: 0,
			action_index: 0,
			focus: FocusRegion::Files,
			annotating: false,
			annotation_scope: AnnotationScope::Line,
			editing_annotation_index: None,
			finished: false,
			discard_armed: false,
			text_source: None,
			text_lines: Vec::new(),
			text_source_edited: false,
			annotations: Vec::new(),
			text_annotations: Vec::new(),
			actions: CODE_REVIEW_ACTIONS.iter().map(|s| (*s).to_string()).collect(),
			editor_text: String::new(),
		}
	}
}

impl AnnotationOverlayState {
	/// Create code-review state.
	#[must_use]
	pub fn for_code_review(files: &[ReviewDiffFile]) -> Self {
		let mut s = Self::default();
		s.actions = CODE_REVIEW_ACTIONS.iter().map(|s| (*s).to_string()).collect();
		// Start on Paste (index 1) as in TS.
		s.action_index = 1;
		if files.is_empty() {
			s.focus = FocusRegion::Actions;
		}
		s
	}

	/// Create text-review state.
	#[must_use]
	pub fn for_text_source(source: TextReviewSource) -> Self {
		let mut s = Self::default();
		s.actions = TEXT_REVIEW_ACTIONS.iter().map(|x| (*x).to_string()).collect();
		s.focus = FocusRegion::Diff;
		s.text_lines = split_text_lines(&source.text);
		s.text_source = Some(source);
		s
	}

	#[must_use]
	pub fn is_text_mode(&self) -> bool {
		self.text_source.is_some()
	}

	/// Replace text source with retarget/drop logic for line annotations.
	/// Returns number of dropped line annotations.
	pub fn replace_text_source(&mut self, text: String) -> usize {
		let Some(source) = self.text_source.as_mut() else {
			return 0;
		};
		source.text = text;
		self.text_source_edited = true;
		self.text_lines = split_text_lines(&source.text);
		if self.text_lines.is_empty() {
			self.source_index = 0;
		} else {
			self.source_index = self.source_index.min(self.text_lines.len() - 1);
		}
		let mut dropped = 0usize;
		let mut kept: Vec<CommittedTextAnnotation> = Vec::new();
		for entry in std::mem::take(&mut self.text_annotations) {
			match &entry.annotation {
				TextReviewAnnotation::Text { .. } => kept.push(entry),
				TextReviewAnnotation::Line { quote, note, .. } => {
					if self.text_lines.get(entry.source_index).map(String::as_str) == Some(quote.as_str()) {
						kept.push(entry);
						continue;
					}
					let matches: Vec<usize> = self
						.text_lines
						.iter()
						.enumerate()
						.filter_map(|(i, l)| if l == quote { Some(i) } else { None })
						.collect();
					if matches.len() == 1 {
						let only = matches[0];
						kept.push(CommittedTextAnnotation {
							source_index: only,
							annotation: TextReviewAnnotation::Line {
								line: (only + 1) as u32,
								quote: quote.clone(),
								note: note.clone(),
							},
						});
					} else {
						dropped += 1;
					}
				}
			}
		}
		self.text_annotations = kept;
		dropped
	}

	#[must_use]
	pub fn get_annotations(&self) -> Vec<CodeReviewAnnotation> {
		self.annotations.iter().map(|e| e.annotation.clone()).collect()
	}

	#[must_use]
	pub fn get_text_annotations(&self) -> Vec<TextReviewAnnotation> {
		self.text_annotations.iter().map(|e| e.annotation.clone()).collect()
	}

	#[must_use]
	pub fn has_annotations(&self) -> bool {
		if self.is_text_mode() {
			!self.text_annotations.is_empty()
		} else {
			!self.annotations.is_empty()
		}
	}

	#[must_use]
	pub fn action_disabled(&self, index: usize) -> bool {
		let Some(action) = self.actions.get(index) else {
			return true;
		};
		if action == DISCARD_ANNOTATIONS_ACTION {
			return false;
		}
		let has = self.has_annotations();
		if self.is_text_mode() {
			!has
		} else {
			index == 1 && !has
		}
	}

	/// Apply discard action; returns true if actually discarded/finished now.
	pub fn confirm_discard(&mut self, discard_armed: bool) -> bool {
		if !discard_armed {
			self.discard_armed = true;
			return false;
		}
		self.annotations.clear();
		self.text_annotations.clear();
		self.text_source_edited = false;
		self.annotating = false;
		self.editing_annotation_index = None;
		self.editor_text.clear();
		self.finished = true;
		true
	}
}

/// Is a diff row a source row?
#[must_use]
pub fn is_source_row(row: &ReviewDiffRow) -> bool {
	matches!(row, ReviewDiffRow::Source(_))
}

/// Display label for a diff file.
#[must_use]
pub fn display_file_label(file: &ReviewDiffFile) -> String {
	if file.occurrence > 1 {
		format!("{} ({})", file.path, file.occurrence)
	} else {
		file.path.clone()
	}
}

/// Sanitize status text to a single safe line.
#[must_use]
pub fn sanitize_status_text(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut prev_space = false;
	for ch in text.chars() {
		let mapped = if ch.is_control() { ' ' } else { ch };
		if mapped.is_whitespace() {
			if !prev_space {
				out.push(' ');
				prev_space = true;
			}
		} else {
			out.push(mapped);
			prev_space = false;
		}
	}
	out.trim().to_string()
}

/// Split text source into annotatable lines (1-based logical lines).
#[must_use]
pub fn split_text_lines(text: &str) -> Vec<String> {
	// split on \r\n, \n, \r preserving empty segments
	let mut lines = Vec::new();
	let mut current = String::new();
	let mut chars = text.chars().peekable();
	while let Some(ch) = chars.next() {
		if ch == '\r' {
			if matches!(chars.peek(), Some('\n')) {
				chars.next();
			}
			lines.push(std::mem::take(&mut current));
		} else if ch == '\n' {
			lines.push(std::mem::take(&mut current));
		} else {
			current.push(ch);
		}
	}
	lines.push(current);
	lines
}

fn source_row_range(_details_len: usize) -> (usize, usize) {
	// Simplified: TS computes segment row range from rendered details after bare "Command:" row.
	// Without rendered detail context here, return empty; host can compute precisely.
	(0, 0)
}

/// Path split helper: `a/b/c` -> (`a/b`, `c`)
#[must_use]
pub fn split_path(path: &str) -> (&str, &str) {
	match path.rfind('/') {
		Some(i) => (&path[..i], &path[i + 1..]),
		None => ("", path),
	}
}

/// Sidebar width: 28% of the overlay, clamped to 18..=32 cells.
#[must_use]
pub fn sidebar_width(width: usize) -> usize {
	(((width * 28) + 50) / 100).clamp(18, 32)
}

/// First visible file index for the sidebar window.
#[must_use]
pub fn sidebar_start(file_index: usize, files_len: usize, rows: usize) -> usize {
	file_index.saturating_sub(rows / 2).min(files_len.saturating_sub(rows))
}

/// Sidebar badge: ` +{added}/-{removed}` plus ` ✎{count}` when annotated.
#[must_use]
pub fn sidebar_badge(file: &ReviewDiffFile, count: usize, theme: &Theme) -> String {
	let changes = theme
		.fg("dim", &format!(" +{}/-{}", file.added, file.removed))
		.unwrap_or_else(|_| format!(" +{}/-{}", file.added, file.removed));
	if count == 0 {
		changes
	} else {
		let note = theme
			.fg("warning", &format!(" ✎{count}"))
			.unwrap_or_else(|_| format!(" ✎{count}"));
		changes + &note
	}
}

/// Cells left for the filename after the cursor and badge.
#[must_use]
pub fn sidebar_label_width(file: &ReviewDiffFile, count: usize, width: usize, theme: &Theme) -> usize {
	width.saturating_sub(visible_width(&sidebar_badge(file, count, theme)) + 2)
}

/// Sidebar rows: cursor, truncated label, badge; the selected row is highlighted.
#[must_use]
pub fn sidebar_rows(
	files: &[ReviewDiffFile],
	file_index: usize,
	focus: FocusRegion,
	rows: usize,
	width: usize,
	theme: &Theme,
) -> Vec<String> {
	let start = sidebar_start(file_index, files.len(), rows);
	(0..rows)
		.map(|row_index| {
			let index = start + row_index;
			let Some(file) = files.get(index) else {
				return String::new();
			};
			let selected = index == file_index;
			let count = file.annotation_count;
			let available = sidebar_label_width(file, count, width, theme);
			let label = truncate_to_width(
				&sanitize_status_text(&display_file_label(file)),
				available,
				Ellipsis::Unicode,
			);
			let cursor = if selected {
				if focus == FocusRegion::Files {
					"› "
				} else {
					"▎ "
				}
			} else {
				"  "
			};
			let badge = sidebar_badge(file, count, theme);
			let line = pad_to_width(
				&truncate_to_width(&format!("{cursor}{label}{badge}"), width, Ellipsis::Omit),
				width,
			);
			if selected && focus == FocusRegion::Files {
				theme.bg("selectedBg", &theme.bold(&line)).unwrap_or(line)
			} else {
				theme.fg(if selected { "accent" } else { "muted" }, &line).unwrap_or(line)
			}
		})
	.collect()
}

/// Float the selected file's full path over the body rows while file focus is
/// active and the sidebar label is truncated.
#[must_use]
pub fn reveal_selected_file(
	output: &mut [String],
	files: &[ReviewDiffFile],
	file_index: usize,
	focus: FocusRegion,
	annotating: bool,
	sidebar_shown: bool,
	body_height: usize,
	width: usize,
	theme: &Theme,
) {
	let Some(file) = files.get(file_index) else {
		return;
	};
	if !sidebar_shown || focus != FocusRegion::Files || annotating {
		return;
	}
	let path_column = 4usize;
	let line_width = width.saturating_sub(path_column + 1);
	if line_width == 0 {
		return;
	}
	let selected_row = 1 + file_index - sidebar_start(file_index, files.len(), body_height + 1);
	let interior_top = 1usize;
	let last_body_row = body_height + 1;
	let interior_rows = last_body_row - interior_top + 1;
	if selected_row < interior_top || selected_row > last_body_row || interior_rows == 0 {
		return;
	}
	let label = sanitize_status_text(&display_file_label(file));
	if label.is_empty() {
		return;
	}
	let count = file.annotation_count;
	if visible_width(&label) <= sidebar_label_width(file, count, sidebar_width(width), theme) {
		return;
	}
	let mut wrapped = wrap_text_with_ansi(&label, line_width);
	if wrapped.len() > interior_rows {
		wrapped = wrapped.split_off(wrapped.len() - interior_rows);
		if let Some(first) = wrapped.first_mut() {
			*first = truncate_to_width(&format!("…{first}"), line_width, Ellipsis::Omit);
		}
	}
	let overlay_top = interior_top.max(selected_row.min(last_body_row.saturating_sub(wrapped.len()) + 1));
	let overlay_width = line_width.min(wrapped.iter().map(|line| visible_width(line)).max().unwrap_or(0) + 1);
	for (index, line) in wrapped.iter().enumerate() {
		let fitted = pad_to_width(&truncate_to_width(line, overlay_width, Ellipsis::Omit), overlay_width);
		let highlighted = theme.bg("selectedBg", &theme.bold(&fitted)).unwrap_or(fitted);
		let target_row = overlay_top + index;
		if overlay_width > 0 {
			if let Some(base) = output.get(target_row).cloned() {
				output[target_row] =
					crate::render::render_utils::composite_line_at(&base, &highlighted, path_column, overlay_width, width);
			}
		}
	}
}

/// Frame cells the overlay reserves around the note editor.
pub const ANNOTATION_EDITOR_FRAME_CELLS: usize = 4;
/// Prompt gutter shown before the note text.
pub const ANNOTATION_EDITOR_GUTTER: &str = "> ";

/// Note editor content width at an overlay width.
#[must_use]
pub fn editor_content_width(overlay_width: usize) -> usize {
	let editor_width = overlay_width.saturating_sub(ANNOTATION_EDITOR_FRAME_CELLS).max(1);
	let gutter = ANNOTATION_EDITOR_GUTTER.chars().count().min(editor_width);
	editor_width - gutter
}

/// Wrapped note-editor rows with the prompt gutter and software cursor.
#[must_use]
pub fn editor_rows(text: &str, overlay_width: usize, theme: &Theme) -> Vec<String> {
	let capacity = editor_content_width(overlay_width);
	let wrapped = wrap_text_with_ansi(text, capacity);
	let cursor = theme.nav().cursor;
	let mut rows = Vec::with_capacity(wrapped.len());
	for (index, chunk) in wrapped.iter().enumerate() {
		let gutter = if index == 0 { ANNOTATION_EDITOR_GUTTER } else { "  " };
		let mut row = format!("{gutter}{chunk}");
		if index + 1 == wrapped.len() {
			row.push_str(&cursor);
		}
		rows.push(row);
	}
	rows
}

/// Pluralize count noun.
#[must_use]
pub fn plural(count: usize, noun: &str) -> String {
	if count == 1 {
		format!("{count} {noun}")
	} else {
		format!("{count} {noun}s")
	}
}

/// Build code-review draft from state.
#[must_use]
pub fn build_code_review_draft(state: &AnnotationOverlayState) -> AnnotationOverlayDraft {
	AnnotationOverlayDraft::Code(crate::overlays::annotation_types::CodeReviewOverlayDraft {
		annotations: state.annotations.iter().map(|e| e.annotation.clone()).collect(),
		file_index: Some(state.file_index),
		editor: if state.annotating {
			Some(crate::overlays::annotation_types::AnnotationDraftEditor {
				file_index: Some(state.file_index),
				source_index: state.source_index,
				scope: match state.annotation_scope {
					AnnotationScope::File => "file".into(),
					_ => "line".into(),
				},
				editing_annotation_index: state.editing_annotation_index,
				text: state.editor_text.clone(),
			})
		} else {
			None
		},
	})
}

/// Build text-review draft from state.
#[must_use]
pub fn build_text_review_draft(state: &AnnotationOverlayState) -> AnnotationOverlayDraft {
	AnnotationOverlayDraft::Text(crate::overlays::annotation_types::TextReviewOverlayDraft {
		annotations: state.text_annotations.iter().map(|e| e.annotation.clone()).collect(),
		edited_text: if state.text_source_edited {
			state.text_source.as_ref().map(|s| s.text.clone())
		} else {
			None
		},
		editor: if state.annotating {
			Some(crate::overlays::annotation_types::AnnotationDraftEditor {
				file_index: None,
				source_index: state.source_index,
				scope: match state.annotation_scope {
					AnnotationScope::Text => "text".into(),
					_ => "line".into(),
				},
				editing_annotation_index: state.editing_annotation_index,
				text: state.editor_text.clone(),
			})
		} else {
			None
		},
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn split_text_lines_handles_crlf_and_lf() {
		assert_eq!(split_text_lines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
		assert_eq!(split_text_lines(""), vec![""]);
		assert_eq!(split_text_lines("a\n"), vec!["a", ""]);
	}

	#[test]
	fn display_label_and_status_sanitize_and_path_helpers() {
		let f = ReviewDiffFile {
			path: "x.rs".into(),
			occurrence: 2,
			diff: String::new(),
			binary: false,
			rows: vec![],
			..Default::default()
		};
		assert_eq!(display_file_label(&f), "x.rs (2)");
		assert_eq!(sanitize_status_text(" a \n b\t"), "a b");
		assert_eq!(split_path("src/ui/app.ts"), ("src/ui", "app.ts"));
		assert_eq!(split_path("app.ts"), ("", "app.ts"));
		assert_eq!(plural(1, "file"), "1 file");
		assert_eq!(plural(3, "file"), "3 files");
	}

	#[test]
	fn replace_text_source_retains_exact_retarges_unique_and_drops_ambiguous() {
		let source = TextReviewSource {
			id: "s".into(),
			label: "s".into(),
			title: None,
			provenance: TextReviewSourceProvenance::Prompt,
			text: "a\nb\nc".into(),
			parent_entry_id: None,
			turn_index: None,
			entry_index: None,
			cursor_offset: None,
		};
		let mut st = AnnotationOverlayState::for_text_source(source);
		st.text_annotations.push(CommittedTextAnnotation {
			source_index: 1,
			annotation: TextReviewAnnotation::Line { line: 2, quote: "b".into(), note: "n1".into() },
		});
		st.text_annotations.push(CommittedTextAnnotation {
			source_index: 2,
			annotation: TextReviewAnnotation::Line { line: 3, quote: "c".into(), note: "n2".into() },
		});
		// move "b" to line 1, keep "c" absent => "b" retargeted, "c" dropped
		let dropped = st.replace_text_source("b\na\nd".into());
		assert_eq!(dropped, 1);
		assert_eq!(st.text_annotations.len(), 1);
		match &st.text_annotations[0].annotation {
			TextReviewAnnotation::Line { line, .. } => {
				assert_eq!(*line, 1);
				assert_eq!(st.text_annotations[0].source_index, 0);
			}
			_ => panic!("expected line annotation"),
		}
	}

	#[test]
	fn action_disable_and_discard_logic() {
		let mut st = AnnotationOverlayState::for_code_review(&[]);
		// index 1 (paste) disabled with no annotations, discard enabled
		assert!(st.action_disabled(1));
		assert!(!st.action_disabled(2));
		assert!(!st.confirm_discard(false));
		assert!(st.discard_armed);
		assert!(st.confirm_discard(true));
		assert!(st.finished);
		assert!(!st.has_annotations());
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{
	bottom_border, divider, row, split_body_width, top_border, OverlayTheme,
};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::mouse::MouseEvent;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Fullscreen annotation overlay component.
///
/// Reference: `AnnotationOverlay` in `l/src/overlays/annotation-overlay.ts`.
///
/// Deviations: diff rendering/wrapping, `ScrollView`/`Editor` widgets, theme
/// styling, native tree/details, external-editor hooks, and completion payload
/// formatting are host-driven. This component ports core state, focus/selection/
/// action transitions, draft/annotation state handling, and bordered rendering.
pub struct AnnotationOverlayComponent<CB: AnnotationOverlayCallbacks> {
	pub state: AnnotationOverlayState,
	pub files: Vec<ReviewDiffFile>,
	pub mode: String,
	/// Body rows available to the diff/sidebar pane.
	pub body_height: usize,
	callbacks: CB,
	theme: OverlayTheme,
	ui_theme: Theme,
}

impl<CB: AnnotationOverlayCallbacks> std::fmt::Debug for AnnotationOverlayComponent<CB> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AnnotationOverlayComponent")
			.field("focus", &self.state.focus)
			.field("annotating", &self.state.annotating)
			.field("finished", &self.state.finished)
			.finish_non_exhaustive()
	}
}

impl<CB: AnnotationOverlayCallbacks> AnnotationOverlayComponent<CB> {
	pub fn new_code_review(files: Vec<ReviewDiffFile>, mode: impl Into<String>, callbacks: CB) -> Self {
		Self {
			state: AnnotationOverlayState::for_code_review(&files),
			files,
			mode: mode.into(),
			body_height: 40,
			callbacks,
			theme: OverlayTheme::default(),
			ui_theme: default_theme(),
		}
	}

	pub fn new_text_review(source: TextReviewSource, callbacks: CB) -> Self {
		Self {
			state: AnnotationOverlayState::for_text_source(source),
			files: Vec::new(),
			mode: "Annotating text".to_string(),
			body_height: 40,
			callbacks,
			theme: OverlayTheme::default(),
			ui_theme: default_theme(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	pub fn set_ui_theme(&mut self, theme: Theme) {
		self.ui_theme = theme;
	}

	fn can_show_sidebar(&self, width: usize) -> bool {
		width >= SIDEBAR_MIN_TOTAL_WIDTH
			&& split_body_width(width, sidebar_width(width)) >= SIDEBAR_MIN_BODY_WIDTH
	}

	fn title(&self) -> &str {
		if self.state.is_text_mode() { TEXT_OVERLAY_TITLE } else { OVERLAY_TITLE }
	}

	fn cycle_focus(&mut self, dir: isize) {
		if self.state.is_text_mode() {
			self.state.focus = match self.state.focus {
				FocusRegion::Diff => FocusRegion::Actions,
				_ => FocusRegion::Diff,
			};
			return;
		}
		let order = [FocusRegion::Files, FocusRegion::Diff, FocusRegion::Actions];
		let idx = order.iter().position(|f| *f == self.state.focus).unwrap_or(0);
		let next = (idx as isize + dir).rem_euclid(order.len() as isize) as usize;
		self.state.focus = order[next];
	}

	fn select_relative_file(&mut self, delta: isize) {
		if self.files.is_empty() {
			return;
		}
		let cur = self.state.file_index as isize;
		let next = (cur + delta).clamp(0, self.files.len().saturating_sub(1) as isize) as usize;
		self.state.file_index = next;
		self.state.source_index = 0;
	}

	fn escape(&mut self) {
		if self.state.annotating {
			self.state.annotating = false;
			self.state.editor_text.clear();
			return;
		}
		self.finish_cancel();
	}

	fn finish_cancel(&mut self) {
		if self.state.finished {
			return;
		}
		self.state.finished = true;
		if self.state.is_text_mode() {
			self.callbacks.on_complete_text(None);
		} else {
			self.callbacks.on_complete_code(None);
		}
	}

	fn finish_action(&mut self, discard_armed: bool) {
		let idx = self.state.action_index.min(self.state.actions.len().saturating_sub(1));
		if self.state.action_disabled(idx) {
			return;
		}
		let action = self.state.actions.get(idx).cloned().unwrap_or_default();
		if action == DISCARD_ANNOTATIONS_ACTION {
			if self.state.confirm_discard(discard_armed) {
				if self.state.is_text_mode() {
					self.callbacks.on_complete_text(None);
				} else {
					self.callbacks.on_complete_code(None);
				}
			}
			return;
		}
		self.state.finished = true;
		if self.state.is_text_mode() {
			self.callbacks.on_complete_text(Some(TextReviewOverlayResult {
				action: if action == PASTE_CODE_REVIEW_ACTION { "paste".into() } else { action },
				annotations: self.state.get_text_annotations(),
				edited_text: if self.state.text_source_edited {
					self.state.text_source.as_ref().map(|s| s.text.clone())
				} else {
					None
				},
				draft: None,
			}));
		} else {
			self.callbacks.on_complete_code(Some(CodeReviewOverlayResult {
				action: if action == CONTINUE_CODE_REVIEW_ACTION {
					"review".into()
				} else if action == PASTE_CODE_REVIEW_ACTION {
					"paste".into()
				} else {
					action
				},
				annotations: self.state.get_annotations(),
				draft: None,
			}));
		}
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let mut out = vec![top_border(width, self.title(), &self.theme)];
		out.push(row(&format!("Mode: {}", self.mode), width, &self.theme));
		if self.state.is_text_mode() {
			let lines = &self.state.text_lines;
			out.push(row(&format!("Lines: {}", lines.len()), width, &self.theme));
			let start = if lines.is_empty() { 0 } else { self.state.source_index.min(lines.len() - 1) };
			for (i, l) in lines.iter().enumerate().skip(start).take(8) {
				let cursor = if i == self.state.source_index { "›" } else { " " };
				out.push(row(&format!("{cursor} {:>4}  {}", i + 1, l), width, &self.theme));
			}
		} else {
			out.push(row(&format!("Files: {}", self.files.len()), width, &self.theme));
			if let Some(f) = self.files.get(self.state.file_index) {
				out.push(row(&format!("File: {}", display_file_label(f)), width, &self.theme));
				out.push(row(&format!("Rows: {}", f.rows.len()), width, &self.theme));
			}
		}
		out.push(divider(width, &self.theme));
		for (i, a) in self.state.actions.iter().enumerate() {
			let sel = if i == self.state.action_index { "›" } else { " " };
			let dis = if self.state.action_disabled(i) { " (disabled)" } else { "" };
			out.push(row(&format!("{sel} {a}{dis}"), width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		let hint = match self.state.focus {
			FocusRegion::Files => "↑/↓ move file · tab focus · [/] file · esc cancel",
			FocusRegion::Diff => "↑/↓ move line · A note · tab focus · esc cancel",
			FocusRegion::Actions => "↑/↓ action · enter confirm · tab focus · esc cancel",
		};
		out.push(row(hint, width, &self.theme));
		if self.state.annotating {
			out.extend(editor_rows(&self.state.editor_text, width as usize, &self.ui_theme));
		}
		if !self.state.is_text_mode() && self.can_show_sidebar(width as usize) {
			reveal_selected_file(
				&mut out,
				&self.files,
				self.state.file_index,
				self.state.focus,
				self.state.annotating,
				true,
				self.body_height,
				width as usize,
				&self.ui_theme,
			);
		}
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<CB: AnnotationOverlayCallbacks> Component for AnnotationOverlayComponent<CB> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.state.finished {
			return;
		}
		let discard_armed = self.state.discard_armed;
		self.state.discard_armed = false;
		if self.state.annotating {
			if keys::is_escape(key) {
				self.escape();
			} else if keys::is_enter(key) {
				self.state.annotating = false;
			} else if let Key::Char(c) = key {
				self.state.editor_text.push(*c);
			} else if keys::is_backspace(key) {
				self.state.editor_text.pop();
			}
			return;
		}
		if keys::is_escape(key) {
			self.escape();
			return;
		}
		if matches!(key, Key::Tab | Key::BackTab) {
			self.cycle_focus(if matches!(key, Key::BackTab) { -1 } else { 1 });
			return;
		}
		match self.state.focus {
			FocusRegion::Files => match key {
				Key::Up => self.select_relative_file(-1),
				Key::Down => self.select_relative_file(1),
				_ => {}
			},
			FocusRegion::Diff => match key {
				Key::Up => {
					self.state.source_index = self.state.source_index.saturating_sub(1);
				}
				Key::Down => {
					let max = if self.state.is_text_mode() {
						self.state.text_lines.len()
					} else {
						self.files.get(self.state.file_index).map(|f| f.rows.len()).unwrap_or(0)
					};
					if max > 0 {
						self.state.source_index = (self.state.source_index + 1).min(max - 1);
					}
				}
				Key::Char('A') => {
					self.state.annotating = true;
					self.state.annotation_scope = if self.state.is_text_mode() {
						AnnotationScope::Text
					} else {
						AnnotationScope::Line
					};
				}
				_ => {}
			},
			FocusRegion::Actions => match key {
				Key::Up => {
					self.state.action_index = self.state.action_index.saturating_sub(1);
				}
				Key::Down => {
					if !self.state.actions.is_empty() {
						self.state.action_index =
							(self.state.action_index + 1).min(self.state.actions.len() - 1);
					}
				}
				Key::Enter => self.finish_action(discard_armed),
				_ => {}
			},
		}
		if matches!(key, Key::Char('[')) && !self.state.is_text_mode() {
			self.select_relative_file(-1);
		}
		if matches!(key, Key::Char(']')) && !self.state.is_text_mode() {
			self.select_relative_file(1);
		}
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		if event.is_scroll_up() {
			self.state.source_index = self.state.source_index.saturating_sub(1);
		} else if event.is_scroll_down() {
			self.state.source_index = self.state.source_index.saturating_add(1);
		}
	}
}

impl<CB: AnnotationOverlayCallbacks> NativeDescribe for AnnotationOverlayComponent<CB> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;
	use std::cell::RefCell;

	#[derive(Default)]
	struct Cb {
		done: RefCell<bool>,
	}
	impl AnnotationOverlayCallbacks for Cb {
		fn on_complete_code(&self, _result: Option<CodeReviewOverlayResult>) {
			*self.done.borrow_mut() = true;
		}
		fn on_complete_text(&self, _result: Option<TextReviewOverlayResult>) {
			*self.done.borrow_mut() = true;
		}
	}

	#[test]
	fn annotation_overlay_component_text_mode_renders_and_navigates() {
		let src = TextReviewSource { provenance: TextReviewSourceProvenance::Custom, text: "a\nb\nc".into() };
		let mut c = AnnotationOverlayComponent::new_text_review(src, Cb::default());
		let lines = c.render(80);
		assert!(lines.iter().any(|l| l.contains("Annotate Text")));
		c.state.focus = FocusRegion::Diff;
		c.handle_input(&Key::Down);
		assert_eq!(c.state.source_index, 1);
		c.state.focus = FocusRegion::Actions;
		c.state.action_index = 1; // discard
		c.handle_input(&Key::Enter); // arm
		c.handle_input(&Key::Enter); // confirm
		assert!(c.state.finished);
	}

	#[test]
	fn annotation_overlay_component_describes_rows() {
		let src = TextReviewSource { provenance: TextReviewSourceProvenance::Custom, text: "x".into() };
		let c = AnnotationOverlayComponent::new_text_review(src, Cb::default());
		let node = c.describe_native(&DescribeContext { cols: 50, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
