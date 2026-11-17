//! Real `BtwPanelComponent` overlay port.
//!
//! Reference: `l/src/overlays/btw-panel.ts` (`BtwPanelComponent` extending
//! `OverlayPanel`, streaming answer, copy/inject/follow-up/branch actions,
//! footer and native describe).
//!
//! Deviations: `OverlayPanel`, `StreamingPanelContent`, markdown widgets,
//! TUI component-scoped render requests, and clipboard/branch hosts are
//! abstracted. This port owns panel state/answer/error/inject/copied flags,
//! footer/action availability, and renders a bordered panel plus native rows
//! describe capturing state/actions.

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::Key;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::overlays::btw_history::{split_btw_inject, BtwInjectParts};
use crate::overlays::btw_inject::{describe_btw_inject_answer, BtwInjectAnswerText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtwPanelState {
	Running,
	Complete,
	Branching,
	Aborted,
	Error,
}

/// Host hooks for panel actions/capabilities.
pub trait BtwPanelCallbacks {
	fn request_render(&self) {}
	fn can_branch(&self) -> bool { false }
	fn can_follow_up(&self) -> bool { false }
	fn can_inject(&self) -> bool { false }
}

fn sanitize_line(s: &str) -> String {
	s.replace(['\n', '\r', '\t'], " ")
}

fn replace_tabs(s: &str) -> String {
	s.replace('\t', "    ")
}

/// Inline BTW answer panel component.
///
/// Reference: `BtwPanelComponent` in `l/src/overlays/btw-panel.ts`.
pub struct BtwPanelComponent<C: BtwPanelCallbacks> {
	callbacks: C,
	pub question: String,
	pub base_title: String,
	pub title: String,
	pub state: BtwPanelState,
	pub answer: String,
	pub error_message: Option<String>,
	pub visible_answer: String,
	pub closed: bool,
	pub copied: bool,
	pub injected: bool,
	theme: OverlayTheme,
}

impl<C: BtwPanelCallbacks> std::fmt::Debug for BtwPanelComponent<C> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BtwPanelComponent")
			.field("state", &self.state)
			.field("copied", &self.copied)
			.field("injected", &self.injected)
			.finish_non_exhaustive()
	}
}

impl<C: BtwPanelCallbacks> BtwPanelComponent<C> {
	#[must_use]
	pub fn new(question: impl Into<String>, title: Option<String>, callbacks: C) -> Self {
		let question = question.into();
		let base_title = replace_tabs(&title.unwrap_or_else(|| format!("/btw {question}")));
		Self {
			callbacks,
			question,
			title: base_title.clone(),
			base_title,
			state: BtwPanelState::Running,
			answer: String::new(),
			error_message: None,
			visible_answer: String::new(),
			closed: false,
			copied: false,
			injected: false,
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	pub fn append_text(&mut self, delta: &str) {
		if delta.is_empty() || self.closed {
			return;
		}
		self.answer.push_str(delta);
		self.visible_answer = replace_tabs(&self.answer).trim().to_string();
		self.set_copied(false);
		self.callbacks.request_render();
	}

	pub fn set_answer(&mut self, text: impl Into<String>) {
		if self.closed {
			return;
		}
		self.answer = text.into();
		self.visible_answer = replace_tabs(&self.answer).trim().to_string();
		self.set_copied(false);
		self.callbacks.request_render();
	}

	pub fn mark_complete(&mut self) {
		if self.closed {
			return;
		}
		self.state = BtwPanelState::Complete;
		self.error_message = None;
		self.set_copied(false);
		self.callbacks.request_render();
	}

	pub fn mark_branching(&mut self) {
		if self.closed {
			return;
		}
		self.state = BtwPanelState::Branching;
		self.error_message = None;
		self.set_copied(false);
		self.callbacks.request_render();
	}

	pub fn mark_aborted(&mut self) {
		if self.closed {
			return;
		}
		self.state = BtwPanelState::Aborted;
		self.error_message = None;
		self.set_copied(false);
		self.callbacks.request_render();
	}

	pub fn mark_error(&mut self, message: impl Into<String>) {
		if self.closed {
			return;
		}
		self.state = BtwPanelState::Error;
		self.error_message = Some(message.into());
		self.set_copied(false);
		self.callbacks.request_render();
	}

	#[must_use]
	pub fn is_copyable(&self) -> bool {
		self.state == BtwPanelState::Complete && !self.visible_answer.trim().is_empty()
	}

	pub fn mark_copied(&mut self) {
		if self.closed || !self.is_copyable() {
			return;
		}
		self.set_copied(true);
		self.callbacks.request_render();
	}

	pub fn set_injected(&mut self, injected: bool) {
		if self.closed || self.injected == injected {
			return;
		}
		self.injected = injected;
		self.callbacks.request_render();
	}

	fn set_copied(&mut self, copied: bool) {
		self.copied = copied;
		self.title = if copied {
			format!("{} ✓ Copied", self.base_title)
		} else {
			self.base_title.clone()
		};
	}

	fn inject_parts(&self) -> Option<BtwInjectParts> {
		if self.callbacks.can_inject() {
			return split_btw_inject(&self.visible_answer);
		}
		None
	}

	fn footer_line(&self) -> String {
		match self.state {
			BtwPanelState::Running => "esc to cancel".to_string(),
			BtwPanelState::Complete => {
				let mut actions = Vec::new();
				if self.callbacks.can_inject() {
					actions.push("i to inject".to_string());
				}
				if self.is_copyable() {
					actions.push(if self.copied { "c to copy again" } else { "c to copy" }.to_string());
				}
				if self.callbacks.can_follow_up() {
					actions.push("f to follow up".to_string());
				}
				if self.callbacks.can_branch() {
					actions.push("b to branch".to_string());
				}
				actions.push("esc to close".to_string());
				if self.copied {
					format!("✓ Copied · {}", actions.join(" · "))
				} else {
					actions.join(" · ")
				}
			}
			BtwPanelState::Branching => "Branching to chat…".to_string(),
			BtwPanelState::Aborted => "Cancelled · esc to close".to_string(),
			BtwPanelState::Error => "Error · esc to close".to_string(),
		}
	}

	fn body_lines(&self, width: usize) -> Vec<String> {
		let maxw = width.max(10);
		if self.state == BtwPanelState::Error {
			return vec![sanitize_line(self.error_message.as_deref().unwrap_or("Unknown error"))]
				.into_iter()
				.map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l })
				.collect();
		}
		if self.visible_answer.is_empty() {
			let waiting = if self.state == BtwPanelState::Running {
				"Waiting for response…"
			} else {
				"No text returned."
			};
			return vec![waiting.to_string()];
		}
		if let Some(parts) = self.inject_parts() {
			let desc = describe_btw_inject_answer(&parts, self.injected);
			let mut lines = Vec::new();
			if let Some(b) = desc.before {
				lines.push(b);
			}
			for l in desc.block.lines {
				lines.push(l);
			}
			if let Some(a) = desc.after {
				lines.push(a);
			}
			return lines
				.into_iter()
				.map(|l| sanitize_line(&l))
				.map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l })
				.collect();
		}
		self.visible_answer
			.lines()
			.map(sanitize_line)
			.map(|l| if l.len() > maxw { l.chars().take(maxw).collect() } else { l })
			.collect()
	}
}

impl<C: BtwPanelCallbacks> Component for BtwPanelComponent<C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner_w = width.saturating_sub(4) as usize;
		let mut out = vec![top_border(width, &self.title, &self.theme)];
		for l in self.body_lines(inner_w.max(10)).into_iter().take(12) {
			out.push(row(&l, width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		out.push(row(&sanitize_line(&self.footer_line()), width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}

	fn handle_input(&mut self, _key: &Key) {
		// Host routes escape/copy/branch/follow-up/inject commands; component
		// exposes state transitions via methods.
	}
}

impl<C: BtwPanelCallbacks> NativeDescribe for BtwPanelComponent<C> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = {
			let inner_w = cx.cols.saturating_sub(4) as usize;
			let mut out = vec![self.title.clone()];
			out.extend(self.body_lines(inner_w.max(10)).into_iter().take(12));
			out.push(self.footer_line());
			out
		};
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("state", serde_json::Value::String(format!("{:?}", self.state)));
		props.insert("copied", serde_json::Value::Bool(self.copied));
		props.insert("injected", serde_json::Value::Bool(self.injected));
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

	#[derive(Default)]
	struct Cb {
		inject: bool,
		branch: bool,
		follow: bool,
		renders: RefCell<usize>,
	}
	impl BtwPanelCallbacks for Cb {
		fn request_render(&self) { *self.renders.borrow_mut() += 1; }
		fn can_branch(&self) -> bool { self.branch }
		fn can_follow_up(&self) -> bool { self.follow }
		fn can_inject(&self) -> bool { self.inject }
	}

	#[test]
	fn btw_panel_component_streams_completes_and_copies() {
		let mut p = BtwPanelComponent::new("why?", None, Cb { follow: true, branch: true, ..Default::default() });
		p.append_text("hello");
		assert_eq!(p.visible_answer, "hello");
		p.mark_complete();
		assert!(p.is_copyable());
		p.mark_copied();
		assert!(p.copied);
		assert!(p.title.contains("Copied"));
		let lines = p.render(60);
		assert!(lines.iter().any(|l| l.contains("/btw")));
		let node = p.describe_native(&DescribeContext { cols: 60, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}

	#[test]
	fn btw_panel_component_error_abort_inject() {
		let mut p = BtwPanelComponent::new("q", Some("/sitrep q".into()), Cb { inject: true, ..Default::default() });
		p.set_answer("before\n<inject>cmd</inject>\nafter");
		p.mark_complete();
		assert!(p.inject_parts().is_some());
		p.set_injected(true);
		assert!(p.injected);
		p.mark_aborted();
		assert_eq!(p.state, BtwPanelState::Aborted);
		p.mark_error("boom");
		assert_eq!(p.state, BtwPanelState::Error);
		assert!(p.render(50).iter().any(|l| l.contains("boom")));
	}
}
