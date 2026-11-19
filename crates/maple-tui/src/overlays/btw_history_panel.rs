//! Real `BtwHistoryPanel` overlay port.
//!
//! Reference: `l/src/overlays/btw-history-panel.ts` (`BtwHistoryPanel`,
//! two-pane history/detail, follow-up composer, inject/copy/branch actions,
//! scroll and focus handling).
//!
//! Deviations: TUI widgets (`ScrollView`, `SplitPane`, `Input`, `Markdown`,
//! bracketed paste/space-hold/image paste hooks, async follow-up, and host
//! clipboard/branch/inject services) are injected or reduced to state-driven
//! text/native-row rendering. This port owns record selection, list/answer focus,
//! follow-up composer state, action enablement, copy/inject/branch/cancel
//! transitions, scroll offsets, and describe rows.

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::mouse::{MouseEvent, MouseKind};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::overlays::btw_history::{
	get_btw_copy_text, get_btw_latest_turn, get_btw_record_inject, get_btw_topic_label, get_btw_turns,
	BtwHistoryRecord, BtwHistoryTurn, BtwInjectParts,
};
use crate::overlays::btw_inject::BtwInjectState;

pub type ThemeColor = String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowUpComposer {
	pub record_id: String,
	pub text: String,
	pub notice: Option<String>,
	pub submit_queued: bool,
	pub follow_up_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneFocus {
	List,
	Answer,
}

/// Host hooks for history-panel actions.
pub trait BtwHistoryPanelCallbacks {
	fn on_close(&self);
	fn request_render(&self);
	fn on_copy(&self, _record: &BtwHistoryRecord) {}
	fn on_cancel(&self, _record: &BtwHistoryRecord) {}
	fn can_follow_up(&self, _record: &BtwHistoryRecord) -> bool { false }
	fn can_branch(&self, _record: &BtwHistoryRecord) -> bool { false }
	fn on_branch(&self, _record: &BtwHistoryRecord) {}
	fn on_follow_up(&self, _record: &BtwHistoryRecord, _question: &str) -> bool { false }
	fn can_inject(&self, _record: &BtwHistoryRecord) -> bool { false }
	fn inject_state(&self, _record: &BtwHistoryRecord, _turn_index: usize) -> Option<BtwInjectState> { None }
	fn send_inject(&self, _record: &BtwHistoryRecord) {}
}

/// Session-local side questions history panel.
///
/// Reference: `BtwHistoryPanel` in `l/src/overlays/btw-history-panel.ts`.
pub struct BtwHistoryPanel<C: BtwHistoryPanelCallbacks> {
	callbacks: C,
	pub records: Vec<BtwHistoryRecord>,
	pub selected_index: usize,
	pub focus: PaneFocus,
	pub answer_scroll: usize,
	pub list_scroll: usize,
	pub focused: bool,
	pub escape_hides: bool,
	pub composer: Option<FollowUpComposer>,
	pub notice: Option<String>,
	pub follow_up_pending: bool,
	pub last_copied_id: Option<String>,
	theme: OverlayTheme,
}

impl<C: BtwHistoryPanelCallbacks> std::fmt::Debug for BtwHistoryPanel<C> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BtwHistoryPanel")
			.field("records", &self.records.len())
			.field("selected_index", &self.selected_index)
			.field("focus", &self.focus)
			.finish_non_exhaustive()
	}
}

impl<C: BtwHistoryPanelCallbacks> BtwHistoryPanel<C> {
	#[must_use]
	pub fn new(records: Vec<BtwHistoryRecord>, callbacks: C) -> Self {
		Self {
			callbacks,
			records,
			selected_index: 0,
			focus: PaneFocus::List,
			answer_scroll: 0,
			list_scroll: 0,
			focused: false,
			escape_hides: false,
			composer: None,
			notice: None,
			follow_up_pending: false,
			last_copied_id: None,
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	pub fn set_focused(&mut self, focused: bool) {
		self.focused = focused;
	}

	pub fn set_escape_hides(&mut self, hide: bool) {
		self.escape_hides = hide;
	}

	#[must_use]
	pub fn selected(&self) -> Option<&BtwHistoryRecord> {
		self.records.get(self.selected_index)
	}

	pub fn select(&mut self, index: usize) {
		if self.records.is_empty() {
			self.selected_index = 0;
		} else {
			self.selected_index = index.min(self.records.len() - 1);
		}
		self.answer_scroll = 0;
	}

	pub fn select_by_id(&mut self, id: &str) -> bool {
		if let Some(i) = self.records.iter().position(|r| r.id == id) {
			self.select(i);
			true
		} else {
			false
		}
	}

	pub fn open_follow_up(&mut self) {
		let Some(record) = self.selected() else { return };
		if !self.callbacks.can_follow_up(record) {
			self.notice = Some("Follow-up unavailable".to_string());
			return;
		}
		self.composer = Some(FollowUpComposer {
			record_id: record.id.clone(),
			text: String::new(),
			notice: None,
			submit_queued: false,
			follow_up_pending: false,
		});
		self.focus = PaneFocus::Answer;
	}

	pub fn cancel_follow_up(&mut self) {
		self.composer = None;
		self.follow_up_pending = false;
	}

	pub fn submit_follow_up(&mut self) {
		let Some(mut c) = self.composer.clone() else { return };
		let Some(record) = self.selected().cloned() else { return };
		let question = c.text.trim().to_string();
		if question.is_empty() {
			c.notice = Some("Enter a follow-up question".to_string());
			self.composer = Some(c);
			return;
		}
		self.follow_up_pending = true;
		c.follow_up_pending = true;
		self.composer = Some(c);
		if self.callbacks.on_follow_up(&record, &question) {
			self.composer = None;
			self.follow_up_pending = false;
		} else {
			self.follow_up_pending = false;
			if let Some(c) = &mut self.composer {
				c.follow_up_pending = false;
				c.notice = Some("Follow-up failed to start".to_string());
			}
		}
	}

	fn copy_selected(&mut self) {
		let Some(record) = self.selected() else { return };
		if get_btw_copy_text(record).is_none() {
			return;
		}
		self.callbacks.on_copy(record);
		self.last_copied_id = Some(record.id.clone());
	}

	fn branch_selected(&mut self) {
		let Some(record) = self.selected() else { return };
		if self.callbacks.can_branch(record) {
			self.callbacks.on_branch(record);
		}
	}

	fn inject_selected(&mut self) {
		let Some(record) = self.selected() else { return };
		if self.callbacks.can_inject(record) && get_btw_record_inject(record).is_some() {
			self.callbacks.send_inject(record);
		}
	}

	fn status_label(status: &str) -> &'static str {
		match status {
			"running" => "Running",
			"complete" => "Complete",
			"cancelled" => "Cancelled",
			"error" => "Error",
			"interrupted" => "Interrupted",
			_ => "Unknown",
		}
	}

	fn render_list(&self, width: usize) -> Vec<String> {
		if self.records.is_empty() {
			return vec!["(no side questions)".to_string()];
		}
		self.records
			.iter()
			.enumerate()
			.map(|(i, r)| {
				let cursor = if i == self.selected_index { "›" } else { " " };
				let topic = get_btw_topic_label(r);
				format!("{cursor} {} · {}", Self::status_label(&r.status), topic)
			})
			.map(|l| if l.len() > width { l.chars().take(width).collect() } else { l })
			.collect()
	}

	fn render_detail(&self, width: usize) -> Vec<String> {
		let Some(record) = self.selected() else {
			return vec!["No selection".to_string()];
		};
		let mut lines = vec![format!("Topic: {}", get_btw_topic_label(record))];
		if let Some(title) = &record.title {
			if !title.trim().is_empty() {
				lines.push(format!("Title: {title}"));
			}
		}
		for (idx, turn) in get_btw_turns(record).iter().enumerate() {
			lines.push(format!("Q{}: {}", idx + 1, turn.question));
			if !turn.answer.trim().is_empty() {
				lines.push(format!("A{}: {}", idx + 1, turn.answer));
			} else if turn.status == "running" {
				lines.push(format!("A{}: (running)", idx + 1));
			}
			if let Some(err) = &turn.error {
				lines.push(format!("Error: {err}"));
			}
			if let Some(state) = self.callbacks.inject_state(record, idx) {
				let _ = state;
				if let Some(inject) = crate::overlays::btw_history::get_btw_inject(&turn.answer) {
					lines.push(format!("Inject: {inject}"));
				}
			}
		}
		if let Some(c) = &self.composer {
			lines.push(String::new());
			lines.push(format!("Follow-up: {}", c.text));
			if let Some(n) = &c.notice {
				lines.push(format!("Notice: {n}"));
			}
		}
		let maxw = width.max(10);
		lines
			.into_iter()
			.map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l })
			.collect()
	}

	fn render_frame(&self, width: u16) -> Vec<String> {
		let inner_w = width.saturating_sub(4) as usize;
		let title = "BTW history";
		let mut out = vec![top_border(width, title, &self.theme)];
		let list = self.render_list(inner_w / 3);
		let detail = self.render_detail(inner_w.saturating_sub(inner_w / 3 + 3));
		let n = list.len().max(detail.len()).clamp(1, 14);
		let left_w = (inner_w / 3).max(16);
		let right_w = inner_w.saturating_sub(left_w + 3).max(16);
		for i in 0..n {
			let l = list.get(i).cloned().unwrap_or_default();
			let r = detail
				.get(i + self.answer_scroll)
				.cloned()
				.or_else(|| detail.get(i).cloned())
				.unwrap_or_default();
			out.push(row(&format!("{:<lw$} │ {}", l, r, lw = left_w), width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		let hint = self.notice.clone().unwrap_or_else(|| {
			"↑/↓ select · tab pane · enter/f follow-up · c copy · b branch · i inject · esc close".to_string()
		});
		out.push(row(&hint, width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<C: BtwHistoryPanelCallbacks> Component for BtwHistoryPanel<C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_frame(width)
	}

	fn handle_input(&mut self, key: &Key) {
		self.notice = None;
		if let Some(mut c) = self.composer.take() {
			match key {
				Key::Esc => {
					self.composer = None;
					self.callbacks.request_render();
					return;
				}
				Key::Enter => {
					self.composer = Some(c);
					self.submit_follow_up();
					self.callbacks.request_render();
					return;
				}
				Key::Backspace => {
					c.text.pop();
					self.composer = Some(c);
					self.callbacks.request_render();
					return;
				}
				Key::Char(ch) => {
					c.text.push(*ch);
					self.composer = Some(c);
					self.callbacks.request_render();
					return;
				}
				_ => {
					self.composer = Some(c);
				}
			}
		}

		if keys::is_escape(key) {
			let running = self.selected().map(get_btw_latest_turn).map(|t| t.status == "running").unwrap_or(false);
			if running && self.escape_hides {
				self.callbacks.on_close();
				return;
			}
			if running {
				if let Some(r) = self.selected() {
					self.callbacks.on_cancel(r);
				}
				return;
			}
			self.callbacks.on_close();
			return;
		}
		match key {
			Key::Tab | Key::BackTab => {
				self.focus = match self.focus {
					PaneFocus::List => PaneFocus::Answer,
					PaneFocus::Answer => PaneFocus::List,
				};
			}
			Key::Up | Key::Char('k') if self.focus == PaneFocus::List => {
				self.select(self.selected_index.saturating_sub(1));
			}
			Key::Down | Key::Char('j') if self.focus == PaneFocus::List => {
				self.select(self.selected_index.saturating_add(1));
			}
			Key::Up | Key::Char('k') if self.focus == PaneFocus::Answer => {
				self.answer_scroll = self.answer_scroll.saturating_sub(1);
			}
			Key::Down | Key::Char('j') if self.focus == PaneFocus::Answer => {
				self.answer_scroll = self.answer_scroll.saturating_add(1);
			}
			Key::Char('f') | Key::Enter => self.open_follow_up(),
			Key::Char('c') => self.copy_selected(),
			Key::Char('b') => self.branch_selected(),
			Key::Char('i') => self.inject_selected(),
			_ => {}
		}
		self.callbacks.request_render();
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		match event.kind {
			MouseKind::ScrollUp => {
				if self.focus == PaneFocus::List {
					self.select(self.selected_index.saturating_sub(1));
				} else {
					self.answer_scroll = self.answer_scroll.saturating_sub(2);
				}
			}
			MouseKind::ScrollDown => {
				if self.focus == PaneFocus::List {
					self.select(self.selected_index.saturating_add(1));
				} else {
					self.answer_scroll = self.answer_scroll.saturating_add(2);
				}
			}
			_ => {}
		}
		self.callbacks.request_render();
	}

	fn invalidate(&mut self) {}
}

impl<C: BtwHistoryPanelCallbacks> NativeDescribe for BtwHistoryPanel<C> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_frame(cx.cols.max(40));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(40)));
		props.insert("focus", serde_json::Value::String(format!("{:?}", self.focus)));
		props.insert(
			"selectedId",
			serde_json::Value::String(self.selected().map(|r| r.id.clone()).unwrap_or_default()),
		);
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;

	struct Cb {
		closed: RefCell<bool>,
		copied: RefCell<bool>,
	}
	impl BtwHistoryPanelCallbacks for Cb {
		fn on_close(&self) { *self.closed.borrow_mut() = true; }
		fn request_render(&self) {}
		fn on_copy(&self, _record: &BtwHistoryRecord) { *self.copied.borrow_mut() = true; }
	}

	fn rec(id: &str, q: &str, a: &str) -> BtwHistoryRecord {
		BtwHistoryRecord {
			id: id.into(),
			title: None,
			turns: vec![BtwHistoryTurn { question: q.into(), answer: a.into(), status: "complete".into(), error: None, aborted: None }],
			question: q.into(),
			answer: a.into(),
			status: "complete".into(),
			cancelled: false,
			error: None,
			asked_at: 0,
			completed_at: None,
			consulted_tools: None,
		}
	}

	#[test]
	fn btw_history_panel_renders_selects_and_copies() {
		let mut p = BtwHistoryPanel::new(vec![rec("1", "q1", "a1"), rec("2", "q2", "a2")], Cb { closed: RefCell::new(false), copied: RefCell::new(false) });
		let lines = p.render(80);
		assert!(lines.iter().any(|l| l.contains("BTW history")));
		p.handle_input(&Key::Down);
		assert_eq!(p.selected_index, 1);
		p.handle_input(&Key::Char('c'));
		assert!(*p.callbacks.copied.borrow());
		let node = p.describe_native(&DescribeContext { cols: 60, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}

	#[test]
	fn btw_history_panel_follow_up_and_close() {
		struct Cb2;
		impl BtwHistoryPanelCallbacks for Cb2 {
			fn on_close(&self) {}
			fn request_render(&self) {}
			fn can_follow_up(&self, _record: &BtwHistoryRecord) -> bool { true }
			fn on_follow_up(&self, _record: &BtwHistoryRecord, _question: &str) -> bool { true }
		}
		let mut p = BtwHistoryPanel::new(vec![rec("1", "q1", "a1")], Cb2);
		p.handle_input(&Key::Char('f'));
		assert!(p.composer.is_some());
		p.handle_input(&Key::Char('x'));
		p.handle_input(&Key::Enter);
		assert!(p.composer.is_none());
	}
}
