//! Port of `l/test/btw-history-panel-cursor.test.ts`.
//!
//! The reference parks the terminal hardware cursor after typing a follow-up
//! draft. The crate panel is a state model with no terminal cursor, so the
//! composer state machine (Enter opens, typing edits, Enter submits) is
//! asserted directly; hardware-cursor parking is ignored.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::btw_history::BtwHistoryRecord;
use maple_tui::overlays::btw_history_panel::{BtwHistoryPanel, BtwHistoryPanelCallbacks, PaneFocus};

struct Callbacks {
	follow_ups: Rc<RefCell<Vec<(String, String)>>>,
}

impl BtwHistoryPanelCallbacks for Callbacks {
	fn on_close(&self) {}
	fn request_render(&self) {}
	fn can_follow_up(&self, _record: &BtwHistoryRecord) -> bool {
		true
	}
	fn on_follow_up(&self, record: &BtwHistoryRecord, question: &str) -> bool {
		self.follow_ups.borrow_mut().push((record.id.clone(), question.to_string()));
		true
	}
}

fn record() -> BtwHistoryRecord {
	BtwHistoryRecord {
		id: "topic".to_string(),
		leaf_id: Some("leaf".to_string()),
		question: "hello".to_string(),
		answer: "Hello answer".to_string(),
		status: "complete".to_string(),
		created_at: 1,
		updated_at: 2,
		..Default::default()
	}
}

#[test]
fn enter_opens_the_composer_for_the_selected_topic() {
	let mut panel = BtwHistoryPanel::new(vec![record()], Callbacks { follow_ups: Rc::new(RefCell::new(Vec::new())) });
	panel.set_focused(true);

	panel.handle_input(&Key::Enter);
	let composer = panel.composer.as_ref().expect("composer open");
	assert_eq!(composer.record_id, "topic");
	assert_eq!(composer.text, "");
	assert_eq!(panel.focus, PaneFocus::Answer);
}

#[test]
fn typing_edits_the_draft_and_the_render_shows_it() {
	let mut panel = BtwHistoryPanel::new(vec![record()], Callbacks { follow_ups: Rc::new(RefCell::new(Vec::new())) });
	panel.set_focused(true);
	panel.handle_input(&Key::Enter);

	panel.handle_input(&Key::Char('f'));
	panel.handle_input(&Key::Char('o'));
	panel.handle_input(&Key::Backspace);
	assert_eq!(panel.composer.as_ref().expect("composer open").text, "f");
	assert!(panel.render(80).iter().any(|line| line.contains("Follow-up: f")));
}

#[test]
fn enter_submits_the_draft_to_the_selected_topic() {
	let follow_ups = Rc::new(RefCell::new(Vec::new()));
	let mut panel = BtwHistoryPanel::new(vec![record()], Callbacks { follow_ups: Rc::clone(&follow_ups) });
	panel.set_focused(true);
	panel.handle_input(&Key::Enter);
	panel.handle_input(&Key::Char('f'));
	panel.handle_input(&Key::Enter);

	assert_eq!(*follow_ups.borrow(), vec![("topic".to_string(), "f".to_string())]);
	assert!(panel.composer.is_none());
	assert!(!panel.follow_up_pending);
}

#[test]
fn escape_cancels_the_composer() {
	let mut panel = BtwHistoryPanel::new(vec![record()], Callbacks { follow_ups: Rc::new(RefCell::new(Vec::new())) });
	panel.set_focused(true);
	panel.handle_input(&Key::Enter);
	panel.handle_input(&Key::Char('f'));
	panel.handle_input(&Key::Esc);

	assert!(panel.composer.is_none());
}

#[test]
#[ignore = "missing facility: no terminal backend; hardware cursor parking/IME preedit anchoring is not observable"]
fn parks_the_hardware_cursor_at_the_follow_up_caret() {
	let mut panel = BtwHistoryPanel::new(vec![record()], Callbacks { follow_ups: Rc::new(RefCell::new(Vec::new())) });
	panel.set_focused(true);
	panel.handle_input(&Key::Enter);
	panel.handle_input(&Key::Char('f'));
	assert_eq!(panel.composer.as_ref().map(|c| c.text.as_str()), Some("f"));
}
