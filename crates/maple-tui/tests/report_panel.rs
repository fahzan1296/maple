//! Reference: `l/test/report-panel.test.ts`.
//!
//! Ports the panel-level cases: row cap, close-key hint, scrolling the body,
//! and the native sheet (body + Close action). The reference's Composer-level
//! mounts (report above the editor, editor pinned to the bottom row, scrollback
//! retention) need `Composer`, `VirtualRenderScheduler` and the terminal
//! multiplexer helper, none of which the crate has, so those cases are not
//! asserted.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::components::Text;
use maple_tui::keys::Key;
use maple_tui::native::backend::NativeUiEvent;
use maple_tui::native::node::{DescribeContext, NodeKind, ScrollBy};
use maple_tui::overlays::report_panel::{ReportPanel, ReportPanelOptions, TEXT_CHROME_ROWS};
use maple_tui::strip_ansi;

fn entry_rows(count: usize) -> Text {
	Text::new((0..count).map(|index| format!("Entry {index}")).collect::<Vec<_>>().join("\n"))
}

fn entries(rows: &[String]) -> Vec<usize> {
	rows.iter()
		.filter_map(|row| {
			let plain = strip_ansi(row);
			let start = plain.find("Entry ")? + "Entry ".len();
			let rest = &plain[start..];
			let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
			rest[..end].parse().ok()
		})
		.collect()
}

fn panel(body: Text, cap: Option<usize>, close_key: &str) -> (ReportPanel, Rc<RefCell<usize>>) {
	let closed = Rc::new(RefCell::new(0));
	let sink = Rc::clone(&closed);
	let mut options = ReportPanelOptions::new("Full Changelog", Box::new(body), close_key, move || {
		*sink.borrow_mut() += 1;
	});
	if let Some(cap) = cap {
		options = options.with_max_rows(move || Some(cap));
	}
	(ReportPanel::new(options), closed)
}

#[test]
fn never_renders_past_its_row_cap() {
	let (mut panel, _) = panel(entry_rows(20), Some(12), "Esc");
	assert_eq!(panel.render(60).len(), 12);

	let (mut panel, _) = self::panel(entry_rows(20), Some(4), "Esc");
	let shrunk = panel.render(60);
	assert_eq!(shrunk.len(), 4);
	assert!(strip_ansi(&shrunk[0]).contains("Full Changelog"), "{shrunk:?}");
}

#[test]
fn caps_a_taller_report_and_pages_through_it() {
	let (mut panel, _) = panel(entry_rows(80), Some(20), "Esc");
	let rows = panel.render(80);
	assert_eq!(rows.len(), 20);
	assert!(strip_ansi(&rows[rows.len() - 2]).contains("scroll"), "{rows:?}");
	let first = entries(&rows);
	assert_eq!(first[0], 0);
	assert_eq!(first.len(), 20 - TEXT_CHROME_ROWS);

	panel.handle_input(&Key::PageDown);
	let rows = panel.render(80);
	let paged = entries(&rows);
	assert!(paged[0] > 0, "paged: {paged:?}");
	assert_eq!(paged.len(), first.len());

	panel.handle_input(&Key::End);
	let rows = panel.render(80);
	assert_eq!(*entries(&rows).last().unwrap(), 79);
}

#[test]
fn names_the_key_that_closes_it() {
	let (mut panel, closed) = panel(Text::new("body"), None, "Ctrl+Q");
	let rows = panel.render(60);
	assert!(strip_ansi(&rows[rows.len() - 2]).contains("Ctrl+Q to close"), "{rows:?}");

	panel.hold_focus();
	let rows = panel.render(60);
	assert!(!strip_ansi(&rows[rows.len() - 2]).contains("Ctrl+Q"), "{rows:?}");
	assert!(strip_ansi(&rows[rows.len() - 2]).contains("esc to close"), "{rows:?}");

	panel.handle_input(&Key::Esc);
	assert_eq!(*closed.borrow(), 1);
}

#[test]
fn describes_the_body_then_close_and_closes_like_esc() {
	let (mut panel, closed) = panel(Text::new("body"), None, "Esc");
	let spec = panel.overlay_spec();
	assert_eq!(spec.role, "omp.overlay.report");
	assert_eq!(spec.size, "lg");
	assert_eq!(spec.anchor, "center");
	assert_eq!(spec.head.as_plain(), Some("Full Changelog"));

	let cx = DescribeContext::new(80);
	let sheet = panel.describe_sheet(&cx);
	assert_eq!(sheet.kind, NodeKind::Col);
	assert_eq!(sheet.children[0].key.as_deref(), Some("body"));
	assert_eq!(sheet.children[0].props.role().as_deref(), Some("omp.report.body"));
	assert_eq!(sheet.children[1].props.role().as_deref(), Some("omp.actions"));

	panel.handle_input(&Key::PageDown);
	let sheet = panel.describe_sheet(&cx);
	let scroll = sheet.children[0].scroll.clone().expect("scroll request");
	assert_eq!(scroll.by, ScrollBy::PageDown);
	assert_eq!(scroll.n, 1);

	let event = NativeUiEvent::Action {
		key: vec!["report-actions".into()],
		act: "close".into(),
		value: None,
		mods: Vec::new(),
	};
	assert!(panel.handle_native_event(&event));
	assert_eq!(*closed.borrow(), 1);
}
