//! Pure autoresearch experiment data helpers.
//!
//! Reference: `l/src/apps/autoresearch-data.ts`.

use crate::tools::autoresearch::{ExperimentResult, MetricDef, MetricDirection, NumericMetricMap};

/// Format an experiment duration for display.
#[must_use]
pub fn format_elapsed(milliseconds: u64) -> String {
	let total_seconds = milliseconds / 1000;
	let minutes = total_seconds / 60;
	let seconds = total_seconds % 60;
	if minutes > 0 {
		format!("{minutes}m {seconds:02}s")
	} else {
		format!("{seconds}s")
	}
}

/// Compare metrics using the configured optimization direction.
#[must_use]
pub fn is_better(current: f64, best: f64, direction: MetricDirection) -> bool {
	match direction {
		MetricDirection::Lower => current < best,
		MetricDirection::Higher => current > best,
	}
}

/// Select results belonging to the active experiment segment.
#[must_use]
pub fn current_results(results: &[ExperimentResult], segment: i64) -> Vec<&ExperimentResult> {
	results.iter().filter(|result| result.segment == segment).collect()
}

/// Find the first unflagged kept experiment in a segment.
#[must_use]
pub fn find_baseline_result(results: &[ExperimentResult], segment: i64) -> Option<&ExperimentResult> {
	current_results(results, segment)
		.into_iter()
		.find(|result| result.status == crate::tools::autoresearch::ExperimentStatus::Keep && !result.flagged)
}

/// Read the baseline primary metric for a segment.
#[must_use]
pub fn find_baseline_metric(results: &[ExperimentResult], segment: i64) -> Option<f64> {
	find_baseline_result(results, segment).map(|result| result.metric)
}

/// Resolve a baseline run number, including legacy unnumbered results.
#[must_use]
pub fn find_baseline_run_number(results: &[ExperimentResult], segment: i64) -> Option<i64> {
	let baseline = find_baseline_result(results, segment)?;
	if let Some(run_number) = baseline.run_number {
		return Some(run_number);
	}
	let index = results.iter().position(|result| std::ptr::eq(result, baseline))?;
	Some(index as i64 + 1)
}

/// Collect secondary baselines from the earliest unflagged metric samples.
#[must_use]
pub fn find_baseline_secondary(
	results: &[ExperimentResult],
	segment: i64,
	known_metrics: &[MetricDef],
) -> NumericMetricMap {
	let mut values = find_baseline_result(results, segment)
		.map(|result| result.metrics.clone())
		.unwrap_or_default();
	for metric in known_metrics {
		if values.contains_key(&metric.name) {
			continue;
		}
		for result in current_results(results, segment) {
			if result.flagged {
				continue;
			}
			if let Some(value) = result.metrics.get(&metric.name) {
				values.insert(metric.name.clone(), *value);
				break;
			}
		}
	}
	values
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tools::autoresearch::ExperimentStatus;

	fn result(
		run_number: Option<i64>,
		segment: i64,
		metric: f64,
		status: ExperimentStatus,
		flagged: bool,
	) -> ExperimentResult {
		ExperimentResult {
			run_number,
			commit: String::new(),
			metric,
			metrics: NumericMetricMap::new(),
			status,
			description: String::new(),
			timestamp: 0,
			segment,
			confidence: None,
			asi: None,
			modified_paths: Vec::new(),
			scope_deviations: Vec::new(),
			justification: None,
			flagged,
			flagged_reason: None,
		}
	}

	#[test]
	fn format_elapsed_matches_reference() {
		assert_eq!(format_elapsed(0), "0s");
		assert_eq!(format_elapsed(999), "0s");
		assert_eq!(format_elapsed(1_500), "1s");
		assert_eq!(format_elapsed(60_000), "1m 00s");
		assert_eq!(format_elapsed(65_000), "1m 05s");
	}

	#[test]
	fn is_better_respects_direction() {
		assert!(is_better(1.0, 2.0, MetricDirection::Lower));
		assert!(!is_better(3.0, 2.0, MetricDirection::Lower));
		assert!(is_better(3.0, 2.0, MetricDirection::Higher));
		assert!(!is_better(1.0, 2.0, MetricDirection::Higher));
	}

	#[test]
	fn baseline_selection_skips_flagged_and_non_keep() {
		let results = vec![
			result(Some(1), 0, 10.0, ExperimentStatus::Keep, false),
			result(Some(2), 0, 20.0, ExperimentStatus::Keep, true),
			result(Some(3), 0, 30.0, ExperimentStatus::Discard, false),
			result(Some(4), 1, 40.0, ExperimentStatus::Keep, false),
		];
		let baseline = find_baseline_result(&results, 0).unwrap();
		assert_eq!(baseline.run_number, Some(1));
		assert_eq!(find_baseline_metric(&results, 0), Some(10.0));
		assert_eq!(find_baseline_run_number(&results, 0), Some(1));
		assert_eq!(find_baseline_run_number(&results, 1), Some(4));
	}

	#[test]
	fn legacy_baseline_run_number_uses_position() {
		let mut results = vec![
			result(None, 0, 10.0, ExperimentStatus::Keep, false),
			result(None, 0, 20.0, ExperimentStatus::Keep, false),
		];
		results[0].run_number = None;
		assert_eq!(find_baseline_run_number(&results, 0), Some(1));
	}

	#[test]
	fn secondary_baseline_fills_from_earliest_unflagged_sample() {
		let mut first = result(Some(1), 0, 10.0, ExperimentStatus::Keep, false);
		first.metrics.insert("primary".into(), 10.0);
		first.metrics.insert("extra".into(), 1.0);
		let mut flagged = result(Some(2), 0, 20.0, ExperimentStatus::Keep, true);
		flagged.metrics.insert("other".into(), 9.0);
		let mut later = result(Some(3), 0, 30.0, ExperimentStatus::Keep, false);
		later.metrics.insert("other".into(), 3.0);
		let results = vec![first, flagged, later];
		let known = vec![
			MetricDef { name: "primary".into(), unit: String::new() },
			MetricDef { name: "other".into(), unit: String::new() },
		];
		let secondary = find_baseline_secondary(&results, 0, &known);
		assert_eq!(secondary.get("primary"), Some(&10.0));
		assert_eq!(secondary.get("extra"), Some(&1.0));
		assert_eq!(secondary.get("other"), Some(&3.0));
	}
}
