//! Reference: `l/src/overlays/report-panel.ts`.
//!
//! A read-only command report (`/changelog`, `/context`, `/tools`, …).
//!
//! Text mode draws a titled box with the body and an Esc hint: above the editor
//! like the `/btw` panel while it fits there (the editor keeps focus and its Esc
//! takes it away), or as a full-screen page when it is taller, whose body
//! scrolls on the arrow/page/Home/End keys and the wheel. Natively it is a
//! sheet like `/usage`: a centred `lg` sheet titled by the report whose body the
//! terminal scrolls, then a Close button; Esc closes it.
//!
//! Deviations:
//! - `OverlayPanel` is a concrete crate component, not a base class, so the
//!   panel keeps the title/role itself and draws the rounded chrome with
//!   [`top_border`]/[`row`]/[`bottom_border`].
//! - The reference reads the global theme and keybindings; this port takes a
//!   [`KeybindingsManager`] for the Esc/scroll keys and the footer hint text,
//!   and leaves ANSI styling of the footer to the host.
//! - The native description is rebuilt per describe instead of memoized on the
//!   scroll-request identity.

use std::cell::RefCell;

use crate::chrome::overlay_box::{bottom_border, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::components::scroll_view::ScrollView;
use crate::components::scroll_viewport::ScrollbarMode;
use crate::components::text::Text;
use crate::keybinding_matchers::{
	matches_select_cancel, matches_select_down, matches_select_page_down, matches_select_page_up,
	matches_select_up,
};
use crate::keybindings::KeybindingsManager;
use crate::keys::{self, Key};
use crate::mouse::{MouseEvent, MouseKind};
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{col, describe_child, keyed};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Scroll, ScrollBy, Text as NodeText};
use crate::native::overlay::{action_bar, action_button, format_tooltip_key, ActionButtonOptions};
use crate::text_shape::Ellipsis;

/// Text-mode rows around the scrolled body: the borders, two spacers and the
/// footer.
pub const TEXT_CHROME_ROWS: usize = 5;

/// Options of a [`ReportPanel`].
pub struct ReportPanelOptions {
	/// Plain title: the text-mode border title, and the native sheet head unless
	/// [`head`](ReportPanelOptions::head) is given.
	pub title: String,
	/// Styled native sheet head.
	pub head: Option<NodeText>,
	/// The read-only report; it draws no frame or title of its own.
	pub body: Box<dyn Component>,
	/// Display form of the key that dismisses the report while it sits above the
	/// editor.
	pub close_key: String,
	/// Runs when the focused panel (native sheet, text-mode full-screen page)
	/// closes on Esc or its Close button.
	pub on_close: Box<dyn FnMut()>,
	/// Most rows the text-mode panel may take, borders included; a taller
	/// report scrolls inside it. `None` leaves it uncapped.
	pub max_rows: Option<Box<dyn Fn() -> Option<usize>>>,
}

impl ReportPanelOptions {
	/// Options for `body` titled `title`, dismissed by `close_key`.
	pub fn new(title: impl Into<String>, body: Box<dyn Component>, close_key: impl Into<String>, on_close: impl FnMut() + 'static) -> Self {
		ReportPanelOptions {
			title: title.into(),
			head: None,
			body,
			close_key: close_key.into(),
			on_close: Box::new(on_close),
			max_rows: None,
		}
	}

	/// Use a styled native sheet head.
	pub fn with_head(mut self, head: NodeText) -> Self {
		self.head = Some(head);
		self
	}

	/// Cap the text-mode panel at `max_rows` (read on every render).
	pub fn with_max_rows(mut self, max_rows: impl Fn() -> Option<usize> + 'static) -> Self {
		self.max_rows = Some(Box::new(max_rows));
		self
	}
}

/// A read-only command report panel.
pub struct ReportPanel {
	title: String,
	role: String,
	head: Option<NodeText>,
	body: RefCell<Box<dyn Component>>,
	on_close: Box<dyn FnMut()>,
	max_rows: Option<Box<dyn Fn() -> Option<usize>>>,
	view: ScrollView,
	footer: Text,
	close_hint: String,
	body_rows: usize,
	scroll: Option<Scroll>,
	bindings: KeybindingsManager,
	theme: OverlayTheme,
}

impl std::fmt::Debug for ReportPanel {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ReportPanel")
			.field("title", &self.title)
			.field("role", &self.role)
			.field("body_rows", &self.body_rows)
			.field("close_hint", &self.close_hint)
			.finish()
	}
}

impl ReportPanel {
	/// Build the panel from `options`.
	pub fn new(options: ReportPanelOptions) -> Self {
		let close_hint = format!("{} to close", options.close_key);
		let mut view = ScrollView::new(Vec::new(), 0);
		view.set_scrollbar_mode(ScrollbarMode::Auto);
		view.set_ellipsis(Ellipsis::Omit);
		ReportPanel {
			title: options.title,
			role: "omp.overlay.report".to_string(),
			head: options.head,
			body: RefCell::new(options.body),
			on_close: options.on_close,
			max_rows: options.max_rows,
			view,
			footer: Text::new(close_hint.clone()),
			close_hint,
			body_rows: 0,
			scroll: None,
			bindings: KeybindingsManager::new(),
			theme: OverlayTheme::default(),
		}
	}
}

impl Component for ReportPanel {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let lines = self.body.borrow_mut().render(inner);
		let rows = lines.len();
		let cap = self.max_rows.as_ref().and_then(|max| max());
		let body_max = match cap {
			None => rows,
			Some(cap) => cap.saturating_sub(TEXT_CHROME_ROWS).max(1),
		};
		let overflows = rows > body_max;
		self.body_rows = rows.min(body_max);
		self.view.set_lines(lines);
		self.view.set_height(self.body_rows);
		let scroll_hint = format!(
			"{}/{} scroll · ",
			self.hint_key("tui.select.pageUp"),
			self.hint_key("tui.select.pageDown")
		);
		self.footer.set_text(if overflows {
			format!("{scroll_hint}{}", self.close_hint)
		} else {
			self.close_hint.clone()
		});

		let mut out = Vec::with_capacity(self.body_rows + TEXT_CHROME_ROWS);
		out.push(top_border(width, &self.title, &self.theme));
		out.push(row("", width, &self.theme));
		for line in self.view.render(inner) {
			out.push(row(&line, width, &self.theme));
		}
		out.push(row("", width, &self.theme));
		out.push(row(self.footer.text(), width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		match cap {
			Some(cap) if out.len() > cap => out.truncate(cap),
			_ => {}
		}
		out
	}

	fn handle_input(&mut self, key: &Key) {
		if matches_select_cancel(key, &self.bindings) {
			(self.on_close)();
			return;
		}
		let by = if matches_select_up(key, &self.bindings) {
			ScrollBy::LineUp
		} else if matches_select_down(key, &self.bindings) {
			ScrollBy::LineDown
		} else if matches_select_page_up(key, &self.bindings) {
			ScrollBy::PageUp
		} else if matches_select_page_down(key, &self.bindings) {
			ScrollBy::PageDown
		} else if keys::is_home(key) {
			ScrollBy::Start
		} else if keys::is_end(key) {
			ScrollBy::End
		} else {
			return;
		};
		self.request_scroll(by);
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		match event.kind {
			MouseKind::ScrollUp => {
				self.view.scroll_by(-3);
			}
			MouseKind::ScrollDown => {
				self.view.scroll_by(3);
			}
			_ => {}
		}
	}
}

impl ReportPanel {
	/// The panel is shown with focus (native sheet, text-mode full-screen page):
	/// its own [`Component::handle_input`] closes it, so the hint names that key.
	pub fn hold_focus(&mut self) {
		self.close_hint = format!("{} to close", self.hint_key("tui.select.cancel"));
		self.footer.set_text(self.close_hint.clone());
	}

	/// Text mode: rows the whole box takes at `width` when nothing caps it.
	pub fn height_at(&self, width: u16) -> usize {
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		self.body.borrow_mut().render(inner).len() + TEXT_CHROME_ROWS
	}

	/// Replace the panel theme.
	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	/// Use a host keybinding set for the close/scroll keys and the hints.
	pub fn set_keybindings(&mut self, bindings: KeybindingsManager) {
		self.bindings = bindings;
	}

	/// The collapsed title shown in the top border.
	pub fn title(&self) -> &str {
		&self.title
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The footer hint currently drawn.
	pub fn close_hint(&self) -> &str {
		&self.close_hint
	}

	/// Scrolled body rows currently in view.
	pub fn body_rows(&self) -> usize {
		self.body_rows
	}

	/// The pending keyboard scroll request, if none has been forwarded yet.
	pub fn scroll_request(&self) -> Option<&Scroll> {
		self.scroll.as_ref()
	}

	/// Close runs what Esc runs.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) -> bool {
		match event {
			NativeUiEvent::Action { act, .. } if act == "close" => {
				(self.on_close)();
				true
			}
			_ => false,
		}
	}

	/// Natively the terminal scrolls the sheet body; text mode scrolls the box.
	fn request_scroll(&mut self, by: ScrollBy) {
		let n = self.scroll.as_ref().map_or(1, |scroll| scroll.n + 1);
		self.scroll = Some(Scroll { by, n });
		let page = self.body_rows.saturating_sub(1).max(1) as i64;
		match by {
			ScrollBy::Start => self.view.scroll_to_top(),
			ScrollBy::End => self.view.scroll_to_bottom(),
			ScrollBy::LineUp => self.view.scroll_by(-1),
			ScrollBy::LineDown => self.view.scroll_by(1),
			ScrollBy::PageUp => self.view.scroll_by(-page),
			ScrollBy::PageDown => self.view.scroll_by(page),
		}
	}

	fn hint_key(&self, action: &str) -> String {
		self.bindings
			.get_keys(action)
			.first()
			.map(|key| format_tooltip_key(key))
			.unwrap_or_default()
	}

	/// The sheet body (the overlay is the frame, and its body the scroller): the
	/// report as `omp.report.body`, then Close (Esc) as the sheet's footer.
	pub fn describe_sheet(&self, cx: &DescribeContext) -> NativeNode {
		let mut body_props = Props::new();
		body_props.set_role("omp.report.body");
		let child = describe_child(&mut **self.body.borrow_mut(), cx);
		let mut body = col(vec![child], body_props);
		body.kind = NodeKind::Col;
		body.key = Some("body".to_string());
		body.scroll = self.scroll.clone();
		let close = action_button(
			"Close",
			"close",
			ActionButtonOptions {
				keys: Some("escape".to_string()),
				..ActionButtonOptions::default()
			},
		);
		let mut props = Props::new();
		props.set_str("gap", "md");
		col(vec![body, action_bar(&[None, Some(close)], "report-actions")], props)
	}

	/// The native sheet head: the styled head when given, else the plain title.
	pub fn sheet_head(&self) -> NodeText {
		self.head.clone().unwrap_or_else(|| NodeText::plain(self.title.clone()))
	}

	/// The sheet's overlay spec: a centred `lg` sheet with this panel's role.
	pub fn overlay_spec(&self) -> ReportOverlaySpec {
		ReportOverlaySpec {
			role: self.role.clone(),
			size: "lg",
			anchor: "center",
			head: self.sheet_head(),
		}
	}

	/// The native sheet body keyed by its stable child identity.
	pub fn keyed_sheet(&self, cx: &DescribeContext) -> NativeNode {
		keyed(self.describe_sheet(cx), "report")
	}
}

/// How the host places the report's native sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportOverlaySpec {
	/// Semantic role (`omp.overlay.report`).
	pub role: String,
	/// Sheet size token.
	pub size: &'static str,
	/// Sheet anchor token.
	pub anchor: &'static str,
	/// Sheet head.
	pub head: NodeText,
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	/// A body of fixed rows, so the panel's row math is observable.
	struct Lines {
		rows: Vec<String>,
	}

	impl Lines {
		fn new(count: usize) -> Self {
			Lines {
				rows: (0..count).map(|index| format!("row {index}")).collect(),
			}
		}
	}

	impl Component for Lines {
		fn render(&mut self, _width: u16) -> Vec<String> {
			self.rows.clone()
		}
	}

	fn panel(count: usize, cap: Option<usize>, closed: Rc<RefCell<usize>>) -> ReportPanel {
		let mut options = ReportPanelOptions::new("Changelog", Box::new(Lines::new(count)), "esc", move || {
			*closed.borrow_mut() += 1;
		});
		if let Some(cap) = cap {
			options = options.with_max_rows(move || Some(cap));
		}
		ReportPanel::new(options)
	}

	#[test]
	fn renders_title_body_and_footer_between_borders() {
		let closed = Rc::new(RefCell::new(0));
		let mut panel = panel(2, None, closed);
		let rows = panel.render(40);
		assert_eq!(rows.len(), 2 + TEXT_CHROME_ROWS);
		assert!(rows[0].contains("Changelog"), "{rows:?}");
		assert!(rows[2].contains("row 0"), "{rows:?}");
		assert!(rows[3].contains("row 1"), "{rows:?}");
		assert!(rows[5].contains("to close"), "{rows:?}");
		assert_eq!(panel.body_rows(), 2);
		assert!(panel.scroll_request().is_none());
	}

	#[test]
	fn caps_the_box_and_scrolls_a_taller_body() {
		let closed = Rc::new(RefCell::new(0));
		let mut panel = panel(20, Some(9), closed);
		let rows = panel.render(40);
		assert_eq!(rows.len(), 9);
		assert_eq!(panel.body_rows(), 9 - TEXT_CHROME_ROWS);
		assert!(rows[7].contains("scroll"), "{rows:?}");
		assert!(rows[7].contains("to close"), "{rows:?}");
	}

	#[test]
	fn height_at_counts_body_rows_and_chrome() {
		let closed = Rc::new(RefCell::new(0));
		let panel = panel(4, None, closed);
		assert_eq!(panel.height_at(40), 4 + TEXT_CHROME_ROWS);
	}

	#[test]
	fn esc_closes_and_scroll_keys_move_the_body() {
		let closed = Rc::new(RefCell::new(0));
		let mut panel = panel(30, Some(8), Rc::clone(&closed));
		panel.render(40);
		panel.handle_input(&Key::PageDown);
		assert_eq!(panel.scroll_request().map(|scroll| scroll.by), Some(ScrollBy::PageDown));
		assert_eq!(panel.scroll_request().map(|scroll| scroll.n), Some(1));
		panel.handle_input(&Key::End);
		assert_eq!(panel.scroll_request().map(|scroll| scroll.by), Some(ScrollBy::End));
		assert_eq!(panel.scroll_request().map(|scroll| scroll.n), Some(2));
		panel.handle_input(&Key::Esc);
		assert_eq!(*closed.borrow(), 1);
	}

	#[test]
	fn hold_focus_names_the_focus_key_in_the_hint() {
		let closed = Rc::new(RefCell::new(0));
		let mut panel = panel(1, None, closed);
		panel.hold_focus();
		assert!(panel.close_hint().ends_with("to close"));
		assert_ne!(panel.close_hint(), "esc to close");
		let rows = panel.render(30);
		assert!(rows[rows.len() - 2].contains(panel.close_hint()), "{rows:?}");
	}

	#[test]
	fn wheel_scrolls_three_rows() {
		let closed = Rc::new(RefCell::new(0));
		let mut panel = panel(30, Some(8), closed);
		panel.render(40);
		panel.handle_mouse(&MouseEvent {
			kind: MouseKind::ScrollDown,
			column: 0,
			row: 0,
			shift: false,
			ctrl: false,
			alt: false,
		});
		assert_eq!(panel.view.scroll_offset(), 3);
	}

	#[test]
	fn close_action_and_sheet_description() {
		let closed = Rc::new(RefCell::new(0));
		let mut panel = panel(3, None, Rc::clone(&closed));
		let event = NativeUiEvent::Action {
			key: vec!["report".to_string()],
			act: "close".to_string(),
			value: None,
			mods: Vec::new(),
		};
		assert!(panel.handle_native_event(&event));
		assert_eq!(*closed.borrow(), 1);
		let spec = panel.overlay_spec();
		assert_eq!(spec.role, "omp.overlay.report");
		assert_eq!(spec.size, "lg");
		assert_eq!(spec.anchor, "center");
		assert_eq!(spec.head.as_plain(), Some("Changelog"));
		let cx = DescribeContext::new(60);
		let sheet = panel.describe_sheet(&cx);
		assert_eq!(sheet.kind, NodeKind::Col);
		assert_eq!(sheet.children.len(), 2);
		assert_eq!(sheet.children[0].key.as_deref(), Some("body"));
		panel.handle_input(&Key::Down);
		let sheet = panel.describe_sheet(&cx);
		let scroll = sheet.children[0].scroll.clone().expect("scroll");
		assert_eq!(scroll.by, ScrollBy::LineDown);
	}
}
