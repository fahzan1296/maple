//! Live board and scoreboard for `omp if-bench`.
//!
//! Reference: `l/src/apps/if-bench-board.ts`.
//!
//! Interactive terminals get one repainted row per model and a permanent
//! verdict line per model as it settles. Non-TTY output falls back to one
//! plain line per turn.
//!
//! Deviations:
//! - Depends in TS on `@oh-my-pi/pi-utils`, theme/table/live-board/chrome
//!   helpers. This port implements pure formatting/rendering logic and a
//!   [`LiveBoardOutput`] trait plus an in-memory [`IfBenchBoard`] driver
//!   without terminal timers or theme objects.
//! - ANSI styling is represented with simple SGR helpers (`fg_*`) so output
//!   remains deterministic in tests; `theme.bold` intent preserved as bold SGR.

use std::collections::BTreeMap;

/// Failure category displayed in benchmark verdicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IfBenchFailure {
	Result,
	Cat,
	ResultCat,
	Format,
	Provider,
}

impl IfBenchFailure {
	pub fn as_key(&self) -> &'static str {
		match self {
			IfBenchFailure::Result => "result",
			IfBenchFailure::Cat => "cat",
			IfBenchFailure::ResultCat => "result+cat",
			IfBenchFailure::Format => "format",
			IfBenchFailure::Provider => "provider",
		}
	}
}

/// Human-readable verdict for each scored failure mode.
pub fn failure_text(f: IfBenchFailure) -> &'static str {
	match f {
		IfBenchFailure::Result => "wrong array",
		IfBenchFailure::Cat => "no cat sound",
		IfBenchFailure::ResultCat => "wrong array + no cat sound",
		IfBenchFailure::Format => "no <result> block",
		IfBenchFailure::Provider => "provider error",
	}
}

/// Scored turn fields consumed by the live board and failure details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfBenchTurnRecord {
	pub turn: u32,
	pub cumulative_actions: u32,
	pub placement: String,
	pub duration_ms: u64,
	pub passed: bool,
	pub failure: Option<IfBenchFailure>,
	pub expected: String,
	pub response: String,
}

/// Per-model scoreboard and verdict fields.
#[derive(Clone, Debug, PartialEq)]
pub struct IfBenchModelReport {
	pub label: String,
	pub turns: Vec<IfBenchTurnRecord>,
	pub turns_passed: u32,
	pub actions_passed: u32,
	pub failure: Option<(u32, IfBenchFailure)>,
	pub duration_ms: u64,
	pub output_tokens: u64,
	pub cost: f64,
}

/// Benchmark results consumed by the ranked scoreboard.
#[derive(Clone, Debug, PartialEq)]
pub struct IfBenchSummary {
	pub max_turns: u32,
	pub models: Vec<IfBenchModelReport>,
}

/// Optional live-progress callbacks accepted by a benchmark runner.
pub trait IfBenchObserver {
	fn model_started(&mut self, _label: &str) {}
	fn turn_started(&mut self, _label: &str, _turn: u32, _actions: u32) {}
	fn turn_finished(&mut self, _label: &str, _record: &IfBenchTurnRecord) {}
	fn model_finished(&mut self, _report: &IfBenchModelReport) {}
}

/// Rendering surface for one `omp if-bench` run.
pub trait IfBenchBoard: IfBenchObserver {
	fn interactive(&self) -> bool;
	/// Print a permanent line above the live rows (plain write when non-TTY).
	fn log(&mut self, text: &str);
	/// Clear the live area and restore the cursor. Idempotent.
	fn close(&mut self);
}

pub trait LiveBoardOutput {
	fn is_tty(&self) -> bool;
	fn write(&mut self, text: &str);
}

/// Static run parameters shown in the board header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfBenchBoardMeta {
	pub max_turns: u32,
	pub array_length: u32,
	pub nya_max: u32,
}

#[derive(Clone, Debug, Default)]
struct RowState {
	started_at_ms: u64,
	turn: u32,
	actions: u32,
	passed: u32,
	failed: bool,
	placement: String,
	_last_duration_ms: u64,
}

const LADDER_WIDTH: usize = 28;
const LABEL_WIDTH: usize = 34;
const DETAIL_WIDTH: usize = 120;

fn fg(code: &str, text: &str) -> String {
	format!("\x1b[{code}m{text}\x1b[0m")
}
fn fg_dim(t: &str) -> String { fg("2", t) }
fn fg_success(t: &str) -> String { fg("32", t) }
fn fg_warning(t: &str) -> String { fg("33", t) }
fn fg_error(t: &str) -> String { fg("31", t) }
fn fg_accent(t: &str) -> String { fg("36", t) }
fn bold(t: &str) -> String { fg("1", t) }

fn format_duration_ms(ms: u64) -> String {
	if ms < 1000 {
		format!("{ms}ms")
	} else if ms < 60_000 {
		let s = ms / 1000;
		let rem = (ms % 1000) / 100;
		if rem == 0 { format!("{s}s") } else { format!("{s}.{rem}s") }
	} else {
		let m = ms / 60_000;
		let s = (ms % 60_000) / 1000;
		if s == 0 { format!("{m}m") } else { format!("{m}m{s}s") }
	}
}

fn format_number(n: u64) -> String {
	let s = n.to_string();
	let mut out = String::new();
	for (i, c) in s.chars().rev().enumerate() {
		if i > 0 && i % 3 == 0 { out.push(','); }
		out.push(c);
	}
	out.chars().rev().collect()
}

fn replace_tabs(s: &str) -> String { s.replace('\t', "   ") }

fn truncate_to_width(s: &str, width: usize) -> String {
	if s.chars().count() <= width { s.to_string() }
	else { s.chars().take(width.saturating_sub(1)).collect::<String>() + "…" }
}

/// Turn meter ladder.
pub fn ladder(passed: u32, turn: u32, failed: bool, max_turns: u32, spinner: &str) -> String {
	let width = (max_turns as usize).min(LADDER_WIDTH).max(1);
	let scale = width as f64 / (max_turns.max(1) as f64);
	let filled = ((passed as f64) * scale).round() as usize;
	let filled = filled.min(width);
	let active = if failed || filled >= width {
		0
	} else {
		let a = scale.round() as usize;
		a.max(1).min(width - filled)
	};
	let rest = width.saturating_sub(filled + active);
	let mut s = String::new();
	s.push_str(&fg_success(&"█".repeat(filled)));
	if failed {
		s.push_str(&fg_error("▚"));
	} else {
		s.push_str(&fg_warning(&spinner.repeat(active)));
	}
	s.push_str(&fg_dim(&"░".repeat(rest)));
	s
}

pub fn format_turn_line(label: &str, record: &IfBenchTurnRecord) -> String {
	let head = format!(
		"[turn {}] {} {} acts cat@{}",
		record.turn, label, record.cumulative_actions, record.placement
	);
	if record.passed {
		format!("{head} PASS {}", format_duration_ms(record.duration_ms))
	} else {
		let kind = record.failure.map(failure_text).unwrap_or("format");
		format!("{head} FAIL {kind}")
	}
}

fn mean_turn_ms(report: &IfBenchModelReport) -> u64 {
	if report.turns.is_empty() { 0 } else { report.duration_ms / report.turns.len() as u64 }
}

fn format_cost(cost: f64) -> String {
	if cost >= 0.095 { format!("${cost:.2}") } else { format!("${cost:.3}") }
}

pub fn format_verdict(report: &IfBenchModelReport, meta: &IfBenchBoardMeta) -> String {
	let mut stats = vec![
		format!("{}/{} turns", report.turns_passed, meta.max_turns),
		format!("{} actions", report.actions_passed),
		format!("{}/turn", format_duration_ms(mean_turn_ms(report))),
	];
	if report.output_tokens > 0 { stats.push(format!("{} tok", format_number(report.output_tokens))); }
	if report.cost > 0.0 { stats.push(format_cost(report.cost)); }
	let body = format!("{} {}", bold(&report.label), stats.join(&fg_dim(" · ")));
	match report.failure {
		None => format!("{} {body}", fg_success("✓")),
		Some((turn, kind)) => format!(
			"{} {body} {} {}",
			fg_error("✗"),
			fg_dim("·"),
			fg_error(&format!("broke on turn {turn}: {}", failure_text(kind)))
		),
	}
}

/// Expected/actual pair for the turn that ended a run, clipped.
pub fn failure_detail(report: &IfBenchModelReport) -> Vec<String> {
	let Some(failed) = report.turns.last() else { return vec![]; };
	let one_line = |text: &str| truncate_to_width(&replace_tabs(text).split_whitespace().collect::<Vec<_>>().join(" "), DETAIL_WIDTH);
	if failed.failure == Some(IfBenchFailure::Provider) {
		return vec![format!("    {} {}", fg_dim("provider"), fg_error(&one_line(&failed.response)))];
	}
	vec![
		format!("    {} <{}>", fg_dim("expected"), failed.expected),
		format!("    {} {}", fg_dim("actual  "), one_line(&failed.response)),
	]
}

/// Ranked comparison table.
pub fn format_if_bench_scoreboard(summary: &IfBenchSummary) -> String {
	let mut ranked = summary.models.clone();
	ranked.sort_by(|a, b| {
		b.turns_passed
			.cmp(&a.turns_passed)
			.then(b.actions_passed.cmp(&a.actions_passed))
			.then(mean_turn_ms(a).cmp(&mean_turn_ms(b)))
	});
	let headers = ["model", "turns", "actions", "broke on", "per turn", "tokens", "cost"];
	let rows: Vec<Vec<String>> = ranked
		.iter()
		.map(|r| {
			vec![
				r.label.clone(),
				format!("{}/{}", r.turns_passed, summary.max_turns),
				r.actions_passed.to_string(),
				r.failure
					.map(|(t, k)| format!("turn {t} · {}", failure_text(k)))
					.unwrap_or_else(|| "survived".into()),
				format_duration_ms(mean_turn_ms(r)),
				if r.output_tokens > 0 { format_number(r.output_tokens) } else { "-".into() },
				if r.cost > 0.0 { format_cost(r.cost) } else { "-".into() },
			]
		})
		.collect();
	let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
	for row in &rows {
		for (i, c) in row.iter().enumerate() { widths[i] = widths[i].max(c.len()); }
	}
	let layout = |cells: &[String]| -> String {
		cells.iter().enumerate().map(|(i, c)| {
			if i == 0 { format!("{:<w$}", c, w = widths[i]) }
			else if matches!(i, 1|2|4|5|6) { format!("  {:>w$}", c, w = widths[i]) }
			else { format!("  {:<w$}", c, w = widths[i]) }
		}).collect::<String>().trim_end().to_string()
	};
	let mut lines = vec![fg_dim(&layout(&headers.iter().map(|s| s.to_string()).collect::<Vec<_>>()))];
	for (i, row) in rows.iter().enumerate() {
		let line = layout(row);
		if i == 0 && ranked[0].turns_passed > 0 { lines.push(fg_success(&line)); } else { lines.push(line); }
	}
	format!("{}\n", lines.join("\n"))
}

/// In-memory board driver for tests/hosts.
pub struct MemoryIfBenchBoard<O: LiveBoardOutput> {
	meta: IfBenchBoardMeta,
	output: O,
	rows: BTreeMap<String, RowState>,
	interactive: bool,
	closed: bool,
	logs: Vec<String>,
	now_ms: u64,
}

impl<O: LiveBoardOutput> MemoryIfBenchBoard<O> {
	pub fn new(meta: IfBenchBoardMeta, output: O) -> Self {
		let interactive = output.is_tty();
		Self { meta, output, rows: BTreeMap::new(), interactive, closed: false, logs: vec![], now_ms: 0 }
	}
	pub fn set_now_ms(&mut self, now: u64) { self.now_ms = now; }
	pub fn logs(&self) -> &[String] { &self.logs }
}

impl<O: LiveBoardOutput> IfBenchObserver for MemoryIfBenchBoard<O> {
	fn model_started(&mut self, label: &str) {
		self.rows.insert(label.to_string(), RowState { started_at_ms: self.now_ms, ..Default::default() });
	}
	fn turn_started(&mut self, label: &str, turn: u32, _actions: u32) {
		if let Some(r) = self.rows.get_mut(label) { r.turn = turn; }
	}
	fn turn_finished(&mut self, label: &str, record: &IfBenchTurnRecord) {
		if let Some(r) = self.rows.get_mut(label) {
			r.actions = record.cumulative_actions;
			r.placement = record.placement.clone();
			r._last_duration_ms = record.duration_ms;
			if record.passed { r.passed = record.turn; } else { r.failed = true; }
		}
		if !self.interactive {
			let line = format_turn_line(label, record);
			self.output.write(&format!("{line}\n"));
		}
	}
	fn model_finished(&mut self, report: &IfBenchModelReport) {
		self.rows.remove(&report.label);
		let v = format_verdict(report, &self.meta);
		self.log(&v);
		for line in failure_detail(report) { self.log(&line); }
	}
}

impl<O: LiveBoardOutput> IfBenchBoard for MemoryIfBenchBoard<O> {
	fn interactive(&self) -> bool { self.interactive }
	fn log(&mut self, text: &str) {
		self.logs.push(text.to_string());
		self.output.write(&format!("{text}\n"));
	}
	fn close(&mut self) { self.closed = true; }
}

pub use create_if_bench_board as createIfBenchBoard;
pub use format_if_bench_scoreboard as formatIfBenchScoreboard;

pub fn create_if_bench_board<O: LiveBoardOutput>(meta: IfBenchBoardMeta, output: O) -> MemoryIfBenchBoard<O> {
	MemoryIfBenchBoard::new(meta, output)
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Out(bool, Vec<String>);
	impl LiveBoardOutput for Out {
		fn is_tty(&self) -> bool { self.0 }
		fn write(&mut self, text: &str) { self.1.push(text.to_string()); }
	}

	fn rec(turn: u32, passed: bool, failure: Option<IfBenchFailure>) -> IfBenchTurnRecord {
		IfBenchTurnRecord {
			turn, cumulative_actions: turn * 2, placement: "3".into(), duration_ms: 1000,
			passed, failure, expected: "<[1]>".into(), response: if passed { "<[1]>".into() } else { "bad".into() },
		}
	}
	fn report(label: &str, turns_passed: u32, failure: Option<(u32, IfBenchFailure)>) -> IfBenchModelReport {
		IfBenchModelReport {
			label: label.into(), turns: vec![], turns_passed, actions_passed: turns_passed * 2,
			failure, duration_ms: 2000, output_tokens: 100, cost: 0.01,
		}
	}

	#[test]
	fn failure_texts_match_reference() {
		assert_eq!(failure_text(IfBenchFailure::Result), "wrong array");
		assert_eq!(failure_text(IfBenchFailure::Cat), "no cat sound");
		assert_eq!(failure_text(IfBenchFailure::ResultCat), "wrong array + no cat sound");
		assert_eq!(failure_text(IfBenchFailure::Format), "no <result> block");
		assert_eq!(failure_text(IfBenchFailure::Provider), "provider error");
	}

	#[test]
	fn ladder_renders_cells() {
		let s = ladder(2, 3, false, 5, "◐");
		assert!(s.contains("█"));
		assert!(s.contains("░"));
	}

	#[test]
	fn format_turn_line_pass_and_fail() {
		assert!(format_turn_line("m", &rec(1, true, None)).contains("PASS"));
		assert!(format_turn_line("m", &rec(1, false, Some(IfBenchFailure::Format))).contains("FAIL"));
	}

	#[test]
	fn verdict_and_scoreboard() {
		let meta = IfBenchBoardMeta { max_turns: 5, array_length: 3, nya_max: 2 };
		let ok = report("A", 5, None);
		let bad = report("B", 2, Some((3, IfBenchFailure::Result)));
		let v_ok = format_verdict(&ok, &meta);
		let v_bad = format_verdict(&bad, &meta);
		assert!(v_ok.contains("✓"));
		assert!(v_bad.contains("broke on turn 3"));
		let sum = IfBenchSummary { max_turns: 5, models: vec![bad, ok] };
		let table = format_if_bench_scoreboard(&sum);
		assert!(table.contains("model"));
		assert!(table.lines().next().unwrap_or("").contains("model"));
	}

	#[test]
	fn non_tty_board_writes_turn_lines() {
		let meta = IfBenchBoardMeta { max_turns: 3, array_length: 2, nya_max: 1 };
		let out = Out(false, vec![]);
		let mut b = create_if_bench_board(meta, out);
		b.model_started("m");
		b.turn_finished("m", &rec(1, true, None));
		assert!(b.interactive() == false);
	}
}
