//! Autoresearch/experiment tool render-model types and helpers.
//!
//! Reference: `l/src/tools/autoresearch.ts`.
//!
//! Deviations:
//! - TS renders live TUI components/native views with `Theme`, `Text`,
//!   `ansi/kv/row/badge/span`, `OwnerMemo`, truncation/file-path formatting and
//!   streaming progress. Rust ports data types, numeric formatting, and plain/
//!   native describe/render helpers (init/log/run/update-notes). Live component
//!   rendering, memoization, and spinner/timer behavior are host concerns for
//!   renderer-completion pass.
//! - `TruncationResult` and path shortening are represented by local/simple
//!   fields/helpers where crate-specific types are not required by callers.
//! - `ASIValue`/JSON-like payloads use `serde_json::Value`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::native::describe::{ansi, compact, kv, node, row, span as nspan, text as ntext};
use crate::native::node::{NodeKind, NodeText, Props, Span, Tone};
use crate::tools::native_view::{error_view, result_text, tool_head, NativePreview, NativeToolView, ToolHeadDetail};

/// Whether a lower or higher metric is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricDirection {
	Lower,
	Higher,
}

/// Disposition of a completed experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
	Keep,
	Discard,
	Crash,
	ChecksFailed,
}

impl ExperimentStatus {
	/// String label.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Keep => "keep",
			Self::Discard => "discard",
			Self::Crash => "crash",
			Self::ChecksFailed => "checks_failed",
		}
	}
	/// Uppercase badge label.
	pub fn badge(self) -> &'static str {
		match self {
			Self::Keep => "KEEP",
			Self::Discard => "DISCARD",
			Self::Crash => "CRASH",
			Self::ChecksFailed => "CHECKS_FAILED",
		}
	}
	/// Native/TUI tone token.
	pub fn tone(self) -> Tone {
		match self {
			Self::Keep => Tone::Success,
			Self::Discard => Tone::Warning,
			Self::Crash | Self::ChecksFailed => Tone::Error,
		}
	}
}

/// JSON-compatible additional structured experiment information.
pub type ASIValue = Value;
/// Named structured experiment observations.
pub type ASIData = BTreeMap<String, Value>;
/// Numeric metrics indexed by name.
pub type NumericMetricMap = BTreeMap<String, f64>;

/// Name and unit of an experiment metric.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricDef {
	pub name: String,
	pub unit: String,
}

/// Recorded metric and disposition of an experiment run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentResult {
	pub run_number: Option<i64>,
	pub commit: String,
	pub metric: f64,
	#[serde(default)]
	pub metrics: NumericMetricMap,
	pub status: ExperimentStatus,
	pub description: String,
	pub timestamp: i64,
	pub segment: i64,
	pub confidence: Option<f64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub asi: Option<ASIData>,
	#[serde(default)]
	pub modified_paths: Vec<String>,
	#[serde(default)]
	pub scope_deviations: Vec<String>,
	pub justification: Option<String>,
	#[serde(default)]
	pub flagged: bool,
	pub flagged_reason: Option<String>,
}

/// Current experiment baseline, history, and scope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentState {
	#[serde(default)]
	pub results: Vec<ExperimentResult>,
	pub best_metric: Option<f64>,
	pub best_direction: MetricDirection,
	pub metric_name: String,
	pub metric_unit: String,
	#[serde(default)]
	pub secondary_metrics: Vec<MetricDef>,
	pub name: Option<String>,
	pub goal: Option<String>,
	pub current_segment: i64,
	pub max_experiments: Option<i64>,
	pub confidence: Option<f64>,
	#[serde(default)]
	pub scope_paths: Vec<String>,
	#[serde(default)]
	pub off_limits: Vec<String>,
	#[serde(default)]
	pub constraints: Vec<String>,
	#[serde(default)]
	pub notes: String,
	pub branch: Option<String>,
	pub baseline_commit: Option<String>,
	pub session_id: Option<i64>,
}

/// Live elapsed time and output metadata of a benchmark.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunExperimentProgressDetails {
	#[serde(default)]
	pub phase: String,
	#[serde(default)]
	pub elapsed: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub truncation: Option<Value>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub full_output_path: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub run_directory: Option<String>,
}

/// Completed benchmark measurements, status, and output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunDetails {
	pub run_number: i64,
	pub run_directory: String,
	pub benchmark_log_path: String,
	pub command: String,
	pub exit_code: Option<i32>,
	pub duration_seconds: f64,
	pub passed: bool,
	pub crashed: bool,
	pub timed_out: bool,
	#[serde(default)]
	pub tail_output: String,
	pub parsed_metrics: Option<NumericMetricMap>,
	pub parsed_primary: Option<f64>,
	pub parsed_asi: Option<ASIData>,
	pub metric_name: String,
	pub metric_unit: String,
	#[serde(default)]
	pub pre_run_dirty_paths: Vec<String>,
	pub abandoned_prior_run: Option<i64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub truncation: Option<Value>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub full_output_path: Option<String>,
}

/// Logged experiment result and updated baseline state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogDetails {
	pub experiment: ExperimentResult,
	pub state: ExperimentState,
	pub wall_clock_seconds: Option<f64>,
	#[serde(default)]
	pub scope_deviations: Vec<String>,
	pub justification: Option<String>,
	#[serde(default)]
	pub flagged_runs: Vec<FlaggedRun>,
}

/// Flagged run entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlaggedRun {
	pub run_id: i64,
	pub reason: String,
}

/// Initialization outcome and experiment state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InitExperimentDetails {
	pub state: ExperimentState,
	pub created_session: bool,
	pub bumped_segment: bool,
	pub abandoned_runs: i64,
	pub harness_committed: bool,
	pub baseline_commit: Option<String>,
}

/// Persisted experiment notes after an update.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateNotesDetails {
	pub notes: String,
}

/// Filename of the benchmark harness.
pub const HARNESS_FILENAME: &str = "autoresearch.sh";
/// Default command that executes the benchmark harness.
pub const DEFAULT_HARNESS_COMMAND: &str = "bash autoresearch.sh";

/// Format an integer with comma-separated digit groups.
pub fn commas(value: i64) -> String {
	let sign = if value < 0 { "-" } else { "" };
	let digits = value.abs().to_string();
	let mut groups = Vec::new();
	let bytes = digits.as_bytes();
	let len = bytes.len();
	let mut i = len;
	while i > 0 {
		let start = i.saturating_sub(3);
		groups.push(&digits[start..i]);
		i = start;
	}
	groups.reverse();
	format!("{sign}{}", groups.join(","))
}

/// Format a number with grouped digits and optional decimals.
pub fn fmt_num(value: f64, decimals: usize) -> String {
	if decimals == 0 {
		return commas(value.round() as i64);
	}
	let abs = value.abs();
	let whole = abs.trunc() as i64;
	let frac = format!("{:.*}", decimals, abs - abs.trunc());
	let frac_part = frac.strip_prefix('0').unwrap_or(&frac);
	format!("{}{}{}", if value < 0.0 { "-" } else { "" }, commas(whole), frac_part)
}

/// Format an optional metric value with its unit.
pub fn format_num(value: Option<f64>, unit: &str) -> String {
	match value {
		None => "-".to_string(),
		Some(v) if (v.fract() == 0.0 && v.is_finite()) => format!("{}{}", fmt_num(v, 0), unit),
		Some(v) => format!("{}{}", fmt_num(v, 2), unit),
	}
}

/// Plain first line of a free-form argument.
pub fn first_line(value: Option<&str>) -> String {
	value
		.unwrap_or("")
		.trim()
		.lines()
		.next()
		.unwrap_or("")
		.to_string()
}

fn shorten_path_like(path: &str) -> String {
	if let Some(home) = std::env::var_os("HOME") {
		let home = home.to_string_lossy();
		if let Some(rest) = path.strip_prefix(home.as_ref()) {
			return format!("~{rest}");
		}
	}
	path.to_string()
}

/// Plain init call line.
pub fn render_init_call_line(name: &str) -> String {
	format!("init_experiment {}", replace_tabs(first_line(Some(name)).as_str()))
}

/// Plain init describe.
pub fn describe_init_call(name: &str) -> NativeToolView {
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(
			"init_experiment",
			&[ToolHeadDetail::Span(Span::styled(first_line(Some(name)), "accent"))],
		))),
		..NativeToolView::default()
	}
}

/// Plain log summary text.
pub fn render_log_summary_plain(details: &LogDetails) -> String {
	let exp = &details.experiment;
	let state = &details.state;
	let mut summary = format!(
		"{} {}",
		exp.status.badge(),
		replace_tabs(&exp.description)
	);
	summary.push_str(&format!(" {}={}", state.metric_name, format_num(Some(exp.metric), &state.metric_unit)));
	if let Some(best) = state.best_metric {
		summary.push_str(&format!(" baseline {}", format_num(Some(best), &state.metric_unit)));
	}
	if let Some(conf) = state.confidence {
		summary.push_str(&format!(" conf {conf:.1}x"));
	}
	if !details.scope_deviations.is_empty() {
		summary.push_str(&format!(" deviations:{}", details.scope_deviations.len()));
	}
	summary
}

/// Native log describe summary.
pub fn describe_log_summary(details: &LogDetails) -> NativeToolView {
	let exp = &details.experiment;
	let state = &details.state;
	let tone = exp.status.tone();
	let mut badge_props = Props::new();
	badge_props.set_str("text", exp.status.badge());
	badge_props.set_str("tone", match tone {
		Tone::Success => "success",
		Tone::Warning => "warning",
		_ => "error",
	});
	let badge = node(NodeKind::Badge, badge_props, Vec::new(), None);
	let desc_text = ntext(NodeText::Spans(vec![Span::styled(exp.description.clone(), "muted")]), {
		let mut p = Props::new();
		p.set_str("wrap", "word");
		p.set_bool("grow", true);
		p
	});
	let top = row(vec![badge, desc_text], {
		let mut p = Props::new();
		p.set_str("gap", "sm");
		p
	});
	let mut pairs: Vec<(String, NodeText)> = Vec::new();
	pairs.push((state.metric_name.clone(), NodeText::Spans(vec![Span::styled(format_num(Some(exp.metric), &state.metric_unit), "accent num")])));
	if let Some(best) = state.best_metric {
		pairs.push(("baseline".into(), NodeText::Spans(vec![Span::styled(format_num(Some(best), &state.metric_unit), "num")])));
	}
	if let Some(conf) = state.confidence {
		pairs.push(("confidence".into(), NodeText::Spans(vec![Span::styled(format!("{conf:.1}x"), "num")])));
	}
	if !details.scope_deviations.is_empty() {
		pairs.push(("deviations".into(), NodeText::Spans(vec![Span::styled(details.scope_deviations.len().to_string(), "warning")])));
	}
	let mut body = vec![top];
	if let Some(grid) = kv(pairs) {
		body.push(grid);
	}
	NativeToolView { tone: Some(tone), body: Some(body), ..NativeToolView::default() }
}

/// Describe log call.
pub fn describe_log_call(status: Option<ExperimentStatus>, description: Option<&str>) -> NativeToolView {
	let mut details = Vec::new();
	if let Some(s) = status {
		details.push(ToolHeadDetail::Span(Span::styled(s.as_str().to_string(), match s.tone() {
			Tone::Success => "success",
			Tone::Warning => "warning",
			_ => "error",
		})));
	}
	if let Some(d) = description.map(|s| first_line(Some(s))).filter(|s| !s.is_empty()) {
		details.push(ToolHeadDetail::Span(Span::styled(d, "muted")));
	}
	NativeToolView { head: Some(NodeText::Spans(tool_head("log_experiment", &details))), ..NativeToolView::default() }
}

/// Describe log result.
pub fn describe_log_result(is_error: bool, output: &str, details: Option<&LogDetails>) -> NativeToolView {
	if is_error || details.is_none() {
		return describe_text_result("log_experiment", is_error, output, None);
	}
	describe_log_summary(details.unwrap())
}

/// Run status summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunStatusSummary {
	pub tone: Tone,
	pub label: String,
	pub meta: String,
}

/// Describe run status.
pub fn describe_run_status(details: &RunDetails) -> RunStatusSummary {
	let duration = format!("{:.1}s", details.duration_seconds);
	if details.timed_out {
		return RunStatusSummary { tone: Tone::Error, label: "TIMEOUT".into(), meta: duration };
	}
	if details.exit_code != Some(0) {
		return RunStatusSummary {
			tone: Tone::Error,
			label: "FAIL".into(),
			meta: format!("exit={} {}", details.exit_code.unwrap_or(-1), duration),
		};
	}
	let meta = if let Some(primary) = details.parsed_primary {
		format!("{duration} {}={}", details.metric_name, format_num(Some(primary), &details.metric_unit))
	} else {
		duration
	};
	RunStatusSummary { tone: Tone::Success, label: "PASS".into(), meta }
}

/// Plain run status line.
pub fn render_run_status_plain(details: &RunDetails) -> String {
	if details.timed_out {
		return format!("TIMEOUT {:.1}s", details.duration_seconds);
	}
	if details.exit_code != Some(0) {
		return format!("FAIL exit={} {:.1}s", details.exit_code.unwrap_or(-1), details.duration_seconds);
	}
	let metric = details
		.parsed_primary
		.map(|p| format!(" {}={}", details.metric_name, format_num(Some(p), &details.metric_unit)))
		.unwrap_or_default();
	format!("PASS {:.1}s{metric}", details.duration_seconds)
}

/// Native run status node/view helper.
pub fn describe_run_status_node(details: &RunDetails) -> crate::native::node::NativeNode {
	let s = describe_run_status(details);
	let tone_str = match s.tone {
		Tone::Error => "error",
		_ => "success",
	};
	let mut bp = Props::new();
	bp.set_str("text", &s.label);
	bp.set_str("tone", tone_str);
	let badge = node(NodeKind::Badge, bp, Vec::new(), None);
	let mut tp = Props::new();
	tp.set_str("truncate", "end");
	let txt = ntext(NodeText::Spans(vec![Span::styled(s.meta, tone_str)]), tp);
	let mut rp = Props::new();
	rp.set_str("gap", "sm");
	row(vec![badge, txt], rp)
}

/// Describe run call.
pub fn describe_run_call() -> NativeToolView {
	NativeToolView {
		head: Some(NodeText::Spans(tool_head(
			"run_experiment",
			&[ToolHeadDetail::Span(Span::styled(DEFAULT_HARNESS_COMMAND.to_string(), "muted"))],
		))),
		..NativeToolView::default()
	}
}

/// Describe run progress.
pub fn describe_run_progress(elapsed: &str, output: &str) -> NativeToolView {
	let mut body = Vec::new();
	let mut spinner_p = Props::new();
	spinner_p.set_str("label", &format!("Running {elapsed}…"));
	body.push(node(NodeKind::Spinner, spinner_p, Vec::new(), None));
	if !output.trim().is_empty() {
		let mut p = Props::new();
		p.set_bool("follow", true);
		body.push(ansi(output, p));
	}
	NativeToolView { tone: Some(Tone::Warning), preview: Some(NativePreview::Lines(6)), body: Some(body), ..NativeToolView::default() }
}

/// Describe run result.
pub fn describe_run_result(is_error: bool, output: &str, details: Option<&RunDetails>, progress: Option<&RunExperimentProgressDetails>) -> NativeToolView {
	if let Some(p) = progress.filter(|p| p.phase == "running") {
		return describe_run_progress(&p.elapsed, output);
	}
	if is_error || details.is_none() {
		return describe_text_result("run_experiment", is_error, output, None);
	}
	let details = details.unwrap();
	let status = describe_run_status_node(details);
	let mut body = vec![status];
	if !details.tail_output.trim().is_empty() {
		let mut p = Props::new();
		p.set_bool("follow", true);
		body.push(ansi(details.tail_output.clone(), p));
	}
	if details.truncation.is_some() {
		if let Some(path) = &details.full_output_path {
			body.push(ntext(NodeText::Spans(vec![
				Span::styled("Full output: ".to_string(), "warning"),
				Span::styled(shorten_path_like(path), "path"),
			]), {
				let mut p = Props::new();
				p.set_str("truncate", "middle");
				p
			}));
		}
	}
	NativeToolView { tone: Some(describe_run_status(details).tone), preview: Some(NativePreview::Lines(6)), body: Some(body), ..NativeToolView::default() }
}

/// Describe update-notes call.
pub fn describe_update_notes_call(preview: Option<&str>) -> NativeToolView {
	let details = preview
		.map(|s| first_line(Some(s)))
		.filter(|s| !s.is_empty())
		.map(|s| vec![ToolHeadDetail::Span(Span::styled(s, "muted"))])
		.unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("update_notes", &details))), ..NativeToolView::default() }
}

/// Generic text result describe (error card or wrapped body).
pub fn describe_text_result(title: &str, is_error: bool, output: &str, style: Option<&str>) -> NativeToolView {
	let body_text = output.trim_end().to_string();
	if is_error {
		return NativeToolView { ..error_view(title, if body_text.is_empty() { "failed" } else { &body_text }, &[]) };
	}
	if body_text.is_empty() {
		return NativeToolView::default();
	}
	let spans = if let Some(s) = style {
		vec![Span::styled(body_text, s)]
	} else {
		vec![Span::new(body_text)]
	};
	let mut p = Props::new();
	p.set_str("wrap", "word");
	NativeToolView { body: Some(vec![ntext(NodeText::Spans(spans), p)]), ..NativeToolView::default() }
}

fn replace_tabs(s: &str) -> String {
	s.replace('\t', " ")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn commas_and_fmt() {
		assert_eq!(commas(1234567), "1,234,567");
		assert_eq!(commas(-12), "-12");
		assert_eq!(fmt_num(1234.0, 0), "1,234");
		assert!(fmt_num(1234.56, 2).starts_with("1,234."));
		assert_eq!(format_num(None, "ms"), "-");
		assert_eq!(format_num(Some(10.0), "ms"), "10ms");
	}

	#[test]
	fn status_and_tone() {
		assert_eq!(ExperimentStatus::Keep.tone(), Tone::Success);
		assert_eq!(ExperimentStatus::Discard.badge(), "DISCARD");
	}

	#[test]
	fn first_line_helper() {
		assert_eq!(first_line(Some("a\nb")), "a");
		assert_eq!(first_line(None), "");
	}

	#[test]
	fn render_run_status_plain_variants() {
		let base = RunDetails {
			run_number: 1,
			run_directory: "d".into(),
			benchmark_log_path: "b".into(),
			command: "c".into(),
			exit_code: Some(0),
			duration_seconds: 1.2,
			passed: true,
			crashed: false,
			timed_out: false,
			tail_output: String::new(),
			parsed_metrics: None,
			parsed_primary: Some(3.0),
			parsed_asi: None,
			metric_name: "score".into(),
			metric_unit: "pt".into(),
			pre_run_dirty_paths: vec![],
			abandoned_prior_run: None,
			truncation: None,
			full_output_path: None,
		};
		let s = render_run_status_plain(&base);
		assert!(s.contains("PASS"));
		assert!(s.contains("score=3pt"));
	}

	#[test]
	fn describe_helpers_build() {
		let call = describe_init_call("exp\nmore");
		assert!(call.head.is_some());
		let rc = describe_run_call();
		assert!(rc.head.is_some());
		let un = describe_update_notes_call(Some("note\nx"));
		assert!(un.head.is_some());
		let p = describe_run_progress("2s", "out");
		assert_eq!(p.tone, Some(Tone::Warning));
	}
}
