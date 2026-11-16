//! Autoresearch experiment selection helpers.
//!
//! Reference: `l/src/apps/autoresearch-data.ts`. The reference imports
//! `ExperimentResult`/`MetricDef`/`MetricDirection`/`NumericMetricMap` from
//! `l/src/tools/autoresearch`; that module is not ported yet, so the types are
//! declared here and will move with it.
//!
//! Deviations:
//! - [`current_results`] and [`find_baseline_result`] borrow instead of cloning
//!   the filtered rows.
//! - [`NumericMetricMap`] is a `BTreeMap`, so metric iteration is ordered.

use std::collections::BTreeMap;

/// Whether a lower or higher metric is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricDirection {
    Lower,
    Higher,
}

impl MetricDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Higher => "higher",
        }
    }
}

/// Disposition of a completed experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentStatus {
    Keep,
    Discard,
    Crash,
    ChecksFailed,
}

impl ExperimentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Discard => "discard",
            Self::Crash => "crash",
            Self::ChecksFailed => "checks_failed",
        }
    }
}

/// JSON-compatible additional structured experiment information.
#[derive(Clone, Debug, PartialEq)]
pub enum AsiValue {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Array(Vec<AsiValue>),
    Object(BTreeMap<String, AsiValue>),
}

/// Named structured experiment observations.
pub type AsiData = BTreeMap<String, AsiValue>;

/// Numeric metrics indexed by name.
pub type NumericMetricMap = BTreeMap<String, f64>;

/// Name and unit of an experiment metric.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricDef {
    pub name: String,
    pub unit: String,
}

/// Recorded metric and disposition of an experiment run.
#[derive(Clone, Debug, PartialEq)]
pub struct ExperimentResult {
    pub run_number: Option<u32>,
    pub commit: String,
    pub metric: f64,
    pub metrics: NumericMetricMap,
    pub status: ExperimentStatus,
    pub description: String,
    pub timestamp: i64,
    pub segment: u32,
    pub confidence: Option<f64>,
    pub asi: Option<AsiData>,
    pub modified_paths: Vec<String>,
    pub scope_deviations: Vec<String>,
    pub justification: Option<String>,
    pub flagged: bool,
    pub flagged_reason: Option<String>,
}

/// Format an experiment duration for display.
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
pub fn is_better(current: f64, best: f64, direction: MetricDirection) -> bool {
    match direction {
        MetricDirection::Lower => current < best,
        MetricDirection::Higher => current > best,
    }
}

/// Select results belonging to the active experiment segment.
pub fn current_results<'a>(
    results: &'a [ExperimentResult],
    segment: u32,
) -> Vec<&'a ExperimentResult> {
    results
        .iter()
        .filter(|result| result.segment == segment)
        .collect()
}

/// Index of the first unflagged kept experiment in a segment, if any.
fn baseline_index(results: &[ExperimentResult], segment: u32) -> Option<usize> {
    results.iter().position(|result| {
        result.segment == segment && result.status == ExperimentStatus::Keep && !result.flagged
    })
}

/// Find the first unflagged kept experiment in a segment.
pub fn find_baseline_result<'a>(
    results: &'a [ExperimentResult],
    segment: u32,
) -> Option<&'a ExperimentResult> {
    baseline_index(results, segment).map(|index| &results[index])
}

/// Read the baseline primary metric for a segment.
pub fn find_baseline_metric(results: &[ExperimentResult], segment: u32) -> Option<f64> {
    find_baseline_result(results, segment).map(|baseline| baseline.metric)
}

/// Resolve a baseline run number, including legacy unnumbered results.
pub fn find_baseline_run_number(results: &[ExperimentResult], segment: u32) -> Option<u32> {
    let baseline = find_baseline_result(results, segment)?;
    if let Some(run_number) = baseline.run_number {
        return Some(run_number);
    }
    let index = baseline_index(results, segment)?;
    u32::try_from(index + 1).ok()
}

/// Collect secondary baselines from the earliest unflagged metric samples.
pub fn find_baseline_secondary(
    results: &[ExperimentResult],
    segment: u32,
    known_metrics: &[MetricDef],
) -> NumericMetricMap {
    let mut values = match find_baseline_result(results, segment) {
        Some(baseline) => baseline.metrics.clone(),
        None => NumericMetricMap::new(),
    };
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

    fn result(run: Option<u32>, metric: f64, segment: u32) -> ExperimentResult {
        ExperimentResult {
            run_number: run,
            commit: format!("c{run:?}"),
            metric,
            metrics: NumericMetricMap::new(),
            status: ExperimentStatus::Keep,
            description: String::new(),
            timestamp: 0,
            segment,
            confidence: None,
            asi: None,
            modified_paths: Vec::new(),
            scope_deviations: Vec::new(),
            justification: None,
            flagged: false,
            flagged_reason: None,
        }
    }

    #[test]
    fn elapsed_formats_minutes_and_seconds() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(999), "0s");
        assert_eq!(format_elapsed(5_000), "5s");
        assert_eq!(format_elapsed(60_000), "1m 00s");
        assert_eq!(format_elapsed(90_000), "1m 30s");
        assert_eq!(format_elapsed(3_600_000), "60m 00s");
    }

    #[test]
    fn better_follows_direction() {
        assert!(is_better(1.0, 2.0, MetricDirection::Lower));
        assert!(!is_better(2.0, 1.0, MetricDirection::Lower));
        assert!(is_better(2.0, 1.0, MetricDirection::Higher));
        assert!(!is_better(1.0, 2.0, MetricDirection::Higher));
    }

    #[test]
    fn baselines_prefer_unflagged_kept_runs() {
        let mut flagged = result(Some(1), 0.5, 2);
        flagged.flagged = true;
        let kept = result(Some(2), 0.7, 2);
        let other_segment = result(Some(3), 0.1, 3);
        let results = vec![flagged, kept, other_segment];

        assert_eq!(current_results(&results, 2).len(), 2);
        assert_eq!(find_baseline_metric(&results, 2), Some(0.7));
        assert_eq!(find_baseline_run_number(&results, 2), Some(2));
        assert_eq!(find_baseline_metric(&results, 1), None);
    }

    #[test]
    fn legacy_baseline_falls_back_to_position() {
        let results = vec![result(None, 0.4, 1), result(Some(9), 0.9, 1)];
        assert_eq!(find_baseline_run_number(&results, 1), Some(1));
    }

    #[test]
    fn secondary_baselines_fill_from_earliest_unflagged() {
        let mut first = result(Some(1), 0.4, 1);
        first.metrics.insert("loss".into(), 0.25);
        let mut second = result(Some(2), 0.5, 1);
        second.metrics.insert("loss".into(), 0.4);
        second.metrics.insert("acc".into(), 0.8);
        let mut flagged = result(Some(3), 0.1, 1);
        flagged.flagged = true;
        flagged.metrics.insert("acc".into(), 0.95);
        let results = vec![first, second, flagged];

        let known = vec![
            MetricDef { name: "loss".into(), unit: "".into() },
            MetricDef { name: "acc".into(), unit: "".into() },
        ];
        let secondary = find_baseline_secondary(&results, 1, &known);
        assert_eq!(secondary.get("loss"), Some(&0.25));
        // `acc` is absent from the baseline, so the earliest unflagged run wins.
        assert_eq!(secondary.get("acc"), Some(&0.8));

        let results = vec![result(Some(1), 0.4, 1)];
        assert!(find_baseline_secondary(&results, 1, &[]).is_empty());
    }
}
