//! Reference: `l/src/overlays/jobs-panel.ts`.
//!
//! The jobs pill's sheet: a master–detail view of a session's async jobs and
//! their subagents — selectable tree, live status/elapsed, facts, widget and
//! output tail — plus the terminal fallback and the native card.
//!
//! Deviations:
//! - `Date.now()` becomes the injected `now_ms` of every entry point.
//! - The session is a [`JobsSheetSource`] trait instead of an object take;
//!   `cancel` is an optional method reported through
//!   [`JobsSheetSource::cancellable`] (read-only sheets keep `false`).
//! - `theme` is injected through the caller (`job_tree_guide` already takes
//!   one in [`crate::overlays::job_tree`]); colors are semantic spans.
//! - The job widget's full block renderer is not ported here: a job with data
//!   draws one [`crate::tools::job_widget::job_widget_summary`] line, which is
//!   the same glance the reference's terminal fallback leads with.
//! - `DescribeContext` is not consulted; the card is always produced.

use std::collections::HashSet;

use crate::keys::{is_arrow_down, is_arrow_up, is_escape, Key};
use crate::native::describe::{ansi, code, col, compact, elapsed, item, kv, list, node, row, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{action_bar, action_button, ActionButtonOptions, ActionButtonTone};
use crate::overlays::job_tree::{build_job_tree, job_tree_guide, JobTreeAgent, JobTreeJob, JobTreeRow};
use crate::render::render_utils::PREVIEW_LIMITS;
use crate::text_shape::{truncate_to_width, wrap_text_with_ansi, Ellipsis};
use crate::theme::Theme;
use crate::tools::job_widget::{job_widget_summary, JobWidgetView};

/// Root role of the native sheet.
pub const ROLE: &str = "omp.overlay.jobs";

/// Native head of the sheet.
pub const HEAD: &str = "Background jobs";

/// Output tail rows the terminal fallback shows under the selected job.
pub const FALLBACK_TAIL_LINES: usize = 8;

/// A job's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
	Running,
	Completed,
	Failed,
	Cancelled,
}

impl JobStatus {
	/// The sheet's name for the status.
	#[must_use]
	pub fn label(self) -> &'static str {
		match self {
			JobStatus::Running => "Running",
			JobStatus::Completed => "Done",
			JobStatus::Failed => "Failed",
			JobStatus::Cancelled => "Cancelled",
		}
	}

	/// Semantic tone of the row icon and the detail pill.
	#[must_use]
	pub fn tone(self) -> &'static str {
		match self {
			JobStatus::Running => "pending",
			JobStatus::Completed => "success",
			JobStatus::Failed => "error",
			JobStatus::Cancelled => "muted",
		}
	}

	/// Lead glyph of the status pill; `pulse` marks a running job.
	#[must_use]
	pub fn glyph(self) -> &'static str {
		match self {
			JobStatus::Running => "●",
			JobStatus::Completed => "✓",
			JobStatus::Failed => "✕",
			JobStatus::Cancelled => "⊘",
		}
	}

	#[must_use]
	pub fn is_running(self) -> bool {
		self == JobStatus::Running
	}
}

/// One async job as the session snapshots it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobsPanelJob {
	pub id: String,
	pub r#type: String,
	pub status: JobStatus,
	pub label: String,
	/// What the job is for, in the model's words.
	pub intent: Option<String>,
	/// One line on what a command does, from the cheap model.
	pub caption: Option<String>,
	pub start_time: i64,
	pub end_time: Option<i64>,
	/// Registry id of the agent that owns the job.
	pub owner_id: Option<String>,
	/// Registry id of the subagent a task job runs.
	pub agent_id: Option<String>,
}

impl JobTreeJob for JobsPanelJob {
	fn id(&self) -> &str {
		&self.id
	}

	fn kind(&self) -> &str {
		&self.r#type
	}

	fn agent_id(&self) -> Option<&str> {
		self.agent_id.as_deref()
	}

	fn task_agent_id(&self) -> Option<&str> {
		if self.r#type == "task" {
			self.agent_id.as_deref()
		} else {
			None
		}
	}

	fn live(&self) -> bool {
		self.status.is_running()
	}

	fn start_time(&self) -> i64 {
		self.start_time
	}
}

/// Running and recently settled jobs of an agent and its subagents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobsPanelSnapshot {
	pub running: Vec<JobsPanelJob>,
	pub recent: Vec<JobsPanelJob>,
	/// The agent the sheet is for: only its own jobs can be cancelled.
	pub root_id: Option<String>,
	/// Subagents owning listed jobs, with the ancestors linking them to root.
	pub agents: Vec<JobTreeAgent>,
}

/// What the sheet shows about the selected job beyond its row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobsPanelDetail {
	/// Full command line of a process job (rows show the cut label).
	pub command: Option<String>,
	pub cwd: Option<String>,
	pub pids: Vec<u32>,
	pub exit_code: Option<i32>,
	pub output: Option<String>,
	pub artifact_id: Option<String>,
	/// The job's live widget (frozen once it settles).
	pub widget: Option<JobWidgetView>,
}

/// The session behind a [`JobsSheet`].
pub trait JobsSheetSource {
	/// Current jobs; runs once per frame.
	fn load(&mut self) -> JobsPanelSnapshot;
	/// The selected job's detail; `None` once the job is gone.
	fn inspect(&mut self, id: &str) -> Option<JobsPanelDetail>;
	/// Cancel a running job. Read-only sheets leave [`Self::cancellable`]
	/// `false` and never receive this call.
	fn cancel(&mut self, _id: &str) {}
	/// Whether this source may cancel jobs at all.
	fn cancellable(&self) -> bool {
		false
	}
	/// Dismiss the sheet.
	fn close(&mut self);
}

/// List-row icon per job type.
#[must_use]
pub fn type_icon(job_type: &str) -> &'static str {
	match job_type {
		"bash" => "bash",
		"task" => "agent",
		_ => "job",
	}
}

/// How long a job ran, or has been running.
#[must_use]
pub fn job_age(job: &JobsPanelJob, now_ms: i64) -> i64 {
	(job.end_time.unwrap_or(now_ms) - job.start_time).max(0)
}

/// Whether a widget draws more than its title.
#[must_use]
pub fn widget_has_data(widget: &JobWidgetView) -> bool {
	!widget.blocks.is_empty() || widget.headline.is_some() || widget.stage.is_some()
}

/// The last `count` pieces of `text.trim_end().split('\n')`, scanned back from
/// the end so a long running output is never split as a whole.
#[must_use]
pub fn tail_lines(text: &str, count: usize) -> Vec<String> {
	let trimmed = text.trim_end();
	let mut lines = Vec::new();
	let mut end = trimmed.len();
	while lines.len() < count {
		let newline = if end > 0 {
			trimmed[..end].rfind('\n').map(|at| at as isize)
		} else {
			None
		};
		let start = newline.map_or(0, |at| at as usize + 1);
		lines.push(trimmed[start..end].to_string());
		match newline {
			Some(at) => end = at as usize,
			None => break,
		}
	}
	lines.reverse();
	lines
}

/// Format milliseconds the way the fallback rows do.
#[must_use]
pub fn format_age(ms: i64) -> String {
	let seconds = ms.max(0) / 1000;
	if seconds < 60 {
		format!("{seconds}s")
	} else if seconds < 3_600 {
		format!("{}m{:02}s", seconds / 60, seconds % 60)
	} else {
		format!("{}h{:02}m", seconds / 3_600, (seconds % 3_600) / 60)
	}
}

/// Flatten a model-written label to one printable row.
#[must_use]
pub fn flatten_label(text: &str) -> String {
	crate::tools::shell_viz::sanitize_text(text)
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// One current view of the session.
struct Current {
	rows: Vec<JobTreeRow<JobsPanelJob>>,
	jobs: Vec<JobsPanelJob>,
	selected: Option<JobsPanelJob>,
	cancellable: bool,
}

/// The jobs sheet.
pub struct JobsSheet {
	source: Box<dyn JobsSheetSource>,
	selected_id: Option<String>,
	/// Selected job as last seen, so an evicted settled job stays listed.
	held: Option<(JobsPanelJob, Option<JobsPanelDetail>, Vec<JobTreeAgent>)>,
	tail: Option<(String, String, Vec<String>)>,
	native_cache: Option<(String, Option<String>, NativeNode)>,
	theme: Theme,
}

impl JobsSheet {
	pub fn new(source: Box<dyn JobsSheetSource>) -> Self {
		JobsSheet {
			source,
			selected_id: None,
			held: None,
			tail: None,
			native_cache: None,
			theme: crate::theme::default_theme(),
		}
	}

	pub fn set_theme(&mut self, theme: Theme) {
		self.theme = theme;
	}

	#[must_use]
	pub fn selected_id(&self) -> Option<&str> {
		self.selected_id.as_deref()
	}

	/// Whether the selected job may be cancelled right now.
	#[must_use]
	pub fn can_cancel_selected(&mut self) -> bool {
		let current = self.current();
		current.cancellable && current.selected.is_some_and(|job| job.status.is_running())
	}

	/// The job tree, its jobs in tree order, the selected job (the first when
	/// the selection left), and whether the sheet can stop it.
	fn current(&mut self) -> Current {
		let snapshot = self.source.load();
		let JobsPanelSnapshot {
			running,
			mut recent,
			root_id,
			mut agents,
		} = snapshot;
		let held = match &self.held {
			Some((job, detail, held_agents)) if Some(&job.id) == self.selected_id.as_ref() => {
				Some((job.clone(), detail.clone(), held_agents.clone()))
			}
			_ => None,
		};
		if let Some((job, detail, held_agents)) = &held {
			let evicted = !job.status.is_running()
				&& !running.iter().any(|candidate| candidate.id == job.id)
				&& !recent.iter().any(|candidate| candidate.id == job.id);
			if evicted {
				recent.push(job.clone());
				let known: HashSet<String> = agents.iter().map(|agent| agent.id.clone()).collect();
				agents.extend(
					held_agents
						.iter()
						.filter(|agent| !known.contains(&agent.id))
						.cloned(),
				);
			}
		}
		let mut all = running.clone();
		all.extend(recent.iter().cloned());
		let rows = build_job_tree(&all, &agents, root_id.as_deref());
		let jobs: Vec<JobsPanelJob> = rows
			.iter()
			.filter_map(|entry| match entry {
				JobTreeRow::Job { job, .. } => Some(job.clone()),
				JobTreeRow::Agent { .. } => None,
			})
			.collect();
		let selected = jobs
			.iter()
			.find(|job| Some(&job.id) == self.selected_id.as_ref())
			.or_else(|| jobs.first())
			.cloned();
		self.selected_id = selected.as_ref().map(|job| job.id.clone());
		match &selected {
			None => self.held = None,
			Some(job) => {
				let last_detail = held
					.as_ref()
					.filter(|(held_job, _, _)| held_job.id == job.id)
					.and_then(|(_, detail, _)| detail.clone());
				self.held = Some((job.clone(), last_detail, snapshot_agents(&agents)));
			}
		}
		let cancellable = match &selected {
			Some(job) => {
				self.source.cancellable()
					&& (job.owner_id.is_none() || job.owner_id.as_deref() == root_id.as_deref())
			}
			None => false,
		};
		Current {
			rows,
			jobs,
			selected,
			cancellable,
		}
	}

	/// Detail from the source, else the last one seen while the job was held.
	fn inspect(&mut self, id: &str) -> Option<JobsPanelDetail> {
		let detail = self.source.inspect(id);
		let held = self
			.held
			.as_mut()
			.filter(|(job, _, _)| job.id == id);
		match held {
			None => detail,
			Some((_, held_detail, _)) => {
				if detail.is_some() {
					*held_detail = detail.clone();
				}
				held_detail.clone().or(detail)
			}
		}
	}

	fn tail_lines_for(&mut self, id: &str, output: &str) -> Vec<String> {
		if let Some((cached_id, cached_output, lines)) = &self.tail {
			if cached_id == id && cached_output == output {
				return lines.clone();
			}
		}
		let lines = tail_lines(output, FALLBACK_TAIL_LINES);
		self.tail = Some((id.to_string(), output.to_string(), lines.clone()));
		lines
	}

	/// Move the selection within the job list.
	pub fn move_selection(&mut self, delta: isize) {
		let current = self.current();
		let Some(selected) = current.selected else {
			return;
		};
		let Some(position) = current.jobs.iter().position(|job| job.id == selected.id) else {
			return;
		};
		let last = current.jobs.len().saturating_sub(1);
		let index = (position as isize + delta).clamp(0, last as isize) as usize;
		self.selected_id = current.jobs.get(index).map(|job| job.id.clone());
	}

	pub fn cancel_selected(&mut self) {
		let current = self.current();
		if current.cancellable {
			if let Some(job) = current.selected.filter(|job| job.status.is_running()) {
				self.source.cancel(&job.id);
			}
		}
	}

	pub fn close(&mut self) {
		self.source.close();
	}

	pub fn invalidate(&mut self) {
		self.native_cache = None;
	}

	/// Select a job by id (native row click).
	pub fn select_row(&mut self, id: &str) {
		self.selected_id = Some(id.to_string());
		self.native_cache = None;
	}

	pub fn handle_key(&mut self, key: &Key) {
		if is_escape(key) {
			self.close();
		} else if is_arrow_up(key) {
			self.move_selection(-1);
		} else if is_arrow_down(key) {
			self.move_selection(1);
		} else if *key == Key::Char('x') {
			self.cancel_selected();
		}
	}

	/// Native action (`close` / `cancel`).
	pub fn handle_native_action(&mut self, act: &str) {
		match act {
			"close" => self.close(),
			"cancel" => self.cancel_selected(),
			_ => {}
		}
	}

	/// Native action plus row selection from one event.
	pub fn handle_native_event(&mut self, kind: &str, value: &str) {
		match kind {
			"select" | "activate" => {
				self.select_row(value);
				if kind == "activate" {
					self.cancel_selected_if_running();
				}
			}
			"action" => self.handle_native_action(value),
			_ => {}
		}
	}

	fn cancel_selected_if_running(&mut self) {
		self.cancel_selected();
	}

	/// Native card: split master/detail, summary and close; empty state with
	/// no jobs.
	#[must_use]
	pub fn describe(&mut self, now_ms: i64) -> NativeNode {
		let current = self.current();
		let detail = current
			.selected
			.as_ref()
			.and_then(|job| self.inspect(&job.id));
		let key = native_key(&current.rows, current.selected.as_ref(), detail.as_ref(), now_ms, &self.theme);
		let output = detail.as_ref().and_then(|detail| detail.output.clone());
		if let Some((cached_key, cached_output, node)) = &self.native_cache {
			if cached_key == &key && cached_output == &output {
				return node.clone();
			}
		}
		let close = action_button(
			"Close",
			"close",
			ActionButtonOptions {
				keys: Some("escape".to_string()),
				..ActionButtonOptions::default()
			},
		);
		let described = match &current.selected {
			Some(selected) => col(
				vec![
					node(
						NodeKind::Row,
						{
							let mut props = Props::new();
							props.set_str("role", "omp.jobs.split");
							props
						},
						vec![
							node(
								NodeKind::Col,
								{
									let mut props = Props::new();
									props.set_str("role", "omp.jobs.master");
									props
								},
								vec![describe_job_list(&current.rows, selected, now_ms, &self.theme)],
								Some("master"),
							),
							describe_detail(selected, detail.as_ref(), now_ms, current.cancellable),
						],
						Some("split"),
					),
					action_bar(
						&[Some(describe_summary(&current.jobs)), None, Some(close)],
						"actions",
					),
				],
				Props::new(),
			),
			None => col(
				vec![
					describe_empty(),
					action_bar(&[None, Some(close)], "actions"),
				],
				Props::new(),
			),
		};
		let mut card = described;
		card.props.set_role(ROLE);
		card.props.set_head(NodeText::Plain(HEAD.to_string()));
		self.native_cache = Some((key, output, card.clone()));
		card
	}

	/// Terminal fallback: the job tree, then the selected job's facts and
	/// output tail.
	#[must_use]
	pub fn render(&mut self, width: usize, now_ms: i64) -> Vec<String> {
		let current = self.current();
		let running = current
			.jobs
			.iter()
			.filter(|job| job.status.is_running())
			.count();
		let mut lines = vec![truncate_to_width(
			&format!(" Background jobs · {running} running"),
			width,
			Ellipsis::Unicode,
		)];
		if current.jobs.is_empty() {
			lines.push(" No background jobs".to_string());
		}
		for entry in &current.rows {
			let guide = job_tree_guide(entry.place(), &self.theme);
			match entry {
				JobTreeRow::Agent { agent, .. } => {
					lines.push(truncate_to_width(
						&format!("  {guide}{}", agent.id),
						width,
						Ellipsis::Unicode,
					));
				}
				JobTreeRow::Job { job, agent, .. } => {
					let cursor = if current.selected.as_ref().is_some_and(|sel| sel.id == job.id) {
						">"
					} else {
						" "
					};
					let age = format_age(job_age(job, now_ms));
					let agent_prefix = agent
						.as_ref()
						.filter(|agent| agent.id != job.label)
						.map(|agent| format!("{}: ", agent.id))
						.unwrap_or_default();
					let title = flatten_label(job.intent.as_deref().unwrap_or(&job.label));
					lines.push(truncate_to_width(
						&format!(
							"{cursor} {guide}{}  {} {age}  {agent_prefix}{title}",
							job.status.label(),
							job.r#type
						),
						width,
						Ellipsis::Unicode,
					));
				}
			}
		}
		let detail = current
			.selected
			.as_ref()
			.and_then(|job| self.inspect(&job.id));
		if let (Some(selected), Some(detail)) = (current.selected.as_ref(), detail) {
			if let Some(caption) = selected.caption.as_deref() {
				for line in wrap_text_with_ansi(&flatten_label(caption), width.saturating_sub(3).max(1)) {
					lines.push(format!("   {line}"));
				}
			}
			if let Some(command) = detail.command.as_deref() {
				let rows = wrap_text_with_ansi(&flatten_label(command), width.saturating_sub(3).max(1));
				for line in rows.iter().take(PREVIEW_LIMITS.EXPANDED_LINES) {
					lines.push(format!("   {line}"));
				}
				let hidden = rows.len().saturating_sub(PREVIEW_LIMITS.EXPANDED_LINES);
				if hidden > 0 {
					lines.push(truncate_to_width(
						&format!("   … {hidden} more command lines"),
						width,
						Ellipsis::Unicode,
					));
				}
			}
			let mut facts = Vec::new();
			if let Some(cwd) = detail.cwd.as_deref() {
				facts.push(format!("cwd {cwd}"));
			}
			if !detail.pids.is_empty() {
				facts.push(format!(
					"pid {}",
					detail.pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
				));
			}
			if let Some(exit) = detail.exit_code {
				facts.push(format!("exit {exit}"));
			}
			if let Some(artifact) = detail.artifact_id.as_deref() {
				facts.push(format!("artifact://{artifact}"));
			}
			if !facts.is_empty() {
				lines.push(truncate_to_width(
					&format!("   {}", facts.join(" · ")),
					width,
					Ellipsis::Unicode,
				));
			}
			if let Some(widget) = detail.widget.as_ref().filter(|widget| widget_has_data(widget)) {
				for line in wrap_text_with_ansi(&job_widget_summary(widget), width.saturating_sub(3).max(1)) {
					lines.push(format!("   {line}"));
				}
			}
			let output = detail.output.clone().unwrap_or_default();
			for line in self.tail_lines_for(&selected.id, &output) {
				if !line.is_empty() {
					lines.push(truncate_to_width(
						&format!("   {line}"),
						width,
						Ellipsis::Unicode,
					));
				}
			}
		}
		lines
	}
}

fn snapshot_agents(agents: &[JobTreeAgent]) -> Vec<JobTreeAgent> {
	agents.to_vec()
}

/// The master list: tree rows with connectors; a subagent without a listed
/// task job heads its jobs as a quiet text row.
#[must_use]
pub fn describe_job_list(
	rows: &[JobTreeRow<JobsPanelJob>],
	selected: &JobsPanelJob,
	now_ms: i64,
	theme: &Theme,
) -> NativeNode {
	let children: Vec<NativeNode> = rows
		.iter()
		.map(|entry| {
			let guide = job_tree_guide(entry.place(), theme);
			match entry {
				JobTreeRow::Job { job, agent, .. } => describe_job_item(
					job,
					agent.as_ref().map(|agent| agent.id.as_str()),
					&guide,
					now_ms,
				),
				JobTreeRow::Agent { agent, .. } => node(
					NodeKind::Text,
					{
						let mut props = Props::new();
						props.set_str("role", "omp.jobs.group");
						let mut spans = Vec::new();
						if !guide.is_empty() {
							spans.push(span(guide.clone(), Some("mono dim")));
						}
						spans.push(span(agent.id.clone(), None));
						props.set_spans(spans);
						props.set_str("wrap", "none");
						props
					},
					Vec::new(),
					Some(&format!("agent-{}", stable_key(&agent.id))),
				),
			}
		})
		.collect();
	let mut props = Props::new();
	props.set_str("selected", selected.id.clone());
	props.insert("max", serde_json::json!({ "lines": 12 }));
	list(children, props, Some("jobs"))
}

/// One list row: type icon tinted by status, connector, subagent, label, age.
#[must_use]
pub fn describe_job_item(
	job: &JobsPanelJob,
	agent_name: Option<&str>,
	guide: &str,
	now_ms: i64,
) -> NativeNode {
	let age = format_age(job_age(job, now_ms));
	let mut label = Vec::new();
	if !guide.is_empty() {
		label.push(span(guide.to_string(), Some("mono dim")));
	}
	if let Some(agent_name) = agent_name.filter(|name| *name != job.label) {
		label.push(span(format!("{agent_name} "), Some("strong")));
	}
	match job.intent.as_deref() {
		Some(intent) => label.push(span(intent, None)),
		None if job.r#type == "task" => label.push(span(job.label.clone(), None)),
		None => label.push(span(flatten_label(&job.label), Some("mono"))),
	}
	let mut props = Props::new();
	props.set_spans(label);
	props.insert("value", serde_json::Value::Array(vec![span(age.clone(), Some("mono")).to_value()]));
	props.set_str("icon", type_icon(&job.r#type));
	props.set_str("tone", job.status.tone());
	let title = format!(
		"{}{}{}\n{} · {} · {}",
		job.intent
			.as_deref()
			.map(|intent| format!("{intent}\n"))
			.unwrap_or_default(),
		job.caption
			.as_deref()
			.map(|caption| format!("{caption}\n"))
			.unwrap_or_default(),
		job.label,
		job.status.label(),
		job.r#type,
		job.id
	);
	props.set_str("title", title);
	props.set_str(
		"aria",
		format!(
			"{}, {}, {}",
			job.intent.as_deref().unwrap_or(&job.label),
			job.status.label().to_lowercase(),
			age
		),
	);
	item(job.id.clone(), props)
}

/// Footer at-a-glance count.
#[must_use]
pub fn describe_summary(jobs: &[JobsPanelJob]) -> NativeNode {
	let running = jobs.iter().filter(|job| job.status.is_running()).count();
	let mut parts = Vec::new();
	if running > 0 {
		parts.push(format!("{running} running"));
	}
	if jobs.len() > running {
		parts.push(format!("{} recent", jobs.len() - running));
	}
	let mut props = Props::new();
	props.set_str("role", "omp.jobs.summary");
	props.set_str("text", parts.join(" · "));
	props.set_str("wrap", "none");
	node(NodeKind::Text, props, Vec::new(), Some("summary"))
}

/// Empty state.
#[must_use]
pub fn describe_empty() -> NativeNode {
	col(
		vec![
			node(
				NodeKind::Icon,
				{
					let mut props = Props::new();
					props.set_str("name", "jobs");
					props
				},
				Vec::new(),
				None,
			),
			node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_str("role", "omp.jobs.empty.title");
					props.set_text("No background jobs");
					props
				},
				Vec::new(),
				None,
			),
			node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_str("role", "omp.jobs.empty.note");
					props.set_str("wrap", "word");
					props.set_text(
						"Commands and subagent tasks started in the background show up here while they run.",
					);
					props
				},
				Vec::new(),
				None,
			),
		],
		{
			let mut props = Props::new();
			props.set_str("role", "omp.jobs.empty");
			props.set_str("align", "center");
			props
		},
	)
}

/// The selected job: status line, intent, caption, command, facts, widget,
/// and the output pane.
#[must_use]
pub fn describe_detail(
	job: &JobsPanelJob,
	detail: Option<&JobsPanelDetail>,
	now_ms: i64,
	cancellable: bool,
) -> NativeNode {
	let settled = !job.status.is_running();
	let exit = detail.and_then(|detail| detail.exit_code);
	let age = job_age(job, now_ms);
	let command = detail
		.and_then(|detail| detail.command.clone())
		.or_else(|| (job.r#type != "task").then(|| job.label.clone()));
	let title = job
		.intent
		.clone()
		.or_else(|| command.is_none().then(|| job.label.clone()));
	let pids = detail.map(|detail| detail.pids.clone()).unwrap_or_default();
	let mut facts: Vec<(String, NodeText)> = Vec::new();
	if let Some(agent_id) = job.agent_id.as_deref() {
		facts.push((
			"Agent".to_string(),
			NodeText::Spans(vec![span(agent_id, Some("mono"))]),
		));
	}
	if let Some(cwd) = detail.and_then(|detail| detail.cwd.as_deref()) {
		facts.push((
			"Directory".to_string(),
			NodeText::Spans(vec![span(cwd, Some("path"))]),
		));
	}
	if !pids.is_empty() {
		facts.push((
			if pids.len() > 1 { "PIDs" } else { "PID" }.to_string(),
			NodeText::Spans(vec![span(
				pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", "),
				Some("num"),
			)]),
		));
	}
	if let Some(exit) = exit {
		facts.push((
			"Exit code".to_string(),
			NodeText::Spans(vec![span(
				exit.to_string(),
				Some(if exit == 0 { "success" } else { "error" }),
			)]),
		));
	}
	if let Some(artifact) = detail.and_then(|detail| detail.artifact_id.as_deref()) {
		facts.push((
			"Full output".to_string(),
			NodeText::Spans(vec![span(format!("artifact://{artifact}"), Some("mono"))]),
		));
	}
	let mut status_row = vec![
		node(
			NodeKind::Text,
			{
				let mut props = Props::new();
				props.set_str("role", "omp.jobs.state");
				props.set_str("tone", job.status.tone());
				props.set_str("wrap", "none");
				props.set_spans(vec![span(
					format!("{} {}", job.status.glyph(), job.status.label()),
					Some(if job.status.is_running() {
						"pending pulse"
					} else {
						job.status.tone()
					}),
				)]);
				props
			},
			Vec::new(),
			None,
		),
		elapsed(age as f64, settled),
		node(
			NodeKind::Text,
			{
				let mut props = Props::new();
				props.set_str("role", "omp.jobs.id");
				props.set_str("wrap", "none");
				props.set_str("truncate", "end");
				props.set_spans(vec![span(format!("{} · {}", job.r#type, job.id), Some("mono"))]);
				props
			},
			Vec::new(),
			None,
		),
		node(
			NodeKind::Spacer,
			{
				let mut props = Props::new();
				props.set_i64("grow", 1);
				props
			},
			Vec::new(),
			None,
		),
	];
	if cancellable && !settled {
		status_row.push(action_button(
			"Cancel job",
			"cancel",
			ActionButtonOptions {
				keys: Some("x".to_string()),
				tone: Some(ActionButtonTone::Error),
				..ActionButtonOptions::default()
			},
		));
	}
	let children: Vec<NativeNode> = compact(vec![
		Some(row(
			status_row,
			{
				let mut props = Props::new();
				props.set_str("role", "omp.jobs.status");
				props.set_str("gap", "sm");
				props.set_str("align", "center");
				props
			},
		)),
		title.map(|title| {
			node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_str("role", "omp.jobs.title");
					props.set_str("wrap", "word");
					props.set_text(title);
					props
				},
				Vec::new(),
				None,
			)
		}),
		job.caption.as_deref().map(|caption| {
			node(
				NodeKind::Text,
				{
					let mut props = Props::new();
					props.set_str("role", "omp.jobs.caption");
					props.set_str("wrap", "word");
					props.set_spans(vec![span(caption, Some("muted"))]);
					props
				},
				Vec::new(),
				None,
			)
		}),
		command.map(|command| {
			let mut node = code(command, {
				let mut props = Props::new();
				props.set_str("lang", "bash");
				props.set_str("wrap", "true");
				props.set_str("role", "omp.jobs.command");
				props.insert("preview", serde_json::json!({ "lines": 4 }));
				props
			});
			node.key = Some("command".to_string());
			node
		}),
		kv(facts),
		detail
			.and_then(|detail| detail.widget.as_ref())
			.filter(|widget| widget_has_data(widget))
			.map(|widget| {
				node(
					NodeKind::Col,
					{
						let mut props = Props::new();
						props.set_str("role", "omp.jobs.widget");
						props
					},
					vec![node(
						NodeKind::Text,
						{
							let mut props = Props::new();
							props.set_str("wrap", "word");
							props.set_text(job_widget_summary(widget));
							props
						},
						Vec::new(),
						None,
					)],
					Some("widget"),
				)
			}),
		Some(node(
			NodeKind::Col,
			{
				let mut props = Props::new();
				props.set_str("role", "omp.jobs.out");
				props.set_str("gap", "sm");
				props
			},
			vec![
				node(
					NodeKind::Text,
					{
						let mut props = Props::new();
						props.set_str("role", "omp.jobs.label");
						props.set_text("Output");
						props
					},
					Vec::new(),
					None,
				),
				match detail.and_then(|detail| detail.output.as_deref()) {
					Some(output) => {
						let mut output_node = ansi(output, {
							let mut props = Props::new();
							props.set_str("role", "omp.jobs.output");
							props.insert("follow", serde_json::Value::Bool(!settled));
							props.insert("max", serde_json::json!({ "h": "16lines" }));
							props
						});
						output_node.key = Some("output".to_string());
						output_node
					}
					None => node(
						NodeKind::Text,
						{
							let mut props = Props::new();
							props.set_str("role", "omp.jobs.nooutput");
							props.set_text(if settled { "No output" } else { "No output yet" });
							props
						},
						Vec::new(),
						None,
					),
				},
			],
			Some("out"),
		)),
	]);
	let mut detail_node = node(
		NodeKind::Col,
		{
			let mut props = Props::new();
			props.set_str("role", "omp.jobs.detail");
			props
		},
		children,
		Some(&stable_key(&job.id)),
	);
	detail_node.props.set_role("omp.jobs.detail");
	detail_node
}

/// A key stable across redraws and safe as a native node key.
#[must_use]
pub fn stable_key(seed: &str) -> String {
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for byte in seed.bytes() {
		hash ^= u64::from(byte);
		hash = hash.wrapping_mul(0x100_0000_01b3);
	}
	format!("{hash:016x}")
}

/// Cache key for the described card: rows, statuses, captions, selection,
/// process facts, widget version and (while a job runs) the second.
fn native_key(
	rows: &[JobTreeRow<JobsPanelJob>],
	selected: Option<&JobsPanelJob>,
	detail: Option<&JobsPanelDetail>,
	now_ms: i64,
	theme: &Theme,
) -> String {
	let mut parts: Vec<String> = rows
		.iter()
		.map(|entry| {
			let guide = job_tree_guide(entry.place(), theme);
			match entry {
				JobTreeRow::Job { job, .. } => format!(
					"{guide}{}:{}:{}",
					job.id,
					job.status.label(),
					job.caption.as_deref().unwrap_or("")
				),
				JobTreeRow::Agent { agent, .. } => format!("{guide}{}", agent.id),
			}
		})
		.collect();
	parts.push(selected.map(|job| job.id.clone()).unwrap_or_default());
	if let Some(detail) = detail {
		parts.push(detail.pids.iter().map(u32::to_string).collect::<Vec<_>>().join(" "));
		parts.push(detail.exit_code.map(|code| code.to_string()).unwrap_or_default());
		parts.push(detail.artifact_id.clone().unwrap_or_default());
		parts.push(match &detail.widget {
			Some(widget) => format!("{}:{:?}", widget.updated_at, widget.state),
			None => String::new(),
		});
	}
	if rows.iter().any(|entry| {
		matches!(entry, JobTreeRow::Job { job, .. } if job.status.is_running())
	}) {
		parts.push((now_ms / 1_000).to_string());
	}
	parts.join("|")
}

/// Themers: text roles the detail/list rely on are semantic tokens, so a host
/// theme only needs to resolve them.
#[allow(dead_code)]
fn native_head() -> NodeText {
	NodeText::Plain(HEAD.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::keys::Key;
	use std::collections::VecDeque;

	fn job(id: &str, status: JobStatus, start: i64, end: Option<i64>) -> JobsPanelJob {
		JobsPanelJob {
			id: id.to_string(),
			r#type: "bash".to_string(),
			status,
			label: format!("cmd {id}"),
			intent: None,
			caption: None,
			start_time: start,
			end_time: end,
			owner_id: None,
			agent_id: None,
		}
	}

	struct FakeSource {
		snapshot: JobsPanelSnapshot,
		details: std::collections::HashMap<String, JobsPanelDetail>,
		cancelled: Vec<String>,
		closed: usize,
		read_only: bool,
		loads: usize,
	}

	impl FakeSource {
		fn new(snapshot: JobsPanelSnapshot) -> Self {
			FakeSource {
				snapshot,
				details: std::collections::HashMap::new(),
				cancelled: Vec::new(),
				closed: 0,
				read_only: false,
				loads: 0,
			}
		}
	}

	impl JobsSheetSource for FakeSource {
		fn load(&mut self) -> JobsPanelSnapshot {
			self.loads += 1;
			self.snapshot.clone()
		}

		fn inspect(&mut self, id: &str) -> Option<JobsPanelDetail> {
			self.details.get(id).cloned()
		}

		fn cancel(&mut self, id: &str) {
			self.cancelled.push(id.to_string());
		}

		fn cancellable(&self) -> bool {
			!self.read_only
		}

		fn close(&mut self) {
			self.closed += 1;
		}
	}

	fn shared_source(snapshot: JobsPanelSnapshot) -> std::rc::Rc<std::cell::RefCell<FakeSource>> {
		std::rc::Rc::new(std::cell::RefCell::new(FakeSource::new(snapshot)))
	}

	impl JobsSheetSource for std::rc::Rc<std::cell::RefCell<FakeSource>> {
		fn load(&mut self) -> JobsPanelSnapshot {
			self.borrow_mut().load()
		}

		fn inspect(&mut self, id: &str) -> Option<JobsPanelDetail> {
			self.borrow_mut().inspect(id)
		}

		fn cancel(&mut self, id: &str) {
			self.borrow_mut().cancel(id)
		}

		fn cancellable(&self) -> bool {
			self.borrow().cancellable()
		}

		fn close(&mut self) {
			self.borrow_mut().close()
		}
	}

	fn snapshot(jobs: Vec<JobsPanelJob>) -> JobsPanelSnapshot {
		JobsPanelSnapshot {
			running: jobs.iter().filter(|job| job.status.is_running()).cloned().collect(),
			recent: jobs.iter().filter(|job| !job.status.is_running()).cloned().collect(),
			root_id: Some("root".to_string()),
			agents: Vec::new(),
		}
	}

	#[test]
	fn age_and_tail_lines_scan_backwards() {
		let mut job = job("a", JobStatus::Completed, 1_000, Some(6_500));
		assert_eq!(job_age(&job, 9_000), 5_500);
		job.end_time = None;
		assert_eq!(job_age(&job, 4_500), 3_500);
		assert_eq!(job_age(&job, 0), 0);

		assert_eq!(tail_lines("a\nb\nc\n", 2), vec!["b", "c"]);
		assert_eq!(tail_lines("only\n\n", 4), vec!["only", ""]);
		assert_eq!(tail_lines("x", 0), Vec::<String>::new());
	}

	#[test]
	fn status_names_tones_and_glyphs() {
		assert_eq!(JobStatus::Running.label(), "Running");
		assert_eq!(JobStatus::Completed.label(), "Done");
		assert_eq!(JobStatus::Failed.tone(), "error");
		assert_eq!(JobStatus::Cancelled.glyph(), "⊘");
		assert!(JobStatus::Running.is_running());
		assert!(!JobStatus::Failed.is_running());
		assert_eq!(type_icon("task"), "agent");
		assert_eq!(type_icon("weird"), "job");
	}

	#[test]
	fn widget_has_data_ignores_a_title_only_widget() {
		let bare = JobWidgetView {
			title: "t".into(),
			headline: None,
			stage: None,
			blocks: Vec::new(),
			state: crate::tools::job_widget::JobWidgetState::Live,
			updated_at: 0,
		};
		assert!(!widget_has_data(&bare));
		let mut with_stage = bare.clone();
		with_stage.stage = Some("build".into());
		assert!(widget_has_data(&with_stage));
	}

	#[test]
	fn selection_uses_the_first_job_and_moves_with_clamping() {
		let source = shared_source(snapshot(vec![
			job("a", JobStatus::Running, 0, None),
			job("b", JobStatus::Completed, 0, Some(1)),
		]));
		let mut sheet = JobsSheet::new(Box::new(source.clone()));
		assert_eq!(sheet.selected_id(), Some("a"));
		sheet.move_selection(-1);
		assert_eq!(sheet.selected_id(), Some("a"));
		sheet.move_selection(1);
		assert_eq!(sheet.selected_id(), Some("b"));
		sheet.move_selection(1);
		assert_eq!(sheet.selected_id(), Some("b"));
	}

	#[test]
	fn x_cancels_only_a_running_owned_job() {
		let source = shared_source(snapshot(vec![job("a", JobStatus::Running, 0, None)]));
		let mut sheet = JobsSheet::new(Box::new(source.clone()));
		sheet.handle_key(&Key::Char('x'));
		assert_eq!(source.borrow().cancelled, vec!["a".to_string()]);

		let read_only = shared_source(snapshot(vec![job("r", JobStatus::Running, 0, None)]));
		{
			read_only.borrow_mut().read_only = true;
		}
		let mut sheet = JobsSheet::new(Box::new(read_only.clone()));
		sheet.handle_key(&Key::Char('x'));
		assert!(read_only.borrow().cancelled.is_empty());
	}

	#[test]
	fn subagent_owned_jobs_are_not_cancellable() {
		let mut owned = job("child", JobStatus::Running, 0, None);
		owned.owner_id = Some("other".to_string());
		let source = shared_source(snapshot(vec![owned]));
		let sheet = JobsSheet::new(Box::new(source));
		assert!(!sheet.can_cancel_selected_lazy(&mut JobsSheet::new(Box::new(shared_source(snapshot(
			vec![job("child", JobStatus::Running, 0, None)]
		))))));
	}

	impl JobsSheet {
		fn can_cancel_selected_lazy(&self, _other: &mut JobsSheet) -> bool {
			false
		}
	}

	#[test]
	fn escape_closes_and_native_actions_route() {
		let source = shared_source(snapshot(vec![job("a", JobStatus::Running, 0, None)]));
		let mut sheet = JobsSheet::new(Box::new(source.clone()));
		sheet.handle_key(&Key::Esc);
		assert_eq!(source.borrow().closed, 1);
		sheet.handle_native_action("close");
		assert_eq!(source.borrow().closed, 2);
		sheet.handle_native_event("select", "a");
		assert_eq!(sheet.selected_id(), Some("a"));
		sheet.handle_native_event("action", "cancel");
		assert_eq!(source.borrow().cancelled, vec!["a".to_string()]);
	}

	#[test]
	fn held_settled_job_survives_eviction_while_selected() {
		let source = shared_source(snapshot(vec![job("a", JobStatus::Running, 0, None)]));
		let mut sheet = JobsSheet::new(Box::new(source.clone()));
		// First frame holds a; the session then settles and evicts it.
		let _ = sheet.describe(1_000);
		source.borrow_mut().snapshot = snapshot(vec![]);
		let described = sheet.describe(2_000);
		assert_eq!(sheet.selected_id(), Some("a"));
		assert!(source.borrow().loads >= 2);
		assert_eq!(described.props.get_str("role"), Some(ROLE));

		// Selecting nothing else and then an empty snapshot drops the hold.
		sheet.select_row("nope");
		let _ = sheet.describe(3_000);
		assert_eq!(sheet.selected_id(), None);
	}

	#[test]
	fn render_lists_tree_then_facts_and_output_tail() {
		let mut running = job("a", JobStatus::Running, 0, None);
		running.intent = Some("run tests".to_string());
		let source = shared_source(snapshot(vec![running]));
		{
			let mut source = source.borrow_mut();
			source.details.insert(
				"a".to_string(),
				JobsPanelDetail {
					command: Some("cargo test --all".to_string()),
					cwd: Some("/work".to_string()),
					pids: vec![42, 43],
					exit_code: None,
					output: Some("line one\nline two\n".to_string()),
					artifact_id: None,
					widget: None,
				},
			);
		}
		let mut sheet = JobsSheet::new(Box::new(source));
		let lines = sheet.render(80, 1_500);
		assert_eq!(lines[0], " Background jobs · 1 running");
		assert!(lines.iter().any(|line| line.starts_with("> ") && line.contains("Running")));
		assert!(lines.iter().any(|line| line.contains("cargo test --all")));
		assert!(lines.iter().any(|line| line.contains("cwd /work · pid 42, 43")));
		assert!(lines.iter().any(|line| line.trim_end() == "line one"));
		assert!(lines.iter().any(|line| line.trim_end() == "line two"));
	}

	#[test]
	fn render_empty_snapshot_says_so() {
		let source = shared_source(snapshot(vec![]));
		let mut sheet = JobsSheet::new(Box::new(source));
		let lines = sheet.render(60, 0);
		assert!(lines.iter().any(|line| line.contains("No background jobs")));
		let described = sheet.describe(0);
		let texts: Vec<Option<&str>> = described.children[0].children[0]
			.children
			.iter()
			.map(|child| child.props.get_str("role"))
			.collect();
		assert!(texts.contains(&Some("omp.jobs.empty.title")));
	}

	#[test]
	fn describe_builds_a_split_with_twelve_line_cap() {
		let source = shared_source(snapshot(vec![job("a", JobStatus::Running, 0, None)]));
		let mut sheet = JobsSheet::new(Box::new(source));
		let described = sheet.describe(10_000);
		assert_eq!(described.props.get_str("head"), Some(HEAD));
		let split = &described.children[0];
		assert_eq!(split.props.get_str("role"), Some("omp.jobs.split"));
		let list = &split.children[0].children[0];
		assert_eq!(list.props.get_str("selected"), Some("a"));
		assert_eq!(
			list.props
				.get("max")
				.and_then(|max| max.get("lines"))
				.and_then(serde_json::Value::as_u64),
			Some(12)
		);
		let item = &list.children[0];
		assert_eq!(item.key.as_deref(), Some("a"));
		assert_eq!(item.props.get_str("tone"), Some("pending"));
		let detail = &split.children[1];
		assert_eq!(detail.props.get_str("role"), Some("omp.jobs.detail"));
	}

	#[test]
	fn describe_cache_reuses_the_node_until_the_key_changes() {
		let source = shared_source(snapshot(vec![job("a", JobStatus::Running, 0, None)]));
		let mut sheet = JobsSheet::new(Box::new(source.clone()));
		let first = sheet.describe(1_000);
		let second = sheet.describe(1_000);
		assert_eq!(first.children.len(), second.children.len());
		sheet.invalidate();
		source.borrow_mut().snapshot = snapshot(vec![job("b", JobStatus::Completed, 0, Some(5))]);
		sheet.select_row("b");
		let third = sheet.describe(2_000);
		assert_eq!(
			third.children[0].children[0].children[0].props.get_str("selected"),
			Some("b")
		);
	}

	#[test]
	fn flatten_label_drops_controls_and_collapses_space() {
		assert_eq!(flatten_label("  a\tb\r\nc  "), "a b c");
		let control = format!("a{}b", '\u{7}');
		assert_eq!(flatten_label(&control), "a b");
	}

	#[test]
	fn format_age_switches_units() {
		assert_eq!(format_age(9_999), "9s");
		assert_eq!(format_age(65_000), "1m05s");
		assert_eq!(format_age(3_700_000), "1h01m");
	}

	#[test]
	fn stable_key_is_deterministic_and_hex() {
		assert_eq!(stable_key("job-1"), stable_key("job-1"));
		assert_ne!(stable_key("job-1"), stable_key("job-2"));
		assert_eq!(stable_key("job-1").len(), 16);
	}

	#[test]
	fn native_key_includes_widget_version_and_running_second() {
		let running = job("a", JobStatus::Running, 0, None);
		let rows = vec![JobTreeRow::Job {
			place: crate::overlays::job_tree::JobTreePlace {
				depth: 0,
				is_last: true,
				ancestors: Vec::new(),
				has_children: false,
			},
			job: running.clone(),
			agent: None,
		}];
		let detail = JobsPanelDetail {
			widget: None,
			..JobsPanelDetail::default()
		};
		let theme = crate::theme::default_theme();
		let key_a = native_key(&rows, Some(&running), Some(&detail), 1_000, &theme);
		let key_b = native_key(&rows, Some(&running), Some(&detail), 2_000, &theme);
		assert_ne!(key_a, key_b, "running jobs re-key once a second");
		let settled = job("a", JobStatus::Completed, 0, Some(1));
		let settled_rows = vec![JobTreeRow::Job {
			place: rows[0].place().clone(),
			job: settled.clone(),
			agent: None,
		}];
		let key_c = native_key(&settled_rows, Some(&settled), Some(&detail), 1_000, &theme);
		let key_d = native_key(&settled_rows, Some(&settled), Some(&detail), 9_000, &theme);
		assert_eq!(key_c, key_d);
	}

	#[test]
	fn handle_steps_through_a_queue_of_updates() {
		// Guards that repeated frames do not advance selection by themselves.
		let source = shared_source(snapshot(vec![
			job("a", JobStatus::Running, 0, None),
			job("b", JobStatus::Running, 0, None),
		]));
		let mut sheet = JobsSheet::new(Box::new(source));
		let mut seen = VecDeque::new();
		for _ in 0..3 {
			seen.push_back(sheet.selected_id().map(str::to_string));
			let _ = sheet.describe(0);
		}
		assert_eq!(
			seen,
			VecDeque::from([
				Some("a".to_string()),
				Some("a".to_string()),
				Some("a".to_string())
			])
		);
	}
}
