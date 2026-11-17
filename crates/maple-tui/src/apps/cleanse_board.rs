//! Live status board and final table for `omp cleanse`.
//!
//! Reference: `l/src/apps/cleanse-board.ts`.
//!
//! Interactive terminals render the live wave as a log bar plus per-lane
//! rows that repaint in place. Non-TTY output prints one line per lane
//! transition instead. The permanent per-run table is written by the caller
//! after the board closes.
//!
//! Deviations:
//! - TS depends on `LiveLogBar`, theme/chrome/table helpers, TTY writes, and
//!   timers for spinners. This port provides a deterministic, host-driven
//!   model/renderer ([`CleanseStatusBoard`], [`CleanseFinalEntry`],
//!   [`format_cleanse_final_table`]) without terminal side effects.
//! - Spinner is represented as a caller-supplied frame string.

use std::collections::BTreeMap;

/// Progress callback contracts.
pub trait CleanseProgressObserver {
	fn on_wave(&mut self, _wave: u32, _active: u32) {}
	fn on_start(&mut self, _lane: &str, _command: &str) {}
	fn on_log(&mut self, _lane: &str, _line: &str) {}
	fn on_done(&mut self, _lane: &str, _ok: bool, _duration_ms: u64, _error: Option<&str>) {}
}

/// Renderable board contract.
pub trait CleanseBoard: CleanseProgressObserver {
	fn interactive(&self) -> bool;
	/// Clear the live area and restore the cursor. Idempotent.
	fn close(&mut self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LaneStatus {
	Pending,
	Running { command: String },
	Done { ok: bool, duration_ms: u64, error: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanseFinalEntry {
	pub label: String,
	pub ran: bool,
	pub ok: bool,
	pub duration_ms: u64,
	pub error: Option<String>,
}

/// Checker identity shown in live rows and verdicts.
pub type CleanseCheckerDescriptor = String;
/// Checker result fields consumed by the verdict line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanseCheckResult {
	pub id: String,
	pub label: String,
	pub diagnostics_count: usize,
}
/// Repair workload fields displayed in agent rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanseAssignment {
	pub index: usize,
	pub weight: u32,
	pub groups: Vec<String>,
}
/// Completion fields displayed when a repair agent settles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanseAgentOutcome {
	pub name: String,
	pub success: bool,
	pub error: Option<String>,
	pub resolved_model: Option<String>,
}

/// Live-view state/model for one cleanse run (data model; rendering host-owned).
pub type CleanseBoardModel = CleanseStatusBoard;

/// In-memory live board.
pub struct CleanseStatusBoard {
	interactive: bool,
	wave: Option<u32>,
	active: u32,
	lanes: BTreeMap<String, LaneStatus>,
	log_lines: Vec<String>,
	closed: bool,
	spinner: String,
}

impl CleanseStatusBoard {
	pub fn new(lane_labels: &[String], interactive: bool) -> Self {
		let mut lanes = BTreeMap::new();
		for l in lane_labels {
			lanes.insert(l.clone(), LaneStatus::Pending);
		}
		Self {
			interactive,
			wave: None,
			active: 0,
			lanes,
			log_lines: Vec::new(),
			closed: false,
			spinner: "◐".to_string(),
		}
	}

	pub fn set_spinner(&mut self, s: impl Into<String>) { self.spinner = s.into(); }
	pub fn is_closed(&self) -> bool { self.closed }
	pub fn log_lines(&self) -> &[String] { &self.log_lines }

	fn format_log(lane: &str, line: &str) -> String {
		let msg = line.split_whitespace().collect::<Vec<_>>().join(" ");
		format!("    {lane}  {msg}")
	}

	/// Render current live view as lines (for hosts/tests).
	pub fn render(&self) -> Vec<String> {
		let mut out = Vec::new();
		if let Some(w) = self.wave {
			out.push(format!("  wave {w}  active {}", self.active));
		}
		for line in &self.log_lines {
			out.push(line.clone());
		}
		for (label, st) in &self.lanes {
			let line = match st {
				LaneStatus::Pending => format!("    ·  {label}  waiting"),
				LaneStatus::Running { command } => format!("  {}  {label}  {command}", self.spinner),
				LaneStatus::Done { ok, duration_ms, error } => {
					let mark = if *ok { "✓" } else { "✗" };
					let base = format!("  {mark}  {label}  {duration_ms}ms");
					match (ok, error) {
						(false, Some(e)) => format!("{base}  {e}"),
						_ => base,
					}
				}
			};
			out.push(line);
		}
		out
	}
}

impl CleanseProgressObserver for CleanseStatusBoard {
	fn on_wave(&mut self, wave: u32, active: u32) {
		self.wave = Some(wave);
		self.active = active;
	}
	fn on_start(&mut self, lane: &str, command: &str) {
		if let Some(s) = self.lanes.get_mut(lane) {
			*s = LaneStatus::Running { command: command.to_string() };
		}
		if !self.interactive {
			self.log_lines.push(format!("  RUN  {lane}  {command}"));
		}
	}
	fn on_log(&mut self, lane: &str, line: &str) {
		self.log_lines.push(Self::format_log(lane, line));
	}
	fn on_done(&mut self, lane: &str, ok: bool, duration_ms: u64, error: Option<&str>) {
		if let Some(s) = self.lanes.get_mut(lane) {
			*s = LaneStatus::Done { ok, duration_ms, error: error.map(|e| e.to_string()) };
		}
		if !self.interactive {
			let mark = if ok { "PASS" } else { "FAIL" };
			let mut msg = format!("  {mark} {lane} {duration_ms}ms");
			if !ok {
				if let Some(e) = error { msg.push_str(&format!("  {e}")); }
			}
			self.log_lines.push(msg);
		}
	}
}

impl CleanseBoard for CleanseStatusBoard {
	fn interactive(&self) -> bool { self.interactive }
	fn close(&mut self) { self.closed = true; }
}

/// Create a live board for the given lane labels.
pub use create_cleanse_board as createCleanseStatusBoard;

pub fn create_cleanse_board(lane_labels: &[String], interactive: bool) -> CleanseStatusBoard {
	CleanseStatusBoard::new(lane_labels, interactive)
}

fn format_duration_ms(ms: u64) -> String {
	if ms < 1000 { format!("{ms}ms") } else { format!("{}.{:01}s", ms / 1000, (ms % 1000) / 100) }
}

/// Format the permanent table written after all lanes complete.
pub fn format_cleanse_final_table(entries: &[CleanseFinalEntry]) -> String {
	let headers = ["checker", "status", "time", "note"];
	let rows: Vec<Vec<String>> = entries.iter().map(|e| {
		let status = if !e.ran { "skip" } else if e.ok { "ok" } else { "fail" };
		let time = if e.ran { format_duration_ms(e.duration_ms) } else { "-".into() };
		let note = e.error.clone().unwrap_or_default();
		vec![e.label.clone(), status.into(), time, note]
	}).collect();
	let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
	for row in &rows {
		for (i, c) in row.iter().enumerate() { widths[i] = widths[i].max(c.len()); }
	}
	let fmt = |cells: &[String]| -> String {
		cells.iter().enumerate().map(|(i, c)| {
			if i == 0 { format!("{:<w$}", c, w = widths[i]) }
			else { format!("  {:<w$}", c, w = widths[i]) }
		}).collect::<String>().trim_end().to_string()
	};
	let mut lines = vec![fmt(&headers.iter().map(|s| s.to_string()).collect::<Vec<_>>())];
	for row in &rows { lines.push(fmt(row)); }
	format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn board_tracks_wave_and_lane_transitions() {
		let labels = vec!["ts".to_string(), "lint".to_string()];
		let mut b = create_cleanse_board(&labels, true);
		b.on_wave(1, 2);
		b.on_start("ts", "tsc");
		b.on_done("ts", true, 1200, None);
		let out = b.render().join("\n");
		assert!(out.contains("wave 1"));
		assert!(out.contains("✓  ts"));
		assert!(b.interactive());
		b.close();
		assert!(b.is_closed());
	}

	#[test]
	fn non_interactive_emits_transition_lines() {
		let mut b = create_cleanse_board(&["x".into()], false);
		b.on_start("x", "cmd");
		b.on_done("x", false, 5, Some("boom"));
		let joined = b.log_lines().join("\n");
		assert!(joined.contains("RUN  x"));
		assert!(joined.contains("FAIL x"));
		assert!(joined.contains("boom"));
	}

	#[test]
	fn final_table_formats_rows() {
		let table = format_cleanse_final_table(&[
			CleanseFinalEntry { label: "ts".into(), ran: true, ok: true, duration_ms: 50, error: None },
			CleanseFinalEntry { label: "lint".into(), ran: false, ok: false, duration_ms: 0, error: Some("skipped".into()) },
		]);
		assert!(table.contains("checker"));
		assert!(table.contains("ok"));
		assert!(table.contains("skip"));
	}
}
