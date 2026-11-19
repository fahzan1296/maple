//! Port of `l/test/annotation-editor-fullscreen.test.ts`.
//!
//! The reference drives a fullscreen `AnnotationOverlay` on the alternate
//! screen with a hardware cursor and asserts cell-exact input rows and cursor
//! placement. The crate ports the overlay's state machine
//! (`AnnotationOverlayComponent`) without any UI runtime, cursor or focus
//! owner, so the full-cell input/editor-capture path is pinned and the cursor
//! placement and software-fallback assertions are ignored.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::annotation_overlay::{
	AnnotationOverlayCallbacks, AnnotationOverlayComponent, FocusRegion,
};
use maple_tui::overlays::annotation_types::{
	CodeReviewOverlayResult, TextReviewOverlayResult, TextReviewSource, TextReviewSourceProvenance,
};

#[derive(Clone, Default)]
struct Cb {
	completed: Rc<RefCell<Option<Option<TextReviewOverlayResult>>>>,
}

impl AnnotationOverlayCallbacks for Cb {
	fn on_complete_code(&self, _result: Option<CodeReviewOverlayResult>) {}
	fn on_complete_text(&self, result: Option<TextReviewOverlayResult>) {
		*self.completed.borrow_mut() = Some(result);
	}
}

fn source(id: &str) -> TextReviewSource {
	TextReviewSource {
		id: id.into(),
		label: "source".into(),
		title: None,
		provenance: TextReviewSourceProvenance::Prompt,
		text: "source text".into(),
		parent_entry_id: None,
		turn_index: None,
		entry_index: None,
		cursor_offset: None,
	}
}

#[test]
fn preserves_full_cell_input_and_cancels_out_of_the_editor() {
	let cb = Cb::default();
	let mut overlay = AnnotationOverlayComponent::new_text_review(source("fullscreen-boundary"), cb.clone());
	assert_eq!(overlay.state.focus, FocusRegion::Diff);
	let frame = overlay.render(40);
	assert!(frame.join("\n").contains("Annotate Text"));

	overlay.handle_input(&Key::Char('A'));
	assert!(overlay.state.annotating);
	for glyph in "QQQ".chars() {
		overlay.handle_input(&Key::Char(glyph));
	}
	assert_eq!(overlay.state.editor_text, "QQQ");

	// Escape leaves the editor without finishing the overlay.
	overlay.handle_input(&Key::Esc);
	assert!(!overlay.state.annotating);
	assert!(overlay.state.editor_text.is_empty());
	assert!(!overlay.state.finished);
	assert!(cb.completed.borrow().is_none());

	// A second escape finishes with a cancellation payload.
	overlay.handle_input(&Key::Esc);
	assert!(overlay.state.finished);
	let completed = cb.completed.borrow_mut().take().expect("completion callback");
	assert!(completed.is_none());
}

#[test]
#[ignore = "missing facility: no hardware-cursor placement, so the cursor cell after a wide glyph cannot be asserted"]
fn positions_the_hardware_cursor_after_a_wide_glyph_at_the_editor_width_boundary() {
	let mut overlay =
		AnnotationOverlayComponent::new_text_review(source("wide-cursor-boundary"), Cb::default());
	overlay.handle_input(&Key::Char('A'));
	overlay.handle_input(&Key::Char('Q'));
	assert_eq!(overlay.state.editor_text, "Q");
}

#[test]
#[ignore = "missing facility: no software-caret fallback, so showing exactly one cursor cannot be asserted"]
fn switches_a_mounted_editor_to_software_fallback_without_showing_two_cursors() {
	let mut overlay =
		AnnotationOverlayComponent::new_text_review(source("fullscreen-cursor-preference"), Cb::default());
	overlay.handle_input(&Key::Char('A'));
	overlay.handle_input(&Key::Char('Q'));
	assert_eq!(overlay.state.editor_text, "Q");
	overlay.handle_input(&Key::Esc);
	overlay.handle_input(&Key::Esc);
	assert!(overlay.state.finished);
}
