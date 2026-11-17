//! Goal tracking types and pure rendering helpers.
//!
//! Reference: `l/src/tools/goal.ts`.
//!
//! Deviations:
//! - TS renders TUI components/native views with `Theme`, spans, and memos.
//!   Rust ports goal data model and pure description/format helpers.
//! - Timestamps are `i64` milliseconds (TS `number` ms); seconds fields kept as
//!   `f64`-compatible integers via `u64` in struct but formatted generically.

/// Lifecycle state of a tracked goal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalStatus {
	/// Active goal.
	Active,
	/// Paused goal.
	Paused,
	/// Budget-limited goal.
	BudgetLimited,
	/// Completed goal.
	Complete,
	/// Dropped goal.
	Dropped,
}

impl GoalStatus {
	/// Status label as used in TS details/views.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Active => "active",
			Self::Paused => "paused",
			Self::BudgetLimited => "budget-limited",
			Self::Complete => "complete",
			Self::Dropped => "dropped",
		}
	}

	/// Badge/tone color token name.
	pub fn badge_color(self) -> &'static str {
		match self {
			Self::Complete => "success",
			Self::BudgetLimited => "warning",
			Self::Paused | Self::Dropped => "muted",
			Self::Active => "accent",
		}
	}
}

/// Serializable goal progress and resource budget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Goal {
	/// Goal id.
	pub id: String,
	/// Objective text.
	pub objective: String,
	/// Status.
	pub status: Option<GoalStatus>,
	/// Token budget.
	pub token_budget: Option<u64>,
	/// Tokens used.
	pub tokens_used: u64,
	/// Time used in seconds.
	pub time_used_seconds: u64,
	/// Creation timestamp (ms).
	pub created_at: i64,
	/// Update timestamp (ms).
	pub updated_at: i64,
}

/// Goal operation names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalOp {
	/// Create/set goal.
	Create,
	/// Get/check goal.
	Get,
	/// Mark complete.
	Complete,
	/// Resume.
	Resume,
	/// Drop.
	Drop,
}

impl GoalOp {
	/// Parse from TS op string.
	pub fn parse(s: &str) -> Option<Self> {
		match s {
			"create" => Some(Self::Create),
			"get" => Some(Self::Get),
			"complete" => Some(Self::Complete),
			"resume" => Some(Self::Resume),
			"drop" => Some(Self::Drop),
			_ => None,
		}
	}

	/// Display description (`set`, `check`, etc.).
	pub fn describe(self) -> &'static str {
		match self {
			Self::Create => "set",
			Self::Complete => "complete",
			Self::Get => "check",
			Self::Resume => "resume",
			Self::Drop => "drop",
		}
	}
}

/// Goal operation outcome displayed in the transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoalToolDetails {
	/// Operation.
	pub op: Option<GoalOp>,
	/// Current goal.
	pub goal: Option<Goal>,
	/// Remaining tokens.
	pub remaining_tokens: Option<i64>,
	/// Completion budget report.
	pub completion_budget_report: Option<String>,
}

/// Goal render call args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoalRenderArgs {
	/// Operation.
	pub op: Option<GoalOp>,
	/// Objective.
	pub objective: Option<String>,
	/// Token budget.
	pub token_budget: Option<u64>,
}

/// Describe op string, defaulting to `"?"`/raw.
pub fn describe_op(op: Option<GoalOp>, raw: Option<&str>) -> String {
	if let Some(op) = op {
		op.describe().to_string()
	} else {
		raw.unwrap_or("?").to_string()
	}
}

/// Format integer with comma grouping.
pub fn format_number_plain(n: u64) -> String {
	let s = n.to_string();
	let mut out = String::with_capacity(s.len() + s.len() / 3);
	for (i, ch) in s.chars().rev().enumerate() {
		if i > 0 && i % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out.chars().rev().collect()
}

/// Format duration in ms to compact text.
pub fn format_duration_plain(ms: u64) -> String {
	if ms < 1000 {
		format!("{ms}ms")
	} else if ms < 60_000 {
		let s = ms as f64 / 1000.0;
		if s < 10.0 {
			format!("{s:.1}s")
		} else {
			format!("{:.0}s", s)
		}
	} else {
		let m = ms / 60_000;
		let s = (ms % 60_000) / 1000;
		if s == 0 {
			format!("{m}m")
		} else {
			format!("{m}m{s}s")
		}
	}
}

/// Plain description of a goal result for native/head/body assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalResultSummary {
	/// Header description (op label).
	pub description: String,
	/// Status label if goal present.
	pub status: Option<String>,
	/// Status tone color token.
	pub status_tone: Option<String>,
	/// Objective line.
	pub objective: Option<String>,
	/// Token/time meta line.
	pub meta: Option<String>,
	/// Completion budget report.
	pub report: Option<String>,
	/// Warning tone/no-goal case.
	pub warning_no_goal: bool,
	/// Error message if failed.
	pub error: Option<String>,
}

/// Build a plain goal result summary.
pub fn describe_goal_result(
	is_error: bool,
	fallback_text: Option<&str>,
	details: Option<&GoalToolDetails>,
	args: Option<&GoalRenderArgs>,
) -> GoalResultSummary {
	let op = details.and_then(|d| d.op).or_else(|| args.and_then(|a| a.op));
	let raw_op = None;
	let description = describe_op(op, raw_op);
	if is_error {
		return GoalResultSummary {
			description,
			status: None,
			status_tone: None,
			objective: None,
			meta: None,
			report: None,
			warning_no_goal: false,
			error: Some(
				fallback_text
					.filter(|s| !s.is_empty())
					.unwrap_or("Goal tool failed")
					.to_string(),
			),
		};
	}
	let goal = details.and_then(|d| d.goal.as_ref());
	let Some(goal) = goal else {
		return GoalResultSummary {
			description,
			status: None,
			status_tone: None,
			objective: None,
			meta: None,
			report: None,
			warning_no_goal: true,
			error: None,
		};
	};
	let status = goal.status.unwrap_or(GoalStatus::Active);
	let used = format_number_plain(goal.tokens_used);
	let tokens = if let Some(budget) = goal.token_budget {
		let left = budget.saturating_sub(goal.tokens_used);
		format!(
			"{used} / {} tokens ({} left)",
			format_number_plain(budget),
			format_number_plain(left)
		)
	} else {
		format!("{used} tokens")
	};
	let mut meta_parts = vec![tokens];
	if goal.time_used_seconds > 0 {
		meta_parts.push(format!(
			"{} elapsed",
			format_duration_plain(goal.time_used_seconds.saturating_mul(1000))
		));
	}
	GoalResultSummary {
		description,
		status: Some(status.as_str().to_string()),
		status_tone: Some(status.badge_color().to_string()),
		objective: Some(format!("\"{}\"", goal.objective.trim())),
		meta: Some(meta_parts.join(" · ")),
		report: details.and_then(|d| d.completion_budget_report.clone()),
		warning_no_goal: false,
		error: None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn status_and_op_helpers() {
		assert_eq!(GoalStatus::Complete.as_str(), "complete");
		assert_eq!(GoalStatus::BudgetLimited.badge_color(), "warning");
		assert_eq!(GoalOp::Create.describe(), "set");
		assert_eq!(GoalOp::parse("resume"), Some(GoalOp::Resume));
	}

	#[test]
	fn no_goal_summary() {
		let s = describe_goal_result(false, None, None, None);
		assert!(s.warning_no_goal);
		assert_eq!(s.description, "?");
	}

	#[test]
	fn goal_summary_with_budget() {
		let details = GoalToolDetails {
			op: Some(GoalOp::Get),
			goal: Some(Goal {
				id: "g1".into(),
				objective: "ship it".into(),
				status: Some(GoalStatus::Active),
				token_budget: Some(1000),
				tokens_used: 250,
				time_used_seconds: 5,
				..Default::default()
			}),
			..Default::default()
		};
		let s = describe_goal_result(false, None, Some(&details), None);
		assert_eq!(s.description, "check");
		assert_eq!(s.status.as_deref(), Some("active"));
		assert!(s.meta.as_ref().unwrap().contains("250 / 1,000 tokens"));
		assert!(s.meta.as_ref().unwrap().contains("elapsed"));
	}
}
