//! Process-scope display data and formatting helpers.
//! Reference: `l/src/apps/ps-data.ts`.

use crate::tools::daemon::{DaemonSnapshot, DaemonSpec, DaemonState};

/// One broker scope: a project runtime dir or a machine-global service dir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsScope {
	pub kind: PsScopeKind,
	pub runtime_dir: String,
	/// Canonical project dir when known.
	pub project_dir: Option<String>,
	/// Global service name (`Global`).
	pub service: Option<String>,
	/// Live broker PID; undefined when no broker owns the scope.
	pub broker_pid: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsScopeKind {
	Project,
	Global,
}

/// One process snapshot and its table display metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct PsDaemonRow {
	pub snapshot: DaemonSnapshot,
	/// Launch command from the persisted spec, when readable.
	pub command: Option<String>,
	pub cwd: Option<String>,
	/// False when the snapshot came from disk with no live broker supervising it.
	pub supervised: bool,
}

/// Process rows grouped under their owning broker scope.
#[derive(Clone, Debug, PartialEq)]
pub struct PsScopeReport {
	pub scope: PsScope,
	pub daemons: Vec<PsDaemonRow>,
}

/// Scope selector shared by every ps action: current project, `--dir`, or `--global`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PsTarget {
	pub dir: Option<String>,
	pub global: Option<String>,
}

/// Process states without a running process or live uptime.
pub fn is_terminal_state(state: DaemonState) -> bool {
	matches!(state, DaemonState::Exited | DaemonState::Failed)
}

/// Combine an executable and its arguments for display.
pub fn format_command(spec: Option<&DaemonSpec>) -> Option<String> {
	spec.map(|s| {
		std::iter::once(s.application.clone())
			.chain(s.args.clone())
			.collect::<Vec<_>>()
			.join(" ")
	})
}

/// Collapse a launch command to one display line (inline scripts embed newlines/tabs).
pub fn collapse_command(command: Option<&str>) -> String {
	command
		.map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
		.unwrap_or_default()
}

/// One-line daemon summary used by action results and detail views.
pub fn daemon_label(daemon: &DaemonSnapshot) -> String {
	let pid = daemon.pid.map(|p| format!(" pid={p}")).unwrap_or_default();
	let exit = daemon.exit_code.map(|e| format!(" exit={e}")).unwrap_or_default();
	format!("{}: {}{pid}{exit}", daemon.name, daemon.state.as_str())
}

fn green(s: &str) -> String {
	format!("\x1b[32m{s}\x1b[39m")
}
fn red(s: &str) -> String {
	format!("\x1b[31m{s}\x1b[39m")
}
fn yellow(s: &str) -> String {
	format!("\x1b[33m{s}\x1b[39m")
}
fn dim(s: &str) -> String {
	format!("\x1b[2m{s}\x1b[22m")
}
fn bold(s: &str) -> String {
	format!("\x1b[1m{s}\x1b[22m")
}

fn format_duration_ms(ms: i64) -> String {
	if ms < 0 {
		return "0s".into();
	}
	let secs = (ms / 1000) as u64;
	let h = secs / 3600;
	let m = (secs % 3600) / 60;
	let s = secs % 60;
	if h > 0 {
		format!("{h}h{m}m")
	} else if m > 0 {
		format!("{m}m{s}s")
	} else {
		format!("{s}s")
	}
}

/// Colored STATE cell, e.g. `ready`, `exited(143)`.
pub fn state_cell(row: &PsDaemonRow) -> String {
	let snapshot = &row.snapshot;
	let mut text = snapshot.state.as_str().to_string();
	if is_terminal_state(snapshot.state) {
		if let Some(code) = snapshot.exit_code {
			text.push_str(&format!("({code})"));
		}
	}
	match snapshot.state {
		DaemonState::Ready | DaemonState::Running => green(&text),
		DaemonState::Failed => red(&text),
		DaemonState::Exited => dim(&text),
		_ => yellow(&text),
	}
}

/// Display persistence and supervision flags for a process row.
pub fn flags_cell(row: &PsDaemonRow) -> String {
	let mut parts: Vec<&str> = Vec::new();
	if row.snapshot.detached {
		parts.push("detached");
	} else if row.snapshot.persist {
		parts.push("persist");
	}
	if !row.supervised && !is_terminal_state(row.snapshot.state) {
		parts.push("unsupervised");
	}
	parts.join(",")
}

/// Display elapsed runtime, or a dash for a terminal process.
pub fn uptime_cell(snapshot: &DaemonSnapshot, now_ms: i64) -> String {
	if is_terminal_state(snapshot.state) {
		return "-".into();
	}
	let started_ms = snapshot.started_at as i64;
	if started_ms <= 0 {
		return "-".into();
	}
	format_duration_ms(now_ms.saturating_sub(started_ms))
}

/// Labels aligned with the process table's display cells.
pub const TABLE_HEADER: [&str; 7] = ["NAME", "STATE", "PID", "UPTIME", "RESTARTS", "FLAGS", "COMMAND"];

/// Raw (possibly colored) cells for one daemon row, aligned with [`TABLE_HEADER`].
pub fn table_cells(row: &PsDaemonRow, now_ms: i64) -> [String; 7] {
	let snap = &row.snapshot;
	let pid = if snap.pid.is_some() && !is_terminal_state(snap.state) {
		snap.pid.unwrap().to_string()
	} else {
		"-".into()
	};
	[
		snap.name.clone(),
		state_cell(row),
		pid,
		uptime_cell(snap, now_ms),
		snap.restart_count.to_string(),
		flags_cell(row),
		collapse_command(row.command.as_deref()),
	]
}

fn basename(path: &str) -> &str {
	path.rsplit('/').next().unwrap_or(path).rsplit('\\').next().unwrap_or(path)
}

/// Scope heading, e.g. `project /work/pi — broker pid 1234`.
pub fn scope_header(scope: &PsScope) -> String {
	let label = match scope.kind {
		PsScopeKind::Global => format!(
			"global {}",
			bold(scope.service.as_deref().unwrap_or(basename(&scope.runtime_dir)))
		),
		PsScopeKind::Project => format!(
			"project {}",
			bold(scope.project_dir.as_deref().unwrap_or(basename(&scope.runtime_dir)))
		),
	};
	let broker = scope
		.broker_pid
		.map(|pid| green(&format!("broker pid {pid}")))
		.unwrap_or_else(|| dim("broker not running"));
	format!("{label} {} {broker}", dim("—"))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn snap(name: &str, state: DaemonState) -> DaemonSnapshot {
		DaemonSnapshot {
			id: "id".into(),
			name: name.into(),
			state,
			pid: Some(42),
			started_at: 1000.0,
			restart_count: 2,
			..DaemonSnapshot::default()
		}
	}

	#[test]
	fn command_and_collapse() {
		let spec = DaemonSpec { application: "node".into(), args: vec!["a".into(), "b".into()], ..Default::default() };
		assert_eq!(format_command(Some(&spec)).as_deref(), Some("node a b"));
		assert_eq!(collapse_command(Some("a\n\t b  c")), "a b c");
		assert_eq!(collapse_command(None), "");
	}

	#[test]
	fn label_state_flags_uptime_cells() {
		let mut s = snap("svc", DaemonState::Ready);
		let row = PsDaemonRow { snapshot: s.clone(), command: Some("echo hi".into()), cwd: None, supervised: true };
		assert!(daemon_label(&s).contains("svc: ready pid=42"));
		assert!(state_cell(&row).contains("ready"));
		assert_eq!(flags_cell(&row), "");
		assert_eq!(uptime_cell(&s, 6000), "5s");
		let cells = table_cells(&row, 6000);
		assert_eq!(cells[0], "svc");
		assert_eq!(cells[2], "42");
		assert_eq!(cells[4], "2");
		s.state = DaemonState::Exited;
		s.exit_code = Some(143);
		s.pid = None;
		let row2 = PsDaemonRow { snapshot: s.clone(), command: None, cwd: None, supervised: true };
		assert!(state_cell(&row2).contains("exited(143)"));
		assert_eq!(uptime_cell(&s, 9999), "-");
		assert_eq!(table_cells(&row2, 9999)[2], "-");
	}

	#[test]
	fn scope_header_and_table_header() {
		assert_eq!(TABLE_HEADER.len(), 7);
		let scope = PsScope {
			kind: PsScopeKind::Project,
			runtime_dir: "/work/pi/.rt".into(),
			project_dir: Some("/work/pi".into()),
			service: None,
			broker_pid: Some(1234),
		};
		let h = scope_header(&scope);
		assert!(h.contains("project"));
		assert!(h.contains("/work/pi"));
		assert!(h.contains("1234"));
	}
}
