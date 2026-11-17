//! Launch-broker daemon types shared by services, `proc://`, and `omp ps`.
//!
//! Reference: `l/src/tools/daemon.ts`.
//!
//! Deviation: `Record<string, string>` becomes `Vec<(String, String)>` (or a
//! `BTreeMap` if callers need lookup) because the crate avoids implicit
//! key-ordering guarantees; the field is spelled `env` and keeps insertion
//! order.

/// Stable lifecycle states exposed by the launch broker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DaemonState {
	/// Starting.
	#[default]
	Starting,
	/// Running.
	Running,
	/// Ready.
	Ready,
	/// Restarting.
	Restarting,
	/// Stopping.
	Stopping,
	/// Exited.
	Exited,
	/// Failed.
	Failed,
}

impl DaemonState {
	/// The reference's wire label.
	pub fn as_str(self) -> &'static str {
		match self {
			DaemonState::Starting => "starting",
			DaemonState::Running => "running",
			DaemonState::Ready => "ready",
			DaemonState::Restarting => "restarting",
			DaemonState::Stopping => "stopping",
			DaemonState::Exited => "exited",
			DaemonState::Failed => "failed",
		}
	}
}

/// Restart behavior applied after an unexpected daemon exit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DaemonRestartPolicy {
	/// Never restart.
	#[default]
	No,
	/// Restart only after a failure.
	OnFailure,
	/// Always restart.
	Always,
}

impl DaemonRestartPolicy {
	/// The reference's wire label.
	pub fn as_str(self) -> &'static str {
		match self {
			DaemonRestartPolicy::No => "no",
			DaemonRestartPolicy::OnFailure => "on-failure",
			DaemonRestartPolicy::Always => "always",
		}
	}
}

/// Readiness conditions; every configured condition must pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DaemonReadySpec {
	/// Log line that signals readiness.
	pub log: Option<String>,
	/// Port that must be listening.
	pub port: Option<u16>,
	/// Host that must be listening.
	pub host: Option<String>,
	/// Readiness timeout in milliseconds.
	pub timeout_ms: u64,
}

/// Immutable launch specification retained for restart and inspection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DaemonSpec {
	/// Daemon name.
	pub name: String,
	/// Executable or script.
	pub application: String,
	/// Arguments.
	pub args: Vec<String>,
	/// Environment assignments.
	pub env: Vec<(String, String)>,
	/// Working directory.
	pub cwd: String,
	/// Whether the daemon runs on a pty.
	pub pty: bool,
	/// Readiness conditions.
	pub ready: Option<DaemonReadySpec>,
	/// Restart policy.
	pub restart: DaemonRestartPolicy,
	/// Whether state persists across restarts.
	pub persist: bool,
	/// Whether the daemon detaches from the broker.
	pub detached: bool,
}

/// Serializable daemon state visible to every client in one broker scope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DaemonSnapshot {
	/// Daemon name.
	pub name: String,
	/// Stable daemon id.
	pub id: String,
	/// Lifecycle state.
	pub state: DaemonState,
	/// Process id.
	pub pid: Option<u32>,
	/// Creation timestamp.
	pub created_at: f64,
	/// Start timestamp.
	pub started_at: f64,
	/// Readiness timestamp.
	pub ready_at: Option<f64>,
	/// Exit timestamp.
	pub exited_at: Option<f64>,
	/// Exit code.
	pub exit_code: Option<i32>,
	/// Exit reason.
	pub exit_reason: Option<String>,
	/// Restart count.
	pub restart_count: u32,
	/// Captured output bytes.
	pub output_bytes: u64,
	/// Owning client.
	pub owner: Option<String>,
	/// Log line that matched readiness.
	pub ready_match: Option<String>,
	/// Readiness conditions still unmet while starting.
	pub ready_pending: Vec<ReadyPending>,
	/// Whether state persists across restarts.
	pub persist: bool,
	/// Whether the daemon detaches from the broker.
	pub detached: bool,
}

/// One unmet readiness condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadyPending {
	/// Log line not seen yet.
	Log,
	/// Port not listening yet.
	Port,
}

impl ReadyPending {
	/// The reference's wire label.
	pub fn as_str(self) -> &'static str {
		match self {
			ReadyPending::Log => "log",
			ReadyPending::Port => "port",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn state_labels_match_reference() {
		let labels = [
			DaemonState::Starting,
			DaemonState::Running,
			DaemonState::Ready,
			DaemonState::Restarting,
			DaemonState::Stopping,
			DaemonState::Exited,
			DaemonState::Failed,
		]
		.map(|state| state.as_str());
		assert_eq!(
			labels,
			[
				"starting",
				"running",
				"ready",
				"restarting",
				"stopping",
				"exited",
				"failed"
			]
		);
	}

	#[test]
	fn restart_policy_labels_match_reference() {
		assert_eq!(DaemonRestartPolicy::No.as_str(), "no");
		assert_eq!(DaemonRestartPolicy::OnFailure.as_str(), "on-failure");
		assert_eq!(DaemonRestartPolicy::Always.as_str(), "always");
	}

	#[test]
	fn pending_labels_match_reference() {
		assert_eq!(ReadyPending::Log.as_str(), "log");
		assert_eq!(ReadyPending::Port.as_str(), "port");
	}

	#[test]
	fn snapshot_defaults_are_empty() {
		let snapshot = DaemonSnapshot::default();
		assert_eq!(snapshot.state, DaemonState::Starting);
		assert_eq!(snapshot.pid, None);
		assert!(snapshot.ready_pending.is_empty());
		assert_eq!(snapshot.restart_count, 0);
	}
}
