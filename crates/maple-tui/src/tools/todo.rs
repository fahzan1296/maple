//! Todo tool render-model types, state helpers, and render/describe logic.
//!
//! Reference: `l/src/tools/todo.ts`.
//!
//! Deviations:
//! - TS renders live TUI components/native views with `Theme`, `chalk`,
//!   `Text`/`Container`, `framedToolCard`, `renderStatusLine`,
//!   `renderTreeList`, `OwnerMemo`, `NativeNode` builders and a provider for
//!   active subagent descriptions. Rust ports the full data/selection/matching/
//!   formatting/checklist logic and provides plain/native describe helper
//!   structures; host wiring (theme objects, memoization, live description
//!   provider storage, component invalidation, ANSI/chalk styling) is expected
//!   by the renderer-completion pass.
//! - `sanitizeText`/ANSI stripping uses local tab/control/CRLF sanitization;
//!   full CSI/OSC stripping is a host concern where not available.
//! - Native node construction is represented via `crate::native::describe`
//!   builders for describe helpers; strike animation/theme rendering remains
//!   host concern.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::native::describe::{list as nlist, node as nnode};
use crate::native::node::{NodeKind, NodeText, Props};
use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolHead, NativeToolView, ToolHeadDetail};

/// Lifecycle state of a todo item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
	Pending,
	InProgress,
	Completed,
	Abandoned,
}

impl Default for TodoStatus {
	fn default() -> Self {
		Self::Pending
	}
}

impl TodoStatus {
	/// String form.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pending => "pending",
			Self::InProgress => "in_progress",
			Self::Completed => "completed",
			Self::Abandoned => "abandoned",
		}
	}
}

/// Operation names accepted by the todo tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoOperation {
	Init,
	Start,
	Done,
	Rm,
	Drop,
	Append,
	View,
}

impl TodoOperation {
	/// String form.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Init => "init",
			Self::Start => "start",
			Self::Done => "done",
			Self::Rm => "rm",
			Self::Drop => "drop",
			Self::Append => "append",
			Self::View => "view",
		}
	}
}

/// A task displayed within a todo phase.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
	pub content: String,
	#[serde(default)]
	pub status: TodoStatus,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub details: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub notes: Option<Vec<String>>,
}

/// A named group of todo tasks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoPhase {
	pub name: String,
	#[serde(default)]
	pub tasks: Vec<TodoItem>,
}

/// A task that became complete in the latest update.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoCompletionTransition {
	pub phase: String,
	pub content: String,
}

/// Todo snapshot and transitions displayed after an operation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoToolDetails {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub op: Option<TodoOperation>,
	#[serde(default)]
	pub phases: Vec<TodoPhase>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub storage: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub completed_tasks: Option<Vec<TodoCompletionTransition>>,
}

/// Minimum overlap (after normalization) required for a substring match.
pub const TODO_DESCRIPTION_MIN_OVERLAP: usize = 6;

fn normalize_for_todo_match(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	let mut prev_space = false;
	for ch in value.chars().flat_map(|c| c.to_lowercase()) {
		if ch.is_alphanumeric() {
			out.push(ch);
			prev_space = false;
		} else if !prev_space {
			out.push(' ');
			prev_space = true;
		}
	}
	out.trim().to_string()
}

fn matches_normalized_candidates(target: &str, candidates: &[String]) -> bool {
	if target.is_empty() {
		return false;
	}
	for candidate in candidates {
		if target == candidate {
			return true;
		}
		if target.chars().count() >= TODO_DESCRIPTION_MIN_OVERLAP && candidate.contains(target) {
			return true;
		}
		if candidate.chars().count() >= TODO_DESCRIPTION_MIN_OVERLAP && target.contains(candidate) {
			return true;
		}
	}
	false
}

/// Report whether `content` likely names the same work as any description.
pub fn todo_matches_any_description(content: &str, descriptions: &[String]) -> bool {
	let target = normalize_for_todo_match(content);
	if target.is_empty() {
		return false;
	}
	let candidates: Vec<String> = descriptions
		.iter()
		.map(|d| normalize_for_todo_match(d))
		.filter(|c| !c.is_empty())
		.collect();
	matches_normalized_candidates(&target, &candidates)
}

/// Whether a todo is settled: completed or deliberately abandoned.
pub fn is_closed_todo(task: &TodoItem) -> bool {
	matches!(task.status, TodoStatus::Completed | TodoStatus::Abandoned)
}

fn normalize_task_part(part: &str) -> String {
	let trimmed = part.trim();
	if trimmed.len() >= 2 {
		let first = trimmed.chars().next().unwrap();
		let last = trimmed.chars().last().unwrap();
		if (first == '"' || first == '\'' || first == '`') && first == last {
			return trimmed[first.len_utf8()..trimmed.len() - last.len_utf8()]
				.trim()
				.to_string();
		}
	}
	trimmed.to_string()
}

fn split_task_list(content: &str, known: &HashSet<String>) -> Vec<String> {
	if known.contains(content) {
		return vec![content.to_string()];
	}
	let mut partial: Option<Vec<String>> = None;
	let tries: Vec<(Vec<String>, bool)> = vec![
		(
			content
				.split(|c| c == '\n' || c == '\r')
				.map(normalize_task_part)
				.filter(|p| !p.is_empty())
				.collect(),
			true,
		),
		(
			content
				.split(';')
				.map(normalize_task_part)
				.filter(|p| !p.is_empty())
				.collect(),
			true,
		),
		(
			content
				.split(',')
				.map(normalize_task_part)
				.filter(|p| !p.is_empty())
				.collect(),
			true,
		),
		(
			content
				.split_whitespace()
				.map(normalize_task_part)
				.filter(|p| !p.is_empty())
				.collect(),
			false,
		),
		(
			content
				.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
				.map(normalize_task_part)
				.filter(|p| !p.is_empty())
				.collect(),
			false,
		),
	];
	for (parts, is_partial) in tries {
		if parts.is_empty() {
			continue;
		}
		let named = parts.iter().filter(|p| known.contains(*p)).count();
		if named > 0 && named == parts.len() {
			return parts;
		}
		if partial.is_none() && is_partial && named > 0 && parts.len() > 1 {
			partial = Some(parts);
		}
	}
	partial.unwrap_or_else(|| vec![content.to_string()])
}

/// Task contents a todo `task` arg targets.
pub fn todo_task_targets(phases: &[TodoPhase], task: Option<&Value>) -> Vec<String> {
	let Some(task) = task else {
		return Vec::new();
	};
	let inputs: Vec<String> = match task {
		Value::String(s) => {
			if s.is_empty() {
				return Vec::new();
			}
			vec![s.clone()]
		}
		Value::Array(arr) => {
			if arr.is_empty() {
				return Vec::new();
			}
			arr.iter()
				.filter_map(|v| v.as_str().map(|s| s.to_string()))
				.collect()
		}
		_ => return Vec::new(),
	};
	let known: HashSet<String> = phases
		.iter()
		.flat_map(|p| p.tasks.iter().map(|t| t.content.clone()))
		.collect();
	inputs
		.into_iter()
		.flat_map(|content| split_task_list(&content, &known))
		.collect()
}

fn is_active_todo(task: &TodoItem, matched: bool) -> bool {
	task.status == TodoStatus::InProgress || (task.status == TodoStatus::Pending && matched)
}

/// Result of collapsed todo selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollapsedTodoSelection {
	pub items: Vec<TodoItem>,
	pub summary: String,
}

/// Closed rows kept directly above the open window.
pub const COLLAPSED_CLOSED_CONTEXT: usize = 1;

fn select_within_cap<F>(base: &[TodoItem], is_matched: &F, cap: usize) -> CollapsedTodoSelection
where
	F: Fn(&TodoItem) -> bool,
{
	if base.len() <= cap + 1 {
		return CollapsedTodoSelection {
			items: base.to_vec(),
			summary: String::new(),
		};
	}
	let active: Vec<TodoItem> = base
		.iter()
		.filter(|t| is_active_todo(t, is_matched(t)))
		.cloned()
		.collect();
	if active.len() > cap {
		let hidden_active = active.len() - cap;
		return CollapsedTodoSelection {
			items: active.into_iter().take(cap).collect(),
			summary: format!(
				"… {hidden_active} more active todo{}",
				if hidden_active == 1 { "" } else { "s" }
			),
		};
	}
	let first_active_idx = if active.is_empty() {
		0
	} else {
		base.iter()
			.position(|t| t.content == active[0].content && t.status == active[0].status)
			.unwrap_or(0)
	};
	let mut fill = Vec::new();
	for t in base.iter().skip(first_active_idx) {
		if active.len() + fill.len() >= cap {
			break;
		}
		if is_active_todo(t, is_matched(t)) {
			continue;
		}
		fill.push(t.clone());
	}
	let mut items = active;
	items.extend(fill);
	let hidden = base.len().saturating_sub(items.len());
	CollapsedTodoSelection {
		summary: if hidden > 0 {
			format!("… {hidden} more todo{}", if hidden == 1 { "" } else { "s" })
		} else {
			String::new()
		},
		items,
	}
}

/// Walking-viewport selection for a phase's collapsed todo preview.
pub fn select_collapsed_todos<F>(tasks: &[TodoItem], is_matched: F, cap: usize) -> CollapsedTodoSelection
where
	F: Fn(&TodoItem) -> bool,
{
	let open: Vec<TodoItem> = tasks.iter().filter(|t| !is_closed_todo(t)).cloned().collect();
	if open.is_empty() {
		return select_within_cap(tasks, &is_matched, cap);
	}
	let mut closed: Vec<TodoItem> = tasks.iter().filter(|t| is_closed_todo(t)).cloned().collect();
	if closed.len() > COLLAPSED_CLOSED_CONTEXT {
		closed = closed.split_off(closed.len() - COLLAPSED_CLOSED_CONTEXT);
	}
	let selected = select_within_cap(&open, &is_matched, cap);
	let mut items = closed;
	items.extend(selected.items);
	CollapsedTodoSelection {
		items,
		summary: selected.summary,
	}
}

/// One-based ASCII roman numeral for display.
pub fn phase_roman_numeral(one_based_index: i32) -> String {
	if one_based_index <= 0 {
		return String::new();
	}
	let pairs: &[(i32, &str)] = &[
		(1000, "M"),
		(900, "CM"),
		(500, "D"),
		(400, "CD"),
		(100, "C"),
		(90, "XC"),
		(50, "L"),
		(40, "XL"),
		(10, "X"),
		(9, "IX"),
		(5, "V"),
		(4, "IV"),
		(1, "I"),
	];
	let mut out = String::new();
	let mut rem = one_based_index;
	for (value, sym) in pairs {
		while rem >= *value {
			out.push_str(sym);
			rem -= *value;
		}
	}
	out
}

/// Replace tabs and collapse CR/LF/control chars for display.
pub fn for_display(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut last_nl = false;
	for ch in text.chars() {
		match ch {
			'\t' => {
				out.push_str("   ");
				last_nl = false;
			}
			'\r' | '\n' => {
				if !last_nl {
					out.push(' ');
				}
				last_nl = true;
			}
			c if c.is_control() => {}
			c => {
				out.push(c);
				last_nl = false;
			}
		}
	}
	out
}

/// Display-only phase header: `I. Foundation`.
pub fn format_phase_display_name(name: &str, one_based_index: i32) -> String {
	format!("{}. {}", phase_roman_numeral(one_based_index), for_display(name))
}

/// Frames held before revealing a completion strike.
pub const TODO_STRIKE_HOLD_FRAMES: u32 = 2;
/// Frames used to reveal a completion strike.
pub const TODO_STRIKE_REVEAL_FRAMES: u32 = 12;
/// Total frames in the completion strike animation.
pub const TODO_STRIKE_TOTAL_FRAMES: u32 = TODO_STRIKE_HOLD_FRAMES + TODO_STRIKE_REVEAL_FRAMES;

/// Checklist item for native terminals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
	pub id: String,
	pub text: String,
	pub status: String,
}

/// Checklist phase for native terminals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistPhase {
	pub id: String,
	pub title: String,
	pub items: Vec<ChecklistItem>,
	pub collapsed: bool,
}

fn checklist_status(status: TodoStatus, matched: bool) -> &'static str {
	match status {
		TodoStatus::Pending if matched => "active",
		TodoStatus::Pending => "pending",
		TodoStatus::InProgress => "active",
		TodoStatus::Completed => "done",
		TodoStatus::Abandoned => "dropped",
	}
}

/// Checklist phases for native terminals.
pub fn todo_checklist_phases<F>(phases: &[TodoPhase], mut is_matched: F) -> Vec<ChecklistPhase>
where
	F: FnMut(&TodoItem) -> bool,
{
	phases
		.iter()
		.enumerate()
		.map(|(p, phase)| {
			let items = phase
				.tasks
				.iter()
				.enumerate()
				.map(|(t, task)| ChecklistItem {
					id: format!("p{p}.{t}"),
					text: for_display(&task.content),
					status: checklist_status(task.status, is_matched(task)).to_string(),
				})
				.collect();
			ChecklistPhase {
				id: format!("p{p}"),
				title: for_display(&phase.name),
				collapsed: !phase.tasks.is_empty() && phase.tasks.iter().all(is_closed_todo),
				items,
			}
		})
		.collect()
}

/// Native/describe result summary for todo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoDescribeResult {
	pub title: String,
	pub target: Option<String>,
	pub closed: usize,
	pub total: usize,
	pub phases: Vec<ChecklistPhase>,
	pub is_error: bool,
	pub error_text: Option<String>,
	pub fallback_note: Option<String>,
}

/// Describe todo result (native/plain), matching TS behavior.
pub fn describe_todo_result(
	phases: &[TodoPhase],
	is_error: bool,
	result_text: &str,
	expanded: bool,
	active_descriptions: &[String],
) -> TodoDescribeResult {
	if is_error {
		return TodoDescribeResult {
			title: "Todo".to_string(),
			target: None,
			closed: 0,
			total: 0,
			phases: Vec::new(),
			is_error: true,
			error_text: Some(if result_text.trim().is_empty() {
				"Todo operation failed".to_string()
			} else {
				for_display(result_text.trim())
			}),
			fallback_note: None,
		};
	}
	let phases_filtered: Vec<TodoPhase> = phases
		.iter()
		.filter(|p| !p.tasks.is_empty())
		.cloned()
		.collect();
	let total: usize = phases_filtered.iter().map(|p| p.tasks.len()).sum();
	if total == 0 {
		return TodoDescribeResult {
			title: "Todo".to_string(),
			target: None,
			closed: 0,
			total: 0,
			phases: Vec::new(),
			is_error: false,
			error_text: None,
			fallback_note: Some(for_display(if result_text.trim().is_empty() {
				"No todos"
			} else {
				result_text.trim()
			})),
		};
	}
	let closed: usize = phases_filtered
		.iter()
		.map(|p| p.tasks.iter().filter(|t| is_closed_todo(t)).count())
		.sum();
	let checklist = if expanded || active_descriptions.is_empty() {
		todo_checklist_phases(&phases_filtered, |_| false)
	} else {
		todo_checklist_phases(&phases_filtered, |task| {
			todo_matches_any_description(&task.content, active_descriptions)
		})
	};
	TodoDescribeResult {
		title: "Todo".to_string(),
		target: Some(format!("{closed}/{total}")),
		closed,
		total,
		phases: checklist,
		is_error: false,
		error_text: None,
		fallback_note: None,
	}
}

/// Describe todo call label.
pub fn describe_todo_call_label(ops: &[String]) -> String {
	if ops.is_empty() {
		"update".to_string()
	} else {
		ops.join(", ")
	}
}

/// Build native checklist node from checklist phases (best-effort, prop-driven).
pub fn checklist_native_node(phases: &[ChecklistPhase]) -> crate::native::node::NativeNode {
	let phase_nodes = phases
		.iter()
		.map(|p| {
			let mut pp = Props::new();
			pp.set_str("id", &p.id);
			pp.set_str("title", &p.title);
			pp.set_bool("collapsed", p.collapsed);
			let item_nodes = p
				.items
				.iter()
				.map(|it| {
					let mut ip = Props::new();
					ip.set_str("id", &it.id);
					ip.set_str("text", &it.text);
					ip.set_str("status", &it.status);
					nnode(NodeKind::Item, ip, Vec::new(), None)
				})
				.collect();
			nnode(NodeKind::Section, pp, item_nodes, Some(&p.id))
		})
		.collect();
	let mut root_p = Props::new();
	root_p.set_str("role", "omp.todo.checklist");
	nnode(NodeKind::List, root_p, phase_nodes, Some("todo-checklist"))
}

/// Native describe view for todo call.
pub fn describe_todo_call_native(ops: &[String]) -> NativeToolView {
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(
			"Todo",
			&[ToolHeadDetail::Text(describe_todo_call_label(ops))],
		))),
		..NativeToolView::default()
	}
}

/// Native describe view for todo result.
pub fn describe_todo_result_native(
	phases: &[TodoPhase],
	is_error: bool,
	result_text: &str,
	expanded: bool,
	active_descriptions: &[String],
) -> NativeToolView {
	let desc = describe_todo_result(phases, is_error, result_text, expanded, active_descriptions);
	if desc.is_error {
		return NativeToolView { ..error_view("Todo", desc.error_text.as_deref().unwrap_or("Todo operation failed"), &[]) };
	}
	let target = desc.target.clone();
	let details = target.into_iter().map(ToolHeadDetail::Text).collect::<Vec<_>>();
	let head = Some(NodeText::Spans(tool_head("Todo", &details)));
	if desc.total == 0 {
		return NativeToolView {
			head,
			tone: Some(crate::native::node::Tone::Warning),
			body: Some(vec![note_text(desc.fallback_note.as_deref().unwrap_or("No todos"), "muted", None)]),
			..NativeToolView::default()
		};
	}
	NativeToolView {
		head,
		body: Some(vec![checklist_native_node(&desc.phases)]),
		..NativeToolView::default()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roman_and_phase_name() {
		assert_eq!(phase_roman_numeral(1), "I");
		assert_eq!(phase_roman_numeral(4), "IV");
		assert_eq!(format_phase_display_name("Plan", 2), "II. Plan");
	}

	#[test]
	fn match_and_closed() {
		let t = TodoItem {
			content: "Review auth".into(),
			status: TodoStatus::Completed,
			details: None,
			notes: None,
		};
		assert!(is_closed_todo(&t));
		let descs = vec!["review   auth".to_string()];
		assert!(todo_matches_any_description("Review auth", &descs));
	}

	#[test]
	fn task_targets_split() {
		let phases = vec![TodoPhase {
			name: "p".into(),
			tasks: vec![
				TodoItem { content: "a".into(), status: TodoStatus::Pending, details: None, notes: None },
				TodoItem { content: "b".into(), status: TodoStatus::Pending, details: None, notes: None },
			],
		}];
		let v = Value::String("a; b".into());
		assert_eq!(todo_task_targets(&phases, Some(&v)), vec!["a".to_string(), "b".to_string()]);
	}

	#[test]
	fn collapsed_selection_keeps_active() {
		let tasks = vec![
			TodoItem { content: "done".into(), status: TodoStatus::Completed, details: None, notes: None },
			TodoItem { content: "active".into(), status: TodoStatus::InProgress, details: None, notes: None },
			TodoItem { content: "p1".into(), status: TodoStatus::Pending, details: None, notes: None },
			TodoItem { content: "p2".into(), status: TodoStatus::Pending, details: None, notes: None },
		];
		let sel = select_collapsed_todos(&tasks, |_| false, 2);
		assert!(sel.items.iter().any(|t| t.content == "active"));
	}

	#[test]
	fn checklist_phases_collapse_done_phase() {
		let phases = vec![TodoPhase {
			name: "Done".into(),
			tasks: vec![TodoItem { content: "x".into(), status: TodoStatus::Completed, details: None, notes: None }],
		}];
		let c = todo_checklist_phases(&phases, |_| false);
		assert!(c[0].collapsed);
		assert_eq!(c[0].items[0].status, "done");
	}

	#[test]
	fn describe_todo_result_counts() {
		let phases = vec![TodoPhase {
			name: "P".into(),
			tasks: vec![
				TodoItem { content: "a".into(), status: TodoStatus::Completed, details: None, notes: None },
				TodoItem { content: "b".into(), status: TodoStatus::Pending, details: None, notes: None },
			],
		}];
		let d = describe_todo_result(&phases, false, "", true, &[]);
		assert_eq!(d.target.as_deref(), Some("1/2"));
		assert_eq!(d.closed, 1);
		assert_eq!(d.total, 2);
	}

	#[test]
	fn native_describe_builds() {
		let phases = vec![TodoPhase {
			name: "P".into(),
			tasks: vec![TodoItem { content: "a".into(), status: TodoStatus::Pending, details: None, notes: None }],
		}];
		let v = describe_todo_result_native(&phases, false, "", true, &[]);
		assert!(v.head.is_some());
		assert!(v.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
	}
}
