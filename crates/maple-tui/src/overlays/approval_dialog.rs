//! Tool-approval prompt data and pure helpers.
//!
//! Reference: `l/src/overlays/approval-dialog.ts`.
//!
//! Deviations:
//! - `ApprovalDialogComponent` (TUI component: `ScrollView`, `OverlayPanel`,
//!   `PanelRows`, countdown/timer UI, key/mouse routing, draft composer, theme
//!   rendering) is not ported. This module ports exported constants, answer/
//!   grant/draft/options/callback types, and pure prompt/detail/segment styling
//!   helpers as data transforms.
//! - ANSI/theme styling (`theme.fg/bold`) and native spans are returned as
//!   structured [`StyledSegment`] data instead of ANSI strings; host renders.
//! - `PREVIEW_LIMITS.COLLAPSED_ITEMS` is inlined as `MAX_DETAIL_ROWS` default 3
//!   (common collapsed preview limit) because render constants module may differ.

/// Detail rows shown before the rest scrolls.
pub const MAX_DETAIL_ROWS: usize = 3;
/// Enter/approve ignored this long after mount to avoid accidental answers.
pub const APPROVE_ARM_MS: u64 = 500;
/// Deny row label while a composer draft holds Enter.
pub const DENY_AND_SEND_LABEL: &str = "Deny and send message";

/// `grant<N>` takes Nth grant; `remember` toggles lifetime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApprovalAnswer {
	Once,
	Session,
	Deny,
	Remember,
	Grant(usize),
}

impl ApprovalAnswer {
	#[must_use]
	pub fn as_wire_str(&self) -> String {
		match self {
			Self::Once => "once".to_string(),
			Self::Session => "session".to_string(),
			Self::Deny => "deny".to_string(),
			Self::Remember => "remember".to_string(),
			Self::Grant(n) => format!("grant{n}"),
		}
	}
}

/// A choice row in the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalChoice {
	pub label: String,
	pub answer: ApprovalAnswer,
}


/// A grant choice that can be remembered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalDialogGrant {
	/// Label while remembered for this session.
	pub label: String,
	/// Label once remember toggle keeps it across sessions.
	pub always_label: String,
}

/// Composer draft mounted beneath the dialog.
pub trait ApprovalDialogDraft {
	/// True while composer holds a draft (text/attachments/landing paste).
	fn has_draft(&self) -> bool;
	fn handle_input(&self, key_data: &str);
}

/// Session-approval option.
#[derive(Clone, Debug)]
pub struct SessionApproval {
	pub label: String,
}

/// Dialog options (data/callbacks; UI handles omitted).
pub struct ApprovalDialogOptions<'a> {
	pub full_prompt: Option<String>,
	pub timeout_ms: Option<u64>,
	pub on_timeout: Option<Box<dyn Fn() + 'a>>,
	pub on_timeout_start: Option<Box<dyn Fn() + 'a>>,
	pub on_timeout_reset: Option<Box<dyn Fn() + 'a>>,
	pub session: Option<SessionApproval>,
	pub grants: Vec<ApprovalDialogGrant>,
	pub on_grant: Option<Box<dyn Fn(&ApprovalDialogGrant, bool) + 'a>>,
}

impl Default for ApprovalDialogOptions<'_> {
	fn default() -> Self {
		Self {
			full_prompt: None,
			timeout_ms: None,
			on_timeout: None,
			on_timeout_start: None,
			on_timeout_reset: None,
			session: None,
			grants: Vec::new(),
			on_grant: None,
		}
	}
}

/// A prompt's detail rows: every line after title, trailing blanks dropped.
#[must_use]
pub fn prompt_details(lines: &[String]) -> Vec<String> {
	if lines.is_empty() {
		return Vec::new();
	}
	let mut details = lines[1..].to_vec();
	while details.last().is_some_and(|l| l.trim().is_empty()) {
		details.pop();
	}
	details
}

/// Segment row classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
	Allowed,
	Prompted,
	Other,
}

/// Styled segment for rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSegment {
	pub text: String,
	pub tone: Option<&'static str>,
	pub bold: bool,
}

/// Styled detail line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledDetailLine {
	pub segments: Vec<StyledSegment>,
}

/// Index range `(start, end)` of compound bash segment rows after bare `Command:` label.
#[must_use]
pub fn segment_row_range(details: &[String]) -> (usize, usize) {
	let Some(label) = details.iter().position(|l| l.starts_with("Command:")) else {
		return (0, 0);
	};
	if details.get(label).map(String::as_str) != Some("Command:") {
		return (0, 0);
	}
	let mut end = label + 1;
	while end < details.len() && details[end].starts_with("  ") {
		end += 1;
	}
	(label + 1, end)
}

fn parse_allowed_segment(line: &str) -> Option<(String, String, String, String)> {
	// ^(\s*)✓(\s+)(.+)(\s+\((?:allowed: .*|\d+ allowed segments?)\))$
	let (indent, rest) = split_leading_ws(line);
	let rest = rest.strip_prefix('✓')?;
	let (space, rest) = split_leading_ws(rest);
	if space.is_empty() {
		return None;
	}
	// suffix is trailing " (allowed: ...)" or " (N allowed segments?)"
	let suffix_start = find_allowed_suffix(rest)?;
	let command = rest[..suffix_start].to_string();
	let suffix = rest[suffix_start..].to_string();
	Some((indent.to_string(), space.to_string(), command, suffix))
}

fn parse_prompted_segment(line: &str) -> Option<(String, String, String, String)> {
	let (indent, rest) = split_leading_ws(line);
	let rest = rest.strip_prefix('?')?;
	let (space, rest) = split_leading_ws(rest);
	if space.is_empty() {
		return None;
	}
	let suffix_start = find_prompted_suffix(rest)?;
	let command = rest[..suffix_start].to_string();
	let suffix = rest[suffix_start..].to_string();
	Some((indent.to_string(), space.to_string(), command, suffix))
}

fn split_leading_ws(s: &str) -> (&str, &str) {
	let idx = s.find(|c: char| !c.is_whitespace()).unwrap_or(s.len());
	(&s[..idx], &s[idx..])
}

fn find_allowed_suffix(s: &str) -> Option<usize> {
	// look for " (allowed: ...)" or " (<digits> allowed segment[s]?)" at end
	for (i, _) in s.char_indices() {
		if !s[i..].starts_with(" (") || !s.ends_with(')') {
			continue;
		}
		let inner = &s[i + 2..s.len() - 1];
		if inner.starts_with("allowed: ") {
			return Some(i);
		}
		if inner.ends_with(" allowed segment") || inner.ends_with(" allowed segments") {
			let num = inner.strip_suffix(" allowed segments").or_else(|| inner.strip_suffix(" allowed segment"))?;
			if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
				return Some(i);
			}
		}
	}
	None
}

fn find_prompted_suffix(s: &str) -> Option<usize> {
	for (i, _) in s.char_indices() {
		if !s[i..].starts_with(" (") || !s.ends_with(')') {
			continue;
		}
		let inner = &s[i + 2..s.len() - 1];
		if inner == "requires approval" || inner.starts_with("prompt: ") || inner == "critical pattern" {
			return Some(i);
		}
	}
	None
}

/// Style one segment line.
#[must_use]
pub fn style_segment_line(line: &str) -> StyledDetailLine {
	if let Some((indent, space, command, suffix)) = parse_allowed_segment(line) {
		let mut segments = Vec::new();
		if !indent.is_empty() {
			segments.push(StyledSegment { text: indent, tone: None, bold: false });
		}
		segments.push(StyledSegment { text: "✓".into(), tone: Some("success"), bold: false });
		segments.push(StyledSegment { text: space, tone: None, bold: false });
		segments.push(StyledSegment { text: format!("{command}{suffix}"), tone: Some("dim"), bold: false });
		return StyledDetailLine { segments };
	}
	if let Some((indent, space, command, suffix)) = parse_prompted_segment(line) {
		let mut segments = Vec::new();
		if !indent.is_empty() {
			segments.push(StyledSegment { text: indent, tone: None, bold: false });
		}
		segments.push(StyledSegment { text: "?".into(), tone: Some("warning"), bold: false });
		segments.push(StyledSegment { text: space, tone: None, bold: false });
		segments.push(StyledSegment { text: command, tone: Some("accent"), bold: true });
		segments.push(StyledSegment { text: suffix, tone: Some("warning"), bold: false });
		return StyledDetailLine { segments };
	}
	StyledDetailLine {
		segments: vec![StyledSegment { text: line.to_string(), tone: Some("accent"), bold: false }],
	}
}

/// Style detail rows (segment rows by marker, rest accent).
#[must_use]
pub fn style_approval_details(details: &[String]) -> Vec<StyledDetailLine> {
	let (start, end) = segment_row_range(details);
	details
		.iter()
		.enumerate()
		.map(|(i, line)| {
			if i >= start && i < end {
				style_segment_line(line)
			} else {
				StyledDetailLine {
					segments: vec![StyledSegment { text: line.clone(), tone: Some("accent"), bold: false }],
				}
			}
		})
		.collect()
}

/// Approval dialog logical state (non-render fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalDialogState {
	pub title: String,
	pub summary: Vec<String>,
	pub full: Option<Vec<String>>,
	pub expanded: bool,
	pub remember: bool,
	pub selected_index: usize,
	pub armed: bool,
}

impl ApprovalDialogState {
	#[must_use]
	pub fn new(title: String, prompt_lines: Vec<String>, full_prompt: Option<String>) -> Self {
		Self {
			title,
			summary: prompt_details(&prompt_lines),
			full: full_prompt.map(|f| prompt_details(&f.split('\n').map(String::from).collect::<Vec<_>>())),
			expanded: false,
			remember: false,
			selected_index: 0,
			armed: false,
		}
	}

	#[must_use]
	pub fn visible_details(&self) -> &[String] {
		if self.expanded {
			self.full.as_deref().unwrap_or(&self.summary)
		} else {
			&self.summary
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn prompt_details_drops_trailing_blanks() {
		let lines = vec!["Title".into(), "a".into(), "b".into(), "".into(), "  ".into()];
		assert_eq!(prompt_details(&lines), vec!["a".to_string(), "b".to_string()]);
	}

	#[test]
	fn segment_range_and_styling() {
		let details = vec![
			"Command:".into(),
			"  ✓ ls (allowed: policy)".into(),
			"  ? rm (requires approval)".into(),
			"note".into(),
		];
		assert_eq!(segment_row_range(&details), (1, 3));
		let styled = style_approval_details(&details);
		assert_eq!(styled.len(), 4);
		assert_eq!(styled[0].segments[0].tone, Some("accent")); // Command:
		assert_eq!(styled[1].segments[1].text, "✓");
		assert_eq!(styled[1].segments[1].tone, Some("success"));
		assert_eq!(styled[2].segments[1].text, "?");
		assert_eq!(styled[2].segments[3].bold, true);
		assert_eq!(styled[3].segments[0].tone, Some("accent"));
	}

	#[test]
	fn approval_answer_wire_and_state() {
		assert_eq!(ApprovalAnswer::Grant(2).as_wire_str(), "grant2");
		let st = ApprovalDialogState::new("t".into(), vec!["t".into(), "d".into()], None);
		assert_eq!(st.visible_details(), &[String::from("d")]);
		assert!(!st.expanded);
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Callbacks for approval dialog decisions/lifecycle.
pub trait ApprovalDialogCallbacks {
	fn on_decision(&self, answer: ApprovalAnswer);
	fn request_render(&self) {}
}

/// Permission/approval dialog component.
///
/// Reference: `ApprovalDialogComponent` in `l/src/overlays/approval-dialog.ts`.
///
/// Deviations: timer/countdown animation, host keybinding/remember hints, and
/// themed segment rendering are simplified to state-driven bordered text/native
/// rows. Approve/deny/remember/expand selection and decision emission are ported.
pub struct ApprovalDialogComponent<CB: ApprovalDialogCallbacks> {
	pub state: ApprovalDialogState,
	pub can_remember: bool,
	pub done: bool,
	callbacks: CB,
	theme: OverlayTheme,
}

impl<CB: ApprovalDialogCallbacks> std::fmt::Debug for ApprovalDialogComponent<CB> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ApprovalDialogComponent")
			.field("title", &self.state.title)
			.field("expanded", &self.state.expanded)
			.field("done", &self.done)
			.finish_non_exhaustive()
	}
}

impl<CB: ApprovalDialogCallbacks> ApprovalDialogComponent<CB> {
	#[must_use]
	pub fn new(
		title: String,
		prompt_lines: Vec<String>,
		full_prompt: Option<String>,
		can_remember: bool,
		callbacks: CB,
	) -> Self {
		Self {
			state: ApprovalDialogState::new(title, prompt_lines, full_prompt),
			can_remember,
			done: false,
			callbacks,
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	fn option_count(&self) -> usize {
		if self.can_remember { 3 } else { 2 }
	}

	fn emit(&mut self, answer: ApprovalAnswer) {
		if self.done {
			return;
		}
		self.done = true;
		self.callbacks.on_decision(answer);
	}

	fn approve(&mut self) {
		if self.can_remember && self.state.remember {
			self.emit(ApprovalAnswer::Grant(2));
		} else {
			self.emit(ApprovalAnswer::Grant(1));
		}
	}

	fn deny(&mut self) {
		self.emit(ApprovalAnswer::Deny);
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let mut out = vec![top_border(width, &self.state.title, &self.theme)];
		for l in self.state.visible_details() {
			out.push(row(l, width, &self.theme));
		}
		if self.state.full.is_some() {
			out.push(divider(width, &self.theme));
			out.push(row(
				if self.state.expanded { "e: collapse" } else { "e: expand" },
				width,
				&self.theme,
			));
		}
		out.push(divider(width, &self.theme));
		let deny_sel = if self.state.selected_index == 0 { "›" } else { " " };
		let allow_sel = if self.state.selected_index == 1 { "›" } else { " " };
		out.push(row(&format!("{deny_sel} Deny"), width, &self.theme));
		out.push(row(&format!("{allow_sel} Allow once"), width, &self.theme));
		if self.can_remember {
			let rem_sel = if self.state.selected_index == 2 { "›" } else { " " };
			let mark = if self.state.remember { "[x]" } else { "[ ]" };
			out.push(row(&format!("{rem_sel} {mark} Always allow"), width, &self.theme));
		}
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<CB: ApprovalDialogCallbacks> Component for ApprovalDialogComponent<CB> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if self.done {
			return;
		}
		match key {
			Key::Up => {
				self.state.selected_index = self.state.selected_index.saturating_sub(1);
			}
			Key::Down => {
				let max = self.option_count().saturating_sub(1);
				self.state.selected_index = (self.state.selected_index + 1).min(max);
			}
			Key::Char('e') if self.state.full.is_some() => {
				self.state.expanded = !self.state.expanded;
			}
			Key::Char(' ') if self.can_remember && self.state.selected_index == 2 => {
				self.state.remember = !self.state.remember;
			}
			Key::Char('y') | Key::Char('Y') => self.approve(),
			Key::Char('n') | Key::Char('N') | Key::Esc => self.deny(),
			Key::Enter => match self.state.selected_index {
				0 => self.deny(),
				1 => self.approve(),
				2 if self.can_remember => {
					self.state.remember = true;
					self.approve();
				}
				_ => {}
			},
			_ => {}
		}
		self.callbacks.request_render();
	}
}

impl<CB: ApprovalDialogCallbacks> NativeDescribe for ApprovalDialogComponent<CB> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("expanded", serde_json::Value::Bool(self.state.expanded));
		props.insert("done", serde_json::Value::Bool(self.done));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;
	use std::cell::RefCell;

	struct Cb {
		answer: RefCell<Option<ApprovalAnswer>>,
	}
	impl ApprovalDialogCallbacks for Cb {
		fn on_decision(&self, answer: ApprovalAnswer) {
			*self.answer.borrow_mut() = Some(answer);
		}
		fn request_render(&self) {}
	}

	#[test]
	fn approval_dialog_component_renders_and_approves() {
		let cb = Cb { answer: RefCell::new(None) };
		let mut d = ApprovalDialogComponent::new(
			"Approve".into(),
			vec!["Title".into(), "cmd".into()],
			Some("Title\nfull\ncmd".into()),
			true,
			cb,
		);
		let lines = d.render(60);
		assert!(lines.iter().any(|l| l.contains("Approve")));
		d.handle_input(&Key::Char('e'));
		assert!(d.state.expanded);
		d.handle_input(&Key::Down); // allow once
		d.handle_input(&Key::Enter);
		assert!(d.done);
	}

	#[test]
	fn approval_dialog_component_describe_and_deny() {
		let cb = Cb { answer: RefCell::new(None) };
		let mut d = ApprovalDialogComponent::new("A".into(), vec!["A".into()], None, false, cb);
		let node = d.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
		d.handle_input(&Key::Esc);
		assert_eq!(*d.callbacks.answer.borrow(), Some(ApprovalAnswer::Deny));
	}
}
