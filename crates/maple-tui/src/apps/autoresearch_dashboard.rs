//! Autoresearch experiment dashboard widget/overlay.
//!
//! Reference: `l/src/apps/autoresearch-dashboard.ts`.
//!
//! Deviations:
//! - TS renders ANSI/native widget+overlay via `@oh-my-pi/pi-tui` components,
//!   theme, native describe nodes, scroll views, spinners/timers, and
//!   extension UI host (`ctx.ui.setWidget/custom`). No equivalent runtime here.
//! - This port provides faithful public data/controller contracts and pure
//!   helpers as a model: [`AutoresearchDashboardRuntime`],
//!   [`AutoresearchDashboardHost`], [`DashboardController`]/
//!   [`DefaultDashboardController`], mode/status/count helpers, summary
//!   selection, and footer/key-hint text builders where applicable.
//! - Rendering (`render*`, `describe*` native nodes) and overlay lifecycle are
//!   represented as state snapshots/strings for hosts/tests instead of direct
//!   terminal drawing.
//! - Types from `l/src/tools/autoresearch` are mirrored minimally here because
//!   that module is not ported; align with `src/apps/autoresearch_data.rs`
//!   concepts where possible.

/// Disposition of a completed experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExperimentStatus {
	Keep,
	Discard,
	Crash,
	ChecksFailed,
}

/// Recorded metric and disposition of an experiment run.
#[derive(Clone, Debug, PartialEq)]
pub struct ExperimentResult {
	pub run_number: Option<u32>,
	pub commit: String,
	pub metric: f64,
	pub metrics: std::collections::BTreeMap<String, f64>,
	pub status: ExperimentStatus,
	pub description: String,
	pub timestamp: i64,
	pub segment: u32,
	pub confidence: Option<f64>,
	pub flagged: bool,
}

/// Metric definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricDef {
	pub name: String,
	pub unit: String,
}

/// Experiment runtime fields rendered by the dashboard.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ExperimentState {
	pub name: Option<String>,
	pub goal: Option<String>,
	pub branch: Option<String>,
	pub metric_name: String,
	pub metric_unit: String,
	pub best_direction: String,
	pub best_metric: Option<f64>,
	pub confidence: Option<f64>,
	pub current_segment: u32,
	pub max_experiments: Option<u32>,
	pub results: Vec<ExperimentResult>,
	pub secondary_metrics: Vec<MetricDef>,
}

/// Summary of the last completed run awaiting logging.
#[derive(Clone, Debug, PartialEq)]
pub struct LastRunSummary {
	pub run_number: u32,
	pub passed: bool,
	pub parsed_primary: Option<f64>,
}

/// Currently running experiment info.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningExperiment {
	pub started_at_ms: i64,
	pub command: String,
}

/// Experiment runtime fields rendered by the dashboard.
#[derive(Clone, Debug, Default)]
pub struct AutoresearchDashboardRuntime {
	pub autoresearch_mode: bool,
	pub dashboard_expanded: bool,
	pub state: ExperimentState,
	pub last_run_summary: Option<LastRunSummary>,
	pub running_experiment: Option<RunningExperiment>,
}

/// Widget and overlay capabilities supplied by an extension context.
pub trait AutoresearchDashboardHost {
	fn has_ui(&self) -> bool { false }
}

/// Dashboard lifecycle and rendering controls.
pub trait DashboardController {
	fn clear(&mut self, _ctx: &dyn AutoresearchDashboardHost) {}
	fn request_render(&mut self) {}
	fn update_widget(&mut self, _ctx: &dyn AutoresearchDashboardHost, _runtime: &AutoresearchDashboardRuntime) {}
	fn show_overlay(&mut self, _ctx: &dyn AutoresearchDashboardHost, _runtime: &AutoresearchDashboardRuntime) {}
}

/// Default no-op controller (host/UI owns actual rendering).
#[derive(Default)]
pub struct DefaultDashboardController {
	pub widget_visible: bool,
	pub overlay_open: bool,
	pub render_requests: u32,
}

impl DashboardController for DefaultDashboardController {
	fn clear(&mut self, _ctx: &dyn AutoresearchDashboardHost) {
		self.widget_visible = false;
		self.overlay_open = false;
	}
	fn request_render(&mut self) { self.render_requests += 1; }
	fn update_widget(&mut self, ctx: &dyn AutoresearchDashboardHost, runtime: &AutoresearchDashboardRuntime) {
		self.widget_visible = ctx.has_ui() && should_show_dashboard(runtime);
	}
	fn show_overlay(&mut self, ctx: &dyn AutoresearchDashboardHost, runtime: &AutoresearchDashboardRuntime) {
		if ctx.has_ui() && should_show_dashboard(runtime) {
			self.overlay_open = true;
		}
	}
}

/// Create a dashboard controller backed by host widget/overlay surfaces.
pub fn create_dashboard_controller() -> DefaultDashboardController {
	DefaultDashboardController::default()
}

pub fn should_show_dashboard(runtime: &AutoresearchDashboardRuntime) -> bool {
	runtime.autoresearch_mode
		|| !runtime.state.results.is_empty()
		|| runtime.running_experiment.is_some()
		|| runtime.last_run_summary.is_some()
}

pub fn current_results(results: &[ExperimentResult], segment: u32) -> Vec<&ExperimentResult> {
	results.iter().filter(|r| r.segment == segment).collect()
}

pub fn status_counts(results: &[&ExperimentResult]) -> (usize, usize, usize, usize) {
	let mut keep = 0usize; let mut discard = 0usize; let mut crash = 0usize; let mut checks_failed = 0usize;
	for r in results {
		match r.status {
			ExperimentStatus::Keep => keep += 1,
			ExperimentStatus::Discard => discard += 1,
			ExperimentStatus::Crash => crash += 1,
			ExperimentStatus::ChecksFailed => checks_failed += 1,
		}
	}
	(keep, discard, crash, checks_failed)
}

pub fn render_mode_status(runtime: &AutoresearchDashboardRuntime) -> &'static str {
	if runtime.autoresearch_mode { "mode on" } else { "mode off" }
}

pub fn render_expanded_header(runtime: &AutoresearchDashboardRuntime, width: usize) -> String {
	let name = runtime.state.name.as_deref().unwrap_or("");
	let label = if name.is_empty() { " autoresearch ".to_string() } else { format!(" autoresearch: {name} ") };
	let hint = format!(" ctrl+x collapse  ctrl+shift+x overlay  {} ", render_mode_status(runtime));
	let fill = width.saturating_sub(label.chars().count() + hint.chars().count());
	format!("{label}{}{hint}", "-".repeat(fill))
}

pub fn render_collapsed_line(runtime: &AutoresearchDashboardRuntime) -> String {
	if let Some(pending) = &runtime.last_run_summary {
		let mut s = format!("autoresearch pending run #{}", pending.run_number);
		s.push_str(if pending.passed { " pass" } else { " fail" });
		if let Some(v) = pending.parsed_primary {
			s.push_str(&format!(" | {}={v}", runtime.state.metric_name));
		}
		s.push_str(" | log_experiment required");
		if !runtime.autoresearch_mode { s.push_str(" | mode off"); }
		return s;
	}
	if runtime.state.results.is_empty() {
		return if runtime.autoresearch_mode {
			format!("autoresearch baseline pending{}", runtime.state.name.as_deref().map(|n| format!(" | {n}")).unwrap_or_default())
		} else {
			format!("autoresearch mode off{}", runtime.state.name.as_deref().map(|n| format!(" | {n}")).unwrap_or_default())
		};
	}
	let current = current_results(&runtime.state.results, runtime.state.current_segment);
	let (keep, _discard, crash, checks_failed) = status_counts(&current);
	let mut s = format!("autoresearch {} runs {keep} kept", current.len());
	if crash > 0 { s.push_str(&format!(" {crash} crash")); }
	if checks_failed > 0 { s.push_str(&format!(" {checks_failed} checks_failed")); }
	if let Some(b) = runtime.state.best_metric { s.push_str(&format!(" | baseline {b}")); }
	s
}

/// Render dashboard lines for a given width (collapsed/expanded summary + key sections).
pub fn render_dashboard_lines(runtime: &AutoresearchDashboardRuntime, width: usize) -> Vec<String> {
	let mut lines = Vec::new();
	if runtime.dashboard_expanded {
		lines.push(render_expanded_header(runtime, width));
	} else {
		lines.push(render_collapsed_line(runtime));
	}
	if let Some(goal) = &runtime.state.goal {
		if !goal.is_empty() { lines.push(format!("goal: {goal}")); }
	}
	if let Some(branch) = &runtime.state.branch {
		if !branch.is_empty() { lines.push(format!("branch: {branch}")); }
	}
	lines
}

pub use create_dashboard_controller as createDashboardController;
pub use render_dashboard_lines as renderDashboardLines;

pub fn git_footer_hints_text(expanded: bool) -> &'static str {
	if expanded { "ctrl+x collapse  ctrl+shift+x overlay" } else { "ctrl+x expand" }
}

#[cfg(test)]
mod tests {
	use super::*;

	fn runtime() -> AutoresearchDashboardRuntime {
		AutoresearchDashboardRuntime {
			autoresearch_mode: true,
			dashboard_expanded: false,
			state: ExperimentState { metric_name: "score".into(), current_segment: 0, ..Default::default() },
			last_run_summary: None,
			running_experiment: None,
		}
	}

	#[test]
	fn should_show_when_mode_or_results_or_running() {
		let mut r = runtime();
		r.autoresearch_mode = false;
		assert!(!should_show_dashboard(&r));
		r.autoresearch_mode = true;
		assert!(should_show_dashboard(&r));
		r.autoresearch_mode = false;
		r.state.results.push(ExperimentResult {
			run_number: Some(1), commit: "c".into(), metric: 1.0, metrics: Default::default(),
			status: ExperimentStatus::Keep, description: "d".into(), timestamp: 0, segment: 0,
			confidence: None, flagged: false,
		});
		assert!(should_show_dashboard(&r));
	}

	#[test]
	fn controller_tracks_visibility_and_requests() {
		struct Host(bool);
		impl AutoresearchDashboardHost for Host { fn has_ui(&self) -> bool { self.0 } }
		let mut c = create_dashboard_controller();
		let r = runtime();
		c.update_widget(&Host(true), &r);
		assert!(c.widget_visible);
		c.show_overlay(&Host(true), &r);
		assert!(c.overlay_open);
		c.request_render();
		assert_eq!(c.render_requests, 1);
		c.clear(&Host(true));
		assert!(!c.widget_visible && !c.overlay_open);
	}

	#[test]
	fn status_counts_and_lines() {
		let r1 = ExperimentResult { run_number: Some(1), commit: "".into(), metric: 0.0, metrics: Default::default(), status: ExperimentStatus::Keep, description: "".into(), timestamp: 0, segment: 0, confidence: None, flagged: false };
		let r2 = ExperimentResult { status: ExperimentStatus::Crash, ..r1.clone() };
		let refs = vec![&r1, &r2];
		assert_eq!(status_counts(&refs), (1, 0, 1, 0));
		let mut rt = runtime();
		rt.last_run_summary = Some(LastRunSummary { run_number: 3, passed: false, parsed_primary: Some(2.5) });
		let line = render_collapsed_line(&rt);
		assert!(line.contains("pending run #3") && line.contains("score=2.5"));
	}

	#[test]
	fn expanded_header_contains_hints() {
		let h = render_expanded_header(&runtime(), 60);
		assert!(h.contains("autoresearch") && h.contains("ctrl+x"));
	}
}
