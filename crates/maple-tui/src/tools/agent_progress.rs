//! Eval `agent()` progress rendering.
//!
//! Reference: `l/src/tools/eval.ts` (`renderAgentProgressEvents`) and
//! `l/src/tools/agent-tree.ts`.
//!
//! Deviations forced by Rust:
//! - The crate has no eval-cell renderer, so this module renders only the
//!   below-box progress tree; the box itself stays with the eval card.
//! - Status text is plain (theme-painted) rows rather than the full TSP tree.

use crate::render::render_utils::{paint, replace_tabs};
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::theme::{Theme, ThemeColor};
use crate::tools::agent_tree::{
	AgentPresentation, AgentStats, AgentStatus, AgentTreeRowOptions, ConfiguredThinkingLevel,
	render_agent_tree_row_themed,
};

/// One coalesced `agent()` status event.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentProgressEvent {
	/// Subagent identifier.
	pub id: Option<String>,
	/// Lifecycle status.
	pub status: AgentStatus,
	/// Tool currently running, if any.
	pub current_tool: Option<String>,
	/// Arguments of the current tool call.
	pub current_tool_args: Option<String>,
	/// Last recorded intent line.
	pub last_intent: Option<String>,
	/// Task preview used while running with no tool/intent yet.
	pub task_preview: Option<String>,
	/// Number of tool calls made.
	pub tool_count: Option<u64>,
	/// Context tokens used.
	pub context_tokens: Option<u64>,
	/// Context window size.
	pub context_window: Option<u64>,
	/// Cost in USD.
	pub cost: Option<f64>,
	/// Wall-clock duration in milliseconds.
	pub duration_ms: Option<u64>,
	/// Resolved model identity (preferred).
	pub resolved_model_identity: Option<String>,
	/// Legacy resolved model field.
	pub resolved_model: Option<String>,
	/// Legacy model field.
	pub model: Option<String>,
	/// Resolved thinking level.
	pub resolved_thinking_level: Option<crate::thinking::ConfiguredThinkingLevel>,
	/// Advisor flag.
	pub advisor: bool,
}

/// Render coalesced `agent()` progress as a Task-tool-style tree.
///
/// One entry per subagent: a status line plus, while running, the current
/// tool/intent line. Drawn below the eval cell box.
pub fn render_agent_progress_events(
	events: &[AgentProgressEvent],
	theme: &Theme,
	width: usize,
	spinner_frame: usize,
) -> Vec<String> {
	let mut lines = Vec::new();
	for (index, event) in events.iter().enumerate() {
		let is_last = index + 1 == events.len();
		let prefix_glyph = if is_last { &theme.tree().last } else { &theme.tree().branch };
		let prefix = paint(theme, ThemeColor::Dim, prefix_glyph);
		let cont = if is_last {
			"   ".to_string()
		} else {
			format!("{}  ", paint(theme, ThemeColor::Dim, &theme.tree().vertical))
		};

		let current_tool = sanitize(event.current_tool.as_deref());
		let last_intent = sanitize(event.last_intent.as_deref());
		let preview = if event.status == AgentStatus::Running && current_tool.is_none() && last_intent.is_none() {
			sanitize(event.task_preview.as_deref())
		} else {
			None
		};
		let model = event
			.resolved_model_identity
			.clone()
			.or_else(|| event.model.clone())
			.or_else(|| event.resolved_model.clone());

		let row = render_agent_tree_row_themed(
			&AgentTreeRowOptions {
				status: event.status,
				presentation: AgentPresentation::Eval,
				prefix,
				id: sanitize(event.id.as_deref()).unwrap_or_else(|| "agent".to_string()),
				width: Some(width),
				model,
				thinking_level: event
					.resolved_thinking_level
					.map(|level| ConfiguredThinkingLevel(level.as_str().to_string())),
				advisor: event.advisor,
				spinner_frame: Some(spinner_frame),
				frozen: false,
				role_badge: None,
				status_badge: None,
				description: None,
				preview: preview.map(|preview| {
					format!(
						" {}",
						paint(theme, ThemeColor::Muted, &truncate_to_width(&replace_tabs(&preview), 48, Ellipsis::Omit))
					)
				}),
				stats: Some(AgentStats {
					tool_count: event.tool_count.filter(|value| *value > 0),
					requests: None,
					context_tokens: event.context_tokens,
					context_window: event.context_window,
					cost: event.cost.filter(|value| *value > 0.0),
				}),
				completion_percent: None,
				duration_ms: matches!(
					event.status,
					AgentStatus::Completed | AgentStatus::Failed | AgentStatus::Aborted
				)
				.then_some(event.duration_ms.unwrap_or(0))
				.filter(|value| *value > 0),
			},
			theme,
		);
		lines.push(row.line);

		if event.status == AgentStatus::Running {
			if let Some(tool) = current_tool {
				let mut tool_line = format!("{cont}{} {}", theme.tree().hook, paint(theme, ThemeColor::Muted, &tool));
				let detail = last_intent.or_else(|| sanitize(event.current_tool_args.as_deref()));
				if let Some(detail) = detail {
					tool_line.push_str(&format!(
						": {}",
						paint(theme, ThemeColor::Dim, &truncate_to_width(&replace_tabs(&detail), 48, Ellipsis::Omit))
					));
				}
				lines.push(truncate_to_width(&tool_line, width, Ellipsis::Omit));
			} else if let Some(intent) = last_intent {
				let tool_line = format!(
					"{cont}{} {}",
					theme.tree().hook,
					paint(theme, ThemeColor::Dim, &truncate_to_width(&replace_tabs(&intent), 48, Ellipsis::Omit))
				);
				lines.push(truncate_to_width(&tool_line, width, Ellipsis::Omit));
			}
		}
	}
	lines
}

/// Drop control characters and collapse whitespace for display.
fn sanitize(text: Option<&str>) -> Option<String> {
	let text = text?;
	let cleaned: String = text.chars().filter(|character| !character.is_control()).collect();
	let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
	if collapsed.is_empty() {
		None
	} else {
		Some(collapsed)
	}
}