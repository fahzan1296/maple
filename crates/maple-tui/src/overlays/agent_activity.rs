//! Agent activity indexing helpers for the agent hub.
//!
//! Reference: `l/src/overlays/agent-activity.ts`.
//!
//! Deviations:
//! - `AgentProgress` is modeled as a local struct capturing the fields read by
//!   `activity_rows_from_progress` (the TS import comes from task tooling not
//!   present in this crate). Hosts can convert from runtime progress snapshots.
//! - Timestamps are `i64` milliseconds; `recent_output` is joined as in TS.

use std::collections::BTreeSet;

/// Kinds of agent activity rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentActivityKind {
	Response,
	Tool,
	Irc,
	Lifecycle,
}

/// Status of an activity row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentActivityStatus {
	Pending,
	Success,
	Error,
	Aborted,
}

/// One row in the agent activity feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentActivityRow {
	pub id: String,
	pub agent_id: String,
	pub timestamp: i64,
	pub kind: AgentActivityKind,
	pub title: String,
	pub summary: String,
	pub status: Option<AgentActivityStatus>,
	pub entry_id: Option<String>,
	pub tool_call_id: Option<String>,
	pub tool_name: Option<String>,
	pub from: Option<String>,
	pub to: Option<String>,
	pub reply_to: Option<String>,
	pub source: String,
}

/// Query for activity rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentActivityQuery {
	pub agent_ids: Option<BTreeSet<String>>,
	pub kinds: Option<BTreeSet<AgentActivityKind>>,
	pub search: Option<String>,
	pub before_timestamp: Option<i64>,
	pub before_id: Option<String>,
	pub limit: Option<usize>,
}

/// Recent tool snapshot inside live progress.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentProgressRecentTool {
	pub tool: String,
	pub args: String,
	pub end_ms: i64,
}

/// Minimal agent progress snapshot used to derive live activity rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentProgress {
	pub id: String,
	pub status: Option<String>,
	pub task: Option<String>,
	pub description: Option<String>,
	pub current_tool: Option<String>,
	pub current_tool_args: Option<String>,
	pub current_tool_start_ms: Option<i64>,
	pub last_intent: Option<String>,
	pub recent_tools: Vec<AgentProgressRecentTool>,
	pub recent_output: Vec<String>,
}

/// Activity index surface rendered by the agent hub.
pub trait AgentActivitySource {
	fn set_live(&self, agent_id: &str, rows: Vec<AgentActivityRow>);
	fn sync(&self, agent_id: &str, session_file: Option<&str>) -> Result<(), String>;
	fn query(&self, query: &AgentActivityQuery) -> Vec<AgentActivityRow>;
	fn recent(&self, agent_id: &str, limit: usize) -> Vec<AgentActivityRow>;
}

/// Collapse an activity label to one readable terminal line.
#[must_use]
pub fn activity_one_line(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	let mut prev_space = false;
	for ch in value.chars() {
		let mapped = match ch {
			'\r' | '\n' | '\t' => ' ',
			_ => ch,
		};
		if mapped.is_whitespace() {
			if !prev_space {
				out.push(' ');
				prev_space = true;
			}
		} else {
			out.push(mapped);
			prev_space = false;
		}
	}
	out.trim().to_string()
}

/// Stable timestamp ordering for merged activity rows.
#[must_use]
pub fn compare_activity_rows(a: &AgentActivityRow, b: &AgentActivityRow) -> std::cmp::Ordering {
	a.timestamp
		.cmp(&b.timestamp)
		.then_with(|| a.id.cmp(&b.id))
}

fn lifecycle_status(progress_status: Option<&str>) -> AgentActivityStatus {
	match progress_status {
		Some("completed") => AgentActivityStatus::Success,
		Some("failed") => AgentActivityStatus::Error,
		Some("aborted") => AgentActivityStatus::Aborted,
		_ => AgentActivityStatus::Pending,
	}
}

fn response_status(progress_status: Option<&str>) -> AgentActivityStatus {
	match progress_status {
		Some("failed") => AgentActivityStatus::Error,
		Some("aborted") => AgentActivityStatus::Aborted,
		_ => AgentActivityStatus::Pending,
	}
}

/// Derive live activity rows from a progress snapshot.
#[must_use]
pub fn activity_rows_from_progress(progress: &AgentProgress, last_update: i64) -> Vec<AgentActivityRow> {
	let mut rows: Vec<AgentActivityRow> = Vec::new();
	for (idx_rev, tool) in progress.recent_tools.iter().rev().enumerate() {
		let index = progress.recent_tools.len().saturating_sub(1).saturating_sub(idx_rev);
		rows.push(AgentActivityRow {
			id: format!("live:{}:tool:{}:{index}", progress.id, tool.end_ms),
			agent_id: progress.id.clone(),
			timestamp: tool.end_ms,
			kind: AgentActivityKind::Tool,
			title: tool.tool.clone(),
			summary: if tool.args.is_empty() {
				tool.tool.clone()
			} else {
				tool.args.clone()
			},
			status: Some(AgentActivityStatus::Success),
			entry_id: None,
			tool_call_id: None,
			tool_name: Some(tool.tool.clone()),
			from: None,
			to: None,
			reply_to: None,
			source: "live".to_string(),
		});
	}
	if let Some(current_tool) = &progress.current_tool {
		let summary = progress
			.last_intent
			.clone()
			.or_else(|| progress.current_tool_args.clone())
			.unwrap_or_else(|| current_tool.clone());
		rows.push(AgentActivityRow {
			id: format!("live:{}:current-tool", progress.id),
			agent_id: progress.id.clone(),
			timestamp: progress.current_tool_start_ms.unwrap_or(last_update),
			kind: AgentActivityKind::Tool,
			title: current_tool.clone(),
			summary,
			status: Some(AgentActivityStatus::Pending),
			entry_id: None,
			tool_call_id: None,
			tool_name: Some(current_tool.clone()),
			from: None,
			to: None,
			reply_to: None,
			source: "live".to_string(),
		});
	}
	let lifecycle_title = progress
		.status
		.clone()
		.unwrap_or_else(|| "running".to_string());
	let lifecycle_summary = progress
		.task
		.clone()
		.or_else(|| progress.description.clone())
		.unwrap_or_else(|| "Agent activity".to_string());
	rows.push(AgentActivityRow {
		id: format!("live:{}:lifecycle", progress.id),
		agent_id: progress.id.clone(),
		timestamp: last_update,
		kind: AgentActivityKind::Lifecycle,
		title: lifecycle_title,
		summary: lifecycle_summary,
		status: Some(lifecycle_status(progress.status.as_deref())),
		entry_id: None,
		tool_call_id: None,
		tool_name: None,
		from: None,
		to: None,
		reply_to: None,
		source: "live".to_string(),
	});
	let response = activity_one_line(&progress.recent_output.join(" "));
	if !response.is_empty() {
		rows.push(AgentActivityRow {
			id: format!("live:{}:response", progress.id),
			agent_id: progress.id.clone(),
			timestamp: last_update,
			kind: AgentActivityKind::Response,
			title: "Response".to_string(),
			summary: response,
			status: Some(response_status(progress.status.as_deref())),
			entry_id: None,
			tool_call_id: None,
			tool_name: None,
			from: None,
			to: None,
			reply_to: None,
			source: "live".to_string(),
		});
	}
	rows.sort_by(compare_activity_rows);
	rows
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn activity_one_line_collapses_whitespace() {
		assert_eq!(activity_one_line("  a\n\tb   c\r\n"), "a b c");
		assert_eq!(activity_one_line("   "), "");
	}

	#[test]
	fn compare_activity_rows_orders_by_time_then_id() {
		let a = AgentActivityRow {
			id: "a".into(),
			agent_id: "x".into(),
			timestamp: 10,
			kind: AgentActivityKind::Tool,
			title: "t".into(),
			summary: "s".into(),
			status: None,
			entry_id: None,
			tool_call_id: None,
			tool_name: None,
			from: None,
			to: None,
			reply_to: None,
			source: "live".into(),
		};
		let mut b = a.clone();
		b.id = "b".into();
		assert_eq!(compare_activity_rows(&a, &b), std::cmp::Ordering::Less);
		b.timestamp = 9;
		assert_eq!(compare_activity_rows(&a, &b), std::cmp::Ordering::Greater);
	}

	#[test]
	fn activity_rows_from_progress_builds_lifecycle_and_response() {
		let progress = AgentProgress {
			id: "agent1".into(),
			status: Some("completed".into()),
			task: Some("do thing".into()),
			recent_output: vec!["hello".into(), "world".into()],
			..Default::default()
		};
		let rows = activity_rows_from_progress(&progress, 123);
		assert!(rows.iter().any(|r| r.kind == AgentActivityKind::Lifecycle && r.status == Some(AgentActivityStatus::Success)));
		assert!(rows.iter().any(|r| r.kind == AgentActivityKind::Response && r.summary == "hello world"));
		assert!(rows.iter().all(|r| r.agent_id == "agent1"));
	}

	#[test]
	fn activity_rows_from_progress_includes_current_and_recent_tools() {
		let progress = AgentProgress {
			id: "a".into(),
			status: Some("running".into()),
			current_tool: Some("bash".into()),
			current_tool_args: Some("ls".into()),
			current_tool_start_ms: Some(50),
			recent_tools: vec![AgentProgressRecentTool {
				tool: "read".into(),
				args: "file".into(),
				end_ms: 40,
			}],
			..Default::default()
		};
		let rows = activity_rows_from_progress(&progress, 60);
		let tool_rows: Vec<_> = rows.iter().filter(|r| r.kind == AgentActivityKind::Tool).collect();
		assert_eq!(tool_rows.len(), 2);
		assert!(tool_rows.iter().any(|r| r.id.ends_with(":current-tool") && r.status == Some(AgentActivityStatus::Pending)));
		assert!(tool_rows.iter().any(|r| r.tool_name.as_deref() == Some("read") && r.status == Some(AgentActivityStatus::Success)));
	}
}
