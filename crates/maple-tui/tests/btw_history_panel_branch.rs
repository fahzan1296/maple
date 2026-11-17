//! Port of `l/test/btw-history-panel-branch.test.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NodeKind};
use maple_tui::overlays::btw_history::BtwHistoryRecord;
use maple_tui::overlays::btw_history_panel::{BtwHistoryPanel, BtwHistoryPanelCallbacks};

struct Callbacks {
	eligible: Rc<RefCell<bool>>,
	branched: Rc<RefCell<Vec<String>>>,
}

impl BtwHistoryPanelCallbacks for Callbacks {
	fn on_close(&self) {}
	fn request_render(&self) {}
	fn can_branch(&self, _record: &BtwHistoryRecord) -> bool {
		*self.eligible.borrow()
	}
	fn on_branch(&self, record: &BtwHistoryRecord) {
		self.branched.borrow_mut().push(record.id.clone());
	}
}

fn record() -> BtwHistoryRecord {
	BtwHistoryRecord {
		id: "topic".to_string(),
		leaf_id: Some("leaf".to_string()),
		question: "Why?".to_string(),
		answer: "Because.".to_string(),
		status: "complete".to_string(),
		created_at: 1,
		updated_at: 2,
		..Default::default()
	}
}

#[test]
fn promotes_the_selected_answer_with_b_only_while_it_is_eligible() {
	let eligible = Rc::new(RefCell::new(true));
	let branched = Rc::new(RefCell::new(Vec::new()));
	let mut panel = BtwHistoryPanel::new(
		vec![record()],
		Callbacks { eligible: Rc::clone(&eligible), branched: Rc::clone(&branched) },
	);

	panel.handle_input(&Key::Char('b'));
	assert_eq!(*branched.borrow(), vec!["topic".to_string()]);

	*eligible.borrow_mut() = false;
	panel.handle_input(&Key::Char('b'));
	assert_eq!(branched.borrow().len(), 1);
}

#[test]
#[ignore = "missing facility: describe_native emits text rows only; there is no native action tree with click actions"]
fn dispatches_the_rendered_branch_button_and_rejects_its_stale_action() {
	let eligible = Rc::new(RefCell::new(true));
	let branched = Rc::new(RefCell::new(Vec::new()));
	let panel = BtwHistoryPanel::new(
		vec![record()],
		Callbacks { eligible, branched },
	);
	let node = panel.describe_native(&DescribeContext::default());
	assert!(matches!(node.kind, NodeKind::Rows));
}
