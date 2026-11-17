//! Types and host traits for the agent hub overlays.
//!
//! Reference: `l/src/overlays/agent-hub-types.ts`.
//!
//! Deviations:
//! - `ThinkingLevel`, `Model`, `AgentMessage`, session/registry implementations,
//!   and IRC bus types from `@oh-my-pi/pi-agent-core` / `@oh-my-pi/pi-ai` have
//!   no crate equivalents. This port models the structural surface the hub needs
//!   with traits and lightweight owned data; concrete hosts implement the traits.
//! - `ensureLive(...).prompt(...)` returns `Result<(), String>` instead of a
//!   framework promise/value.
//! - `session.agent.state.messages` and model details are opaque
//!   (`serde_json::Value`/strings) because message/model schemas are not ported.

use serde_json::Value;
use std::sync::Arc;

/// Identity of the ambient main session in the agent roster.
pub const MAIN_AGENT_ID: &str = "Main";

/// Lifecycle state shown in the roster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
	Running,
	Idle,
	Parked,
	Aborted,
}

/// Whether duration measures active time, span, or is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationKind {
	Active,
	Span,
	Unknown,
}

/// Usage summary consumed by the roster and inspector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentMetricsSummary {
	pub tokens: u64,
	pub requests: u64,
	pub tools: u64,
	pub cost: f64,
	pub duration_ms: u64,
	pub duration_kind: Option<DurationKind>,
	pub context_tokens: Option<u64>,
	pub context_window: Option<u64>,
}

/// Token counters from session stats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionTokenStats {
	pub input: u64,
	pub output: u64,
	pub cache_write: u64,
}

/// Context usage snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionContextUsage {
	pub tokens: u64,
	pub context_window: u64,
}

/// Aggregated session stats for a live agent session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionStats {
	pub tokens: SessionTokenStats,
	pub assistant_messages: u64,
	pub tool_calls: u64,
	pub cost: f64,
	pub context_usage: Option<SessionContextUsage>,
}

/// Usage statistics exposed by a session ledger.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionUsageStatistics {
	pub cost: f64,
	pub subagent_cost: f64,
}

/// Serving model information for a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServingModel {
	pub selector: String,
	pub is_fallback: bool,
}

/// Minimal session-manager view used by the hub.
pub trait SessionManagerView {
	fn get_usage_statistics(&self) -> SessionUsageStatistics;
}

/// Minimal agent-state view (opaque messages).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentStateView {
	pub messages: Vec<Value>,
}

/// Minimal agent wrapper view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentView {
	pub state: AgentStateView,
}

/// Live session data and actions used by the agent hub.
pub trait AgentHubSession {
	fn thinking_level(&self) -> Option<String>;
	fn model(&self) -> Option<Value>;
	fn serving_model(&self) -> Option<ServingModel>;
	fn agent(&self) -> Option<AgentView>;
	fn session_manager(&self) -> Option<Arc<dyn SessionManagerView + Send + Sync>>;
	fn get_session_stats(&self) -> SessionStats;
	fn abort(&self, reason: Option<&str>) -> Result<(), String>;
}

/// Persisted/history metadata for an agent record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentRecordHistory {
	pub agent: Option<String>,
	pub model_role: Option<String>,
	pub resolved_model: Option<String>,
	pub resolved_model_is_fallback: Option<bool>,
	pub metrics: Option<AgentMetricsSummary>,
	pub direct_cost: Option<f64>,
	pub read_only: Option<bool>,
	pub output_path: Option<String>,
	pub patch_path: Option<String>,
	pub branch_name: Option<String>,
	pub nested_patch_paths: Option<Vec<String>>,
}

/// Lifecycle timestamps for an agent record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRecordLifecycle {
	pub response_at: Option<i64>,
	pub accepted_at: Option<i64>,
	pub terminal_at: Option<i64>,
}

/// Structural view of an agent-registry record.
pub trait AgentRecordLike {
	fn id(&self) -> &str;
	fn display_name(&self) -> &str;
	fn kind(&self) -> &str;
	fn parent_id(&self) -> Option<&str>;
	fn status(&self) -> AgentStatus;
	fn session(&self) -> Option<Arc<dyn AgentHubSession + Send + Sync>>;
	fn session_file(&self) -> Option<&str>;
	fn created_at(&self) -> i64;
	fn last_activity(&self) -> i64;
	fn activity(&self) -> Option<&str>;
	fn history(&self) -> Option<AgentRecordHistory>;
	fn lifecycle(&self) -> Option<AgentRecordLifecycle>;
}

/// Registry operations needed to display a live roster.
pub trait AgentHubRegistry {
	type Record: AgentRecordLike + ?Sized;

	fn list(&self) -> Vec<Arc<Self::Record>>;
	fn get(&self, id: &str) -> Option<Arc<Self::Record>>;
	fn on_change(&self, listener: Box<dyn Fn() + Send + Sync>) -> Box<dyn FnOnce() + Send>;
}

/// Prompt handle returned by lifecycle `ensure_live`.
pub trait LivePromptHandle {
	fn prompt(&self, text: &str, streaming_behavior: &str) -> Result<(), String>;
}

/// Lifecycle actions exposed by the host.
pub trait AgentLifecycleLike<R: AgentHubRegistry + ?Sized> {
	fn ensure_live(&self, id: &str) -> Result<Arc<dyn LivePromptHandle + Send + Sync>, String>;
	fn release(&self, id: &str, expected: &R::Record, tombstone: bool) -> Result<bool, String>;
}

/// Unread-message counts supplied by the host IRC bus.
pub trait IrcBusLike {
	fn unread_count(&self, id: &str) -> usize;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn main_agent_id_is_stable() {
		assert_eq!(MAIN_AGENT_ID, "Main");
	}

	#[test]
	fn agent_status_variants_exist() {
		let statuses = [
			AgentStatus::Running,
			AgentStatus::Idle,
			AgentStatus::Parked,
			AgentStatus::Aborted,
		];
		assert_eq!(statuses.len(), 4);
		assert_ne!(AgentStatus::Running, AgentStatus::Idle);
	}

	#[test]
	fn metrics_summary_defaults_are_zeroed() {
		let m = AgentMetricsSummary::default();
		assert_eq!(m.tokens, 0);
		assert_eq!(m.requests, 0);
		assert_eq!(m.tools, 0);
		assert_eq!(m.cost, 0.0);
		assert!(m.duration_kind.is_none());
	}
}
