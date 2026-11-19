//! Helpers for the running-subagent badge.
//!
//! Reference: `l/src/overlays/running-subagent-badge.ts`.
//!
//! Deviations:
//! - Registry records are accessed through [`crate::overlays::agent_hub_types`]
//!   traits instead of concrete `AgentHubRegistry` objects.
//! - `get_running_subagent_badge_registry` takes borrowed references (Rust has
//!   no ambient collab guest object model).

use crate::overlays::agent_hub_types::{
	AgentHubRegistry, AgentRecordLike, AgentStatus, MAIN_AGENT_ID,
};

/// Source providing an agent registry for a collab guest session.
pub trait RunningSubagentRegistrySource<R: AgentHubRegistry + ?Sized> {
	fn agent_registry(&self) -> &R;
}

/// Return the guest registry when present, otherwise the local registry.
pub fn get_running_subagent_badge_registry<'a, R, S>(
	collab_guest: Option<&'a S>,
	local_registry: &'a R,
) -> &'a R
where
	R: AgentHubRegistry + ?Sized,
	S: RunningSubagentRegistrySource<R> + ?Sized,
{
	match collab_guest {
		Some(guest) => guest.agent_registry(),
		None => local_registry,
	}
}

/// IDs of running subagents in the registry.
pub fn get_running_subagent_badge_agent_ids<R: AgentHubRegistry + ?Sized>(registry: &R) -> Vec<String> {
	registry
		.list()
		.into_iter()
		.filter(|record| record.kind() == "sub" && record.status() == AgentStatus::Running)
		.map(|record| record.id().to_string())
		.collect()
}

/// Count inspectable subagents (every registry ref except main), any status.
pub fn get_inspectable_subagent_count<R: AgentHubRegistry + ?Sized>(registry: &R) -> usize {
	registry
		.list()
		.into_iter()
		.filter(|record| record.id() != MAIN_AGENT_ID)
		.count()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::agent_hub_types::{
		AgentHubSession, AgentRecordHistory, AgentRecordLifecycle, AgentRecordLike, AgentStatus,
	};
	use std::sync::Arc;

	#[derive(Clone)]
	struct Rec {
		id: String,
		kind: String,
		status: AgentStatus,
	}

	impl AgentRecordLike for Rec {
		fn id(&self) -> &str {
			&self.id
		}
		fn display_name(&self) -> &str {
			&self.id
		}
		fn kind(&self) -> &str {
			&self.kind
		}
		fn parent_id(&self) -> Option<&str> {
			None
		}
		fn status(&self) -> AgentStatus {
			self.status
		}
		fn session(&self) -> Option<Arc<dyn AgentHubSession + Send + Sync>> {
			None
		}
		fn session_file(&self) -> Option<&str> {
			None
		}
		fn created_at(&self) -> i64 {
			0
		}
		fn last_activity(&self) -> i64 {
			0
		}
		fn activity(&self) -> Option<&str> {
			None
		}
		fn history(&self) -> Option<AgentRecordHistory> {
			None
		}
		fn lifecycle(&self) -> Option<AgentRecordLifecycle> {
			None
		}
	}

	struct Reg {
		items: Vec<Rec>,
	}

	impl AgentHubRegistry for Reg {
		type Record = Rec;
		fn list(&self) -> Vec<Arc<Self::Record>> {
			self.items.iter().cloned().map(Arc::new).collect()
		}
		fn get(&self, id: &str) -> Option<Arc<Self::Record>> {
			self.items
				.iter()
				.find(|r| r.id == id)
				.cloned()
				.map(Arc::new)
		}
		fn on_change(&self, _listener: Box<dyn Fn() + Send + Sync>) -> Box<dyn FnOnce() + Send> {
			Box::new(|| {})
		}
	}

	struct Guest<'a> {
		reg: &'a Reg,
	}

	impl<'a> RunningSubagentRegistrySource<Reg> for Guest<'a> {
		fn agent_registry(&self) -> &Reg {
			self.reg
		}
	}

	#[test]
	fn picks_guest_registry_when_present() {
		let local = Reg { items: vec![] };
		let guest_reg = Reg { items: vec![] };
		let guest = Guest { reg: &guest_reg };
		let chosen: &Reg = get_running_subagent_badge_registry(Some(&guest), &local);
		assert!(std::ptr::eq(chosen, &guest_reg));
	}

	#[test]
	fn running_badge_ids_only_include_running_subagents() {
		let reg = Reg {
			items: vec![
				Rec { id: "Main".into(), kind: "main".into(), status: AgentStatus::Running },
				Rec { id: "a".into(), kind: "sub".into(), status: AgentStatus::Running },
				Rec { id: "b".into(), kind: "sub".into(), status: AgentStatus::Idle },
				Rec { id: "c".into(), kind: "advisor".into(), status: AgentStatus::Running },
			],
		};
		assert_eq!(get_running_subagent_badge_agent_ids(&reg), vec!["a".to_string()]);
	}

	#[test]
	fn inspectable_count_excludes_main() {
		let reg = Reg {
			items: vec![
				Rec { id: "Main".into(), kind: "main".into(), status: AgentStatus::Running },
				Rec { id: "a".into(), kind: "sub".into(), status: AgentStatus::Parked },
			],
		};
		assert_eq!(get_inspectable_subagent_count(&reg), 1);
	}
}
