//! Port of `l/test/context-viewer.test.ts` (viewer state/render parts).
//!
//! Deviations: the reference asserts native describe node props and TUI widget
//! trees. The crate's [`ContextViewer`] owns the same navigation state and a
//! bordered render, so the port drives keys/mouse through the `Component`
//! interface and checks rendered rows plus the native describe payload.

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::mouse::{MouseEvent, MouseKind};
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NodeKind};
use maple_tui::overlays::context_viewer::{
	ContextViewCategory, ContextViewData, ContextViewEntry, ContextViewer, ContextViewerCallbacks,
	ContextViewerState,
};
use std::cell::RefCell;

struct Cb {
	closed: RefCell<bool>,
}

impl ContextViewerCallbacks for Cb {
	fn on_close(&self) {
		*self.closed.borrow_mut() = true;
	}
}

fn entry(id: &str, content: &str) -> ContextViewEntry {
	ContextViewEntry { id: id.to_string(), label: id.to_string(), content: content.to_string(), thinking_group: None }
}

fn data() -> ContextViewData {
	ContextViewData {
		model_label: Some("longcat".to_string()),
		context_window: 1000,
		used_tokens: 250,
		categories: vec![
			ContextViewCategory {
				id: "sys".into(),
				label: "System".into(),
				tokens: 100,
				color: None,
				entries: vec![entry("prompt", "system prompt body")],
			},
			ContextViewCategory {
				id: "msgs".into(),
				label: "Messages".into(),
				tokens: 150,
				color: None,
				entries: vec![entry("u1", "first\nsecond\nthird"), entry("a1", "reply")],
			},
		],
		notices: vec!["auto-compact soon".to_string()],
		snapcompact_lines: vec![],
	}
}

fn viewer() -> ContextViewer<Cb> {
	ContextViewer::new(
		data(),
		Cb { closed: RefCell::new(false) },
	)
}

#[test]
fn overview_reports_usage_model_and_categories() {
	let mut v = viewer();
	let lines = v.render(80).join("\n");
	assert!(lines.contains("250 of 1000 tokens · 25.0%"), "{lines}");
	assert!(lines.contains("750 free"), "{lines}");
	assert!(lines.contains("Model: longcat"), "{lines}");
	assert!(lines.contains("System (100 tokens)"), "{lines}");
	assert!(lines.contains("Messages (150 tokens)"), "{lines}");
	assert!(lines.contains("note: auto-compact soon"), "{lines}");
	assert!(lines.contains("› System"), "{lines}");
	assert_eq!(v.state, ContextViewerState::Overview);

	let node = v.describe_native(&DescribeContext { cols: 80, ..Default::default() });
	assert!(matches!(node.kind, NodeKind::Rows));
	assert_eq!(node.props.get_str("state").map(str::to_string), Some("Overview".to_string()));
}

#[test]
fn navigation_opens_category_then_entry_and_back_out() {
	let mut v = viewer();
	v.handle_input(&Key::Down);
	v.handle_input(&Key::Enter);
	assert_eq!(v.state, ContextViewerState::Category);
	assert_eq!(v.category_index, Some(1));
	let category = v.render(80).join("\n");
	assert!(category.contains("Messages  (2 entries)"), "{category}");

	v.handle_input(&Key::Enter);
	assert_eq!(v.state, ContextViewerState::Entry);
	let entry_view = v.render(80).join("\n");
	assert!(entry_view.contains("[u1]"), "{entry_view}");
	assert!(entry_view.contains("first"), "{entry_view}");
	assert!(entry_view.contains("— Messages —"), "{entry_view}");

	v.handle_input(&Key::Esc);
	assert_eq!(v.state, ContextViewerState::Category);
	v.handle_input(&Key::Esc);
	assert_eq!(v.state, ContextViewerState::Overview);
	assert_eq!(v.category_index, None);
}

#[test]
fn a_single_entry_category_opens_the_entry_directly_and_escape_closes() {
	let mut v = viewer();
	v.handle_input(&Key::Enter);
	assert_eq!(v.state, ContextViewerState::Entry, "one entry skips the category list");
	assert_eq!(v.entry_parent, ContextViewerState::Overview);
	let lines = v.render(80).join("\n");
	assert!(lines.contains("[prompt]"), "{lines}");
	assert!(lines.contains("system prompt body"), "{lines}");

	v.handle_input(&Key::Esc);
	assert_eq!(v.state, ContextViewerState::Overview);
	v.handle_input(&Key::Esc);
	assert!(*v.callbacks().closed.borrow(), "escape at the overview closes the viewer");
}

#[test]
fn entry_scrolling_clamps_and_mouse_scroll_moves_it() {
	let mut v = viewer();
	v.handle_input(&Key::Down);
	v.handle_input(&Key::Enter);
	v.handle_input(&Key::Enter);
	v.handle_input(&Key::Up);
	assert_eq!(v.entry_scroll, 0, "scroll never goes negative");

	for _ in 0..5 {
		v.handle_input(&Key::Down);
	}
	v.handle_input(&Key::PageDown);
	let paged = v.entry_scroll;
	assert!(paged >= 5, "page down jumps by the body height");
	v.handle_input(&Key::Home);
	assert_eq!(v.entry_scroll, 0);

	let before = v.entry_scroll;
	v.handle_mouse(&mouse(MouseKind::ScrollDown));
	assert_eq!(v.entry_scroll, before + 1);
	v.handle_mouse(&mouse(MouseKind::ScrollUp));
	assert_eq!(v.entry_scroll, before);
}

fn mouse(kind: MouseKind) -> MouseEvent {
	MouseEvent { kind, column: 0, row: 0, shift: false, ctrl: false, alt: false }
}
