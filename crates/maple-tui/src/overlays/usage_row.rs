//! Per-turn usage row/block helpers.
//!
//! Reference: `l/src/overlays/usage-row.ts`.
//!
//! Deviations:
//! - `@oh-my-pi/pi-ai` `Usage`/`AssistantMessage` types and TUI container/
//!   metrics row components are not ported. This module provides the portable
//!   logic: duration/throughput formatting, turn usage tally, summary text/title,
//!   and metric specs as data.
//! - `formatUsageTimestamp` uses a simple UTC-style formatter from epoch ms
//!   instead of locale `Intl.DateTimeFormat` (locale APIs not available).
//! - Rendering (`UsageRowBlock`, `createUsageRowBlock`, `isUsageRowBlock`) is not
//!   implemented as a transcript component because the crate's message/chrome
//!   block APIs differ; callers can build rows from [`format_turn_usage`] and
//!   [`usage_row_specs`].

/// Below this the rate is nonsense (cached/instant responses yield absurd tok/s).
pub const MIN_DURATION_MS: i64 = 100;

/// Token/cost usage for one request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
	pub input: u64,
	pub output: u64,
	pub cache_read: u64,
	pub cache_write: u64,
	pub total_tokens: u64,
	pub cost_total: f64,
}

impl From<&crate::chat::messages::Usage> for Usage {
	fn from(usage: &crate::chat::messages::Usage) -> Self {
		Self {
			input: usage.input,
			output: usage.output,
			cache_read: usage.cache_read,
			cache_write: usage.cache_write,
			total_tokens: usage.total_tokens,
			cost_total: 0.0,
		}
	}
}

/// A finished turn's totals.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnUsageSummary {
	pub requests: usize,
	pub total_tokens: u64,
	pub input: u64,
	pub output: u64,
	pub cache_read: u64,
	pub cache_write: u64,
	pub cost: f64,
	pub duration_ms: Option<i64>,
}

/// Prompt→yield wall time for a turn.
#[must_use]
pub fn turn_elapsed_ms(turn_started_at: Option<i64>, completed_at: Option<i64>) -> Option<i64> {
	let start = turn_started_at?;
	let end = completed_at?;
	Some((end - start).max(0))
}

/// Local-like ISO timestamp (UTC fallback, no locale APIs).
#[must_use]
pub fn format_usage_timestamp(ms: i64, _hour12: bool) -> String {
	// Simple UTC YYYY-MM-DD HH:MM:SS rendering from epoch ms.
	let secs = (ms / 1000).max(0);
	let days = secs / 86_400;
	let rem = secs % 86_400;
	let hh = rem / 3_600;
	let mm = (rem % 3_600) / 60;
	let ss = rem % 60;
	let (year, month, day) = civil_from_days(days);
	format!("{year:04}-{month:02}-{day:02} {hh:02}:{mm:02}:{ss:02}")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
	let z = days + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
	let y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = mp + if mp < 10 { 3 } else { -9 };
	let year = y + if m <= 2 { 1 } else { 0 };
	(year, m as u32, d as u32)
}

/// Sums requests of a turn and emits summary on turn end.
#[derive(Clone, Debug, Default)]
pub struct TurnUsageTally {
	summary: Option<TurnUsageSummary>,
}

impl TurnUsageTally {
	#[must_use]
	pub fn new() -> Self {
		Self { summary: None }
	}

	/// Add a request; returns turn summary when the run ends (`stop != "toolUse"`).
	pub fn add(
		&mut self,
		usage: &Usage,
		stop: &str,
		request_duration_ms: Option<i64>,
		turn_elapsed: Option<i64>,
	) -> Option<TurnUsageSummary> {
		let s = self.summary.get_or_insert_with(TurnUsageSummary::default);
		s.requests += 1;
		s.total_tokens = s.total_tokens.saturating_add(usage.total_tokens);
		s.input = s.input.saturating_add(usage.input);
		s.output = s.output.saturating_add(usage.output);
		s.cache_read = s.cache_read.saturating_add(usage.cache_read);
		s.cache_write = s.cache_write.saturating_add(usage.cache_write);
		s.cost += usage.cost_total;
		if let Some(d) = request_duration_ms {
			s.duration_ms = Some(s.duration_ms.unwrap_or(0).saturating_add(d));
		}
		if stop == "toolUse" {
			return None;
		}
		if let Some(elapsed) = turn_elapsed {
			s.duration_ms = Some(elapsed);
		}
		self.summary.take()
	}
}

fn usage_throughput(usage: &Usage, duration_ms: Option<i64>) -> Option<f64> {
	let duration = duration_ms?;
	if duration < MIN_DURATION_MS || usage.output == 0 {
		return None;
	}
	Some(usage.output as f64 / duration as f64 * 1000.0)
}

/// Metric kind for a usage row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetricKind {
	Text,
	Rate,
}

/// One metric spec in a usage row.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricSpec {
	pub key: String,
	pub kind: MetricKind,
	pub label: String,
	pub text: String,
	pub title: Option<String>,
}

/// Build base metric specs (no throughput).
#[must_use]
pub fn usage_row_base_specs(
	usage: &Usage,
	ttft_ms: Option<i64>,
	timestamp_ms: Option<i64>,
	turn_elapsed: Option<i64>,
) -> Vec<MetricSpec> {
	let mut specs = Vec::new();
	if let Some(t) = timestamp_ms {
		specs.push(MetricSpec {
			key: "ts".into(),
			kind: MetricKind::Text,
			label: "Time".into(),
			text: format_usage_timestamp(t, false),
			title: None,
		});
	}
	if let Some(elapsed) = turn_elapsed {
		specs.push(MetricSpec {
			key: "dur".into(),
			kind: MetricKind::Text,
			label: "Turn".into(),
			text: format!("{:.1}s", elapsed as f64 / 1000.0),
			title: None,
		});
	}
	specs.push(MetricSpec {
		key: "tok".into(),
		kind: MetricKind::Text,
		label: "Tokens".into(),
		text: format!("{}", usage.total_tokens),
		title: Some(format!(
			"in {} · out {} · cache r {} / w {}",
			usage.input, usage.output, usage.cache_read, usage.cache_write
		)),
	});
	if usage.cost_total > 0.0 {
		specs.push(MetricSpec {
			key: "cost".into(),
			kind: MetricKind::Text,
			label: "Cost".into(),
			text: format!("${:.4}", usage.cost_total),
			title: None,
		});
	}
	if let Some(ttft) = ttft_ms {
		specs.push(MetricSpec {
			key: "ttft".into(),
			kind: MetricKind::Text,
			label: "TTFT".into(),
			text: format!("{ttft}ms"),
			title: None,
		});
	}
	specs
}

/// Build metric specs including throughput rate when meaningful.
#[must_use]
pub fn usage_row_specs(
	usage: &Usage,
	duration_ms: Option<i64>,
	ttft_ms: Option<i64>,
	timestamp_ms: Option<i64>,
	turn_elapsed: Option<i64>,
) -> Vec<MetricSpec> {
	let mut specs = usage_row_base_specs(usage, ttft_ms, timestamp_ms, turn_elapsed);
	if let Some(rate) = usage_throughput(usage, duration_ms) {
		specs.push(MetricSpec {
			key: "rate".into(),
			kind: MetricKind::Rate,
			label: "Rate".into(),
			text: format!("{rate:.1} tok/s"),
			title: None,
		});
	}
	specs
}

/// Native turn line text and tooltip.
#[must_use]
pub fn format_turn_usage(summary: &TurnUsageSummary) -> (String, String) {
	let mut parts = Vec::new();
	if let Some(ms) = summary.duration_ms {
		parts.push(format!("{:.1}s", ms as f64 / 1000.0));
	}
	parts.push(format!("{} tok", summary.total_tokens));
	if summary.cost > 0.0 {
		parts.push(format!("${:.2} this turn", summary.cost));
	}
	let text = parts.join(" · ");
	let title = format!(
		"{} request(s) · in {} · out {} · cache r {} / w {} · ${:.4}",
		summary.requests,
		summary.input,
		summary.output,
		summary.cache_read,
		summary.cache_write,
		summary.cost
	);
	(text, title)
}

/// Format metrics shared by standalone usage blocks.
#[must_use]
pub fn format_usage_row(
	usage: &Usage,
	duration_ms: Option<i64>,
	ttft_ms: Option<i64>,
	timestamp_ms: Option<i64>,
	turn_elapsed: Option<i64>,
) -> String {
	usage_row_specs(usage, duration_ms, ttft_ms, timestamp_ms, turn_elapsed)
		.into_iter()
		.map(|s| match s.kind {
			MetricKind::Rate => s.text,
			MetricKind::Text => format!("{} {}", s.label.to_lowercase(), s.text),
		})
		.collect::<Vec<_>>()
		.join(" · ")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn turn_elapsed_ms_clamps_negative_to_zero() {
		assert_eq!(turn_elapsed_ms(Some(10), Some(5)), Some(0));
		assert_eq!(turn_elapsed_ms(None, Some(5)), None);
	}

	#[test]
	fn usage_throughput_requires_min_duration_and_output() {
		let u = Usage { output: 100, ..Default::default() };
		assert!(usage_throughput(&u, Some(50)).is_none());
		assert!(usage_throughput(&u, Some(200)).is_some());
		assert!(usage_throughput(&Usage::default(), Some(200)).is_none());
	}

	#[test]
	fn turn_usage_tally_returns_summary_on_non_tool_use() {
		let mut tally = TurnUsageTally::new();
		let usage = Usage { total_tokens: 10, input: 3, output: 7, cost_total: 0.01, ..Default::default() };
		assert!(tally.add(&usage, "toolUse", Some(50), None).is_none());
		let summary = tally.add(&usage, "stop", Some(50), Some(120)).unwrap();
		assert_eq!(summary.requests, 2);
		assert_eq!(summary.total_tokens, 20);
		assert_eq!(summary.duration_ms, Some(120));
	}

	#[test]
	fn format_turn_usage_includes_duration_and_cost() {
		let s = TurnUsageSummary { requests: 1, total_tokens: 19_000, cost: 0.02, duration_ms: Some(12_400), ..Default::default() };
		let (text, title) = format_turn_usage(&s);
		assert!(text.contains("12.4s"));
		assert!(text.contains("19000 tok"));
		assert!(text.contains("$0.02 this turn"));
		assert!(title.contains("1 request"));
	}

	#[test]
	fn format_usage_row_contains_metrics() {
		let usage = Usage { total_tokens: 42, input: 10, output: 32, cost_total: 0.0, ..Default::default() };
		let row = format_usage_row(&usage, Some(500), Some(20), None, None);
		assert!(row.contains("tokens 42"));
		assert!(row.contains("ttft 20ms"));
		assert!(row.contains("tok/s"));
	}
}
