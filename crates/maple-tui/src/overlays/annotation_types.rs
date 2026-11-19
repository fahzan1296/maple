//! Types for annotation/review overlays.
//!
//! Reference: `l/src/overlays/annotation-types.ts`.
//!
//! Deviations:
//! - TypeScript interfaces become Rust structs/enums with owned `String`/`Vec`
//!   fields; `undefined` maps to `Option`.
//! - Union shapes are modeled as enums with serde-friendly field layouts where
//!   useful, but serialization is not required by current callers.

use serde::{Deserialize, Serialize};

/// Source row kind in a code review diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewSourceRowKind {
	Context,
	Added,
	Removed,
}

/// One row of source context in a review diff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSourceRow {
	pub kind: ReviewSourceRowKind,
	pub raw: String,
	pub content: String,
	pub old_line: Option<u32>,
	pub new_line: Option<u32>,
	pub hunk_header: String,
}

/// One row in a review diff view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewDiffRow {
	Source(ReviewSourceRow),
	Hunk {
		raw: String,
		hunk_header: String,
	},
	NoNewline {
		raw: String,
		hunk_header: String,
	},
}

/// A file diff shown in the code review overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewDiffFile {
	pub path: String,
	pub occurrence: usize,
	pub diff: String,
	pub status: String,
	pub rows: Vec<ReviewDiffRow>,
	pub annotation_count: usize,
	pub added: usize,
	pub removed: usize,
	pub binary: bool,
	pub generated: bool,
	pub truncated: bool,
}

/// Shared fields of a code review annotation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeReviewAnnotationBase {
	pub id: String,
	pub file: String,
	pub hunk_header: String,
	pub note: String,
}

/// A code review annotation, scoped to a file or a specific line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeReviewAnnotation {
	File(CodeReviewAnnotationBase),
	Line {
		base: CodeReviewAnnotationBase,
		line: u32,
		side: ReviewSourceRowKind,
		raw: String,
		content: String,
	},
}

/// Result returned by the code review overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeReviewOverlayResult {
	pub action: String,
	pub annotations: Vec<CodeReviewAnnotation>,
	pub draft: Option<AnnotationOverlayDraft>,
}
/// Provenance of text reviewed in the text review overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TextReviewSourceProvenance {
	LatestAssistant { entry_id: String },
	Session { entry_id: String },
	File { path: String },
	#[default]
	Prompt,
}

/// Source material for a text review.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextReviewSource {
	pub id: String,
	pub label: String,
	pub title: Option<String>,
	pub provenance: TextReviewSourceProvenance,
	pub text: String,
	pub parent_entry_id: Option<String>,
	pub turn_index: Option<usize>,
	pub entry_index: Option<usize>,
	pub cursor_offset: Option<usize>,
}

/// A text review annotation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextReviewAnnotation {
	Text { note: String },
	Line { line: u32, quote: String, note: String },
}

/// Result returned by the text review overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextReviewOverlayResult {
	pub action: String,
	pub annotations: Vec<TextReviewAnnotation>,
	pub edited_text: Option<String>,
	pub draft: Option<AnnotationOverlayDraft>,
}

/// Editor state captured while composing a note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationDraftEditor {
	pub file_index: Option<usize>,
	pub source_index: usize,
	pub scope: String,
	pub editing_annotation_index: Option<usize>,
	pub text: String,
}

/// Code-review draft handed to an external editor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeReviewOverlayDraft {
	pub annotations: Vec<CodeReviewAnnotation>,
	pub file_index: Option<usize>,
	pub editor: Option<AnnotationDraftEditor>,
}

/// Text-review draft handed to an external editor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextReviewOverlayDraft {
	pub annotations: Vec<TextReviewAnnotation>,
	pub edited_text: Option<String>,
	pub editor: Option<AnnotationDraftEditor>,
}

/// Draft state for either review overlay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationOverlayDraft {
	Code(CodeReviewOverlayDraft),
	Text(TextReviewOverlayDraft),
}

impl AnnotationOverlayDraft {
	/// Distinguish code vs text draft.
	#[must_use]
	pub fn is_code(&self) -> bool {
		matches!(self, Self::Code(_))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn review_source_row_kind_values_are_distinct() {
		assert_ne!(ReviewSourceRowKind::Context, ReviewSourceRowKind::Added);
		assert_ne!(ReviewSourceRowKind::Added, ReviewSourceRowKind::Removed);
	}

	#[test]
	fn annotation_overlay_draft_discriminates_kind() {
		let code = AnnotationOverlayDraft::Code(CodeReviewOverlayDraft {
			annotations: Vec::new(),
			file_index: Some(0),
			editor: None,
		});
		let text = AnnotationOverlayDraft::Text(TextReviewOverlayDraft {
			annotations: Vec::new(),
			edited_text: None,
			editor: None,
		});
		assert!(code.is_code());
		assert!(!text.is_code());
	}

	#[test]
	fn text_review_annotation_variants_hold_data() {
		let a = TextReviewAnnotation::Text {
			note: "hello".to_string(),
		};
		let b = TextReviewAnnotation::Line {
			line: 3,
			quote: "q".to_string(),
			note: "n".to_string(),
		};
		match a {
			TextReviewAnnotation::Text { note } => assert_eq!(note, "hello"),
			TextReviewAnnotation::Line { .. } => panic!("wrong variant"),
		}
		match b {
			TextReviewAnnotation::Line { line, .. } => assert_eq!(line, 3),
			TextReviewAnnotation::Text { .. } => panic!("wrong variant"),
		}
	}
}
