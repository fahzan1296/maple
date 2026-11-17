//! Port of `l/test/annotation-overlay.test.ts` (component-level scenarios).
//!
//! The crate overlay ports state, focus/selection/action transitions, the
//! note-editor rows, and the floating filename reveal. Tests that need the
//! reference's full diff body, scroll view, chooser, or native event routing
//! live beside those facilities; here the component's own behavior is asserted.

use std::cell::RefCell;

use maple_tui::keys::Key;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NodeKind};
use maple_tui::overlays::annotation_overlay::{
	AnnotationOverlayCallbacks, AnnotationOverlayComponent, CodeReviewOverlayResult, FocusRegion,
	TextReviewOverlayResult,
};
use maple_tui::overlays::annotation_types::{
	ReviewDiffFile, ReviewSourceRow, ReviewSourceRowKind, TextReviewSource, TextReviewSourceProvenance,
};
use maple_tui::strip_ansi;

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

fn file(path: &str) -> ReviewDiffFile {
	ReviewDiffFile {
		path: path.to_string(),
		rows: vec![ReviewSourceRow {
			kind: ReviewSourceRowKind::Context,
			raw: " context".to_string(),
			content: "context".to_string(),
			old_line: Some(1),
			new_line: Some(1),
			hunk_header: "@@ -1 +1 @@".to_string(),
		}],
		..ReviewDiffFile::default()
	}
}

#[test]
fn renders_title_and_files() {
	let files = vec![file("src/a.ts")];
	let mut overlay = AnnotationOverlayComponent::new_code_review(files, "Reviewing changes", Cb::default());
	let lines = overlay.render(90);
	let plain: Vec<String> = lines.iter().map(|line| strip_ansi(line)).collect();
	assert!(plain.iter().any(|line| line.contains("Code Review")));
	assert!(plain.iter().any(|line| line.contains("Files: 1")));
	assert!(plain.iter().any(|line| line.contains("src/a.ts")));
}

#[test]
fn floats_selected_filepath_while_file_focus_is_active() {
	let path = format!("src/{}selected-file.ts", "deep-module/".repeat(5));
	let files = vec![file(&path)];
	let mut overlay = AnnotationOverlayComponent::new_code_review(files, "Reviewing changes", Cb::default());
	overlay.state.focus = FocusRegion::Files;
	let plain: Vec<String> = overlay.render(90).iter().map(|line| strip_ansi(line)).collect();
	assert!(plain.iter().any(|line| line.contains(&path)));

	overlay.state.focus = FocusRegion::Diff;
	let plain: Vec<String> = overlay.render(90).iter().map(|line| strip_ansi(line)).collect();
	assert!(!plain.iter().any(|line| line.contains(&path)));
}

#[test]
fn note_editor_wraps_at_input_capacity_with_software_cursor() {
	let source = TextReviewSource {
		provenance: TextReviewSourceProvenance::Custom,
		text: "source text".to_string(),
	};
	let mut overlay = maple_tui::overlays::annotation_overlay::AnnotationOverlayComponent::new_text_review(
		source,
		Cb::default(),
	);
	overlay.state.focus = FocusRegion::Diff;
	overlay.handle_input(&Key::Char('A'));
	assert!(overlay.state.annotating);
	for _ in 0..35 {
		overlay.handle_input(&Key::Char('Q'));
	}
	let plain: Vec<String> = overlay.render(40).iter().map(|line| strip_ansi(line)).collect();
	let q_rows: Vec<&String> = plain.iter().filter(|line| line.contains('Q')).collect();
	assert_eq!(q_rows.len(), 2);
	assert_eq!(q_rows[0].matches('Q').count(), 34);
	assert_eq!(q_rows[1].matches('Q').count(), 1);
}

#[test]
fn text_mode_navigates_and_confirms_discard_only_twice() {
	let source = TextReviewSource {
		provenance: TextReviewSourceProvenance::Custom,
		text: "a\nb\nc".to_string(),
	};
	let mut overlay = maple_tui::overlays::annotation_overlay::AnnotationOverlayComponent::new_text_review(
		source,
		Cb::default(),
	);
	overlay.state.focus = FocusRegion::Diff;
	overlay.handle_input(&Key::Down);
	assert_eq!(overlay.state.source_index, 1);
	overlay.state.focus = FocusRegion::Actions;
	overlay.state.action_index = 1;
	overlay.handle_input(&Key::Enter);
	assert!(!overlay.state.finished);
	overlay.handle_input(&Key::Enter);
	assert!(overlay.state.finished);
}

#[test]
fn describes_rows_for_native() {
	let overlay = AnnotationOverlayComponent::new_code_review(files, "Reviewing changes", Cb::default());
	let node = overlay.describe_native(&DescribeContext { cols: 50, ..Default::default() });
	assert!(matches!(node.kind, NodeKind::Rows));
}
