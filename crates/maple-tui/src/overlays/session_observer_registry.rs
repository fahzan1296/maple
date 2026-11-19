//! Registry of observable subagent sessions and their progress.
//!
//! Reference: `l/src/overlays/session-observer-registry.ts`.
//!
//! Deviations:
//! - `EventBus` is abstracted behind [`EventBusLike`] and receives payloads as
//!   `serde_json::Value` (the concrete bus/types from the host runtime are not
//!   ported).
//! - `attach` does not auto-wire callbacks into a trait object bus because this
//!   standalone port stores mutable state behind interior mutability on `&self`
//!   and cannot capture `self` by `Arc` internally; hosts should forward bus
//!   events to [`SessionObserverRegistry::handle_progress_value`] /
//!   [`SessionObserverRegistry::handle_lifecycle_value`].
//! - `AgentProgress`/source shapes are represented as lightweight values.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// EventBus channel for aggregated subagent progress.
pub const TASK_SUBAGENT_PROGRESS_CHANNEL: &str = "task:subagent:progress";
/// EventBus channel for subagent lifecycle (start/end).
pub const TASK_SUBAGENT_LIFECYCLE_CHANNEL: &str = "task:subagent:lifecycle";

/// Coarse source of an observer change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionObserverChangeKind {
	Main,
	Reset,
	Lifecycle,
	Progress,
}

/// Observable session status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ObservableSessionStatus {
	#[default]
	Running,
	Completed,
	Failed,
	Aborted,
}

/// Minimal progress payload stored on observable sessions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentProgress {
	pub data: Value,
}

/// Progress payload emitted on the progress channel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubagentProgressPayload {
	pub index: Option<usize>,
	pub agent: Option<String>,
	pub agent_source: Option<String>,
	pub task: Option<String>,
	pub parent_tool_call_id: Option<String>,
	pub assignment: Option<String>,
	pub progress: AgentProgress,
	pub session_file: Option<String>,
	pub detached: Option<bool>,
}

/// Lifecycle payload emitted on the lifecycle channel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubagentLifecyclePayload {
	pub id: Option<String>,
	pub agent: Option<String>,
	pub agent_source: Option<String>,
	pub description: Option<String>,
	pub status: Option<String>,
	pub session_file: Option<String>,
	pub parent_tool_call_id: Option<String>,
	pub index: Option<usize>,
	pub detached: Option<bool>,
}

/// Minimal event subscription surface supplied by the host.
pub trait EventBusLike: Send + Sync {
	fn on(&self, channel: &str, listener: Box<dyn Fn(Value) + Send + Sync>)
		-> Box<dyn FnOnce() + Send>;
}

/// Observable session record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObservableSession {
	pub id: String,
	pub agent: Option<String>,
	pub agent_source: Option<String>,
	pub task: Option<String>,
	pub status: ObservableSessionStatus,
	pub progress: Option<AgentProgress>,
	pub session_file: Option<String>,
	pub parent_tool_call_id: Option<String>,
	pub assignment: Option<String>,
	pub detached: bool,
	pub index: Option<usize>,
}

type ObserverListener = Arc<dyn Fn(SessionObserverChangeKind) + Send + Sync>;

/// Tracks observable subagent sessions from event-bus updates.
#[derive(Default)]
pub struct SessionObserverRegistry {
	sessions: Mutex<HashMap<String, ObservableSession>>,
	observers: Mutex<Vec<ObserverListener>>,
}

impl SessionObserverRegistry {
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// Placeholder attach hook (see module deviations).
	pub fn attach(&self, _bus: &Arc<dyn EventBusLike>) -> Vec<Box<dyn FnOnce() + Send>> {
		Vec::new()
	}

	/// Current observable sessions snapshot.
	pub fn sessions(&self) -> Vec<ObservableSession> {
		let guard = self.sessions.lock().expect("session observer poisoned");
		let mut items: Vec<_> = guard.values().cloned().collect();
		items.sort_by(|a, b| a.id.cmp(&b.id));
		items
	}

	pub fn observe(&self, listener: ObserverListener) {
		self.observers
			.lock()
			.expect("session observer poisoned")
			.push(listener);
	}

	fn emit(&self, kind: SessionObserverChangeKind) {
		let listeners = self
			.observers
			.lock()
			.expect("session observer poisoned")
			.clone();
		for listener in listeners {
			listener(kind);
		}
	}

	pub fn reset(&self) {
		self.sessions.lock().expect("session observer poisoned").clear();
		self.emit(SessionObserverChangeKind::Reset);
	}

	pub fn handle_progress_value(&self, value: Value) {
		let payload = progress_from_value(&value);
		let Some(id) = session_id_from_progress(&payload) else {
			return;
		};
		{
			let mut sessions = self.sessions.lock().expect("session observer poisoned");
			let entry = sessions.entry(id.clone()).or_insert_with(|| ObservableSession {
				id: id.clone(),
				status: ObservableSessionStatus::Running,
				..Default::default()
			});
			if let Some(agent) = payload.agent.clone() {
				entry.agent = Some(agent);
			}
			if let Some(source) = payload.agent_source.clone() {
				entry.agent_source = Some(source);
			}
			if let Some(task) = payload.task.clone() {
				entry.task = Some(task);
			}
			entry.progress = Some(payload.progress.clone());
			if let Some(file) = payload.session_file.clone() {
				entry.session_file = Some(file);
			}
			if let Some(parent) = payload.parent_tool_call_id.clone() {
				entry.parent_tool_call_id = Some(parent);
			}
			if let Some(assignment) = payload.assignment.clone() {
				entry.assignment = Some(assignment);
			}
			if let Some(detached) = payload.detached {
				entry.detached = detached;
			}
			if payload.index.is_some() {
				entry.index = payload.index;
			}
			entry.status = ObservableSessionStatus::Running;
		}
		self.emit(SessionObserverChangeKind::Progress);
	}

	pub fn handle_lifecycle_value(&self, value: Value) {
		let payload = lifecycle_from_value(&value);
		let Some(id) = payload.id.clone().or_else(|| payload.agent.clone()) else {
			return;
		};
		{
			let mut sessions = self.sessions.lock().expect("session observer poisoned");
			let entry = sessions.entry(id.clone()).or_insert_with(|| ObservableSession {
				id: id.clone(),
				..Default::default()
			});
			if let Some(agent) = payload.agent.clone() {
				entry.agent = Some(agent);
			}
			if let Some(source) = payload.agent_source.clone() {
				entry.agent_source = Some(source);
			}
			if let Some(file) = payload.session_file.clone() {
				entry.session_file = Some(file);
			}
			if let Some(parent) = payload.parent_tool_call_id.clone() {
				entry.parent_tool_call_id = Some(parent);
			}
			if let Some(detached) = payload.detached {
				entry.detached = detached;
			}
			if payload.index.is_some() {
				entry.index = payload.index;
			}
			entry.status = map_status(payload.status.as_deref());
			if let Some(desc) = payload.description.clone() {
				entry.task = Some(desc);
			}
		}
		self.emit(SessionObserverChangeKind::Lifecycle);
	}
}

fn map_status(status: Option<&str>) -> ObservableSessionStatus {
	match status {
		Some("completed") => ObservableSessionStatus::Completed,
		Some("failed") => ObservableSessionStatus::Failed,
		Some("aborted") => ObservableSessionStatus::Aborted,
		_ => ObservableSessionStatus::Running,
	}
}

fn session_id_from_progress(payload: &SubagentProgressPayload) -> Option<String> {
	if let Some(parent) = &payload.parent_tool_call_id {
		let idx = payload.index.unwrap_or(0);
		return Some(format!("{parent}:{idx}"));
	}
	payload.agent.clone()
}

fn progress_from_value(value: &Value) -> SubagentProgressPayload {
	let obj = value.as_object();
	SubagentProgressPayload {
		index: obj
			.and_then(|o| o.get("index"))
			.and_then(Value::as_u64)
			.map(|v| v as usize),
		agent: obj
			.and_then(|o| o.get("agent"))
			.and_then(Value::as_str)
			.map(str::to_string),
		agent_source: obj
			.and_then(|o| o.get("agentSource"))
			.and_then(Value::as_str)
			.map(str::to_string),
		task: obj
			.and_then(|o| o.get("task"))
			.and_then(Value::as_str)
			.map(str::to_string),
		parent_tool_call_id: obj
			.and_then(|o| o.get("parentToolCallId"))
			.and_then(Value::as_str)
			.map(str::to_string),
		assignment: obj
			.and_then(|o| o.get("assignment"))
			.and_then(Value::as_str)
			.map(str::to_string),
		progress: AgentProgress {
			data: obj
				.and_then(|o| o.get("progress"))
				.cloned()
				.unwrap_or(Value::Null),
		},
		session_file: obj
			.and_then(|o| o.get("sessionFile"))
			.and_then(Value::as_str)
			.map(str::to_string),
		detached: obj.and_then(|o| o.get("detached")).and_then(Value::as_bool),
	}
}

fn lifecycle_from_value(value: &Value) -> SubagentLifecyclePayload {
	let obj = value.as_object();
	SubagentLifecyclePayload {
		id: obj
			.and_then(|o| o.get("id"))
			.and_then(Value::as_str)
			.map(str::to_string),
		agent: obj
			.and_then(|o| o.get("agent"))
			.and_then(Value::as_str)
			.map(str::to_string),
		agent_source: obj
			.and_then(|o| o.get("agentSource"))
			.and_then(Value::as_str)
			.map(str::to_string),
		description: obj
			.and_then(|o| o.get("description"))
			.and_then(Value::as_str)
			.map(str::to_string),
		status: obj
			.and_then(|o| o.get("status"))
			.and_then(Value::as_str)
			.map(str::to_string),
		session_file: obj
			.and_then(|o| o.get("sessionFile"))
			.and_then(Value::as_str)
			.map(str::to_string),
		parent_tool_call_id: obj
			.and_then(|o| o.get("parentToolCallId"))
			.and_then(Value::as_str)
			.map(str::to_string),
		index: obj
			.and_then(|o| o.get("index"))
			.and_then(Value::as_u64)
			.map(|v| v as usize),
		detached: obj.and_then(|o| o.get("detached")).and_then(Value::as_bool),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn progress_creates_and_updates_session() {
		let reg = SessionObserverRegistry::new();
		reg.handle_progress_value(json!({
			"parentToolCallId": "p1",
			"index": 0,
			"agent": "worker",
			"task": "do thing",
			"progress": {"step": 1}
		}));
		let sessions = reg.sessions();
		assert_eq!(sessions.len(), 1);
		assert_eq!(sessions[0].id, "p1:0");
		assert_eq!(sessions[0].agent.as_deref(), Some("worker"));
		assert_eq!(sessions[0].status, ObservableSessionStatus::Running);
	}

	#[test]
	fn lifecycle_updates_status() {
		let reg = SessionObserverRegistry::new();
		reg.handle_lifecycle_value(json!({
			"id": "a",
			"status": "completed",
			"description": "done"
		}));
		let sessions = reg.sessions();
		assert_eq!(sessions.len(), 1);
		assert_eq!(sessions[0].status, ObservableSessionStatus::Completed);
		assert_eq!(sessions[0].task.as_deref(), Some("done"));
	}

	#[test]
	fn reset_clears_sessions() {
		let reg = SessionObserverRegistry::new();
		reg.handle_lifecycle_value(json!({"id":"a","status":"running"}));
		assert_eq!(reg.sessions().len(), 1);
		reg.reset();
		assert!(reg.sessions().is_empty());
	}

	#[test]
	fn map_status_defaults_to_running() {
		assert_eq!(map_status(None), ObservableSessionStatus::Running);
		assert_eq!(map_status(Some("unknown")), ObservableSessionStatus::Running);
		assert_eq!(map_status(Some("failed")), ObservableSessionStatus::Failed);
	}
}
