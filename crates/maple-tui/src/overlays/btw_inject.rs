//! BTW `<inject>` block presentation helpers.
//!
//! Reference: `l/src/overlays/btw-inject.ts`.
//!
//! Deviations:
//! - Terminal rendering (`BtwInjectAnswer` Component, ANSI/theme/wrap) and
//!   native node construction (`card/col/md/text/span`) depend on TUI theme and
//!   native describe primitives unavailable in this crate. This module ports
//!   labels, state, and deterministic line/structure builders that hosts can
//!   render.
//! - `sanitizeDisplayText`/markdown theming are abstracted as pass-through line
//!   splitting over formatted inject/prose text.

use crate::overlays::btw_history::{format_btw_inject_text, BtwInjectParts};

/// Labels a steering prompt that has not been sent yet.
pub const BTW_INJECT_LABEL: &str = "Inject into main session";
/// Labels a sent steering prompt.
pub const BTW_INJECTED_LABEL: &str = "Injected into main session";

/// A displayed `<inject>` block state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtwInjectState {
	Pending,
	Sent,
}

/// Terminal line model for an inject block (host applies gutter/marker/color).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtwInjectBlockLine {
	pub body: String,
	pub is_label: bool,
	pub sent: bool,
}

/// Render inject block content into labeled line records (wrapping delegated to host).
#[must_use]
pub fn render_block_lines(inject: &str, sent: bool) -> Vec<BtwInjectBlockLine> {
	let mut out = Vec::new();
	for line in format_btw_inject_text(inject).split('\n') {
		if line.trim().is_empty() {
			out.push(BtwInjectBlockLine {
				body: String::new(),
				is_label: false,
				sent,
			});
		} else {
			out.push(BtwInjectBlockLine {
				body: line.to_string(),
				is_label: false,
				sent,
			});
		}
	}
	out.push(BtwInjectBlockLine {
		body: if sent { BTW_INJECTED_LABEL } else { BTW_INJECT_LABEL }.to_string(),
		is_label: true,
		sent,
	});
	out
}

/// Text-mode answer split around its inject block: prose lines, block, prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtwInjectAnswerText {
	pub inject: String,
	pub sent: bool,
	pub before: Option<String>,
	pub after: Option<String>,
	pub lines: Vec<String>,
}

impl BtwInjectAnswerText {
	#[must_use]
	pub fn new(parts: &BtwInjectParts, sent: bool) -> Self {
		let before = prose(parts.before.as_str());
		let after = prose(parts.after.as_str());
		let mut lines = Vec::new();
		if let Some(b) = &before {
			lines.push(b.clone());
			lines.push(String::new());
		}
		for bl in render_block_lines(&parts.inject, sent) {
			lines.push(bl.body);
		}
		if let Some(a) = &after {
			lines.push(String::new());
			lines.push(a.clone());
		}
		Self {
			inject: parts.inject.clone(),
			sent,
			before,
			after,
			lines,
		}
	}
}

fn prose(text: &str) -> Option<String> {
	let trimmed = text.trim();
	if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_string())
	}
}

/// Native-style description model for an inject answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtwInjectNativeBlock {
	pub role: &'static str,
	pub tone: &'static str,
	pub lines: Vec<String>,
	pub label: String,
	pub sent: bool,
}

/// Describe answer for native surfaces as structured data.
#[must_use]
pub fn describe_btw_inject_answer(parts: &BtwInjectParts, sent: bool) -> BtwInjectNativeDescription {
	let before = parts.before.trim();
	let after = parts.after.trim();
	let label = if sent {
		format!("↵ {BTW_INJECTED_LABEL}")
	} else {
		BTW_INJECT_LABEL.to_string()
	};
	let block = BtwInjectNativeBlock {
		role: "omp.btw.inject",
		tone: if sent { "success" } else { "user" },
		lines: format_btw_inject_text(&parts.inject)
			.split('\n')
			.map(str::to_string)
			.collect(),
		label: label.clone(),
		sent,
	};
	BtwInjectNativeDescription {
		before: if before.is_empty() { None } else { Some(before.to_string()) },
		block,
		after: if after.is_empty() { None } else { Some(after.to_string()) },
	}
}

/// Structured native description of a split answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtwInjectNativeDescription {
	pub before: Option<String>,
	pub block: BtwInjectNativeBlock,
	pub after: Option<String>,
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parts(before: &str, inject: &str, after: &str) -> BtwInjectParts {
		BtwInjectParts {
			before: before.into(),
			inject: inject.into(),
			after: after.into(),
			closed: true,
		}
	}

	#[test]
	fn render_block_lines_adds_label() {
		let lines = render_block_lines("a\n\nb", false);
		assert_eq!(lines.len(), 4);
		assert_eq!(lines[0].body, "a");
		assert!(lines[1].body.is_empty());
		assert_eq!(lines[3].body, BTW_INJECT_LABEL);
		assert!(lines[3].is_label);
		assert!(!lines[3].sent);
		let sent = render_block_lines("x", true);
		assert_eq!(sent.last().unwrap().body, BTW_INJECTED_LABEL);
		assert!(sent.last().unwrap().sent);
	}

	#[test]
	fn answer_text_splits_prose_and_block() {
		let t = BtwInjectAnswerText::new(&parts("hello", "cmd", "world"), false);
		assert_eq!(t.before.as_deref(), Some("hello"));
		assert_eq!(t.after.as_deref(), Some("world"));
		assert!(t.lines.contains(&"cmd".to_string()));
		assert!(t.lines.contains(&BTW_INJECT_LABEL.to_string()));
		let t2 = BtwInjectAnswerText::new(&parts("  ", "cmd", ""), true);
		assert!(t2.before.is_none());
		assert!(t2.after.is_none());
	}

	#[test]
	fn describe_native_answer_structure() {
		let d = describe_btw_inject_answer(&parts("a", "x\ny", "b"), true);
		assert_eq!(d.before.as_deref(), Some("a"));
		assert_eq!(d.after.as_deref(), Some("b"));
		assert_eq!(d.block.tone, "success");
		assert_eq!(d.block.lines, vec!["x".to_string(), "y".to_string()]);
		assert!(d.block.label.contains(BTW_INJECTED_LABEL));
		let d2 = describe_btw_inject_answer(&parts("", "z", ""), false);
		assert!(d2.before.is_none() && d2.after.is_none());
		assert_eq!(d2.block.tone, "user");
		assert_eq!(d2.block.label, BTW_INJECT_LABEL);
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::component::Component;
use crate::keys::Key;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Renderable inject-answer block component.
///
/// Reference: `BtwInjectAnswer` in `l/src/overlays/btw-inject.ts` (Component +
/// native describe helper).
pub struct BtwInjectAnswer {
	pub text: BtwInjectAnswerText,
}

impl std::fmt::Debug for BtwInjectAnswer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BtwInjectAnswer")
			.field("sent", &self.text.sent)
			.finish_non_exhaustive()
	}
}

impl BtwInjectAnswer {
	#[must_use]
	pub fn new(parts: &BtwInjectParts, sent: bool) -> Self {
		Self { text: BtwInjectAnswerText::new(parts, sent) }
	}

	#[must_use]
	pub fn describe(&self, _cols: u16) -> BtwInjectNativeDescription {
		// Reconstruct description from rendered text/state.
		let inject = self.text.inject.clone();
		describe_btw_inject_answer(
			&BtwInjectParts {
				before: self.text.before.clone().unwrap_or_default(),
				inject,
				after: self.text.after.clone().unwrap_or_default(),
				closed: true,
			},
			self.text.sent,
		)
	}
}

impl Component for BtwInjectAnswer {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.text.lines.clone()
	}

	fn handle_input(&mut self, _key: &Key) {}
}

impl NativeDescribe for BtwInjectAnswer {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let d = self.describe(cx.cols.max(20));
		let mut props = Props::new();
		props.insert("role", serde_json::Value::String(d.block.role.to_string()));
		props.insert("tone", serde_json::Value::String(d.block.tone.to_string()));
		props.insert("sent", serde_json::Value::Bool(d.block.sent));
		props.insert("label", serde_json::Value::String(d.block.label));
		props.insert(
			"lines",
			serde_json::Value::Array(d.block.lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;

	#[test]
	fn btw_inject_answer_component_renders_and_describes() {
		let parts = BtwInjectParts { before: "hi".into(), inject: "do".into(), after: "".into(), closed: true };
		let mut c = BtwInjectAnswer::new(&parts, false);
		let lines = c.render(40);
		assert!(lines.iter().any(|l| l.contains("do")));
		let node = c.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
