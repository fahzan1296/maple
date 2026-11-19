//! Port of `l/test/autoresearch-dashboard-native.test.ts` (pure parts).
//!
//! Deviations: the reference opens a native overlay sheet and asserts describe
//! nodes (`omp.autoresearch.runs`, `omp.autoresearch.empty.title`) plus key
//! scrolling. The crate's dashboard is host-owned rendering with pure state
//! helpers, so the port covers run selection/order, status counts, best-metric
//! state, collapsed/expanded text, the empty state, and controller
//! show/update gating.

use maple_tui::apps::autoresearch_dashboard::{
	AutoresearchDashboardHost, AutoresearchDashboardRuntime, DashboardController,
	DefaultDashboardController, ExperimentResult, ExperimentState, ExperimentStatus,
	current_results, render_collapsed_line, render_dashboard_lines, render_expanded_header,
	status_counts,
};
use std::collections::BTreeMap;

fn result(run: u32, metric: f64, status: ExperimentStatus) -> ExperimentResult {
	ExperimentResult {
		run_number: Some(run),
		commit: format!("c0ffee{run}"),
		metric,
		metrics: BTreeMap::new(),
		status,
		description: format!("run {run}"),
		timestamp: i64::from(run),
		segment: 0,
		confidence: None,
		flagged: false,
	}
}

fn runtime(results: Vec<ExperimentResult>) -> AutoresearchDashboardRuntime {
	AutoresearchDashboardRuntime {
		autoresearch_mode: true,
		dashboard_expanded: false,
		state: ExperimentState {
			name: Some("Speed up the parser".to_string()),
			goal: Some("Cut parse time.".to_string()),
			metric_name: "p50_ms".to_string(),
			metric_unit: "ms".to_string(),
			best_direction: "lower".to_string(),
			best_metric: results
				.iter()
				.filter(|r| r.status == ExperimentStatus::Keep)
				.map(|r| r.metric)
				.reduce(f64::min),
			results,
			..ExperimentState::default()
		},
		last_run_summary: None,
		running_experiment: None,
	}
}

struct Host(bool);
impl AutoresearchDashboardHost for Host {
	fn has_ui(&self) -> bool {
		self.0
	}
}

#[test]
fn segment_runs_keep_order_and_counts_with_best_metric() {
	let results = vec![
		result(1, 24.0, ExperimentStatus::Keep),
		result(2, 0.0, ExperimentStatus::Crash),
		result(3, 19.0, ExperimentStatus::Keep),
		result(4, 21.0, ExperimentStatus::Discard),
	];
	let rt = runtime(results);
	let segment = current_results(&rt.state.results, 0);
	assert_eq!(
		segment.iter().filter_map(|r| r.run_number).collect::<Vec<_>>(),
		vec![1, 2, 3, 4],
		"the segment keeps the recorded run order"
	);
	assert_eq!(status_counts(&segment), (2, 1, 1, 0));
	assert_eq!(rt.state.best_metric, Some(19.0), "lower is better: run #3 wins");
	assert_eq!(
		segment
			.iter()
			.find(|r| r.run_number == Some(2))
			.map(|r| r.metric),
		Some(0.0),
		"a crash records its metric as written"
	);
}

#[test]
fn empty_state_says_baseline_pending_and_mode_off() {
	let mut rt = runtime(vec![]);
	rt.state.best_metric = None;
	let collapsed = render_collapsed_line(&rt);
	assert!(collapsed.contains("baseline pending"), "{collapsed}");
	assert!(collapsed.contains("Speed up the parser"), "{collapsed}");

	rt.autoresearch_mode = false;
	let off = render_collapsed_line(&rt);
	assert!(off.contains("mode off"), "{off}");
	assert!(!off.contains("baseline pending"), "{off}");
}

#[test]
fn dashboard_lines_title_the_experiment_and_report_best() {
	let rt = runtime(vec![result(1, 24.0, ExperimentStatus::Keep), result(2, 20.0, ExperimentStatus::Keep)]);
	let expanded = render_expanded_header(&rt, 120);
	assert!(expanded.contains("autoresearch: Speed up the parser"), "{expanded}");

	let lines = render_dashboard_lines(&rt, 120).join("\n");
	assert!(lines.contains("autoresearch: Speed up the parser"), "{lines}");
	assert!(lines.contains("goal: Cut parse time."), "{lines}");

	let mut expanded_rt = rt.clone();
	expanded_rt.dashboard_expanded = true;
	let lines = render_dashboard_lines(&expanded_rt, 120).join("\n");
	assert!(lines.contains("ctrl+x collapse"), "{lines}");
}

#[test]
fn controller_only_shows_a_widget_when_the_host_has_ui() {
	let rt = runtime(vec![result(1, 24.0, ExperimentStatus::Keep)]);
	let mut controller = DefaultDashboardController::default();
	controller.show_overlay(&Host(false), &rt);
	assert!(!controller.overlay_open, "no host UI means no overlay");
	controller.show_overlay(&Host(true), &rt);
	assert!(controller.overlay_open);
	controller.update_widget(&Host(true), &rt);
	assert!(controller.widget_visible);

	let quiet = AutoresearchDashboardRuntime::default();
	controller.update_widget(&Host(true), &quiet);
	assert!(!controller.widget_visible, "nothing to show when the dashboard is quiet");
}
