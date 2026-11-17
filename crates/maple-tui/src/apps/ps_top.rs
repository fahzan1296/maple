//! Interactive TUI monitor for jobs managed by this application.
//!
//! Reference: `l/src/apps/ps-top.ts`.
//!
//! Rendered as an alternate-screen table sorted by runtime desc, with
//! running rows above completed rows, and a live expanded row showing the
//! last 3 output lines for the selected task.
//!
//! Deviations:
//! - TS uses `@oh-my-pi/pi-tui` (`TUI`, `Container`, `TruncatedText`,
//!   `matchesKey`, theme, process terminal lifecycle) and real job handles.
//! - This port models data/state, host capabilities, key actions, and render
//!   lines as portable logic ([`PsTopModel`], [`PsTopHost`], [`TaskRow`]).
//!   Actual terminal rendering/alt-screen and job process control are host
//!   responsibilities.
//! - Preview lines are supplied/retained in-row (or via host) rather than read
//!   from live pipe handles.
//! - `run_ps_top` delegates to an injected runner because the TUI lifecycle is
//!   external to this crate (no internal stubbed no-op).

/// Task status values exposed by the monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
	Running,
	Completed,
	Failed,
	Cancelled,
}

impl JobStatus {
	pub fn as_str(&self) -> &'static str {
		match self {
			JobStatus::Running => "running",
			JobStatus::Completed => "completed",
			JobStatus::Failed => "failed",
			JobStatus::Cancelled => "cancelled",
		}
	}
}

/// Opaque job handle state snapshot required by the monitor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
	pub id: String,
	pub label: String,
	pub kind: String,
	pub start_time_ms: i64,
	pub end_time_ms: Option<i64>,
	pub status: JobStatus,
	pub working_dir: String,
	pub preview_lines: Vec<String>,
}

/// Host capabilities for listing and killing jobs.
pub trait PsTopHost {
	fn list_tasks(&self) -> Vec<TaskRow>;
	fn kill(&mut self, id: &str) -> Result<(), String>;
	fn now_ms(&self) -> i64;
}

/// User action inferred from key input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsTopAction {
	None,
	Quit,
	MoveUp,
	MoveDown,
	KillSelected,
}

pub fn action_for_key(data: &str) -> PsTopAction {
	match data {
		"q" | "ctrl+c" | "escape" => PsTopAction::Quit,
		"up" => PsTopAction::MoveUp,
		"down" => PsTopAction::MoveDown,
		"ctrl+k" => PsTopAction::KillSelected,
		_ => PsTopAction::None,
	}
}

fn runtime_ms(row: &TaskRow, now_ms: i64) -> i64 {
	let end = row.end_time_ms.unwrap_or(now_ms);
	(end - row.start_time_ms).max(0)
}

/// Sort: running before non-running, then runtime desc.
pub fn sort_rows(rows: &mut [TaskRow], now_ms: i64) {
	rows.sort_by(|a, b| {
		let ar = matches!(a.status, JobStatus::Running);
		let br = matches!(b.status, JobStatus::Running);
		br.cmp(&ar)
			.then(runtime_ms(b, now_ms).cmp(&runtime_ms(a, now_ms)))
	});
}

fn format_duration_ms(ms: i64) -> String {
	let ms = ms.max(0) as u64;
	if ms < 1000 {
		format!("{ms}ms")
	} else if ms < 60_000 {
		format!("{}.{:01}s", ms / 1000, (ms % 1000) / 100)
	} else {
		format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1000)
	}
}

fn shorten_path(path: &str) -> String {
	let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
	if parts.len() <= 3 {
		path.to_string()
	} else {
		parts[parts.len() - 3..].join("/")
	}
}

fn sanitize_one_line(s: &str) -> String {
	s.chars()
		.map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
		.collect()
}

/// Portable ps-top model.
pub struct PsTopModel {
	selected: usize,
}

impl PsTopModel {
	pub fn new() -> Self {
		Self { selected: 0 }
	}
	pub fn selected(&self) -> usize {
		self.selected
	}

	pub fn apply_action(&mut self, action: PsTopAction, row_count: usize) {
		match action {
			PsTopAction::MoveUp => {
				if self.selected > 0 {
					self.selected -= 1;
				}
			}
			PsTopAction::MoveDown => {
				if row_count > 0 && self.selected + 1 < row_count {
					self.selected += 1;
				}
			}
			_ => {}
		}
	}

	pub fn clamp(&mut self, row_count: usize) {
		if row_count == 0 {
			self.selected = 0;
		} else if self.selected >= row_count {
			self.selected = row_count - 1;
		}
	}

	/// Render table lines (header + rows + optional preview for selected).
	pub fn render_lines(&self, mut rows: Vec<TaskRow>, now_ms: i64, width: usize) -> Vec<String> {
		sort_rows(&mut rows, now_ms);
		let mut out = Vec::new();
		let header = "ID       ST   TIME     KIND     CWD                     CMD";
		out.push(header.chars().take(width.max(1)).collect());
		for (i, r) in rows.iter().enumerate() {
			let mark = if i == self.selected { ">" } else { " " };
			let id = if r.id.len() > 8 { &r.id[..8] } else { &r.id };
			let st = match r.status {
				JobStatus::Running => "run",
				JobStatus::Completed => "done",
				JobStatus::Failed => "fail",
				JobStatus::Cancelled => "cncl",
			};
			let time = format_duration_ms(runtime_ms(r, now_ms));
			let cwd = shorten_path(&r.working_dir);
			let line = format!(
				"{mark}{:<8} {:<4} {:<8} {:<8} {:<23} {}",
				id,
				st,
				time,
				r.kind,
				cwd,
				sanitize_one_line(&r.label)
			);
			out.push(line.chars().take(width.max(1)).collect());
			if i == self.selected {
				for p in r.preview_lines.iter().rev().take(3).rev() {
					out.push(format!("  │ {}", sanitize_one_line(p)));
				}
			}
		}
		if rows.is_empty() {
			out.push("(no jobs)".to_string());
		}
		out
	}
}

impl Default for PsTopModel {
	fn default() -> Self {
		Self::new()
	}
}

/// Run ps-top interactive loop via injected runner.
///
/// The real terminal/TUI lifecycle is host-owned and not present in this
/// crate; the runner drives the monitor and returns its result.
pub fn run_ps_top<R>(host: &mut dyn PsTopHost, runner: &mut R) -> Result<(), String>
where
	R: FnMut(&mut dyn PsTopHost) -> Result<(), String>,
{
	runner(host)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn row(id: &str, status: JobStatus, start: i64, end: Option<i64>) -> TaskRow {
		TaskRow {
			id: id.into(),
			label: "cmd".into(),
			kind: "bash".into(),
			start_time_ms: start,
			end_time_ms: end,
			status,
			working_dir: "/a/b/c/d".into(),
			preview_lines: vec!["l1".into(), "l2".into()],
		}
	}

	struct Host {
		rows: Vec<TaskRow>,
		now: i64,
		killed: Vec<String>,
		fail_kill: bool,
	}
	impl PsTopHost for Host {
		fn list_tasks(&self) -> Vec<TaskRow> {
			self.rows.clone()
		}
		fn kill(&mut self, id: &str) -> Result<(), String> {
			if self.fail_kill {
				Err("boom".into())
			} else {
				self.killed.push(id.into());
				Ok(())
			}
		}
		fn now_ms(&self) -> i64 {
			self.now
		}
	}

	#[test]
	fn key_actions() {
		assert_eq!(action_for_key("q"), PsTopAction::Quit);
		assert_eq!(action_for_key("escape"), PsTopAction::Quit);
		assert_eq!(action_for_key("up"), PsTopAction::MoveUp);
		assert_eq!(action_for_key("down"), PsTopAction::MoveDown);
		assert_eq!(action_for_key("ctrl+k"), PsTopAction::KillSelected);
		assert_eq!(action_for_key("x"), PsTopAction::None);
	}

	#[test]
	fn sort_running_first_then_runtime_desc() {
		let mut rows = vec![
			row("a", JobStatus::Completed, 0, Some(1000)),
			row("b", JobStatus::Running, 0, None),
			row("c", JobStatus::Running, 500, None),
		];
		sort_rows(&mut rows, 2000);
		assert_eq!(rows[0].id, "b");
		assert_eq!(rows[1].id, "c");
		assert_eq!(rows[2].id, "a");
	}

	#[test]
	fn render_includes_preview_for_selected() {
		let mut m = PsTopModel::new();
		let rows = vec![row("t1", JobStatus::Running, 0, None)];
		let lines = m.render_lines(rows, 1000, 80);
		assert!(lines[0].starts_with("ID"));
		assert!(lines.iter().any(|l| l.contains("│ l1")));
		m.apply_action(PsTopAction::MoveDown, 1);
		assert_eq!(m.selected(), 0);
	}

	#[test]
	fn status_and_duration_formatting() {
		assert_eq!(JobStatus::Failed.as_str(), "failed");
		assert_eq!(format_duration_ms(500), "500ms");
		assert_eq!(shorten_path("/1/2/3/4/5"), "3/4/5");
	}

	#[test]
	fn run_ps_top_delegates_to_runner() {
		let mut h = Host { rows: vec![], now: 7, killed: vec![], fail_kill: false };
		let mut seen = 0;
		let res = run_ps_top(&mut h, &mut |host| {
			seen = host.now_ms();
			Ok(())
		});
		assert!(res.is_ok());
		assert_eq!(seen, 7);
	}

	#[test]
	fn kill_is_host_controlled() {
		let mut h = Host { rows: vec![], now: 0, killed: vec![], fail_kill: false };
		h.kill("abc").unwrap();
		assert_eq!(h.killed, vec!["abc"]);
		h.fail_kill = true;
		assert_eq!(h.kill("x").unwrap_err(), "boom");
	}
}
