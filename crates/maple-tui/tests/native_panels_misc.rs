//! Reference: `l/test/native-panels-misc.test.ts`.
//!
//! The reference drives native events on `/move`, the annotation overlay, the
//! agent transcript viewer, the BTW history panel and the Codex reset
//! fireworks. The crate ports the transcript viewer's draft/escape semantics
//! and the history panel's selection/composer state; native event routing,
//! scroll props, paste-image hooks and the fireworks controller have no
//! equivalent, so those cases are ignored with the absent facility named.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::agent_transcript_viewer::{
	AgentTranscriptViewer, AgentTranscriptViewerCallbacks,
};
use maple_tui::overlays::btw_history::{BtwHistoryRecord, BtwHistoryTurn};
use maple_tui::overlays::btw_history_panel::{BtwHistoryPanel, BtwHistoryPanelCallbacks};

struct ViewerCb {
	closed: Rc<RefCell<usize>>,
}
impl AgentTranscriptViewerCallbacks for ViewerCb {
	fn on_close(&self) {
		*self.closed.borrow_mut() += 1;
	}
	fn on_hub_close(&self) {}
	fn request_render(&self) {}
}

#[test]
fn transcript_viewer_clears_a_typed_draft_first_then_closes() {
	let closed = Rc::new(RefCell::new(0));
	let mut viewer = AgentTranscriptViewer::new("a1", ViewerCb { closed: closed.clone() }, true);
	viewer.handle_input(&Key::Char('h'));
	viewer.handle_input(&Key::Char('i'));
	assert_eq!(viewer.editor_text, "hi");

	viewer.handle_input(&Key::Esc);
	assert_eq!(viewer.editor_text, "", "escape clears the draft");
	assert_eq!(*closed.borrow(), 0, "clearing the draft must not close the viewer");

	viewer.handle_input(&Key::Esc);
	assert_eq!(*closed.borrow(), 1, "the second escape closes the viewer");
}

struct HistoryCb {
	closed: Rc<RefCell<usize>>,
	cancelled: Rc<RefCell<Vec<String>>>,
	follow_ups: Rc<RefCell<Vec<(String, String)>>>,
}

impl BtwHistoryPanelCallbacks for HistoryCb {
	fn on_close(&self) {
		*self.closed.borrow_mut() += 1;
	}
	fn request_render(&self) {}
	fn on_cancel(&self, record: &BtwHistoryRecord) {
		self.cancelled.borrow_mut().push(record.id.clone());
	}
	fn can_follow_up(&self, _record: &BtwHistoryRecord) -> bool {
		true
	}
	fn on_follow_up(&self, record: &BtwHistoryRecord, question: &str) -> bool {
		self.follow_ups.borrow_mut().push((record.id.clone(), question.to_string()));
		true
	}
}

fn record(id: &str, question: &str, status: &str) -> BtwHistoryRecord {
	BtwHistoryRecord {
		id: id.into(),
		question: question.into(),
		answer: format!("answer to {question}"),
		status: status.into(),
		..BtwHistoryRecord::default()
	}
}

#[test]
fn history_panel_selection_shows_the_selected_record_turn() {
	let cb = HistoryCb {
		closed: Rc::new(RefCell::new(0)),
		cancelled: Rc::new(RefCell::new(Vec::new())),
		follow_ups: Rc::new(RefCell::new(Vec::new())),
	};
	let mut panel =
		BtwHistoryPanel::new(vec![record("a", "first question", "complete"), record("b", "second question", "complete")], cb);
	assert!(panel.select_by_id("b"));
	assert_eq!(panel.selected().map(|record| record.id.as_str()), Some("b"));
	let screen = panel.render(100).join("\n");
	assert!(screen.contains("second question"), "selected record's question missing: {screen}");
}

#[test]
fn history_panel_activate_opens_the_composer_and_submit_sends_the_question() {
	let follow_ups = Rc::new(RefCell::new(Vec::new()));
	let cb = HistoryCb {
		closed: Rc::new(RefCell::new(0)),
		cancelled: Rc::new(RefCell::new(Vec::new())),
		follow_ups: follow_ups.clone(),
	};
	let mut panel = BtwHistoryPanel::new(vec![record("a", "first question", "complete")], cb);
	panel.open_follow_up();
	assert!(panel.composer.is_some(), "activate must open the follow-up composer");
	panel.handle_input(&Key::Char('w'));
	panel.handle_input(&Key::Char('h'));
	panel.handle_input(&Key::Char('y'));
	panel.handle_input(&Key::Enter);
	assert_eq!(*follow_ups.borrow(), vec![("a".to_string(), "why".to_string())]);
	assert!(panel.composer.is_none(), "an accepted follow-up closes the composer");
}

#[test]
fn history_panel_escape_hides_a_running_answer_without_cancelling_it() {
	let closed = Rc::new(RefCell::new(0));
	let cancelled = Rc::new(RefCell::new(Vec::new()));
	let cb = HistoryCb { closed: closed.clone(), cancelled: cancelled.clone(), follow_ups: Rc::new(RefCell::new(Vec::new())) };
	let mut panel = BtwHistoryPanel::new(vec![record("a", "still going", "running")], cb);
	panel.set_escape_hides(true);

	panel.handle_input(&Key::Esc);
	assert_eq!(*closed.borrow(), 1, "escape hides a running answer");
	assert!(cancelled.borrow().is_empty(), "hiding must not cancel the running answer");
}

#[test]
#[ignore = "missing facility: MoveOverlay has no handle_native_event (no select/activate/accept/confirm native routing), so pointer-driven suggestions cannot be asserted"]
fn move_overlay_native_select_and_activate() {}

#[test]
#[ignore = "missing facility: AnnotationOverlay has no handle_native_event esc-close path and no persistent draft across close, so the save-draft-then-close flow cannot be asserted"]
fn annotation_overlay_native_esc_saves_a_draft_then_closes() {}

#[test]
#[ignore = "missing facility: BtwHistoryPanel has no native event routing, scroll-into-view props, paste-image pipeline, footer action buttons or row date formatting, so those native cases cannot be asserted"]
fn btw_history_panel_native_scroll_paste_buttons_and_dates() {}

#[test]
#[ignore = "missing facility: CodexResetFireworksController has no crate equivalent (only the canvas/detect helpers exist), so the no-repaint-loop assertion cannot be made"]
fn codex_reset_fireworks_does_not_run_the_frame_repaint_loop() {}
